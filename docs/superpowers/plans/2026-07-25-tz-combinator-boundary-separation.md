# tz_combinator Core/Interface Boundary Separation Plan

## Goal

Finish the architectural separation begun in
`2026-07-25-tz-combinator-core-operations.md`.

The end state is:

1. `combinator-core` is a pure, interface-neutral library for data models,
   operation semantics, constraints, counting, pagination, and lazy logical
   record generation.
2. Reusable bounded text/record codecs are available as a library so future
   interfaces do not have to reimplement CSV, JSONL, line, NUL, or output
   encoding behavior.
3. `combinator-cli` owns only command-line interaction, source/destination
   resolution, CLI syntax, diagnostics, process behavior, and filesystem
   policy.
4. No library crate depends on `clap`, terminal behavior, process exit codes,
   filesystem paths, the process environment, or shell/network facilities.
5. Existing CLI behavior remains compatible while the implementation moves
   behind the new boundary.

This plan is implementation work only. It does not include publishing crates,
adding REST/GUI interfaces, or changing the public CLI contract except where a
new core boundary requires an explicitly documented compatibility adapter.

## Current-state findings

The current tree already has the core algorithms and new selection operations,
but the separation is incomplete:

| Current responsibility | Current location | Target owner |
| --- | --- | --- |
| Product, zip, concat, join, selection algorithms | `combinator-core` | `combinator-core` |
| Typed constraints and operation validation | `combinator-core` | `combinator-core` |
| Shard range and count primitives | `combinator-core` plus a CLI re-export | `combinator-core` |
| Generic input parsing and CSV/TSV/NUL handling | `combinator-core/src/input.rs` | reusable codec crate |
| String transform implementation and CLI transform parsing | `combinator-core/src/normalize.rs` | typed transforms in core; parser in CLI |
| Text/JSONL/CSV/TSV/NUL formatting | `combinator-core/src/output.rs` | reusable codec crate |
| Template parsing/rendering | `combinator-core/src/template.rs` | reusable codec crate; file loading in CLI |
| Format-specific output-size estimates | `combinator-core/src/estimate.rs` | reusable codec crate |
| Writer-based formatting and byte accounting | `combinator-core/src/execute.rs` | logical generation in core; encoding/writing outside core |
| `clap` parsing and mode translation | `combinator-cli` | `combinator-cli` |
| Paths, stdin/stdout, output files, atomic replacement | `combinator-cli` | `combinator-cli` |
| Join file parsing and JSON/CSV schema policy | `combinator-cli` | CLI/codec adapter |
| Exit codes, stderr wording, warnings, preflight policy | `combinator-cli` | `combinator-cli` |

Evidence that must disappear by the end of this plan:

- `combinator-core` publicly exports `input`, `normalize`, `output`,
  `template`, `estimate`, and writer-oriented `execute` APIs.
- `combinator-cli/src/input.rs`, `output.rs`, and `normalize.rs` primarily
  re-export core implementation instead of owning adapters.
- `combinator-cli/src/main.rs::stream_core` constructs a core
  `ExecutionRequest` containing a writer and format-specific presentation
  fields.
- `combinator-core/Cargo.toml` depends on `csv` and `serde_json` solely for
  adapter/presentation responsibilities.

## Target crate layout

### `combinator-core`

Keep this crate dependency-light and independent of all interface concerns.
Its public API should contain:

- validated text items and structured records;
- `Operation` and operation-specific typed options;
- product, zip, concat, keyed join, permutation, combination, and variation
  semantics;
- typed constraints and typed transforms;
- checked counts, rank/unrank, pagination, sharding, and resource accounting;
- lazy logical record generation;
- typed errors and cancellation hooks that do not mention stderr or exit
  status.

It must not contain:

- `clap` types or CLI expression grammars;
- paths, `File`, stdin/stdout, terminal APIs, or environment access;
- `Write`-based output rendering;
- text/JSONL/CSV/TSV/NUL presentation policy;
- template-file loading;
- exit codes, warning rendering, or preflight disk-space policy.

`std::io::Read`/`Write` may be used only in a separate codec crate. The core
operation layer should not need either trait.

### `combinator-codecs` (new workspace library)

Create a reusable interface-neutral adapter crate for bounded serialization
and parsing. It may depend on `csv` and `serde_json`, but it must not depend
on `clap`, paths, terminals, process APIs, or the CLI crate.

It should provide:

- bounded readers over caller-supplied `Read` values;
- line, NUL, escaped-inline, CSV, TSV, and JSONL codecs;
- generic record encoders for text, JSONL, CSV, TSV, and NUL;
- reusable template parsing/rendering over validated records, if template
  compatibility is retained;
- format-specific exact or conservative size estimates;
- byte/item budgets and checked output accounting over caller-supplied
  writers;
- codec-level typed errors without CLI exit codes or path-specific wording.

The crate should expose configuration structs, not CLI argument types. For
example, use `DelimitedInput { separator, trim, skip_empty }` rather than a
`clap`-derived type or a stringly `--input-format` value.

### `combinator-cli`

Keep CLI-only behavior here:

- `clap` definitions and legacy argument compatibility;
- parsing CLI strings into core typed constraints/transforms/options;
- opening paths, stdin, stdout, and output files;
- source ordering and mixed-source policy;
- atomic replacement, overwrite rules, reparse-point defenses, and output
  destination checks;
- CLI warning text, diagnostics, stable exit codes, and process cancellation;
- available-disk-space preflight and CLI-specific estimate policy;
- join schema/file selection policy and presentation choices.

The CLI should compose core generation with codecs and destination policy:

```text
clap args
  -> validated core options / typed constraints / typed transforms
  -> CLI opens source(s)
  -> codec parses bounded values or records
  -> core counts/validates/generates logical records
  -> codec encodes logical records
  -> CLI enforces destination policy and commits output
```

## Boundary invariants

### Core invariants

- Core APIs accept validated values and typed configuration, never raw CLI
  expression strings.
- Core generation returns logical records or index tuples through an iterator,
  callback, or sink trait. It never formats bytes or writes a destination.
- Core count reports distinguish exact, overflow, and unknown-after-filter
  results. An unevaluated predicate is never reported as an accepted count.
- Core resource limits are enforced during generation, not only by callers'
  preflight checks.
- Core errors contain stable typed categories and structured context. Mapping
  to wording, exit status, JSON diagnostics, or stderr remains an adapter job.
- Core constraints, transforms, and codecs are bounded, deterministic,
  side-effect-free, and panic-resistant.

### Codec invariants

- Codecs operate on caller-provided streams or values; they never open paths
  or inspect the process environment.
- Every reader and encoder has explicit byte, item, field, and nesting limits.
- Partial output and writer failures are reported to the caller; codecs never
  decide whether a destination may be replaced.
- CSV/JSONL/template behavior remains compatible with the current CLI through
  adapter configuration and regression tests.

### CLI invariants

- Existing bare product mode and product/zip/concat/join behavior remain
  unchanged.
- CLI parsing and filesystem checks happen before creating or replacing an
  output destination.
- CLI maps core/codec errors to the existing diagnostic codes and exit
  statuses.
- CLI-specific syntax is parsed once into typed core values; no other
  interface must emulate the CLI grammar.

## Migration phases

### Phase 0: freeze behavior and add boundary checks

1. Inventory every public core item and classify it as domain, algorithm,
   codec, CLI adapter, or filesystem/process behavior.
2. Record current behavior with focused tests before relocating code. Preserve
   the existing black-box tests for product, zip, concat, join, templates,
   input formats, output formats, limits, and atomic output files.
3. Add a dependency-boundary check that fails if `combinator-core` directly
   depends on `clap`, `clap_complete`, `clap_mangen`, `fs2`, `windows-sys`, or
   the CLI crate.
4. Add API documentation describing which types are stable domain API and
   which temporary compatibility modules are scheduled for removal.

Acceptance gate: the baseline workspace tests pass, and the inventory is
checked into this plan or an adjacent architecture document.

### Phase 1: introduce a neutral logical-record generation API

Add a core module, preferably `records.rs` or `generate.rs`, with a public API
similar to:

```rust
pub struct GenerationRequest<'a> {
    pub operation: &'a Operation,
    pub lists: &'a [List],
    pub constraints: &'a [Constraint],
    pub limits: GenerationLimits,
    pub cancel: Option<&'a dyn Fn() -> bool>,
}

pub struct LogicalRecord<'a> {
    pub ordinal: u128,
    pub fields: &'a [usize],
}

pub trait RecordSink {
    fn record(&mut self, record: LogicalRecord<'_>) -> Result<(), CoreError>;
}

pub fn generate<S: RecordSink>(request: GenerationRequest<'_>, sink: &mut S)
    -> Result<GenerationReport, CoreError>;
```

The exact names may differ, but the contract must ensure:

- no `Format`, `Template`, `Write`, `Vec<u8>`, path, or CLI type crosses into
  core generation;
- indices/ordinals remain available for JSON metadata and templates in an
  adapter;
- filtering, accepted-record pagination, cancellation, and generation limits
  are handled once in core;
- join records can use a structured logical-record representation without
  forcing text formatting.

Refactor the current writer-oriented `execute` implementation to use this
generator internally, then make `execute` a temporary compatibility wrapper.
Add direct tests for every existing and new operation through the neutral API.

Acceptance gate: a test-only non-CLI sink can consume product, zip, concat,
join, permutation, combination, and variation records without importing a
formatter or writer.

### Phase 2: create `combinator-codecs`

1. Add the new workspace member and move generic bounded parsing/encoding
   implementations from `combinator-core/src/input.rs` and `output.rs`.
2. Move template parsing/rendering and format-specific estimates into the
   codec crate unless a later review proves they are core domain behavior.
3. Preserve public behavior through codec tests that cover exact boundaries,
   malformed CSV/JSONL, UTF-8 failures, escaping, delimiter limits, template
   validation, and writer failures.
4. Give codec errors stable categories (`InvalidEncoding`, `MalformedRecord`,
   `InputLimitExceeded`, `OutputLimitExceeded`, etc.) rather than CLI codes.
5. Make codecs accept `Read`/`Write` supplied by the caller. Do not pass path
   strings into them except as optional opaque diagnostic labels owned by the
   caller.

Acceptance gate: `combinator-codecs` tests pass independently and no codec
test needs a process, path, terminal, or `clap`.

### Phase 3: move reusable transforms into typed core APIs

The transform algorithms are reusable; their current string syntax is not.

1. Define a typed core enum such as:

   ```rust
   pub enum Transform {
       Trim,
       SkipEmpty,
       Deduplicate,
       RejectDuplicates,
       Sort,
       Lowercase,
       Uppercase,
       FilterGlob(String),
       Replace { from: String, to: String },
       Prefix(String),
       Suffix(String),
   }
   ```

2. Keep validation, ordering, duplicate semantics, and resource limits in
   core.
3. Move parsing of `trim`, `replace=A=>B`, aliases, and other CLI spellings
   into `combinator-cli`.
4. Expose typed transform application to any future interface without making
   it parse CLI strings.
5. Move transform tests with the algorithm into core and retain CLI parser
   tests for syntax/errors separately.

Acceptance gate: a non-CLI caller can construct and apply transforms without
using the CLI parser; malformed CLI syntax remains a CLI usage error.

### Phase 4: migrate the CLI to codecs and neutral generation

1. Replace `combinator-cli/src/input.rs` re-exports with CLI source adapters
   that open paths/stdin and pass streams to `combinator-codecs`.
2. Replace `combinator-cli/src/output.rs` re-exports with codec configuration
   and a CLI writer/destination wrapper.
3. Replace `stream_core`’s `ExecutionRequest` writer path with:
   - a core `GenerationRequest`;
   - a codec encoder sink;
   - CLI output-byte and destination handling.
4. Keep atomic output-file opening/commit entirely in `output_file.rs`.
5. Move preflight size estimation to codec APIs plus CLI available-space and
   max-file-size policy.
6. Ensure dry-run/explain/count-only behavior explicitly declares whether it
   uses logical counts, codec estimates, or filtered scans.
7. Preserve broken-pipe, cancellation, timeout, and partial-output behavior.

Acceptance gate: all existing CLI black-box tests pass without the CLI calling
core format/input/template modules directly.

### Phase 5: complete join and structured-record separation

1. Keep keyed join indexing, join type semantics, duplicate expansion, field
   collision handling, counts, and pagination in core.
2. Define a neutral structured-record input model and join output model in
   core, independent of JSON/CSV schemas.
3. Move JSONL/CSV/TSV record parsing and schema policy into codecs/CLI.
4. Let the CLI choose source paths, key names, format, and output rendering;
   pass validated records and key selectors to core.
5. Add a direct non-CLI join integration test using in-memory records and a
   separate codec/CLI test for file parsing.

Acceptance gate: a future interface can perform a join from in-memory records
without constructing paths or parsing JSON/CSV itself.

### Phase 6: remove compatibility ownership leaks

After migration and deprecation coverage:

1. Remove `input`, `output`, `template`, and `estimate` modules from core.
2. Remove `csv` and `serde_json` from `combinator-core/Cargo.toml` unless a
   remaining domain API demonstrably requires them.
3. Remove the writer-oriented `execute` API or keep it only behind an explicit
   compatibility feature with a removal/versioning note.
4. Delete CLI re-export shims and call codec/core APIs directly.
5. Rename `AppError` so the CLI does not present a core error type as its own
   diagnostic type; add an explicit translation layer.
6. Update crate-level docs and README architecture documentation.
7. Run `cargo tree` and source scans to prove no forbidden dependency or
   path/terminal/process API remains in library crates.

## Error translation design

Use three explicit layers:

1. `combinator-core`: typed domain failures such as invalid operation shape,
   count overflow, constraint limit, cancellation, and generation limit.
2. `combinator-codecs`: malformed input, encoding, schema, and bounded
   reader/writer failures.
3. `combinator-cli`: stable diagnostic codes, human/JSON rendering, exit
   statuses, paths, warnings, and filesystem policy.

The CLI translation table must be tested for every existing code. No core or
codec error should require the CLI to parse a preformatted stderr message.

## Resource and security requirements

- Check bytes before storing untrusted input where practical.
- Check item counts, field counts, nesting, pattern/literal sizes, and output
  bytes with checked arithmetic.
- Treat preflight as advisory; enforce output limits during encoding/writing.
- Keep filtered generation bounded by a core scan/generation budget.
- Never compile or execute user-provided expressions. Glob/predicate matching
  must remain bounded and side-effect-free.
- Keep filesystem race defenses, atomic replacement, permissions, and
  reparse-point handling exclusively in the CLI output adapter.
- Extend no-panic tests to codec parsers, typed transform construction,
  constraint evaluation, and neutral generation.

## Verification matrix

Run focused tests after each phase:

```text
cargo test -p combinator-core --locked
cargo test -p combinator-codecs --locked
cargo test -p combinator-cli --locked
```

Finish with:

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo tree -p combinator-core
cargo tree -p combinator-codecs
```

Add a small architecture check (Rust test or script) that verifies:

- core has no forbidden interface dependencies;
- codec crates have no path/process/terminal/clap dependencies;
- CLI is the only crate importing `clap`, `fs2`, `windows-sys`, or output-file
  replacement code;
- core public APIs do not expose `Format`, `Template`, `Write`, path types, or
  CLI argument structs.

## Acceptance criteria

- A non-CLI Rust caller can construct every operation, typed constraint, typed
  transform, page, limit, and in-memory join request directly from library
  types.
- A non-CLI caller can consume logical records without importing CLI code or
  duplicating operation algorithms.
- A non-CLI caller can use reusable bounded codecs without opening paths or
  parsing CLI syntax.
- `combinator-core` contains no interface-specific or interface-reliant code
  and has no adapter-only dependencies.
- `combinator-cli` contains all command-line syntax, filesystem/process
  behavior, diagnostics, destination policy, and CLI-specific parsers.
- Existing CLI output, diagnostics, exit codes, limits, and filesystem safety
  tests remain green.
- New core, codec, and CLI tests prove ordering, counts, pagination, filters,
  transforms, malformed input handling, cancellation, output limits, and no
  panics.
- The final dependency/source audit proves the intended boundary rather than
  merely relying on passing behavioral tests.
