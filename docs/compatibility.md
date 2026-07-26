# Compatibility policy

`combinator` is the public interface of this project. The workspace Rust
crates, especially `combinator-core`, are internal implementation components
and are not a semver-stable public Rust API.

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

Supported release artifacts are Linux x86_64 and Windows x86_64 GitHub
archives. macOS releases and crates.io publication are not promised by this
policy.
