# tz_combinator

`combinator` is a command-line tool that streams the **ordered Cartesian
product** of two or more text lists. Give it N lists and it emits every
combination — one item from each list, in list order — as plain text or JSON
Lines. It is the reference implementation of the combination engine and the
single, stable interface that downstream consumers (a future REST API, GUI,
or web frontend) are expected to code against: spawn the binary, read its
stdout, and you get the same contract every caller gets.

This repository is a Cargo workspace with two crates:

- `crates/combinator-core` — the engine: counting, size estimation, and the
  lazy product iterator. No I/O.
- `crates/combinator-cli` — the `combinator` binary: argument parsing, input
  gathering, formatting, pre-flight checks, and stdout/stderr/file output.

## Building / installing

```
cargo build --release --locked
```

The binary is produced at `target/release/combinator` (`target/release/combinator.exe`
on Windows). Run it directly, or install it onto your `PATH`:

```
cargo install --path crates/combinator-cli
```

Requires Rust edition 2021 (`rust-version = "1.74"` in `crates/combinator-cli/Cargo.toml`).

The workspace is MIT-licensed. Third-party dependency licensing, including the
`csv` crate used for CSV/TSV parsing, is documented in
[`docs/dependency-licenses.md`](docs/dependency-licenses.md).

## Quick example

```
$ combinator --list "red,blue" --list "car,bike" --sep "-"
red-car
red-bike
blue-car
blue-bike
```

Two lists (`red,blue` and `car,bike`) produce 2×2 = 4 combinations. The
**rightmost** list varies fastest by default. Use `--reverse-fields` to make
the **leftmost** list vary fastest, or `--reverse` to emit the complete output
sequence backwards.

## Input formats and sources

Every list comes from exactly one of two sources, and you must pick one — not
both — for the whole invocation:

- `--list <VALUE>` — an inline list, split on `--list-delim` (default `,`).
  Repeatable; each occurrence adds one list, in the order given on the
  command line.
- `--file <PATH>` — a list read from a file, one item per line (trailing
  `\r` is stripped, so CRLF files work). Repeatable, same ordering rule.
  Pass `--file -` to read that list from **stdin** — stdin is only read when
  `-` is given explicitly; the tool never reads it implicitly.

Mixing `--list` and `--file` requires `--allow-mixed-inputs`; inline lists are
processed first, followed by file sources. Stdin (`--file -`) may appear only
once. Passing neither is rejected as `NO_LISTS`.

`--input-format` selects bounded file/stdin parsing: `lines` (the default),
`csv`, `tsv`, or `nul`. CSV/TSV sources are single-column records, with quoted
delimiters and doubled quotes supported. NUL sources are byte-delimited before
UTF-8 validation, so newlines are ordinary data. Use `--input-format inline`
with `--list` to enable escaped values: `\\n`, `\\r`, `\\t`, `\\0`, `\\xNN`,
`\\\\`, and escaped list delimiters are supported. Malformed escapes, malformed
CSV, invalid UTF-8, and oversized records fail before output-file replacement.

```
$ printf "a\nb\n" | combinator --file -
a
b
```

## Flags

All flags and their defaults, ground-truthed against `crates/combinator-cli/src/cli.rs`:

| Flag | Default | Meaning |
|---|---|---|
| `--list <VALUE>` | — | Inline list, split by `--list-delim`. Repeatable. |
| `--file <PATH>` | — | List from a file using `--input-format` (`-` = stdin). Repeatable. |
| `--input-format <lines\|csv\|tsv\|nul\|inline>` | source default | Select bounded file/stdin parsing, or escaped inline parsing. |
| `--allow-mixed-inputs` | off | Permit `--list` and `--file` together; lists precede files. |
| `--template <TEMPLATE>` | none | Render each value with a bounded positional or named template. Mutually exclusive with `--template-file`. |
| `--template-file <PATH>` | none | Read the UTF-8 output template from a bounded file. Mutually exclusive with `--template`. |
| `--name <NAME>` | none | Name one input field; repeat once per input list, in list order. |
| `--sep <SEP>` | `""` (empty) | Field separator joining the items within one combination. |
| `--rec-sep <SEP>` | `"\n"` | Record separator between combinations. Text format only. |
| `--list-delim <DELIM>` | `","` | Delimiter used to split each inline `--list` value. Must be non-empty. |
| `--transform <EXPR>` | none | Apply a bounded per-list normalization transform; repeatable and applied left-to-right. |
| `--reverse` | off | Emit combinations in reverse of the default order. Mutually exclusive with `--reverse-fields`. |
| `--reverse-fields` | off | Vary the **leftmost** list fastest instead of the rightmost. Mutually exclusive with `--reverse`. |
| `--offset <N>` | `0` | Skip this many leading combinations before emitting. |
| `--limit <N>` | unlimited | Emit at most this many combinations. |
| `--shard-index <N>` | none | Select a zero-based contiguous shard; requires `--shard-count`. |
| `--shard-count <N>` | none | Partition the ordered output into this many contiguous shards; requires `--shard-index`. |
| `--count-only` | off | Print only the total combination count and exit; generates nothing. |
| `--explain` | off | Print a validated execution summary and generate nothing. Use `--format json` for JSON. |
| `--dry-run` | off | Validate the request and print a summary without generating records or creating output files. |
| `--format <text\|jsonl\|csv\|tsv\|nul>` | `text` | Output format. CSV/TSV quote fields as needed; NUL terminates each record with `\\0`. |
| `--lean-output` | off | In `jsonl` format, emit only the value as a bare JSON string per line, instead of the full `{"i":...,"value":...,"fields":[...]}` object. |
| `-o, --output <PATH>` | stdout | Write output to this file instead of stdout. |
| `-f, --overwrite` (alias `--force`) | off | Allow overwriting `--output` if it already exists. |
| `--max-file-size <BYTES>` | none | Optional file-output cap. It is checked during pre-flight and enforced while writing. |
| `--max-output-bytes <BYTES>` | `1073741824` | Maximum output bytes for every invocation, including stdout. |
| `--max-input-bytes <BYTES>` | `67108864` | Maximum bytes read from each file, stdin stream, or inline list. |
| `--max-item-bytes <BYTES>` | `1048576` | Maximum bytes in one list item. |
| `--max-items-per-list <N>` | `1000000` | Maximum items accepted from one list. |
| `--max-lists <N>` | `128` | Maximum number of lists accepted. |
| `--max-total-items <N>` | `5000000` | Maximum total items across all lists. |
| `--max-combinations <N>` | `10000000` | Maximum combinations generated unless `--count-only` is used. |
| `--no-preflight` | off | Skip pre-flight validation for file output. Runtime output limits still apply. |
| `--quiet` | off | Suppress non-fatal warnings; fatal diagnostics are unaffected. |
| `--warnings-as-errors` | off | Convert the first non-fatal warning into a runtime error. Takes precedence over `--quiet`. |
| `--summary` | off | Print `records` and `bytes` to stderr after successful generated output. |
| `-h, --help` | — | Print help. |
| `-V, --version` | — | Print version. |

The `completions <shell>` subcommand generates a completion script for
`bash`, `elvish`, `fish`, `powershell`, or `zsh`. The `man` subcommand writes
the generated roff manual page to stdout. Both are derived from the CLI
definition and keep stdout free of diagnostics.

## Operation modes

`combinator` supports four operations. The bare invocation with no
subcommand — everything shown above — means `product`, and behaves
identically to `combinator product ...`:

```
combinator [OPTIONS]           # product (default)
combinator product [OPTIONS]   # same as above, explicit
combinator zip [OPTIONS]       # positional pairing
combinator concat [OPTIONS]    # sequential concatenation
combinator join [OPTIONS]      # keyed relational join
```

- **`product`** — the ordered Cartesian product described throughout this
  document: one item from each list per combination.
- **`zip`** — pairs lists positionally: item 0 from every list, then item 1,
  and so on. Requires a policy for unequal-length lists via
  `--on-unequal <error|truncate|cycle>` (default `error`, which refuses to
  silently drop data): `truncate` stops at the shortest list's length,
  `cycle` wraps shorter lists to match the longest.

  ```
  $ combinator zip --list "a,b,c" --list "x,y" --sep "-" --on-unequal cycle
  a-x
  b-y
  c-x
  ```
- **`concat`** — emits every item from list 1, then every item from list 2,
  and so on, preserving order. Each output record has exactly one field, so
  `concat` has **no `--sep` flag** (there is nothing to join within a
  record); passing `--sep` to `concat` is a usage error.

  ```
  $ combinator concat --list "a,b" --list "x,y,z"
  a
  b
  x
  y
  z
  ```

Flags that only make sense for one mode are rejected at parse time (exit 2)
under the others: `--reverse-fields` only exists under `product`,
`--on-unequal` only exists under `zip`, and `--sep` does not exist under
`concat`. `--reverse`, `--offset`, `--limit`, `--count-only`, and every
output/format/resource-limit flag in the table above apply to the list-based
modes; join uses its own `--left` and `--right` sources.

### Keyed joins

`join` is distinct from the Cartesian product: it combines structured records
from two files by named keys. CSV/TSV inputs use their first row as headers;
JSONL inputs must contain one object of string fields per line. Join output is
JSONL and preserves left-file order, with duplicate keys expanded in right-file
order. Empty keys do not match, and colliding right-hand fields receive a
`_right` suffix.

```text
combinator join --left users.csv --right accounts.csv \
  --left-key user_id --right-key user_id --type left --format jsonl
```

`--type` accepts `inner`, `left`, `full`, and `anti`. `--max-combinations`
also bounds the complete join result before output begins; input byte, item,
and output byte limits apply as usual.

## Templates and named fields

Use `--template` when the output value needs a fixed structure rather than a
simple separator:

```
$ combinator product --list "server1,server2" --list "80,443" \
    --template "https://{0}:{1}"
https://server1:80
https://server1:443
https://server2:80
https://server2:443
```

Templates contain literal text and positional placeholders such as `{0}` or
`{1}`. Double braces produce literal braces: `{{` emits `{` and `}}` emits
`}`. Templates do not evaluate expressions, invoke commands, read the
environment, or access files. `--template-file` reads the same syntax from a
bounded UTF-8 file.

Field names are optional and follow input-list order:

```
$ combinator product --name host --name port \
    --list server1 --list 443 --template "{host}:{port}" \
    --format jsonl
{"i":0,"value":"server1:443","fields":["server1","443"],"named":{"host":"server1","port":"443"}}
```

Names must be unique identifiers and exactly one name must be supplied for
each input list. Without names, only positional placeholders are available.
With names, full JSONL output adds the ordered `named` object while retaining
the existing `fields` array. `--lean-output` still emits only the rendered
value.

### Normalization transforms (F7)

Transforms run independently on every input list after parsing and before
counting or generation. They are applied in the order supplied. Supported
expressions are `trim`, `skip-empty`, `deduplicate`, `reject-duplicates`,
`sort`, `lower`, `upper`, `filter=<glob>`, `replace=<from>=><to>`,
`prefix=<value>`, and `suffix=<value>`. Glob filters support only `*` (any
sequence) and `?` (one Unicode scalar); other characters are literals. Sorting
uses deterministic Unicode scalar ordering. Transformed values remain subject
to the item and total-item resource limits.

A template replaces `--sep`; combining a template with a non-empty `--sep` is
rejected as `TEMPLATE_SEPARATOR_CONFLICT`. Template syntax and field errors
are usage errors, and are validated before output-file creation. Templates are
supported by `product`, `zip`, and `concat`; a concat record has one field, so
only `{0}` or its assigned name is valid there.

## Output formats

### Text (default)

Each combination's items are joined with `--sep` and terminated with
`--rec-sep`:

```
$ combinator --list "red,blue" --list "car,bike" --sep "-"
red-car
red-bike
blue-car
blue-bike
```

With `--sep ""` (the default), items are concatenated directly.

### JSON Lines (`--format jsonl`)

One JSON object per line, in this exact key order — `i` (the combination's
0-based index, honoring `--offset`), `value` (the joined string, using
`--sep`), and `fields` (the individual items as a JSON array):

```
$ combinator --list "red,blue" --list "car,bike" --sep "-" --format jsonl
{"i":0,"value":"red-car","fields":["red","car"]}
{"i":1,"value":"red-bike","fields":["red","bike"]}
{"i":2,"value":"blue-car","fields":["blue","car"]}
{"i":3,"value":"blue-bike","fields":["blue","bike"]}
```

### `--lean-output` (jsonl only)

Emits just the joined value as a bare JSON string per line — no index, no
fields array:

```
$ combinator --list "red,blue" --list "car,bike" --sep "-" --format jsonl --lean-output
"red-car"
"red-bike"
"blue-car"
"blue-bike"
```

## `--reverse`, `--reverse-fields`, `--offset`, `--limit`, `--count-only`

By default the rightmost list varies fastest (standard odometer order).
`--reverse-fields` changes that so the leftmost list varies fastest:

```
$ combinator --list "red,blue" --list "car,bike" --sep "-" --reverse-fields
red-car
blue-car
red-bike
blue-bike
```

`--reverse` emits the ordinary product sequence from last to first:

```
$ combinator --list "red,blue" --list "car,bike" --sep "-" --reverse
blue-bike
blue-car
red-bike
red-car
```

`--reverse` and `--reverse-fields` are mutually exclusive.

`--offset` and `--limit` page through the product without materializing
skipped combinations; the JSONL `i` field reflects the true index, not a
position within the page:

```
$ combinator --list "a,b" --list "c,d" --format jsonl --offset 1 --limit 2
{"i":1,"value":"ad","fields":["a","d"]}
{"i":2,"value":"bc","fields":["b","c"]}
```

`--shard-index` and `--shard-count` split the selected output ordering into
balanced, contiguous half-open ranges. Earlier shards receive one extra
record when the count is not evenly divisible. Existing `--offset` and
`--limit` are intersected with the shard range, so they can resume or further
page a shard without changing its boundaries. The same rule applies to
`zip` and `concat`; with `--reverse`, ranges are assigned in reverse output
order. The range algorithm is version 1 and is deterministic for stable
inputs:

```
$ combinator --list "a,b" --list "1,2,3" --sep "-" --shard-index 1 --shard-count 2
b-1
b-2
b-3
```

Use `--explain --format json` to obtain the full count plus the effective
offset, limit, shard range, ordering, and output estimate without generating
records.

`--count-only` prints just the total combination count and exits, without
generating any combinations:

```
$ combinator --list "a,b" --list "c,d,e" --count-only
6
```

## Dry-run and explain

Use `--dry-run` to validate inputs, limits, counts, and estimates without
creating an output file or generating records. It prints a human-readable
summary. Use `--explain --format json` for a versioned machine-readable plan:

```
$ combinator --list "a,b" --list "c,d,e" --limit 2 --explain --format json
{"schema_version":1,"operation":"product", ...}
```

The JSON summary reports input list sizes, total combinations, effective
offset/limit, records to emit, a conservative output-byte estimate, the
selected output destination (`stdout` or `file`), and effective resource
limits. It never includes input values or creates the requested output file.
`--format json` is valid only with `--explain` or `--dry-run`.

## Security and resource behavior

Inputs are bounded by default. Files and stdin are read incrementally rather
than loaded without limit; use the `--max-*` flags to tune limits for a trusted
 workload. The compiled hard ceilings cannot be raised by command-line flags.
Generation is bounded by `--max-combinations` and
`--max-output-bytes`; provide an explicit `--limit` for especially large or
attacker-controlled requests.

File output is created exclusively when overwrite is disabled. Overwrites are
staged in a sibling temporary file and committed only after successful writing,
so failures preserve the previous destination. Symlink output targets are
rejected. The pre-flight disk-space check is advisory and is not a reservation;
the runtime byte limit remains authoritative. `--no-preflight` disables only
the early capacity estimate.

For automation, run the binary with `--format jsonl`, capture stdout and stderr
separately, and treat all input and output paths as untrusted unless the caller
has constrained them to an approved directory.

## stdout / stderr discipline

- **stdout** carries only generated data: combination records (text or
  jsonl), or the single number printed by `--count-only`. Nothing else is
  ever written there.
- **stderr** carries all diagnostics: errors, warnings, and optional
  `--summary` output.
  Each diagnostic is one line. When `--format jsonl` is in effect, error
  lines are also JSON, with an `error` object containing `code`, `context`,
  and `message` fields (key order is not part of the contract — parse by
  field name); otherwise they're the plain-text form
  `error[CODE]: message (key=value, ...)`.

This means a consumer can always safely capture stdout alone and get nothing
but generated output (or nothing, on error), and can always safely capture
stderr alone and get nothing but diagnostics.

## Error codes

`combinator` uses stable, machine-readable codes. Usage
errors (bad arguments/input) exit **2**; runtime errors (I/O, capacity, and
similar failures encountered while executing an otherwise-valid command) exit
**1**. `EMPTY_LIST` is the sole non-fatal warning: it is written to stderr but
does not change the exit code (the run still exits **0**, having produced
zero combinations). `--quiet` suppresses it; `--warnings-as-errors` reports it
as a runtime error with exit **1**.

| Code | Exit | Meaning |
|---|---|---|
| `NO_LISTS` | 2 | Neither `--list` nor `--file` was given. |
| `SOURCE_CONFLICT` | 2 | Both `--list` and `--file` were given; only one source is allowed. |
| `DUPLICATE_STDIN` | 2 | Stdin was selected more than once as an input source. |
| `INPUT_FORMAT_INVALID` | 2 | The selected input format is incompatible with the requested source type. |
| `INLINE_ESCAPE_INVALID` | 2 | Escaped inline input contains an unknown or incomplete escape. |
| `CSV_MALFORMED` | 2 | CSV/TSV input contains an unterminated or malformed quoted field. |
| `CSV_MULTIPLE_FIELDS` | 2 | A CSV/TSV input record contains more than one field. |
| `INPUT_NOT_UTF8` | 2 | A text, CSV, TSV, or NUL-delimited record is not valid UTF-8. |
| `TEMPLATE_CONFLICT` | 2 | Both `--template` and `--template-file` were given. |
| `TEMPLATE_SEPARATOR_CONFLICT` | 2 | A template was combined with a non-empty `--sep`. |
| `TEMPLATE_INVALID` | 2 | Template syntax or a template reference is invalid. |
| `TEMPLATE_UNKNOWN_FIELD` | 2 | A template references an unknown field. |
| `TEMPLATE_NAMES_MISMATCH` | 2 | The number of `--name` values does not match the input-list count. |
| `TEMPLATE_DUPLICATE_NAME` | 2 | A field name was supplied more than once. |
| `TEMPLATE_INVALID_NAME` | 2 | A field name is not a valid identifier. |
| `TEMPLATE_TOO_LARGE` | 2 | The template exceeds its 1 MiB security ceiling or configured input limit. |
| `TEMPLATE_FILE_UNREADABLE` | 2 | The template file could not be read or is not valid UTF-8. |
| `FORMAT_UNSUPPORTED` | 2 | `--format json` was used without `--explain` or `--dry-run`. |
| `TRANSFORM_INVALID` | 2 | A `--transform` expression is malformed or uses unsupported syntax. |
| `TRANSFORM_LIMIT` | 2 | The invocation contains too many transformation expressions. |
| `DUPLICATE_ITEM` | 1 | `reject-duplicates` found a duplicate within an input list. |
| `MODE_CONFLICT` | 2 | Mutually exclusive generation-summary modes were combined. |
| `SHARD_ARGUMENTS_INCOMPLETE` | 2 | `--shard-index` and `--shard-count` must be provided together. |
| `SHARD_COUNT_INVALID` | 2 | `--shard-count` must be positive. |
| `SHARD_INDEX_INVALID` | 2 | `--shard-index` must be less than `--shard-count`. |
| `SHARD_COUNT_OVERFLOW` | 1 | The shard range could not be computed safely. |
| `EMPTY_LIST` | 0 (warning) | One of the input lists has zero items, so the product is empty. Written to stderr; not a failure. |
| `BAD_DELIMITER` | 2 | `--sep`, `--rec-sep`, or `--list-delim` exceeds the 4096-byte cap, or `--list-delim` is empty. |
| `RESOURCE_LIMIT_TOO_HIGH` | 2 | A configurable resource limit exceeds the compiled security ceiling. |
| `OUTPUT_EXISTS` | 1 | `--output` names a file that already exists and `--overwrite` was not passed. |
| `INSUFFICIENT_SPACE` | 1 | Pre-flight estimate of the output size exceeds available disk space. |
| `FILE_SIZE_LIMIT` | 1 | Pre-flight estimate of the output size exceeds `--max-file-size`. |
| `COUNT_OVERFLOW` | 1 | The total combination count (for `--count-only`, or for the pre-flight size estimate) is too large to represent exactly. |
| `FILE_UNREADABLE` | 1 | A `--file` path (or stdin, for `--file -`) could not be read. |
| `INPUT_TOO_LARGE` | 1 | A file, stdin stream, or inline list exceeds the input byte limit. |
| `ITEM_TOO_LARGE` | 1 | A list item exceeds the item byte limit. |
| `TOO_MANY_ITEMS` | 1 | A list or the combined inputs exceed an item-count limit. |
| `TOO_MANY_LISTS` | 1 | The invocation exceeds the maximum list count. |
| `COMBINATION_LIMIT_EXCEEDED` | 1 | Generation would exceed the configured combination limit. |
| `ZIP_LENGTH_MISMATCH` | 1 | `zip` with `--on-unequal error` (the default) and input lists of different lengths. |
| `JOIN_LIMIT_EXCEEDED` | 1 | The bounded join result exceeds `--max-combinations`. |
| `JOIN_FORMAT_INVALID` | 2 | Join output must use `--format jsonl`. |
| `JSONL_MALFORMED` | 2 | A JSONL join record is malformed. |
| `JOIN_SCHEMA_INVALID` | 2 | Structured join headers or rows are invalid. |
| `OUTPUT_LIMIT_EXCEEDED` | 1 | The generated output would exceed the configured byte limit. |
| `CANCELLED` | 1 | Execution reached `--timeout-ms` or was cancelled by an embedding caller. |
| `CAPACITY_UNKNOWN` | 1 | Available disk capacity could not be determined during pre-flight. |
| `UNSAFE_OUTPUT_PATH` | 1 | The output path is a symbolic link or otherwise unsafe to overwrite. |
| `WRITE_FAILED` | 1 | Creating or writing to the output file failed. |

Example (`NO_LISTS`, plain-text rendering — the default):

```
$ combinator
error[NO_LISTS]: no input lists were provided
$ echo $?
2
```

The same error under `--format jsonl` is rendered as JSON instead:

```
$ combinator --format jsonl
{"error":{"code":"NO_LISTS","context":{},"message":"no input lists were provided"}}
$ echo $?
2
```

Example (`EMPTY_LIST`, a warning — note the exit code stays 0):

```
$ combinator --file empty.txt
error[EMPTY_LIST]: a list is empty; zero combinations will be produced (list_index=0)
$ echo $?
0
```

## Consuming from other programs

Treat `combinator` as a subprocess you spawn and stream from — this is the
intended integration point for any higher-level consumer (REST API, GUI,
etc.):

1. Spawn the binary with the desired flags, redirecting stdout to a pipe.
2. Read stdout line by line (text mode) or, for structured consumption, run
   with `--format jsonl` and parse each line as a JSON object
   (`{"i": <index>, "value": <string>, "fields": [<string>, ...]}`, or a bare
   JSON string per line if `--lean-output` was also passed).
3. Read stderr separately; a non-empty stderr line means a diagnostic (error,
   warning, or optional summary) — check the process exit code to tell an
   error (1 or 2) from a clean run that merely warned (0).
4. Do not rely on any output before the process exits other than what has
   already been read from the pipe — the tool streams records as they're
   produced, so a consumer can begin processing before generation finishes,
   but should still check the final exit code once the process ends.

This keeps the contract narrow and stable: one binary, two channels (stdout
for data, stderr for diagnostics), one exit-code convention, and one JSON
shape.

## Public-service deployment

The CLI has bounded defaults and a compiled one-hour maximum for
`--timeout-ms`, but the compiled ceilings are intentionally too large to be a
complete multi-tenant service policy. A network-facing wrapper should impose
smaller per-request limits and enforce them outside the process as well.

At minimum, a public wrapper should:

- authenticate and authorize requests before accepting filesystem paths or
  output destinations;
- restrict input and output paths to an application-owned directory, or avoid
  arbitrary paths entirely;
- set a finite `--timeout-ms` and also enforce wall-clock, CPU, memory,
  concurrency, input-rate, output-rate, and disk quotas with the supervisor;
- run each request with a low-privilege identity and, where possible, process
  or container isolation;
- apply per-client and global concurrency limits;
- keep runtime output limits enabled even when `--no-preflight` is used;
- treat pre-flight capacity checks as advisory because they do not reserve
  disk space.

Join requests are streamed during result generation, but the right-side hash
index and both parsed inputs remain in memory. Set service-specific input,
item, record, and output limits below the CLI hard ceilings when handling
untrusted clients.
