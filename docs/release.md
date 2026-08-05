# Release procedure

The release process has one cross-platform entry point:

```text
python scripts/release.py --help
```

The same driver runs on Windows, Linux, and GitHub Actions. It owns version
synchronization, changelog generation and verification, release gates, native
binary construction, normalized archives, checksums, tag creation, artifact
verification, and GitHub release creation. It uses only the Python standard
library.

GitHub Actions remains the execution environment for two capabilities that a
single local machine cannot supply safely: native Windows and Linux builds, and
GitHub OIDC build provenance. The single `Release` workflow is a thin adapter
around the driver for those capabilities. No WSL or PowerShell release script
is required.

The supported binary release platforms are Linux x86_64 and Windows x86_64.
macOS and package-manager publication remain roadmap candidates; see the
[distribution roadmap](compatibility.md#distribution-roadmap).

## Recommended: operate entirely from GitHub

Use the repository's **Actions → Release** workflow for both phases:

1. Run it with `operation=prepare`, the stable version without `v` (for
   example `0.3.0`), and optionally a UTC date in `YYYY-MM-DD` form.
2. Review and merge the generated `release/v<version>` pull request. Edit
   `release-notes/<version>.md` in that PR if the generated prose needs work.
3. Run the same workflow with `operation=publish` and the same version.

The publish operation verifies the prepared metadata on the current default
branch before creating or safely reusing the annotated tag. It then runs the complete release
gates, builds and smoke-tests both native targets, creates normalized
archives and SHA-256 files, verifies the downloaded payloads, generates GitHub
build provenance, and creates the GitHub release from the reviewed notes.

Preparation and publication are deliberately separate operations. That review
boundary prevents a generated changelog or accidental version bump from being
published without human approval, while keeping every mechanical step in one
workflow and one driver.

## Local operation

Local preparation requires Python 3.11 or newer, Git, Rust, and the pinned
changelog generator:

```text
cargo install git-cliff --version 2.12.0 --locked
python scripts/release.py prepare 0.3.0 --date 2026-08-05
```

`prepare` requires a clean worktree and is transactional: if any write or final
metadata verification fails, it restores every pre-existing file and removes
the new release-note fragment. Review the resulting diff before committing it.

If release notes are edited during review, resynchronize and verify them with:

```text
python scripts/release.py sync 0.3.0
python scripts/release.py check 0.3.0
```

After the reviewed preparation commit is on the default branch, create and
push the release tag from a clean worktree:

```text
python scripts/release.py tag 0.3.0 --push
```

Pushing the tag invokes the same `Release` workflow used by the GitHub publish
operation. If the push fails, the driver leaves the annotated tag locally and
reports that state; inspect it before retrying.

For an optional native-only rehearsal on the current machine:

```text
python scripts/release.py check 0.3.0 --release-build
python scripts/release.py package 0.3.0 --output dist
```

`package` supports x86_64 Windows and Linux, runs workspace tests, builds all
release binaries with `--locked`, smoke-tests the CLI, and creates one native
archive plus its checksum. It refuses to overwrite an existing artifact and
restricts output to non-link directories inside the worktree.

## What the driver verifies

The driver fails closed unless all six product crates, their internal path
dependencies, the benchmark harness dependencies, and all six `Cargo.lock`
entries agree with the requested stable version. It also requires the reviewed
release-note fragment to match the corresponding `CHANGELOG.md` section
exactly.

`check --full`, used by the publish workflow, additionally runs:

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo package --workspace --locked --no-verify
cargo install --path crates/combinator-cli --locked
cargo llvm-cov --workspace --all-features --exclude combinator-gui --exclude combinator-tui --summary-only --fail-under-lines 80
cargo audit
cargo deny --all-features check
```

The native packaging jobs independently test and build on their target runners.
The publication job then checks each SHA-256 file and requires the exact
expected archive payload before provenance or release creation.

## Changelog policy

User-visible commits should use these Conventional Commit types:

- `feat:` for additions;
- `fix:` for fixes;
- `security:` for security changes;
- `perf:` and `revert:` for other user-visible changes; and
- `!`, as in `feat!:`, for a breaking change.

Documentation, tests, CI, build work, formatting, refactoring, chores, merge
commits, and unmatched subjects are excluded. Preparation fails rather than
creating an empty release section. `cliff.toml` defines the rendering rules,
and `git-cliff` 2.12.0 is invoked with external command execution disabled.

Released fragments remain in `release-notes/` as the reviewed source for both
the compiled changelog and GitHub release body. Do not add content directly
beneath `## [Unreleased]`; deterministic preparation requires it to remain
empty.
