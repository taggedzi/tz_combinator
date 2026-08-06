# Library usage

The workspace exposes three reusable library crates. `combinator-core` contains
lazy, CLI-independent algorithms. `combinator-codecs` contains bounded input,
template, output, and size-estimation adapters. `combinator-app` provides
shared planning, preview, streaming, join, and safe file-output workflows.
These libraries do not own terminal event loops or command-line parsing.

## Stability status

The CLI is the supported stable integration boundary. The Rust APIs documented
here are the current 0.x workspace APIs and are intentionally not covered by a
semver-stability promise. They may change before a supported public API is
selected. See the [compatibility policy](compatibility.md#rust-library-api-status)
for the decision and the conditions for revisiting it.

Add the crates from the workspace:

```toml
[dependencies]
combinator-core = "0.1"
combinator-codecs = "0.1"
```

## Core operations

All operation iterators yield indexes, not copied strings. This keeps
generation lazy and lets an application choose its own sink and rendering.

```rust
use combinator_core::{combinations, ProductOptions};

let lists = vec![
    vec!["a".into(), "b".into()],
    vec!["1".into(), "2".into()],
];
let indexes: Vec<Vec<usize>> = combinations(&lists, ProductOptions::default()).collect();
assert_eq!(indexes, vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]]);
```

`ProductOptions` has `reverse`, `reverse_fields`, `offset`, and `limit`.
Default order varies the rightmost list fastest; `reverse_fields` makes the
leftmost fastest, while `reverse` reverses the complete sequence.

```rust
use combinator_core::{combinations, ProductOptions};
let lists = vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]];
let page: Vec<_> = combinations(&lists, ProductOptions {
    offset: 1, limit: Some(2), ..Default::default()
}).collect();
assert_eq!(page, vec![vec![0, 1], vec![1, 0]]);
```

### Zip and concat

```rust
use combinator_core::{zip_records, UnequalPolicy, ZipOptions};
let lists = vec![vec!["a".into(), "b".into(), "c".into()], vec!["1".into(), "2".into()]];
let rows: Vec<_> = zip_records(&lists, ZipOptions {
    on_unequal: UnequalPolicy::Cycle, ..Default::default()
}).unwrap().collect();
assert_eq!(rows, vec![vec![0, 0], vec![1, 1], vec![2, 0]]);
```

`UnequalPolicy::Error` returns `ZipLengthMismatch`; `Truncate` uses the
shortest list and `Cycle` uses the longest. `concat_records` yields
`(list_index, item_index)` in list order and accepts `ConcatOptions` with
`reverse`, `offset`, and `limit`.

### Selection operations and counts

```rust
use combinator_core::{select_combinations, permutations, SelectionOptions};
let choices: Vec<_> = select_combinations(3, 2, SelectionOptions::default())
    .unwrap().collect();
assert_eq!(choices, vec![vec![0, 1], vec![0, 2], vec![1, 2]]);
let perms: Vec<_> = permutations(3, SelectionOptions::default()).unwrap().collect();
assert_eq!(perms[0], vec![0, 1, 2]);
```

Use `factorial`, `binomial`, `falling_factorial`, `combination_count`,
`zip_count`, and `concat_count` to preflight work. They return `Count`, which
is either `Count::Exact(u128)` or `Count::Overflow`; callers should treat
overflow as a failed closed preflight.

### Operations, normalization, constraints, and sharding

`Operation` groups product, zip, and concat configurations for shared
validation/counting. `normalize_typed` applies `Transform` values such as
`Trim`, `Lower`, `Sort`, filtering, replacement, prefixes, and suffixes.
`Constraint` expresses typed equality, prefix, suffix, glob, and length
checks. `shard_range(total, index, count)` returns a balanced half-open range;
`shard_page` intersects it with offset/limit paging.

Constraint globs match complete values and use byte-oriented `*` and `?`
semantics: `*` matches zero or more bytes and `?` matches exactly one byte.
Glob evaluation uses constant auxiliary space and charges the checked product
of pattern bytes and value bytes against a 16,777,216 byte-pair budget shared
by all glob constraints evaluated for one candidate. An empty side is charged
as one byte because matching must still scan the other side. Exceeding that
budget returns `CONSTRAINT_WORK_LIMIT_EXCEEDED`. Generation also checks its
cancellation callback periodically within a glob match, so a timeout or caller
cancellation does not have to wait for the next candidate.

```rust
use combinator_core::{shard_range, Count};
assert_eq!(shard_range(5, 1, 2).unwrap().start, 3);
assert_eq!(shard_range(5, 1, 2).unwrap().end, 5);
let _ = Count::Exact(5);
```

## Streaming generation

For a complete application boundary, use `GenerationRequest` and a
`RecordSink`. The engine supplies logical records to the sink while enforcing
`GenerationLimits`; it does not allocate the complete output.

```rust,no_run
use combinator_core::{generate, GenerationLimits, GenerationRequest, LogicalRecord, RecordSink};

struct VecSink(Vec<String>);
impl RecordSink for VecSink {
    fn push(&mut self, record: LogicalRecord<'_>) -> Result<(), combinator_core::CoreError> {
        self.0.push(record.fields.join("-"));
        Ok(())
    }
}

// Construct a GenerationRequest for the desired Operation and limits, then:
// let report = generate(request, &mut VecSink(Vec::new()))?;
```

`generate_with` is the callback-oriented equivalent. `GenerationReport`
contains emitted record and byte accounting. Implement sinks to stream to a
network response, file, or application-specific encoder, and propagate errors
instead of buffering unbounded output.

`combinator_app::ProductRequest` is the shared application request used by the
GUI, TUI, and embedding callers. Its `formula_policy` is
`FormulaPolicy::Warn` by default. During
`plan`, prepared fields for CSV/TSV are checked before a sink receives records:
`Allow` preserves without the targeted warning, `Warn` adds one safe
`DOWNSTREAM_INTERPRETATION_RISK` warning to the `ExecutionPlan`, and `Reject`
returns that code as an error. Call `plan` before opening an application-owned
destination when implementing another frontend.

### Application safety ceilings

`combinator_app::ResourceLimits` defines the caller-selected limits used by
the first-party application boundary. `ResourceLimits::default()` supplies
the normal operational defaults, while `HARD_RESOURCE_LIMITS` and the
`HARD_MAX_*` constants define immutable compiled ceilings shared by the CLI,
GUI, TUI, and embedding callers. `plan`, `preview`, `stream`, the join entry
points, and `read_input_source` reject a value above those ceilings with
`RESOURCE_LIMIT_TOO_HIGH` before opening or reading an input source.

Interfaces may lower these limits but cannot raise them. A network API should
deserialize into a separate untrusted transport type and perform a checked
conversion into `ProductRequest` or `JoinRequest`; do not deserialize client
policy directly into an executable request. A service-owned policy may be
stricter than the compiled ceilings, and a client may only lower that policy.

The optional request timeout preserves desktop and CLI compatibility: `None`
does not create a trusted service deadline. A service must enforce its own
finite deadline and combine a client timeout by selecting the earlier value.

## Codecs

`combinator_codecs::input` provides `InputFormat`, `InputLimits`, and
`InputBudget` for bounded line/CSV/TSV/NUL/inline decoding. `consume_bytes`
and `consume_item` should be called as input is consumed so hostile sources
cannot bypass limits.

`output::Format` supports `Text`, `Jsonl`, `Csv`, `Tsv`, and `Nul`.
`format_record` renders positional fields; `format_record_with` also accepts
an optional template/name context. `Jsonl` produces structured records with
an index, value, and fields unless the caller deliberately selects lean value
output. Both formatting functions require a final output byte limit and stop
before joining fields or encoding JSON/CSV beyond that budget.

`combinator_codecs::is_formula_like_field` exposes the allocation-free version
1 classifier, with `FORMULA_PREFIX_POLICY_VERSION` identifying its contract.
It checks the first Unicode scalar only and does not rewrite data. Destination
policy intentionally remains above the raw formatter so direct codec callers
retain content-preserving CSV/TSV bytes and remain responsible for their own
consumer-specific validation.

```rust
use combinator_codecs::{format_record, Format};
let bytes = format_record(&["a", "b"], 0, "-", "\n", Format::Text, false, 1024)
    .expect("record fits configured limit");
assert_eq!(bytes, "a-b\n");
```

`template::Template::parse` validates literal text and positional/named
placeholders; `validate_name` checks field identifiers. Templates are bounded
by the required byte budget passed to `Template::render` and should be parsed
once, then rendered for each record. `estimate_text_size` gives an exact text
byte count; `estimate_jsonl_size` is a conservative planning estimate and must
not replace runtime output limits.

## Safe file output

`combinator_app::FileSink` provides the staged file-output implementation used
by the CLI, GUI, and TUI. It implements `std::io::Write`, so encoded bytes can
be streamed through a `BufWriter` or written directly without exposing the
underlying file handle. Call `commit()` only after generation and flushing
succeed:

```rust,no_run
use combinator_app::FileSink;
use std::io::Write;

let mut sink = FileSink::open("output.txt", false).expect("open staged output");
sink.write_all(b"generated output\n").expect("write output");
sink.commit().expect("commit output");
```

Opening the sink validates the destination path and creates a secure sibling
temporary file. `commit()` synchronizes the staged file, then installs it
atomically. With `overwrite` set to `false`, commit fails with `OUTPUT_EXISTS`
if another process creates the destination after the sink is opened. Dropping
an uncommitted sink removes only its staged temporary file and leaves any
existing destination unchanged.

Callers that implement a different file sink remain responsible for equivalent
path validation, exclusive creation, atomic replacement, and failure cleanup.
The operating-system and attacker assumptions for `FileSink` are documented in
[Security and deployment](security-and-deployment.md#safe-file-output).

## Errors and safety

Core failures use `CoreError { code, message, context, kind }`; codec failures
use the analogous `CodecError`. `ErrorKind::Usage` identifies invalid input,
while `Runtime` identifies execution/resource failures. Preserve codes and
context when adapting errors to another API.

The core and codec crates remain policy-neutral and enforce the limits supplied
by their direct callers. `combinator-app` adds the compiled first-party safety
ceilings and provides the atomic `FileSink` adapter. No library API enforces
process-level CPU, memory, disk, request-rate, or concurrency quotas, so service
wrappers must add those controls. Use checked arithmetic, prefer a streaming
sink, keep `Count::Overflow` fatal, and do not treat a preflight estimate as an
atomic reservation.
