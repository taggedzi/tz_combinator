#!/usr/bin/env python3
"""Cross-platform release driver for tz_combinator.

Release policy belongs here. GitHub Actions is only an execution adapter for
native builds, GitHub provenance, and publishing.
"""

from __future__ import annotations

import argparse
import datetime as dt
import gzip
import hashlib
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path
from typing import NoReturn, Sequence


GIT_CLIFF_VERSION = "2.12.0"
SEMVER_RE = re.compile(r"^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$")
PRODUCT_MANIFESTS = (
    Path("crates/combinator-app/Cargo.toml"),
    Path("crates/combinator-cli/Cargo.toml"),
    Path("crates/combinator-codecs/Cargo.toml"),
    Path("crates/combinator-core/Cargo.toml"),
    Path("crates/combinator-gui/Cargo.toml"),
    Path("crates/combinator-tui/Cargo.toml"),
)
BENCHMARK_MANIFEST = Path("crates/combinator-benchmarks/Cargo.toml")
PACKAGE_DOCUMENTS = ("LICENSE", "README.md", "CHANGELOG.md", "THIRD_PARTY_LICENSES.md")


class ReleaseError(Exception):
    pass


def die(message: str) -> NoReturn:
    raise ReleaseError(message)


def run(
    args: Sequence[str],
    *,
    capture: bool = False,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            list(args),
            check=check,
            text=True,
            capture_output=capture,
            encoding="utf-8",
        )
    except FileNotFoundError:
        die(f"required command was not found: {args[0]}")
    except subprocess.CalledProcessError as error:
        detail = (error.stderr or error.stdout or "").strip()
        suffix = f": {detail}" if detail else ""
        die(f"command failed ({' '.join(args)}){suffix}")


def git(*args: str, capture: bool = True, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(("git", *args), capture=capture, check=check)


def enter_repo() -> Path:
    root = Path(git("rev-parse", "--show-toplevel").stdout.strip()).resolve()
    os.chdir(root)
    return root


def parse_version(value: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(value)
    if not match:
        die("version must be a stable semantic version such as 0.2.0")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def parse_date(value: str) -> str:
    try:
        parsed = dt.date.fromisoformat(value)
    except ValueError:
        die("release date must be a valid calendar date in YYYY-MM-DD form")
    if parsed.isoformat() != value:
        die("release date must use YYYY-MM-DD")
    return value


def is_link_or_reparse(path: Path) -> bool:
    if path.is_symlink() or getattr(os.path, "isjunction", lambda _: False)(path):
        return True
    try:
        attributes = path.lstat().st_file_attributes  # type: ignore[attr-defined]
    except (AttributeError, FileNotFoundError):
        return False
    return bool(attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT)


def require_regular(path: Path) -> None:
    if not path.exists() or not path.is_file() or is_link_or_reparse(path):
        die(f"{path.as_posix()} must be a regular, non-link file")


def require_directory(path: Path) -> None:
    if not path.exists() or not path.is_dir() or is_link_or_reparse(path):
        die(f"{path.as_posix()} must be an existing, non-link directory")


def safe_output_directory(value: str) -> Path:
    raw = Path(value)
    if ".." in raw.parts:
        die("artifact output must not contain parent traversal")
    repo = Path.cwd().resolve()
    absolute = raw if raw.is_absolute() else repo / raw
    lexical = Path(os.path.abspath(absolute))
    try:
        relative = lexical.relative_to(repo)
    except ValueError:
        die("artifact output must remain inside the repository worktree")

    cursor = repo
    for part in relative.parts:
        cursor /= part
        if cursor.exists() and is_link_or_reparse(cursor):
            die(f"artifact output ancestor {cursor} must not be a link or reparse point")
    resolved = lexical.resolve(strict=False)
    try:
        resolved.relative_to(repo)
    except ValueError:
        die("artifact output must not resolve outside the repository worktree")
    if resolved.exists():
        require_directory(resolved)
    else:
        require_directory(resolved.parent)
        resolved.mkdir()
    return resolved


def atomic_write(path: Path, content: bytes) -> None:
    require_directory(path.parent)
    if path.exists():
        require_regular(path)
        mode = stat.S_IMODE(path.stat().st_mode)
    else:
        mode = 0o644
    fd, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(fd, "wb") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


def read_text(path: Path) -> str:
    require_regular(path)
    return path.read_text(encoding="utf-8")


def ensure_clean_worktree() -> None:
    if git("status", "--porcelain").stdout:
        die("the worktree must be clean")


def tag_exists(version: str) -> bool:
    return git("rev-parse", "--verify", "--quiet", f"refs/tags/v{version}", check=False).returncode == 0


def manifest_version(text: str, path: Path) -> str:
    matches = re.findall(r'^version = "([^"]+)"$', text, re.MULTILINE)
    if len(matches) != 1 or not SEMVER_RE.fullmatch(matches[0]):
        die(f"{path.as_posix()} must contain exactly one stable package version")
    return matches[0]


def update_manifest(text: str, old: str, new: str, *, package: bool) -> str:
    if package:
        text, count = re.subn(
            rf'^version = "{re.escape(old)}"$',
            f'version = "{new}"',
            text,
            count=1,
            flags=re.MULTILINE,
        )
        if count != 1:
            die("failed to update a workspace package version")
    dependency = re.compile(
        rf'^(combinator-[a-z-]+ = \{{[^\n]*\bversion = "){re.escape(old)}("[^\n]*\bpath = [^\n]+)$',
        re.MULTILINE,
    )
    return dependency.sub(rf"\g<1>{new}\g<2>", text)


def update_lock(text: str, old: str, new: str) -> str:
    names = {f"combinator-{name}" for name in ("app", "cli", "codecs", "core", "gui", "tui")}
    updated = 0
    blocks = re.split(r"(?=^\[\[package\]\]$)", text, flags=re.MULTILINE)
    for index, block in enumerate(blocks):
        name_match = re.search(r'^name = "([^"]+)"$', block, re.MULTILINE)
        if not name_match or name_match.group(1) not in names:
            continue
        block, count = re.subn(
            rf'^version = "{re.escape(old)}"$',
            f'version = "{new}"',
            block,
            count=1,
            flags=re.MULTILINE,
        )
        if count != 1:
            die(f"Cargo.lock has an unexpected version for {name_match.group(1)}")
        blocks[index] = block
        updated += 1
    if updated != len(names):
        die("Cargo.lock does not contain exactly the six product packages")
    return "".join(blocks)


def render_notes(version: str, release_date: str) -> str:
    actual = run(("git-cliff", "--version"), capture=True).stdout.strip()
    expected = f"git-cliff {GIT_CLIFF_VERSION}"
    if actual != expected:
        die(f"expected {expected}, found: {actual}")

    if tag_exists(version):
        tag_commit = git("rev-parse", f"refs/tags/v{version}^{{commit}}").stdout.strip()
        head_commit = git("rev-parse", "HEAD").stdout.strip()
        if tag_commit != head_commit:
            die(f"tag v{version} does not point to HEAD")
        range_args = ("--current",)
    else:
        range_args = ("--unreleased", "--tag", version)

    with tempfile.TemporaryDirectory(prefix="tz-combinator-notes-") as temporary:
        output = Path(temporary) / "notes.md"
        run(
            (
                "git-cliff",
                "--config",
                "cliff.toml",
                "--no-exec",
                *range_args,
                "--output",
                str(output),
            )
        )
        text = output.read_text(encoding="utf-8").strip()

    lines = text.splitlines()
    if not lines or lines[0] != f"## [{version}]":
        die("git-cliff produced an unexpected release heading")
    if not any(line.startswith("- ") for line in lines):
        die("no user-visible release commits were found")
    lines[0] = f"## [{version}] - {release_date}"
    return "\n".join(lines) + "\n"


def validate_notes(version: str, notes: str) -> tuple[str, str]:
    heading = re.compile(rf"^## \[{re.escape(version)}\] - (\d{{4}}-\d{{2}}-\d{{2}})$", re.MULTILINE)
    matches = heading.findall(notes)
    if len(matches) != 1 or len(re.findall(r"^## \[", notes, re.MULTILINE)) != 1:
        die(f"release-notes/{version}.md must contain exactly one dated section")
    release_date = parse_date(matches[0])
    if not any(line.startswith("- ") for line in notes.splitlines()):
        die(f"release-notes/{version}.md must contain at least one changelog entry")
    return f"## [{version}] - {release_date}", release_date


def synchronized_changelog(version: str, notes: str, changelog: str) -> str:
    heading, _ = validate_notes(version, notes)
    lines = changelog.splitlines(keepends=True)
    release_indices = [index for index, line in enumerate(lines) if re.match(r"^## \[\d+\.\d+\.\d+\]", line)]
    matching = [index for index, line in enumerate(lines) if line.rstrip("\r\n") == heading]
    if len(matching) > 1:
        die(f"CHANGELOG.md contains duplicate sections for {version}")
    if matching:
        start = matching[0]
        later = [index for index in release_indices if index > start]
        end = later[0] if later else len(lines)
    else:
        if not release_indices:
            die("CHANGELOG.md has no existing release section")
        start = end = release_indices[0]
    prefix = "".join(lines[:start])
    suffix = "".join(lines[end:])
    return prefix.rstrip("\r\n") + "\n\n" + notes.rstrip("\r\n") + "\n\n" + suffix.lstrip("\r\n")


def verify_release(version: str) -> None:
    parse_version(version)
    changelog = read_text(Path("CHANGELOG.md"))
    lock = read_text(Path("Cargo.lock"))
    notes_path = Path(f"release-notes/{version}.md")
    notes = read_text(notes_path)
    heading, _ = validate_notes(version, notes)

    sections = re.findall(
        rf"(?ms)^{re.escape(heading)}\r?\n.*?(?=^## \[|\Z)",
        changelog,
    )
    if len(sections) != 1:
        die(f"CHANGELOG.md must contain exactly one dated section for {version}")
    if sections[0].rstrip() != notes.rstrip():
        die(f"the changelog section does not match {notes_path.as_posix()}")

    for manifest in PRODUCT_MANIFESTS:
        text = read_text(manifest)
        if manifest_version(text, manifest) != version:
            die(f"{manifest.as_posix()} package version is not {version}")
        verify_internal_dependencies(text, manifest, version)

    benchmark = read_text(BENCHMARK_MANIFEST)
    if manifest_version(benchmark, BENCHMARK_MANIFEST) != "0.0.0":
        die("the benchmark package version must remain 0.0.0")
    verify_internal_dependencies(benchmark, BENCHMARK_MANIFEST, version)

    names = {f"combinator-{name}" for name in ("app", "cli", "codecs", "core", "gui", "tui")}
    found: dict[str, str] = {}
    for block in re.split(r"(?=^\[\[package\]\]$)", lock, flags=re.MULTILINE):
        name = re.search(r'^name = "([^"]+)"$', block, re.MULTILINE)
        package_version = re.search(r'^version = "([^"]+)"$', block, re.MULTILINE)
        if name and package_version and name.group(1) in names:
            if name.group(1) in found:
                die(f"Cargo.lock contains duplicate package {name.group(1)}")
            found[name.group(1)] = package_version.group(1)
    if found != {name: version for name in names}:
        die(f"Cargo.lock product packages are not synchronized at {version}")

    print(f"Release metadata for v{version} is synchronized and reproducible.")


def verify_internal_dependencies(text: str, path: Path, version: str) -> None:
    for line in text.splitlines():
        if re.match(r"^combinator-[a-z-]+ = \{", line) and "path = " in line:
            if f'version = "{version}"' not in line:
                die(f"{path.as_posix()} has a stale internal dependency: {line}")


def command_prepare(args: argparse.Namespace) -> None:
    version = args.version
    new_version = parse_version(version)
    release_date = parse_date(args.date or dt.datetime.now(dt.timezone.utc).date().isoformat())
    ensure_clean_worktree()
    if tag_exists(version):
        die(f"tag v{version} already exists")

    require_directory(Path("release-notes"))
    notes_path = Path(f"release-notes/{version}.md")
    if notes_path.exists() or is_link_or_reparse(notes_path):
        die(f"{notes_path.as_posix()} already exists")

    originals: dict[Path, bytes] = {}
    paths = (Path("CHANGELOG.md"), Path("Cargo.lock"), *PRODUCT_MANIFESTS, BENCHMARK_MANIFEST)
    for path in paths:
        require_regular(path)
        originals[path] = path.read_bytes()

    manifest_text = {path: originals[path].decode("utf-8") for path in PRODUCT_MANIFESTS}
    old_versions = {manifest_version(text, path) for path, text in manifest_text.items()}
    if len(old_versions) != 1:
        die("workspace package versions are not synchronized")
    old_version = old_versions.pop()
    if new_version <= parse_version(old_version):
        die(f"new version {version} must be greater than current version {old_version}")

    changelog = originals[Path("CHANGELOG.md")].decode("utf-8")
    unreleased = re.search(r"(?ms)^## \[Unreleased\]\s*(.*?)(?=^## \[)", changelog)
    if not unreleased:
        die("CHANGELOG.md is missing an Unreleased heading")
    if unreleased.group(1).strip():
        die("the Unreleased section must be empty before deterministic generation")

    benchmark = originals[BENCHMARK_MANIFEST].decode("utf-8")
    if manifest_version(benchmark, BENCHMARK_MANIFEST) != "0.0.0":
        die("the benchmark package version must remain 0.0.0")

    notes = render_notes(version, release_date)
    changes: dict[Path, bytes] = {
        notes_path: notes.encode("utf-8"),
        Path("CHANGELOG.md"): synchronized_changelog(version, notes, changelog).encode("utf-8"),
        Path("Cargo.lock"): update_lock(
            originals[Path("Cargo.lock")].decode("utf-8"), old_version, version
        ).encode("utf-8"),
        BENCHMARK_MANIFEST: update_manifest(benchmark, old_version, version, package=False).encode("utf-8"),
    }
    for path, text in manifest_text.items():
        changes[path] = update_manifest(text, old_version, version, package=True).encode("utf-8")

    try:
        for path, content in changes.items():
            atomic_write(path, content)
        verify_release(version)
    except Exception:
        for path, content in originals.items():
            atomic_write(path, content)
        if notes_path.exists() and not is_link_or_reparse(notes_path):
            notes_path.unlink()
        raise

    print(f"Prepared release v{version} ({release_date}). Review the generated changes before committing.")


def command_sync(args: argparse.Namespace) -> None:
    version = args.version
    parse_version(version)
    notes = read_text(Path(f"release-notes/{version}.md"))
    changelog_path = Path("CHANGELOG.md")
    changelog = read_text(changelog_path)
    atomic_write(changelog_path, synchronized_changelog(version, notes, changelog).encode("utf-8"))
    verify_release(version)


def command_check(args: argparse.Namespace) -> None:
    verify_release(args.version)
    commands = (
        ("cargo", "test", "--workspace", "--locked"),
        ("cargo", "fmt", "--all", "--", "--check"),
        ("cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"),
    )
    for command in commands:
        run(command)
    if args.release_build:
        run(("cargo", "build", "--workspace", "--release", "--locked"))
    if args.full:
        run(("cargo", "package", "--workspace", "--locked", "--no-verify"))
        with tempfile.TemporaryDirectory(prefix="tz-combinator-install-") as install_root:
            run(
                (
                    "cargo",
                    "install",
                    "--path",
                    "crates/combinator-cli",
                    "--locked",
                    "--root",
                    install_root,
                )
            )
            extension = ".exe" if platform.system() == "Windows" else ""
            smoke_test(Path(install_root) / "bin" / f"combinator{extension}")
        run(
            (
                "cargo",
                "llvm-cov",
                "--workspace",
                "--all-features",
                "--exclude",
                "combinator-gui",
                "--exclude",
                "combinator-tui",
                "--summary-only",
                "--fail-under-lines",
                "80",
            )
        )
        run(("cargo", "audit"))
        run(("cargo", "deny", "--all-features", "check"))


def native_platform() -> tuple[str, str]:
    machine = platform.machine().lower()
    if machine not in {"x86_64", "amd64"}:
        die(f"unsupported release architecture: {platform.machine()}")
    system = platform.system()
    if system == "Linux":
        return "linux-x86_64", ""
    if system == "Windows":
        return "windows-x86_64", ".exe"
    die(f"unsupported release operating system: {system}")


def smoke_test(binary: Path) -> None:
    run((str(binary), "--version"))
    result = run(
        (str(binary), "--list", "red,blue", "--list", "car,bike", "--sep", "-"),
        capture=True,
    ).stdout.splitlines()
    if result != ["red-car", "red-bike", "blue-car", "blue-bike"]:
        die("release CLI smoke test returned unexpected output")
    jsonl = run(
        (str(binary), "--list", "a,b", "--list", "1,2", "--format", "jsonl"),
        capture=True,
    ).stdout
    if '"i":0' not in jsonl:
        die("release CLI JSONL smoke test returned unexpected output")


def archive_inputs(extension: str) -> list[tuple[str, Path, int]]:
    names = ("combinator", "combinator-gui", "combinator-tui")
    inputs = [(name + extension, Path("target/release") / (name + extension), 0o755) for name in names]
    inputs.extend((name, Path(name), 0o644) for name in PACKAGE_DOCUMENTS)
    for _, path, _ in inputs:
        require_regular(path)
    return inputs


def create_tar_gz(archive: Path, root_name: str, inputs: list[tuple[str, Path, int]]) -> None:
    with archive.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w", format=tarfile.PAX_FORMAT) as output:
                directory = tarfile.TarInfo(root_name + "/")
                directory.type = tarfile.DIRTYPE
                directory.mode = 0o755
                directory.mtime = directory.uid = directory.gid = 0
                directory.uname = directory.gname = ""
                output.addfile(directory)
                for name, source, mode in inputs:
                    data = source.read_bytes()
                    info = tarfile.TarInfo(f"{root_name}/{name}")
                    info.size = len(data)
                    info.mode = mode
                    info.mtime = info.uid = info.gid = 0
                    info.uname = info.gname = ""
                    import io

                    output.addfile(info, io.BytesIO(data))


def create_zip(archive: Path, root_name: str, inputs: list[tuple[str, Path, int]]) -> None:
    with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as output:
        directory = zipfile.ZipInfo(root_name + "/", date_time=(1980, 1, 1, 0, 0, 0))
        directory.external_attr = (stat.S_IFDIR | 0o755) << 16
        output.writestr(directory, b"")
        for name, source, mode in inputs:
            info = zipfile.ZipInfo(f"{root_name}/{name}", date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | mode) << 16
            output.writestr(info, source.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def command_package(args: argparse.Namespace) -> None:
    version = args.version
    verify_release(version)
    platform_name, extension = native_platform()
    run(("cargo", "test", "--workspace", "--locked"))
    run(("cargo", "build", "--workspace", "--release", "--locked"))
    smoke_test(Path("target/release") / f"combinator{extension}")

    output = safe_output_directory(args.output)
    root_name = f"tz-combinator-{version}-{platform_name}"
    suffix = ".zip" if extension else ".tar.gz"
    archive = output / f"{root_name}{suffix}"
    checksum = Path(str(archive) + ".sha256")
    if archive.exists() or checksum.exists() or is_link_or_reparse(archive) or is_link_or_reparse(checksum):
        die(f"refusing to overwrite release artifact {archive}")
    inputs = archive_inputs(extension)
    if extension:
        create_zip(archive, root_name, inputs)
    else:
        create_tar_gz(archive, root_name, inputs)
    atomic_write(checksum, f"{sha256(archive)}  {archive.name}\n".encode("ascii"))
    print(f"Created {archive} and {checksum}.")


def expected_members(version: str, platform_name: str) -> set[str]:
    root = f"tz-combinator-{version}-{platform_name}"
    extension = ".exe" if platform_name.startswith("windows") else ""
    files = {f"{root}/{name}{extension}" for name in ("combinator", "combinator-gui", "combinator-tui")}
    files.update(f"{root}/{name}" for name in PACKAGE_DOCUMENTS)
    return files


def verify_archive(archive: Path, version: str, platform_name: str) -> None:
    expected = expected_members(version, platform_name)
    if archive.name.endswith(".tar.gz"):
        with tarfile.open(archive, "r:gz") as source:
            members = source.getmembers()
            if any(member.issym() or member.islnk() or not (member.isfile() or member.isdir()) for member in members):
                die(f"{archive} contains an unsafe member type")
            actual = {member.name.rstrip("/") for member in members if member.isfile()}
    else:
        with zipfile.ZipFile(archive) as source:
            actual = {name.rstrip("/") for name in source.namelist() if not name.endswith("/")}
    if actual != expected:
        die(f"{archive} does not contain the exact release payload")


def command_verify_artifacts(args: argparse.Namespace) -> None:
    version = args.version
    parse_version(version)
    root = Path(args.artifacts)
    require_directory(root)
    patterns = {
        "linux-x86_64": f"tz-combinator-{version}-linux-x86_64.tar.gz",
        "windows-x86_64": f"tz-combinator-{version}-windows-x86_64.zip",
    }
    for platform_name, filename in patterns.items():
        archives = list(root.rglob(filename))
        if len(archives) != 1:
            die(f"expected exactly one {filename} beneath {root}")
        archive = archives[0]
        require_regular(archive)
        checksum = Path(str(archive) + ".sha256")
        require_regular(checksum)
        expected_line = f"{sha256(archive)}  {archive.name}"
        if checksum.read_text(encoding="ascii").strip() != expected_line:
            die(f"checksum validation failed for {archive}")
        verify_archive(archive, version, platform_name)
    print(f"Release artifacts for v{version} passed checksum and payload verification.")


def command_publish(args: argparse.Namespace) -> None:
    version = args.version
    verify_release(version)
    command_verify_artifacts(args)
    tag = f"v{version}"
    tag_commit = git("rev-parse", f"refs/tags/{tag}^{{commit}}").stdout.strip()
    head_commit = git("rev-parse", "HEAD").stdout.strip()
    if tag_commit != head_commit:
        die(f"tag {tag} does not point to the checked-out commit")
    artifact_root = Path(args.artifacts)
    assets: list[str] = []
    for pattern in (
        f"tz-combinator-{version}-linux-x86_64.tar.gz",
        f"tz-combinator-{version}-linux-x86_64.tar.gz.sha256",
        f"tz-combinator-{version}-windows-x86_64.zip",
        f"tz-combinator-{version}-windows-x86_64.zip.sha256",
    ):
        matches = list(artifact_root.rglob(pattern))
        if len(matches) != 1:
            die(f"expected exactly one {pattern} beneath {artifact_root}")
        assets.append(str(matches[0]))
    run(
        (
            "gh",
            "release",
            "create",
            tag,
            *assets,
            "--verify-tag",
            "--notes-file",
            f"release-notes/{version}.md",
        )
    )


def command_tag(args: argparse.Namespace) -> None:
    version = args.version
    verify_release(version)
    ensure_clean_worktree()
    created = False
    if tag_exists(version):
        tag_commit = git("rev-parse", f"refs/tags/v{version}^{{commit}}").stdout.strip()
        head_commit = git("rev-parse", "HEAD").stdout.strip()
        if not args.allow_existing or tag_commit != head_commit:
            die(f"tag v{version} already exists")
        print(f"Tag v{version} already points to the verified commit; reusing it.")
    else:
        git("tag", "-a", f"v{version}", "-m", f"Release v{version}", capture=False)
        created = True
    if args.push:
        try:
            git("push", "origin", f"v{version}", capture=False)
        except Exception:
            print(
                f"Tag v{version} was created locally but could not be pushed; inspect it before retrying.",
                file=sys.stderr,
            )
            raise
    action = "Created" if created else "Verified"
    print(f"{action}{' and pushed' if args.push else ''} annotated tag v{version}.")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    subparsers = result.add_subparsers(dest="command", required=True)

    prepare = subparsers.add_parser("prepare", help="transactionally generate release metadata")
    prepare.add_argument("version")
    prepare.add_argument("--date", help="UTC release date in YYYY-MM-DD form")
    prepare.set_defaults(handler=command_prepare)

    sync = subparsers.add_parser("sync", help="copy reviewed release notes into CHANGELOG.md")
    sync.add_argument("version")
    sync.set_defaults(handler=command_sync)

    verify = subparsers.add_parser("verify", help="verify release metadata")
    verify.add_argument("version")
    verify.set_defaults(handler=lambda args: verify_release(args.version))

    check = subparsers.add_parser("check", help="run metadata and Rust release gates")
    check.add_argument("version")
    check.add_argument("--release-build", action="store_true", help="also compile release binaries")
    check.add_argument(
        "--full",
        action="store_true",
        help="also validate packaging, installation, coverage, advisories, and dependency policy",
    )
    check.set_defaults(handler=command_check)

    package = subparsers.add_parser("package", help="test, build, and package the native target")
    package.add_argument("version")
    package.add_argument("--output", default="dist", help="artifact directory (default: dist)")
    package.set_defaults(handler=command_package)

    verify_artifacts = subparsers.add_parser("verify-artifacts", help="verify both native release archives")
    verify_artifacts.add_argument("version")
    verify_artifacts.add_argument("--artifacts", default="artifacts", help="download root (default: artifacts)")
    verify_artifacts.set_defaults(handler=command_verify_artifacts)

    publish = subparsers.add_parser("publish", help="verify artifacts and create the GitHub release")
    publish.add_argument("version")
    publish.add_argument("--artifacts", default="artifacts", help="download root (default: artifacts)")
    publish.set_defaults(handler=command_publish)

    tag = subparsers.add_parser("tag", help="create an annotated release tag after verification")
    tag.add_argument("version")
    tag.add_argument("--push", action="store_true", help="push the new tag to origin")
    tag.add_argument(
        "--allow-existing",
        action="store_true",
        help="reuse the tag only when it already points to the verified commit",
    )
    tag.set_defaults(handler=command_tag)
    return result


def main(argv: Sequence[str] | None = None) -> int:
    try:
        args = parser().parse_args(argv)
        enter_repo()
        args.handler(args)
        return 0
    except ReleaseError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
