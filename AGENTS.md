# Project Agent Instructions

## Mission

Work on `tz_combinator` as a security-sensitive Rust CLI/library intended to run in hostile or careless environments. Preserve correctness, predictable resource use, stable CLI behavior, and safe filesystem semantics.

## Token and context efficiency

- Lead with the requested outcome; keep progress updates short and useful.
- Inspect narrowly first: use `rg --files`, targeted `rg -n`, and focused file reads. Do not dump the whole repository unless necessary.
- Prefer parallel read-only checks when they are independent and their output is bounded. On Windows, run shell checks sequentially by default; concurrent PowerShell launches can intermittently fail with sandbox logon-session errors.
- Do not reread files or repeat tests whose results are already known; retain concise evidence summaries.
- Load only the documentation, code, and tests relevant to the current task.
- Use the smallest verification command that meaningfully covers the change, then broaden verification when risk warrants it.
- Avoid speculative refactors, broad formatting, generated artifacts, and unrelated cleanup.
- Before finishing, report changed files, verification performed, and any remaining uncertainty in a compact form.

### Repository inspection safeguards

- Let `rg` honor `.gitignore` for normal searches. Never use `rg -uu` or `rg -uuu` across the repository root as a first pass.
- Treat `target/`, `fuzz/target/`, `.git/`, `.superpowers/`, generated reports, and binary assets as excluded unless the task explicitly targets one of them. If an ignored path must be inspected, name that path directly and narrow the file pattern.
- Bound search and listing output with a focused path/pattern, `rg -m`, `Select-Object -First`, or an equivalent limit. Do not recursively concatenate or print entire files when a signature, count, or tail is sufficient.
- Do not send large generated stdout back through the tool interface. For commands that intentionally produce many records, request a count/summary, select a small sample, or write to a task-scoped temporary file and inspect only bounded excerpts.
- If a Windows command reports `CreateProcessAsUserW` error 1312, retry once with a small sequential command. If it recurs, treat it as an execution-environment/session fault and report it; changing Rust code or widening command output will not fix it.

## Model and reasoning selection

Use capability-based model selection when the execution environment supports it:

- Use a higher-capability model for threat modeling, architecture, security analysis, complex debugging, API/behavior changes, ambiguous requirements, and implementation planning.
- Use a lower-cost model for mechanical searches, straightforward file inventory, repetitive test execution, formatting, simple transformations, and narrowly specified edits.
- Escalate from a lower-cost model when it encounters ambiguity, conflicting requirements, security-sensitive judgment, failing tests it cannot explain, or changes spanning multiple trust boundaries.
- Keep planning and implementation reasoning separate: establish scope and invariants first, then delegate or perform mechanical work.

Model selection is advisory. Do not claim that a particular model was used unless the host explicitly exposes that information.

## Memory and context reuse

- When `codememory_mcp` or an equivalent project-memory tool is available, use it to retrieve relevant prior decisions, invariants, test commands, and known hazards before re-deriving them.
- Store only durable, project-specific facts: architectural decisions, security assumptions, compatibility constraints, and validated commands.
- Do not store secrets, credentials, personal data, transient logs, or unverified speculation.
- If memory is unavailable, continue using repository evidence and do not block on it.
- Treat repository files and current user instructions as authoritative over stale memory.

## Security rules

- Assume inputs, filenames, output paths, environment variables, and filesystem state may be attacker-controlled unless the task explicitly establishes trust.
- Treat preflight checks as advisory unless the operation is enforced atomically at the point of use.
- For output files, consider symlink/reparse-point attacks, TOCTOU races, partial writes, atomic replacement, and permission inheritance.
- Bound input bytes, item counts, item lengths, combinations, output bytes, recursion, and other resources before processing untrusted data.
- Never introduce shell execution, network access, dynamic loading, or unsafe Rust without explicit justification and review.
- Do not log secrets or raw hostile strings without appropriate escaping. Preserve structured JSON diagnostics as valid JSON.
- Preserve checked arithmetic and fail-closed behavior for overflow and resource-limit failures.
- Do not disable security checks merely to make tests or examples pass.

## Change workflow

1. Establish scope, trust boundaries, compatibility requirements, and acceptance criteria.
2. Inspect existing code/tests and worktree state before editing.
3. Make the smallest coherent change using `apply_patch` for manual edits.
4. Add regression tests for security fixes, especially hostile inputs and filesystem races where practical.
5. Run focused tests first, then the relevant workspace tests and static checks.
6. Check whether the change requires updates to user-facing or developer documentation; update relevant documentation when needed, or record why no update is necessary.
7. Review the diff for accidental scope expansion, secret exposure, unsafe defaults, and missing error handling.

## Commit conventions

- Use [Conventional Commits](https://www.conventionalcommits.org/) for commit messages, such as `feat:`, `fix:`, `docs:`, or `refactor:`.

## Rust project conventions

- Maintain Rust 2021 compatibility and the declared minimum Rust version unless the user approves changing it.
- Keep library logic testable independently from CLI/process behavior.
- Preserve stable exit codes and machine-readable error codes unless a requested change requires a documented addition.
- Prefer explicit, checked conversions and arithmetic.
- For binary releases, use a committed `Cargo.lock` and locked builds.
- Do not commit `target/`, temporary files, test outputs, credentials, or local environment configuration.

## Verification expectations

For normal Rust changes, use the narrowest applicable commands, typically:

```text
cargo test -p <affected-package> --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If a command is unavailable, blocked, or too expensive, say so explicitly and run the best available substitute. For security-sensitive filesystem changes, include targeted integration tests rather than relying only on unit tests.

## User authorization and safety

- Read-only review requests authorize inspection and verification, not implementation.
- Change requests authorize implementation only within the stated scope.
- Do not delete, reset, overwrite, publish, push, or modify external systems without clear authorization.
- Preserve unrelated user changes and stop if the requested change conflicts with them.
