# Release procedure

The supported binary release platforms are Linux x86_64 and Windows x86_64.
Release preparation is a separate, human-reviewed step before tagging. It
generates the changelog from Git history, synchronizes all workspace versions,
and opens a pull request. The tag-triggered release workflow then builds the
CLI, GUI, and TUI with the pinned Rust toolchain and `--locked`, packages them
with the license and README, generates and verifies SHA-256 checksums, verifies
the reviewed changelog, and creates a GitHub release.

Only stable semantic versions such as `0.2.0` are currently supported by the
preparation tooling. Version inputs never include the `v` tag prefix.

## Changelog inputs

User-visible commits should use these Conventional Commit types:

- `feat:` for additions;
- `fix:` for fixes;
- `security:` for security changes;
- `perf:` and `revert:` for other user-visible changes; and
- `!`, as in `feat!:`, for a breaking change.

The generator excludes documentation, tests, CI, build work, formatting,
refactoring, chores, merge commits, and unmatched commit subjects. This keeps
implementation-only work out of release notes. A maintainer can add an omitted
user-visible change while reviewing the generated fragment.

Preparation fails rather than creating an empty release section when no
user-visible commit is found.

`cliff.toml` defines the categories and rendering rules. The preparation
workflow pins `git-cliff` 2.12.0 and invokes it with external command execution
disabled. It writes the initial reviewed source to
`release-notes/<version>.md`, then synchronizes that section into
`CHANGELOG.md`. Released fragments remain in the repository so the tag
workflow can verify the packaged changelog exactly.

Do not add text directly beneath `## [Unreleased]`; deterministic preparation
requires that section to remain empty.

## Automated preparation

From the repository's **Actions** page, run **Prepare release** against the
default branch:

1. Enter the next version without `v`, for example `0.2.0`.
2. Optionally enter the intended UTC release date as `YYYY-MM-DD`. When
   omitted, the workflow uses the current UTC date.
3. Wait for changelog generation and the full test, formatting, and clippy
   suite to pass.
4. Open the release-preparation pull request created by the workflow.

The workflow refuses an existing version tag or release branch, validates all
inputs before using them in Git or shell operations, and permits generated
changes only in `CHANGELOG.md`, `Cargo.lock`, workspace manifests, and the new
release-note fragment. It does not create a tag or publish a release.

The repository must allow GitHub Actions to create pull requests. If repository
policy blocks that final operation, the validated `release/v<version>` branch
still exists; open a pull request from it manually.

Review the complete diff. In particular, verify:

- the version bump is appropriate;
- every workspace package and internal path dependency has the same version;
- the generated categories contain user-visible changes only; and
- compatibility, security, and breaking changes are described clearly.

To improve generated prose, edit `release-notes/<version>.md`, not the compiled
changelog section, and run:

```text
scripts/sync-release-notes.sh 0.2.0
scripts/verify-release.sh 0.2.0
```

Commit both the fragment and updated `CHANGELOG.md` to the preparation branch.
The verifier checks the reviewed fragment, changelog section, six manifests,
internal dependency requirements, and six workspace entries in `Cargo.lock`.

## Local preparation

The same preparation can be run locally from a clean default-branch worktree
on Linux or WSL. The scripts use Bash and GNU core utilities:

```text
cargo install git-cliff --version 2.12.0 --locked
scripts/prepare-release.sh 0.2.0 2026-08-01
```

The date is optional and defaults to the current UTC date. Review the changes,
edit and synchronize the release-note fragment if necessary, run the local
verification below, and commit the preparation on a dedicated branch.

## Local verification

Run the release metadata verifier and the same gates used by CI before
tagging:

```text
scripts/verify-release.sh 0.2.0
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --release --locked
cargo audit
cargo deny check
```

Run CLI smoke checks against the release binary on the target platform:

```text
combinator --version
combinator --help
combinator --list "a,b" --list "1,2" --sep -
combinator --list "a,b" --list "1,2" --format jsonl
```

The fuzz smoke job runs each target in `fuzz/` for a bounded number of
executions. Longer fuzz campaigns should be run separately before a major
release.

## Tagging

Merge the reviewed preparation PR first. Update the local default branch and
verify that the merged commit is the exact commit to tag. Create the annotated
tag only after the complete verification matrix passes:

```text
git switch master
git pull --ff-only
git status --short --branch
scripts/verify-release.sh 0.2.0
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin v0.2.0
```

Substitute the prepared version in both tag commands. The release workflow
checks that the tag version, workspace versions, lockfile, reviewed release
fragment, and packaged changelog agree. A mismatch fails before publication.

The release workflow can also be started manually with an existing tag. It does
not publish crates.io packages. Verify the generated `.sha256` files before
distributing archives.

## Filesystem safety

Output writers require an existing parent directory and reject destination or
ancestor symlinks/reparse points and `..` traversal. Output and profile writes
are staged in secure sibling temporary files and committed atomically. The
application still assumes the selected parent directory is not concurrently
replaced by a privileged filesystem attacker; wrappers handling hostile
multi-user paths should constrain destinations to an application-owned
directory.
