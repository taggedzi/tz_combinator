# Security Follow-up Remediation Plan

This plan addresses the residual findings from the post-remediation security scan. Each implementation item will be developed on a separate `security/*` branch, committed, merged into `master`, and verified before the next item begins.

## 1. Enforce non-overridable resource ceilings

The current CLI lets an untrusted caller raise safety limits or disable preflight. Add hard compiled ceilings for input bytes, item bytes/counts, list count, total items, combinations, and output bytes. User-provided values may lower the ceiling but may never raise it. Keep `--no-preflight` limited to skipping estimation; runtime output enforcement remains mandatory.

Acceptance criteria:

- Values above the hard ceiling are rejected before processing.
- `--no-preflight` cannot disable runtime limits.
- All error paths identify the requested and hard limits.
- Tests cover every configurable resource flag at, below, and above its ceiling.

## 2. Enforce aggregate input budgets during ingestion

The current global item limit is checked only after all lists are loaded, and input bytes are limited per source rather than across the invocation. Pass a mutable aggregate budget into each reader and reject before reading/storing data that would exceed remaining bytes or items.

Acceptance criteria:

- Aggregate bytes and items are bounded while reading each source.
- A later source cannot cause already-loaded data to exceed the global budget.
- Files, stdin, and inline lists use the same aggregate policy.
- Tests demonstrate rejection before the excess list is retained.

## 3. Use secure temporary output names

Temporary overwrite names are predictable and limited to 128 attempts. Replace the deterministic counter/PID name with an OS-secure random name or a cryptographically random suffix combined with exclusive creation. Keep collision handling bounded and fail cleanly.

Acceptance criteria:

- Temporary names are not predictable from path and process ID.
- Creation remains exclusive and symlink-safe.
- Collision attacks cannot exhaust an impractically small fixed name set.
- Temporary files are removed on failure.

## 4. Pin CI and build supply-chain inputs

Pin GitHub Actions to immutable commit SHAs, pin the Rust toolchain to an explicit version, and pin the `cargo-audit` installation/version used by CI. Preserve locked Cargo resolution.

Acceptance criteria:

- CI does not use mutable action tags or the moving `stable` toolchain.
- Formatting, tests, clippy, and audit run with locked dependencies.
- The exact toolchain and audit version are documented.

## 5. Make failure cleanup identity-safe

Non-overwrite output cleanup currently removes the destination pathname by name. If another process replaces that pathname after opening, cleanup can remove the replacement entry. Prefer temporary-file-only cleanup or descriptor/identity-aware cleanup that never removes an unrelated replacement.

Acceptance criteria:

- Failed overwrite runs remove only their temporary file.
- Failed new-file runs do not remove a path that no longer refers to the opened file.
- Existing files remain untouched under concurrent path replacement.
- Tests cover failure cleanup and replacement races where the platform permits.

## Verification

After each merge, run focused tests plus:

```text
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

CI must additionally run the pinned dependency audit. The final master worktree must be clean, and the branch/merge history must be recorded here.

## Implementation history

| Area | Branch | Commit |
|---|---|---|
| Follow-up plan | `security/13-followup-plan` | `13a55d9` |
| Hard resource ceilings | `security/14-hard-resource-ceilings` | `7dd6a81` |
| Aggregate ingestion budgets | `security/15-aggregate-input-budgets` | `62a75d1` |
| Secure temporary names | `security/16-secure-temp-names` | `0ffdbbf` |
| Pinned CI supply chain | `security/17-pinned-ci-supply-chain` | `9e8fbf9` |
| Identity-safe cleanup | `security/18-identity-safe-cleanup` | `5ec7fa0` |
| Follow-up documentation | `security/19-followup-documentation` | pending |

All five follow-up implementation branches were merged into local `master` in sequence before the next implementation branch was created.
