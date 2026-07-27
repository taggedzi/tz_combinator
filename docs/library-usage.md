# Library usage

The workspace exposes three reusable library crates. `combinator-core` contains
lazy, CLI-independent algorithms. `combinator-codecs` contains bounded input,
template, output, and size-estimation adapters. `combinator-app` provides
shared planning, preview, streaming, join, and safe file-output workflows.
These libraries do not own terminal event loops or command-line parsing.

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

## Codecs

`combinator_codecs::input` provides `InputFormat`, `InputLimits`, and
`InputBudget` for bounded line/CSV/TSV/NUL/inline decoding. `consume_bytes`
and `consume_item` should be called as input is consumed so hostile sources
cannot bypass limits.

`output::Format` supports `Text`, `Jsonl`, `Csv`, `Tsv`, and `Nul`.
`format_record` renders positional fields; `format_record_with` also accepts
an optional template/name context. `Jsonl` produces structured records with
an index, value, and fields unless the caller deliberately selects lean value
output.

```rust
use combinator_codecs::{format_record, Format};
let bytes = format_record(&["a", "b"], 0, "-", "\n", Format::Text, false);
assert_eq!(bytes, "a-b\n");
```

`template::Template::parse` validates literal text and positional/named
placeholders; `validate_name` checks field identifiers. Templates are bounded
by the caller before reading files and should be parsed once, then rendered
for each record. `estimate_text_size` gives an exact text byte count;
`estimate_jsonl_size` is a conservative planning estimate and must not
replace runtime output limits.

## Errors and safety

Core failures use `CoreError { code, message, context, kind }`; codec failures
use the analogous `CodecError`. `ErrorKind::Usage` identifies invalid input,
while `Runtime` identifies execution/resource failures. Preserve codes and
context when adapting errors to another API.

The library does not enforce filesystem atomicity or process-level quotas.
Applications must bound input bytes, item counts, output bytes, recursion and
concurrency, use checked arithmetic, and implement safe file creation and
replacement. Prefer a streaming sink, keep `Count::Overflow` fatal, and do
not treat a preflight estimate as an atomic reservation.
