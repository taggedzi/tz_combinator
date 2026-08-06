# Compatibility policy

`combinator` is the public interface of this project. The workspace Rust
crates, especially `combinator-core`, are internal implementation components
and are not a semver-stable public Rust API.

## Rust library API status

The project intentionally retains this policy for the pre-1.0 releases:

- The `combinator` CLI is the only supported stable integration boundary.
- `combinator-core`, `combinator-codecs`, and `combinator-app` are reusable
  workspace crates, but their Rust APIs may change without a semver-stability
  promise within the 0.x release line.
- GUI and TUI Rust modules are application internals and are not supported as
  libraries.
- Examples in [library usage](library-usage.md) describe the current workspace
  APIs; they do not constitute a compatibility guarantee.

This policy is a deliberate scope decision, not an indication that the
library crates are unusable. It avoids freezing a broad API before an external
Rust consumer, the required surface, and the compatibility costs are known.

Revisit stabilization when all of the following are true:

1. At least one concrete external Rust consumer needs an in-process API.
2. The smallest required request, execution, sink, and error surface is
   understood and covered by compatibility tests.
3. Migration guidance, semver expectations, and release/versioning policy have
   been reviewed and documented.

A future 1.0 release will not automatically stabilize every currently public
   item. Any supported Rust API must be explicitly identified and documented.

Within a major release:

- Existing flags, defaults, exit codes, and error-code meanings are preserved.
- Exit code `0` means success, `1` means a runtime failure, and `2` means a
  usage/input failure. `stdout` contains records only; diagnostics belong on
  `stderr`.
- Existing JSONL fields retain their meaning. Compatible fields may be added
  only additively, and consumers should ignore unknown fields.
- `--explain --format json` is governed by its integer `schema_version`.
  Incompatible shape or meaning changes require a new schema version.
- Product ordering and sharding algorithm version 1 are deterministic for
  stable inputs. A future incompatible algorithm must be explicitly versioned.
- Operational logging is disabled by default, so existing stdout, stderr,
  files, and exit statuses remain byte-identical when logging is not enabled.
  Explicit logging is an opt-in stderr behavior with a non-stable phase-event
  schema. JSON logging for JSON/JSONL data invocations intentionally changes
  stderr to the documented JSON Lines event stream.
- Raw text, CSV, TSV, and NUL bytes written to files or pipes remain
  byte-identical. Interactive terminals additionally enforce the documented
  control-character policy; trusted callers can restore intentional raw
  terminal behavior with `--allow-unsafe-terminal-output`.

## Distribution roadmap

The supported distribution surface is deliberately narrow:

- **Supported now:** GitHub release archives for Linux x86_64 and Windows
  x86_64, with SHA-256 checksums and provenance metadata. The cross-platform
  release driver verifies metadata and archive contents; the single Release
  workflow supplies the native runners, pinned toolchain, locked dependencies,
  and GitHub OIDC provenance.
- **Not supported:** macOS binaries, crates.io publication, Homebrew, Scoop,
  winget, and native Linux package repositories. No support promise should be
  inferred for these targets.
- **Planned evaluation:** consider macOS and package-manager distribution only
  after the current release process has operated through a stable release
  cycle and a maintainer owns each target. Any proposal must include pinned
  builds, signing/provenance, install and upgrade behavior, and target-specific
  smoke tests before the target is advertised.

The release driver, Release workflow, and this policy intentionally agree: a
target is not supported merely because Rust can compile it. Expansion requires
operational ownership, reproducible artifacts, verification coverage, and a
documented support policy. See the [current release procedure](release.md).
