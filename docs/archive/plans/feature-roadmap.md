# tz_combinator Feature Roadmap

> Archived planning record. This document preserves historical product
> direction and is not the current feature reference.

## Status snapshot (2026-07-31)

This roadmap is retained for historical context. The implementation review
below records what was delivered after the roadmap was written; it is not a
new commitment to complete every original proposal.

| Feature | Current status | Notes |
|---|---|---|
| F1 — operation modes | Implemented | Product, zip, concat, and keyed join are available. |
| F2 — input/output formats | Implemented | Bounded line, CSV, TSV, NUL, JSONL, and text/structured output paths are supported. |
| F3 — templates and field-aware output | Implemented | Templates and structured output are implemented with bounded expansion. |
| F4 — sharding and resumable work | Implemented | Deterministic bounded sharding and paging are available. |
| F5 — dry-run and explain | Implemented | Human and versioned JSON summaries are available. |
| F6 — pipeline ergonomics | Implemented in scoped form | Diagnostics, timeout/cancellation, broken-pipe handling, completions, and man-page generation are implemented; progress remains intentionally conservative. |
| F7 — transformations | Implemented | The bounded transform pipeline is available. |
| F8 — keyed relational joins | Implemented in scoped form | Bounded CSV/TSV/JSONL joins support inner, left, full, and anti semantics; larger-input sort/merge joins remain future work. |
| F9 — Rust library APIs | Implemented as internal workspace APIs | The crates are reusable, but the Rust API is explicitly pre-1.0 and not semver-stable; see [compatibility](../../compatibility.md). |
| F10 — distribution | Partially implemented | Linux x86_64 and Windows x86_64 archives, checksums, provenance, completions, and manuals are in scope; macOS and package-manager publication are deferred. |

Remaining product work is limited to explicitly scoped follow-ups such as
lower-memory join strategies, broader distribution targets, and any future
decision to stabilize a Rust API. Those are separate decisions, not implied
by the unchecked design steps below. The supported CLI contract, resource
limits, deterministic behavior, and safe output policy are current product
requirements.

This document converts the product recommendations for `tz_combinator` into an
implementation roadmap. It is a historical planning document, not the current
feature reference. For implemented behavior, use the
[README](../../../README.md), [CLI usage](../../cli-usage.md), and
[library usage](../../library-usage.md) documents.

The current product has grown beyond the original Cartesian-product baseline:
it now includes multiple list operations, structured joins, reusable
application workflows, a desktop GUI, and a terminal UI. The compatibility
baseline remains bounded deterministic processing with stable diagnostics,
explicit resource limits, and safe file-output behavior.

## Product direction

`tz_combinator` should become a small, dependable data-combination toolkit for
shell scripts, administrators, build systems, test generators, and programs.
It should support several precise meanings of “join” without making the
existing Cartesian product ambiguous or breaking existing invocations.

The central design principles are:

- Existing invocations keep their behavior and exit-code contract.
- New operations are explicit; the default operation remains the current
  Cartesian product.
- Every operation is bounded by input, item, output, CPU, and memory limits.
- Streaming, deterministic ordering, and machine-readable diagnostics remain
  first-class properties.
- Features that need different semantics are implemented as subcommands or
  clearly named modes rather than overloaded flags.
- The CLI remains language-agnostic while the Rust library offers an efficient
  in-process path for Rust consumers.

## Feature catalog

| ID | Feature | Primary users | Priority |
|---|---|---|---|
| F1 | Explicit operation modes: product, zip, concat, join | Everyone; removes semantic ambiguity | P0 |
| F2 | Robust input and output formats: CSV, TSV, NUL, escaped inline data | Admins and data pipelines | P0 — implemented |
| F3 | Templates and field-aware output | Automation, URLs, configs, test generation | P0 — implemented |
| F4 | Deterministic sharding and resumable work | Batch and distributed jobs | P1 — implemented |
| F5 | Dry-run, explain, and operational summaries | Admins and deployment systems | P1 |
| F6 | Pipeline ergonomics and process integration | Shell and subprocess callers | P1 |
| F7 | Normalization and transformation operations | Data cleanup and automation | P1 |
| F8 | Keyed relational joins | Data engineers and administrators | P1 |
| F9 | Public in-process Rust API | Rust applications and services | P1 |
| F10 | Distribution, completions, and release packaging | All users; adoption multiplier | P0/P1 |

F1–F3 establish the product’s semantics and should be implemented before the
larger transformation and relational features. F10 can proceed in parallel
once the CLI contract is stable.

## Compatibility and contract policy

Before implementing new behavior, record the following contracts in tests and
the README:

- Existing no-subcommand invocations mean `product`.
- Existing flags retain their meaning and defaults.
- Existing stdout data remains data-only.
- Diagnostics remain on stderr and retain stable error codes.
- New errors receive new codes; existing codes do not change meaning.
- Every output mode has a documented ordering rule.
- Every operation defines behavior for empty inputs, unequal lengths,
  duplicates, malformed records, and limits.
- Resource limits apply consistently across all operations and formats.

The implementation should add a shared validated request model beneath the
CLI. Argument parsing should produce this model; operation engines should not
depend directly on `clap` types.

## F1 — Explicit operation modes

### User-facing feature

Add these operations:

```text
combinator product ...   # current Cartesian product
combinator zip ...       # positional pairing
combinator concat ...    # sequential concatenation
combinator join ...      # keyed relational join
```

For backward compatibility, `combinator ...` continues to invoke `product`.
The first release should implement `product`, `zip`, and `concat`; keyed
`join` is specified separately in F8 because it has substantially different
input and matching semantics.

### Semantics

- `product`: existing ordered Cartesian product.
- `zip`: item 0 from each list, then item 1, and so on. Require an explicit
  unequal-length policy: `error`, `truncate`, or `cycle`.
- `concat`: emit every item from list 1, then list 2, preserving order.
- All modes support the common output, format, paging, and limit framework.

### Implementation plan

1. Create an operation-neutral request/configuration type in `combinator-core`.
2. Move the existing product options behind a `ProductEngine` or equivalent
   operation interface without changing behavior.
3. Implement lazy `Zip` and `Concat` iterators with checked counters.
4. Add CLI subcommand parsing while preserving legacy top-level parsing.
5. Define mode-specific count and size-estimate functions.
6. Add black-box tests for order, empty lists, unequal lengths, offset/limit,
   JSONL, output limits, and invalid combinations of flags.
7. Document examples and migration guidance.

### Security and acceptance criteria

- No mode materializes generated output.
- `zip` cannot silently discard data unless the user selects `truncate`.
- Counts and estimates use checked arithmetic.
- Existing product tests pass unchanged.
- Each mode has deterministic output for identical inputs.

## F2 — Robust input and output formats

### User-facing feature

Support real-world list data without forcing users to pre-process it:

- CSV and TSV records with quoting and escaped delimiters.
- NUL-delimited input and output for arbitrary filenames and shell-safe data.
- Explicit inline escaping or a structured inline format.
- Optional trimming, empty-item handling, comments, headers, and column
  selection.
- Mixed explicit sources, such as inline values, files, and one stdin source.

Suggested interfaces:

```text
--input-format lines|csv|tsv|nul|inline
--record-input-delim '\0'
--skip-empty --trim --comment-prefix '#'
--header --column name
--allow-mixed-inputs
```

The exact flag names should be finalized after a CLI design pass. Avoid using
`--format` for both input and output; use separate `--input-format` and
`--output-format` names while retaining `--format` as an output alias.

### Implementation plan

1. Define a streaming `ListReader` trait with bounded record production.
2. Keep the current line reader as the reference implementation.
3. Add a CSV/TSV parser with bounded field, record, and nesting behavior.
4. Add byte-oriented NUL reading without assuming text record boundaries.
5. Add a safe escaped-inline parser; reject malformed escapes with a coded error.
6. Add preprocessing options in a fixed, documented order.
7. Permit mixed sources only with explicit syntax and reject duplicate stdin
   consumers.
8. Add format-specific output writers for text, JSONL, CSV, TSV, and NUL.
9. Add adversarial tests for quotes, embedded delimiters, NUL bytes, invalid
   UTF-8 policy, oversized records, and malformed CSV.

### Security and acceptance criteria

- Input byte and item budgets are enforced while parsing, not afterward.
- Parser state cannot grow with unbounded quotes, fields, or records.
- Text mode has a documented UTF-8 policy; binary-safe NUL mode does not
  accidentally pass arbitrary bytes through JSON serialization.
- Output delimiters and quoting produce round-trippable records where promised.
- A malformed source fails before partial file replacement.

## F3 — Templates and field-aware output

### User-facing feature

Allow users to control the rendered combination without shell-specific glue:

```text
--template '{0}@{1}:{2}'
--template-file format.tmpl
```

JSONL should optionally expose named fields:

```json
{"host":"server1","port":"443","value":"server1:443"}
```

Possible additions include `--prefix`, `--suffix`, and per-field separators,
but the template engine should be the underlying primitive.

### Implementation plan

1. Define a small non-Turing-complete template grammar: positional fields,
   named fields, literal text, and a bounded set of escaping functions.
2. Parse and validate templates before reading inputs.
3. Implement a reusable formatter in `combinator-core`.
4. Add named-list syntax, such as `--name host --file hosts.txt`, or a
   structured request file; reject duplicate names.
5. Share rendering logic between text, JSONL, CSV, and future formats.
6. Extend size estimation to account for template literals and escaping.
7. Add tests for missing fields, unused fields, malformed syntax, escaping,
   Unicode, output limits, and exact JSON structure.

### Security and acceptance criteria

- Templates cannot execute commands, access files, or evaluate arbitrary code.
- Template length, expansion size, and recursion are bounded; preferably there
  is no recursion at all.
- Output limits are enforced after all template expansion and serialization.
- Template failures are usage errors before output begins.

## F4 — Deterministic sharding and resumable work

### User-facing feature

Add deterministic work partitioning:

```text
--shard-index 3 --shard-count 16
```

Also provide a machine-readable plan mode that reports the effective offset,
limit, count, and ordering for a shard. Existing `--offset` and `--limit`
remain supported.

### Implementation plan

1. Define shard validation: count must be positive and index must be less than
   count.
2. Use balanced contiguous half-open ranges in the selected output ordering;
   this is algorithm version 1.
3. Compute shard ranges with checked `u128` arithmetic.
4. Intersect the computed range with the existing global offset/limit page.
5. Apply the effective page to product, zip, and concat, including reverse
   output.
6. Report the shard range and effective page in `--explain --format json`.
7. Test exact coverage: no duplicates, no gaps, stable union, and invalid
   parameter handling.

### Security and acceptance criteria

- Sharding never causes an implicit increase to combination or output limits.
- Invalid shard parameters fail before input generation.
- A shard can be rerun and produce byte-identical output for stable inputs.
- The documented algorithm is versioned if future changes could alter ranges.

## F5 — Dry-run, explain, and operational summaries

### User-facing feature

Add modes that inspect a request without generating records:

```text
--dry-run
--explain --format json
```

Report input counts, combination count, selected shard/page, estimated output
bytes, effective limits, output destination, and warnings. Keep generated data
off stdout in explain mode unless explicitly requested.

### Implementation plan

1. Define a stable summary schema and a version field.
2. Reuse count and estimator functions rather than duplicating calculations.
3. Add source metadata without exposing sensitive input values by default.
4. Add plain-text and JSON summary renderers.
5. Define behavior for unknown counts, overflow, stdin, and skipped preflight.
6. Add tests that assert summaries are parseable and consistent with actual
   bounded runs.

### Security and acceptance criteria

- Summaries do not echo hostile values or secrets by default.
- Reading stdin in dry-run mode is explicit and bounded.
- Explain mode cannot bypass validation or resource ceilings.
- JSON summaries are versioned and machine-readable.

## F6 — Pipeline ergonomics and process integration

### User-facing feature

Add options and behavior expected from dependable Unix/Windows pipeline tools:

- `--quiet` for non-fatal warnings.
- `--warnings-as-errors` for strict automation.
- Optional summary/progress reporting on stderr.
- Graceful broken-pipe handling.
- Shell completions and man pages.
- Clear subprocess examples for Rust, Python, PowerShell, and shell.

### Implementation plan

1. Define warning policy and precedence between `--quiet` and
   `--warnings-as-errors`.
2. Centralize warning collection so warnings are not emitted before a later
   usage error unless that behavior is intentionally documented.
3. Detect broken pipes and terminate without noisy secondary diagnostics.
4. Add opt-in progress reporting that never contaminates stdout.
5. Generate completions and man pages from the CLI definition.
6. Add subprocess integration tests for stdout/stderr separation, signals,
   early consumer termination, and exit codes.

### Security and acceptance criteria

- Progress and diagnostics remain one-line, escaped, and stderr-only.
- Quiet mode does not suppress fatal errors.
- Broken-pipe handling does not hide unrelated write failures.
- Progress reporting is disabled or rate-limited by default for automation.

## F7 — Normalization and transformation operations

### User-facing feature

Provide bounded, explicit preprocessing operations:

- deduplicate each list
- sort or preserve input order
- case normalization
- regex or glob filtering
- prefix/suffix removal
- replacement and mapping
- duplicate rejection

These should be a transformation pipeline rather than a growing set of
interacting flags, for example a validated config file or repeated
`--transform` expressions.

### Implementation plan

1. Define transformation ordering and whether each operation is per-list or
   post-combination.
2. Implement simple deterministic transforms first: trim, skip-empty,
   deduplicate, sort, filter, replace.
3. Add a bounded expression grammar; do not add scripting or command
   execution.
4. Define memory costs for deduplication and sorting and account for them in
   limits.
5. Add transformation metadata to explain mode.
6. Add tests for Unicode/case behavior, stable ordering, duplicates, malformed
   expressions, and resource exhaustion.

### Security and acceptance criteria

- No shell, regex denial-of-service, dynamic loading, or arbitrary evaluation.
- Regex patterns have explicit size/complexity limits or use a linear-time
  engine.
- Sorting and deduplication have bounded memory.
- Transformation results are deterministic across supported platforms.

The initial CLI surface uses repeatable `--transform` expressions. Each
expression is applied left-to-right to every list before operation counting:
`trim`, `skip-empty`, `deduplicate`, `reject-duplicates`, `sort`, `lower`,
`upper`, `filter=<glob>`, `replace=<from>=><to>`, `prefix=<value>`, and
`suffix=<value>`. Glob matching is bounded and supports only `*` and `?`.

## F8 — Keyed relational joins

### User-facing feature

Implement SQL-like joins as a separate operation after the simpler modes:

- inner join
- left join
- full outer join
- optional anti-join
- configurable key columns for structured input

Example conceptual interface:

```text
combinator join --left users.csv --right accounts.csv \
  --left-key user_id --right-key user_id --type left
```

### Implementation plan

1. Decide whether joins operate only on CSV/TSV/JSONL records or also on line
   values; structured records should be the initial target.
2. Define duplicate-key semantics, output column collisions, null/missing-key
   behavior, and ordering.
3. Implement a bounded hash join first, with the smaller side selected only
   when that choice is deterministic and observable.
4. Add a sort-merge mode for larger inputs when feasible.
5. Add output schemas and collision policies.
6. Add count/estimate behavior, including unknown counts for streaming joins.
7. Add tests for duplicates, missing keys, empty inputs, malformed records,
   collisions, limits, and deterministic output.

### Security and acceptance criteria

- Hash tables and key sizes are bounded.
- No implicit unbounded buffering of both inputs.
- Duplicate-key expansion is counted and limited before output where possible.
- Join output cannot overwrite a destination until successful completion.
- The feature is documented as distinct from Cartesian product.

## F9 — Public in-process Rust API

### User-facing feature

Expose a supported library API for Rust callers so they can avoid subprocess
overhead while using the same semantics as the CLI.

The API should include validated request types, operation engines, iterators,
formatters, count/estimate helpers, and typed errors. CLI-specific concerns
such as process exit codes and terminal diagnostics should remain in the CLI
crate.

### Implementation plan

1. Define public request/configuration types independent of `clap`.
2. Separate core errors from CLI rendering and exit-code mapping.
3. Support borrowed inputs where practical to reduce copying.
4. Add streaming sink traits with cancellation and write-error propagation.
5. Mark stability expectations and document semver policy.
6. Add API examples and integration tests that compare library and CLI output.
7. Keep expensive operations opt-in and make all limits explicit in requests.

### Security and acceptance criteria

- Library callers cannot accidentally bypass hard ceilings.
- APIs never panic on hostile or malformed data under documented inputs.
- Cancellation stops generation promptly.
- CLI behavior is tested against the same shared engine, not a second
  implementation.

## F10 — Distribution, completions, and release packaging

### User-facing feature

Make installation easy for non-Rust users:

- signed or checksummed binaries for Windows, Linux, and macOS
- GitHub release archives
- package-manager recipes where practical: Homebrew, Scoop, winget, and
  Linux package ecosystems
- shell completions and man pages
- versioned documentation and examples
- a compatibility matrix for platforms and Rust/library support

### Implementation plan

1. Define supported targets and release cadence.
2. Add reproducible release builds using the pinned toolchain and locked
   dependencies.
3. Produce archives with checksums and provenance metadata.
4. Add CI jobs for tests, formatting, clippy, audit, and cross-platform builds.
5. Generate completions/man pages from the CLI source.
6. Publish installation instructions and a minimal smoke-test script.
7. Add release verification for archive contents, version output, and sample
   invocations.

### Security and acceptance criteria

- Release artifacts are built from tagged source with pinned dependencies.
- Checksums and provenance are published with every release.
- Archives contain no `target/`, temporary files, credentials, or local config.
- Installation paths and upgrade behavior are documented.

## Cross-cutting engineering work

These should be treated as shared milestones rather than repeated in every
feature branch:

1. Create a common validated request and operation abstraction.
2. Create shared input readers, output writers, formatters, and limit tracking.
3. Version machine-readable schemas: diagnostics, explain output, and JSONL
   records.
4. Add property tests for ordering, count/estimate consistency, sharding, and
   round trips where applicable.
5. Add hostile-input tests for every new parser and transformation.
6. Benchmark representative small, medium, and large workloads without
   weakening resource ceilings.
7. Run the normal verification set for each coherent change:

```text
cargo test -p <affected-package> --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

## Suggested delivery sequence

### Phase A — Contract and adoption foundation

- F1: product/zip/concat modes
- F3: templates and named fields
- F5: dry-run and explain
- F6: quiet, warnings, broken-pipe behavior, completions
- F10: release binaries and installation documentation

### Phase B — Real-world data handling

- F2: CSV, TSV, NUL, escaped inline input, mixed explicit sources
- F7: deterministic normalization pipeline
- F4: sharding and resumability

### Phase C — Application integration

- F9: stable in-process Rust API
- F8: keyed relational joins
- expanded language examples and integration clients

The phases are ordered so that the project first becomes easier to understand
and install, then handles messy operational data, and only afterward takes on
the more complex memory and schema behavior of relational joins.

## Definition of done for a feature

A feature is not complete when the happy-path code works. It must also have:

- a documented command-line contract and examples
- stable error and exit behavior
- bounded resource behavior
- deterministic ordering
- focused unit, integration, and hostile-input tests
- updated help text and README guidance
- compatibility tests for existing invocations
- benchmark evidence where performance is a stated goal
- security review of paths, parsing, output replacement, and diagnostics
