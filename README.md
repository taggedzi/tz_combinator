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

## Input: `--list` vs `--file`

Every list comes from exactly one of two sources, and you must pick one — not
both — for the whole invocation:

- `--list <VALUE>` — an inline list, split on `--list-delim` (default `,`).
  Repeatable; each occurrence adds one list, in the order given on the
  command line.
- `--file <PATH>` — a list read from a file, one item per line (trailing
  `\r` is stripped, so CRLF files work). Repeatable, same ordering rule.
  Pass `--file -` to read that list from **stdin** — stdin is only read when
  `-` is given explicitly; the tool never reads it implicitly.

Mixing `--list` and `--file` in the same invocation is rejected as
`SOURCE_CONFLICT` (see the error table below). Passing neither is rejected as
`NO_LISTS`.

```
$ printf "a\nb\n" | combinator --file -
a
b
```

## Flags

All flags and their defaults, ground-truthed against `crates/combinator-cli/src/cli.rs`:

| Flag | Default | Meaning |
|---|---|---|
| `--list <VALUE>` | — | Inline list, split by `--list-delim`. Repeatable. Mutually exclusive with `--file`. |
| `--file <PATH>` | — | List from a file, one item per line (`-` = stdin). Repeatable. Mutually exclusive with `--list`. |
| `--template <TEMPLATE>` | none | Render each value with a bounded positional or named template. Mutually exclusive with `--template-file`. |
| `--template-file <PATH>` | none | Read the UTF-8 output template from a bounded file. Mutually exclusive with `--template`. |
| `--name <NAME>` | none | Name one input field; repeat once per input list, in list order. |
| `--sep <SEP>` | `""` (empty) | Field separator joining the items within one combination. |
| `--rec-sep <SEP>` | `"\n"` | Record separator between combinations. Text format only. |
| `--list-delim <DELIM>` | `","` | Delimiter used to split each inline `--list` value. Must be non-empty. |
| `--reverse` | off | Emit combinations in reverse of the default order. Mutually exclusive with `--reverse-fields`. |
| `--reverse-fields` | off | Vary the **leftmost** list fastest instead of the rightmost. Mutually exclusive with `--reverse`. |
| `--offset <N>` | `0` | Skip this many leading combinations before emitting. |
| `--limit <N>` | unlimited | Emit at most this many combinations. |
| `--count-only` | off | Print only the total combination count and exit; generates nothing. |
| `--explain` | off | Print a validated execution summary and generate nothing. Use `--format json` for JSON. |
| `--dry-run` | off | Validate the request and print a summary without generating records or creating output files. |
| `--format <text\|jsonl>` | `text` | Output format. |
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

`combinator` supports three operations. The bare invocation with no
subcommand — everything shown above — means `product`, and behaves
identically to `combinator product ...`:

```
combinator [OPTIONS]           # product (default)
combinator product [OPTIONS]   # same as above, explicit
combinator zip [OPTIONS]       # positional pairing
combinator concat [OPTIONS]    # sequential concatenation
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
output/format/resource-limit flag in the table above apply to all three
modes.

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
| `MODE_CONFLICT` | 2 | Mutually exclusive generation-summary modes were combined. |
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
| `OUTPUT_LIMIT_EXCEEDED` | 1 | The generated output would exceed the configured byte limit. |
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
