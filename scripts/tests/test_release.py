from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).parents[1] / "release.py"
SPEC = importlib.util.spec_from_file_location("release_driver", SCRIPT)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class MetadataTests(unittest.TestCase):
    def test_version_validation_is_strict_and_orderable(self) -> None:
        self.assertEqual(release.parse_version("12.3.40"), (12, 3, 40))
        for invalid in ("v1.2.3", "1.2", "01.2.3", "1.2.3-beta", "1.2.1000000000"):
            with self.subTest(invalid=invalid), self.assertRaises(release.ReleaseError):
                release.parse_version(invalid)

    def test_manifest_update_changes_package_and_internal_dependencies(self) -> None:
        source = """[package]
name = "combinator-cli"
version = "0.2.0"

[dependencies]
combinator-core = { version = "0.2.0", path = "../combinator-core" }
external = "0.2.0"
"""
        actual = release.update_manifest(source, "0.2.0", "0.3.0", package=True)
        self.assertIn('version = "0.3.0"', actual)
        self.assertIn('combinator-core = { version = "0.3.0", path =', actual)
        self.assertIn('external = "0.2.0"', actual)

    def test_changelog_sync_is_idempotent(self) -> None:
        notes = "## [0.3.0] - 2026-08-05\n\n### Added\n\n- A useful change\n"
        old = "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-08-01\n\n- Old\n"
        once = release.synchronized_changelog("0.3.0", notes, old)
        twice = release.synchronized_changelog("0.3.0", notes, once)
        self.assertEqual(once, twice)
        self.assertLess(once.index("## [0.3.0]"), once.index("## [0.2.0]"))

    def test_artifact_output_rejects_parent_traversal(self) -> None:
        with self.assertRaises(release.ReleaseError):
            release.safe_output_directory("../dist")


class ArchiveTests(unittest.TestCase):
    def test_tar_and_zip_have_exact_expected_payloads(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            inputs = []
            for name in ("combinator", "combinator-gui", "combinator-tui"):
                source = root / name
                source.write_bytes(name.encode("ascii"))
                inputs.append((name, source, 0o755))
            for name in release.PACKAGE_DOCUMENTS:
                source = root / name
                source.write_bytes(name.encode("ascii"))
                inputs.append((name, source, 0o644))

            linux_root = "tz-combinator-0.3.0-linux-x86_64"
            linux = root / f"{linux_root}.tar.gz"
            release.create_tar_gz(linux, linux_root, inputs)
            release.verify_archive(linux, "0.3.0", "linux-x86_64")

            windows_inputs = [(name + ".exe", path, mode) if name.startswith("combinator") else (name, path, mode) for name, path, mode in inputs]
            windows_root = "tz-combinator-0.3.0-windows-x86_64"
            windows = root / f"{windows_root}.zip"
            release.create_zip(windows, windows_root, windows_inputs)
            release.verify_archive(windows, "0.3.0", "windows-x86_64")


if __name__ == "__main__":
    unittest.main()
