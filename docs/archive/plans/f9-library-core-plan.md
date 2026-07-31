# F9 Library-Core Refactoring Plan

> Archived planning record. This document preserves historical design context
> and is not current user guidance.

## Objective

Refactor `tz_combinator` so that its combination semantics and bounded data
processing live in a reusable Rust library core, while the CLI remains the
compatibility adapter for command-line parsing, filesystem policy, terminal
behavior, diagnostics, and exit codes.

This implements the useful part of F9 without prematurely committing to a
fully stable third-party Rust API. A stable external API can be defined later
around the subset needed by real Rust consumers.

## Target architecture

```text
Python TUI/GUI -- subprocess --> combinator-cli --\
Rust REST service --------------> combinator-core
Rust application ---------------> combinator-core
CLI ----------------------------> combinator-core
```

### `combinator-core` owns

- Validated, CLI-independent request and configuration types.
- Product, zip, concat, join, and future operation engines.
- Bounded input parsing through generic readers.
- Normalization and transformation behavior.
- Templates and output formatting.
- Counts, estimates, ordering, paging, and sharding semantics.
- Resource-limit enforcement during processing.
- Streaming execution through an abstract output sink.
- Cancellation checks and typed core errors.

### `combinator-cli` owns

- `clap` parsing and legacy invocation compatibility.
- Mapping core errors to stable exit codes and diagnostic formats.
- Opening input files and selecting stdin.
- Safe output-path preflight and atomic file replacement.
- stdout/stderr and terminal behavior.
- Warnings, progress, completions, man pages, and help text.

The core must enforce security and resource limits independently of the CLI.
No frontend should be trusted to perform those checks correctly on its own.

## Non-goals

- Do not create a second implementation of any operation for library callers.
- Do not change existing CLI output, diagnostics, exit codes, or filesystem
  safety behavior unless explicitly required by a feature.
- Do not promise semver stability for every currently public item in
  `combinator-core`.
- Do not move terminal-specific behavior or path-policy decisions into the
  core merely to make the split look symmetrical.
- Do not add a REST service, Python binding, or other frontend as part of this
  refactor.

## Implementation stages

### 1. Freeze the compatibility baseline

Before moving behavior:

- Inventory current responsibilities in `combinator-core` and
  `combinator-cli`.
- Record invariants for legacy product invocation, output bytes, ordering,
  diagnostics, exit codes, limits, and safe file replacement.
- Run focused existing tests and identify missing black-box coverage.
- Add regression tests where moving a boundary could otherwise hide behavior
  changes.

### 2. Establish the core boundary

Introduce or consolidate CLI-independent types for:

- Requests and operation configuration.
- Input formats and bounded input limits.
- Output formats and output limits.
- Transformations and templates.
- Core errors and structured error context.
- Cancellation and execution results.

These types must not depend on `clap`, process exit codes, terminal output, or
CLI-specific path handling.

The existing `Operation`, operation options, counting, estimates, and template
types are the starting point for this boundary.

### 3. Move pure processing behavior

Move or adapt the behavior that has no operating-system policy:

- Normalization and transformations.
- Input parsing logic, exposed through generic `Read`-based functions.
- Template validation and rendering.
- Text, JSONL, CSV, TSV, and NUL formatting.
- Count and size-estimate helpers.
- Shared checked arithmetic and resource-budget tracking.

The CLI may continue to open files and provide readers. The core must perform
the actual bounded parsing and reject malformed or oversized data.

### 4. Add one streaming execution path

Create a core executor that receives a validated request and writes records to
an abstract sink. The sink must propagate write failures and support prompt
cancellation.

The executor owns operation dispatch, ordering, transformations, formatting,
counting, estimates, combination limits, output-byte limits, and cancellation.

It must not write directly to stdout, stderr, or a named path.

### 5. Refactor the CLI into an adapter

Change the CLI flow to:

1. Parse `clap` arguments.
2. Preserve legacy argument interpretation.
3. Convert arguments into a core request.
4. Open or prepare filesystem resources using existing safe policies.
5. Invoke the core executor.
6. Map core results and errors to existing CLI output and exit behavior.

Remove duplicated operation, parser, transformation, formatter, and limit
logic from the CLI after parity tests cover the replacement path.

### 6. Add parity and security coverage

Add tests that compare direct core execution with CLI execution for:

- Product, zip, concat, templates, formats, paging, and sharding.
- Empty inputs, unequal lengths, duplicates, malformed records, and overflow.
- Input, item, combination, output, and aggregate resource limits.
- Cancellation and downstream write failures.
- Stable output ordering and exact serialized bytes.

Retain CLI-specific tests for:

- Exit codes and diagnostic rendering.
- stdin and path handling.
- Symlink/reparse-point and TOCTOU-sensitive output behavior.
- Atomic replacement and prevention of partial destination files.

### 7. Verify and document

Run, as applicable:

```text
cargo test -p combinator-core --locked
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Review the final diff for accidental public API expansion, duplicated
security checks, unchecked arithmetic, partial writes, and changed diagnostic
contracts.

Document the internal core boundary and state that external API stability is
deferred until a concrete Rust consumer justifies selecting and freezing a
smaller supported surface.

## Completion criteria

The refactor is complete when:

- The CLI and direct core execution use the same operation implementation.
- Existing compatibility tests pass unchanged or have an explicitly reviewed
  reason for any update.
- Core limits and hostile-input handling do not depend on frontend behavior.
- Core execution is streamable, cancellable, and writer-error aware.
- CLI-only filesystem and diagnostic responsibilities remain intact.
- No stable third-party API promise has been made accidentally.
- Focused tests, workspace tests, formatting, and clippy pass.

## Future API stabilization trigger

Consider a supported public Rust API only when at least one concrete consumer
exists, such as a Rust REST service or application, and its required surface
is understood. Stabilize the smallest useful request, execution, sink, and
error API rather than exposing all internal modules.
