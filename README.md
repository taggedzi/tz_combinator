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
cargo build --release
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
**rightmost** list varies fastest by default (pass `--reverse` to flip that —
see below).

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
| `--sep <SEP>` | `""` (empty) | Field separator joining the items within one combination. |
| `--rec-sep <SEP>` | `"\n"` | Record separator between combinations. Text format only. |
| `--list-delim <DELIM>` | `","` | Delimiter used to split each inline `--list` value. Must be non-empty. |
| `--reverse` | off | Vary the **leftmost** list fastest instead of the rightmost. |
| `--offset <N>` | `0` | Skip this many leading combinations before emitting. |
| `--limit <N>` | unlimited | Emit at most this many combinations. |
| `--count-only` | off | Print only the total combination count and exit; generates nothing. |
| `--format <text\|jsonl>` | `text` | Output format. |
| `--lean-output` | off | In `jsonl` format, emit only the value as a bare JSON string per line, instead of the full `{"i":...,"value":...,"fields":[...]}` object. |
| `-o, --output <PATH>` | stdout | Write output to this file instead of stdout. |
| `-f, --overwrite` (alias `--force`) | off | Allow overwriting `--output` if it already exists. |
| `--max-file-size <BYTES>` | none | Optional filesystem max-file-size cap, checked during pre-flight when writing to a file. |
| `--no-preflight` | off | Skip pre-flight validation (existence, disk space, size cap) for file output. |
| `-h, --help` | — | Print help. |
| `-V, --version` | — | Print version. |

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

## `--reverse`, `--offset`, `--limit`, `--count-only`

By default the rightmost list varies fastest (standard odometer order).
`--reverse` flips that so the leftmost list varies fastest:

```
$ combinator --list "red,blue" --list "car,bike" --sep "-" --reverse
red-car
blue-car
red-bike
blue-bike
```

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

## stdout / stderr discipline

- **stdout** carries only generated data: combination records (text or
  jsonl), or the single number printed by `--count-only`. Nothing else is
  ever written there.
- **stderr** carries all diagnostics: errors and the `EMPTY_LIST` warning.
  Each diagnostic is one line. When `--format jsonl` is in effect, error
  lines are also JSON, with an `error` object containing `code`, `context`,
  and `message` fields (key order is not part of the contract — parse by
  field name); otherwise they're the plain-text form
  `error[CODE]: message (key=value, ...)`.

This means a consumer can always safely capture stdout alone and get nothing
but generated output (or nothing, on error), and can always safely capture
stderr alone and get nothing but diagnostics.

## Error codes

`combinator` uses a fixed set of ten stable, machine-readable codes. Usage
errors (bad arguments/input) exit **2**; runtime errors (I/O, capacity, and
similar failures encountered while executing an otherwise-valid command) exit
**1**. `EMPTY_LIST` is the sole non-fatal warning: it is written to stderr but
does not change the exit code (the run still exits **0**, having produced
zero combinations).

| Code | Exit | Meaning |
|---|---|---|
| `NO_LISTS` | 2 | Neither `--list` nor `--file` was given. |
| `SOURCE_CONFLICT` | 2 | Both `--list` and `--file` were given; only one source is allowed. |
| `EMPTY_LIST` | 0 (warning) | One of the input lists has zero items, so the product is empty. Written to stderr; not a failure. |
| `BAD_DELIMITER` | 2 | `--sep`, `--rec-sep`, or `--list-delim` exceeds the 4096-byte cap, or `--list-delim` is empty. |
| `OUTPUT_EXISTS` | 1 | `--output` names a file that already exists and `--overwrite` was not passed. |
| `INSUFFICIENT_SPACE` | 1 | Pre-flight estimate of the output size exceeds available disk space. |
| `FILE_SIZE_LIMIT` | 1 | Pre-flight estimate of the output size exceeds `--max-file-size`. |
| `COUNT_OVERFLOW` | 1 | The total combination count (for `--count-only`, or for the pre-flight size estimate) is too large to represent exactly. |
| `FILE_UNREADABLE` | 1 | A `--file` path (or stdin, for `--file -`) could not be read. |
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
3. Read stderr separately; a non-empty stderr line means a diagnostic (error
   or the `EMPTY_LIST` warning) — check the process exit code to tell an
   error (1 or 2) from a clean run that merely warned (0).
4. Do not rely on any output before the process exits other than what has
   already been read from the pipe — the tool streams records as they're
   produced, so a consumer can begin processing before generation finishes,
   but should still check the final exit code once the process ends.

This keeps the contract narrow and stable: one binary, two channels (stdout
for data, stderr for diagnostics), one exit-code convention, and one JSON
shape.
