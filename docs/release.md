# Release procedure

The supported binary release platforms are Linux x86_64 and Windows x86_64.
The release workflow builds the CLI, GUI, and TUI with the pinned Rust
toolchain and `--locked`, packages them with the license and README, generates
SHA-256 checksums, verifies those checksums, and creates a GitHub release.

## Local verification

Run the same gates used by CI before tagging:

```text
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

Create a version tag only after the complete verification matrix passes:

```text
git status --short --branch
git tag -a v0.1.0 -m "Release v0.1.0"
git push origin v0.1.0
```

The release workflow can also be started manually with an existing tag. It
does not publish crates.io packages. Verify the generated `.sha256` files
before distributing archives.

## Filesystem safety

Output writers require an existing parent directory and reject destination or
ancestor symlinks/reparse points and `..` traversal. Output and profile writes
are staged in secure sibling temporary files and committed atomically. The
application still assumes the selected parent directory is not concurrently
replaced by a privileged filesystem attacker; wrappers handling hostile
multi-user paths should constrain destinations to an application-owned
directory.
