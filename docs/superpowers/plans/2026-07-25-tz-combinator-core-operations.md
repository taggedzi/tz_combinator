# tz_combinator Core Boundary and Operations Implementation Plan

**Goal:** Make `combinator-core` the owner of interface-neutral combination
semantics, keep `combinator-cli` responsible for command-line interaction and
presentation, and then add permutations, combinations, variations, and typed
constraint/filter support without duplicating behavior across future
interfaces.

**Scope:** This plan covers architecture and implementation. It does not
include publishing crates, adding a REST/GUI interface, or changing the
existing Cargo package metadata.

## Architecture

`combinator-core` should accept validated, interface-neutral values and return
typed operation results or lazy iterators. It owns:

- product, zip, concat, and keyed-join semantics;
- permutation, combination, and variation algorithms;
- deterministic ordering and rank/index behavior;
- checked counts, overflow handling, pagination, and sharding;
- typed constraints and operation-level validation;
- structured records and core errors.

`combinator-cli` should translate command-line arguments into core requests and
render the result. It owns:

- `clap` definitions and CLI compatibility behavior;
- inline, file, stdin, CSV/TSV, NUL, and JSONL input adapters;
- CLI-specific transform/filter expression parsing;
- templates, text/JSONL/CSV/TSV/NUL output formatting;
- diagnostics, stable CLI exit codes, output files, and filesystem safety;
- CLI-specific preflight and presentation policies.

The core must not depend on `clap`, filesystem paths, terminal behavior,
process exit codes, or CLI-specific string syntax. A future interface should be
able to construct the same core request without emulating command-line parsing.

## Compatibility and security invariants

- Bare `combinator` remains product mode and preserves current output.
- Existing `product`, `zip`, `concat`, and `join` behavior remains covered by
  black-box CLI tests.
- Existing output formats, diagnostics, and exit codes remain stable unless a
  deliberately documented new operation requires an addition.
- Core iterators remain lazy where practical; no operation may silently
  materialize an unbounded result.
- Counts, factorials, binomial coefficients, ranks, and byte/item budgets use
  checked arithmetic and fail closed on overflow.
- No user-provided filter expression becomes executable code or invokes the
  shell, filesystem, network, or environment.
- Resource limits are enforced at the point of generation, not only during
  advisory preflight checks.
- Hostile inputs must not panic; retain and extend the no-panic coverage.

## Phase 0: characterize the current boundary

Before moving code, inventory each public core API and each CLI call site.
Classify it as algorithm, data model, input adapter, presentation, or
filesystem/process behavior. Record behavior tests for any code that will be
relocated.

Pay particular attention to:

- `combinator-core/src/execute.rs`, which currently accepts a writer;
- `combinator-core/src/output.rs` and `template.rs`, which contain formatting;
- `combinator-core/src/input.rs` and `normalize.rs`, which mix reusable logic
  with string-oriented transform parsing;
- `combinator-cli/src/sharding.rs`, which contains operation-independent
  paging logic;
- `combinator-core/src/join.rs`, where the join algorithm should remain core
  while record parsing and rendering remain adapters.

## Phase 1: establish the core/CLI boundary

### 1. Define the interface-neutral core request model

Create or refactor shared types so the core receives validated values rather
than `clap` structs. The model should represent operation options, input
values, ordering, page selection, limits, and typed constraints without knowing
where the values came from.

Keep operation-specific options separate so invalid combinations are rejected
by construction where possible. Preserve the existing product/zip/concat
options and add join requests for structured records without moving file
parsing into the core.

### 2. Move reusable paging and operation execution into core

Move the shard-range calculation from `combinator-cli/src/sharding.rs` into a
core module and make product, zip, concat, join, and future operations use the
same checked page semantics. Replace writer-specific execution with a callback,
iterator, or structured record stream that the CLI can render.

The core should expose counts and logical records/index tuples. The CLI should
be the layer that converts those records into bytes and writes stdout or an
output file.

### 3. Separate formatting and input adapters

Move or refactor the following responsibilities out of core where they are
presentation-specific:

- text/JSONL/CSV/TSV/NUL formatting;
- CLI template syntax and template rendering;
- file/stdin/inline parsing;
- CLI transform-expression parsing;
- writer and output-byte accounting tied to a particular format.

Retain reusable core data structures and generic size/count primitives where
they are format-independent. Keep the CLI’s current output and input behavior
unchanged while replacing direct calls into formatting-oriented core modules.

### 4. Define error translation explicitly

Keep core errors typed and interface-neutral. Map them in the CLI to the
existing diagnostic codes and exit statuses. Do not make the core own stderr
wording or process exit behavior.

### 5. Verify the boundary incrementally

After each relocation, run focused core unit tests and the affected CLI tests.
Finish the phase with:

```text
cargo test -p combinator-core --locked
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Add core-level tests for product/zip/concat/join semantics so their behavior
does not depend only on subprocess tests.

## Phase 2: add new core operations

Before implementing each operation, document its input shape, result shape,
ordering, count behavior, and limit behavior in the core API and CLI design.

Recommended initial semantics:

- **Permutations:** all orderings of one logical item pool, without reuse.
- **Combinations:** selections of size `k` from one logical item pool, where
  order does not matter.
- **Variations:** ordered selections of size `k` from one logical item pool,
  without reuse.
- **Product:** remains the operation for combining independent lists, with
  reuse across positions as it does today.

Initially require one logical pool for permutations, combinations, and
variations unless a later design explicitly defines how multiple `--list`
inputs are flattened. This avoids silently inventing semantics for multiple
heterogeneous lists.

### 6. Implement permutation iteration and counting

Add a core module with checked factorial counts, deterministic input-order
iteration, reverse/rank support where practical, and offset/limit/shard
handling. Test empty, singleton, duplicate-value, overflow, and large-limit
cases. Decide and document whether duplicate input values are treated as
distinct positions or deduplicated values; the recommended default is to
preserve input positions and document duplicate output behavior.

### 7. Implement combination iteration and counting

Add checked binomial counting and lazy combinations of size `k`. Define the
behavior for `k = 0`, `k > n`, empty input, duplicate values, reverse order,
and paging. Ensure count-only and shard calculations fail closed on overflow.

### 8. Implement variation iteration and counting

Add checked `nPk` counting and lazy ordered selections of size `k` without
replacement. Reuse shared ranking/paging primitives where possible, but keep
the operation’s ordering and validation explicit. Test the relationship among
permutations (`k = n`), combinations, and variations.

## Phase 3: typed constraint/filter support

### 9. Define a core constraint model

Create an interface-neutral constraint representation over candidate fields or
records. It should support the first useful bounded predicates without
embedding CLI syntax—for example equality, prefix/suffix, glob matching,
field length, and boolean conjunction/disjunction as appropriate.

The evaluator must be deterministic, bounded, and side-effect-free. Define a
maximum expression depth/size and make malformed or over-limit constraints
typed errors.

### 10. Add constraint-aware generation

Apply constraints lazily during candidate generation. Preserve the distinction
between:

- the logical operation count before filtering;
- the number of accepted records;
- the number of records emitted after offset/limit.

For arbitrary constraints, exact count-only and balanced sharding may require
scanning. Define whether the core reports an unknown/bounded count, performs a
bounded scan, or rejects unsupported count/shard requests. Do not claim an
exact count when the predicate has not been evaluated.

Add tests for filtering every operation where supported, zero matches, all
matches, short-circuiting, ordering preservation, limit enforcement, and
hostile/deep expressions.

### 11. Parse CLI filter syntax in the CLI

Keep the existing or redesigned command-line expression syntax in
`combinator-cli`. Parse it into the typed core constraint model, map parse and
evaluation errors to stable CLI diagnostics, and test that other interfaces
can construct the same constraints without using the CLI parser.

## Phase 4: expose CLI controls without leaking CLI concerns into core

Add subcommands/options only after the core APIs are stable:

- `permutations` for all orderings of one input pool;
- `combinations --choose <k>`;
- `variations --length <k>`;
- constraint/filter options translated into core predicates.

Reuse common CLI input, output, limits, templates, JSONL, count-only, offset,
reverse, and sharding behavior through adapters. Reject unsupported option
combinations at the CLI boundary with clear usage errors, while keeping core
validation authoritative for non-CLI callers.

Update README and command help with examples and precise ordering semantics.

## Acceptance criteria

- Core exposes all existing and new operation semantics without CLI or
  filesystem dependencies.
- CLI behavior remains backward compatible for existing modes and formats.
- Permutations, combinations, variations, and typed constraints have direct
  core tests plus CLI integration tests where exposed.
- Counts and pagination are correct for normal and boundary inputs, including
  overflow and zero-result cases.
- Arbitrary filters never produce unbounded memory use or unsafe side effects.
- Future interfaces can call the core operations without duplicating algorithms
  or parsing command-line strings.
- Full verification passes:

```text
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

