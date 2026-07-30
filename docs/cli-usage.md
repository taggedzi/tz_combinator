# `combinator` CLI usage

This is the reference for the `combinator` executable. Examples use the
binary from `cargo run -p combinator-cli --`; after installation, remove that
prefix and use `combinator`.

## Invocation shape

```text
combinator [OPTIONS] [COMMAND]
```

With no mode, the command is `product`. The explicit form
`combinator product ...` is equivalent. Generated records go to stdout;
diagnostics, warnings, and `--summary` go to stderr. Usage errors exit 2 and
runtime errors exit 1. A successful run exits 0, including an empty-list
warning unless `--warnings-as-errors` is used.

Inspect the installed interface with `combinator --help`,
`combinator <mode> --help`, `combinator completions <shell>`, or
`combinator man`.

## Input options

| Option | Meaning |
|---|---|
| `--list VALUE` | Inline list; repeat for fields. Default delimiter is comma. |
| `--file PATH` | One item per line; repeat for fields. `-` reads stdin. |
| `--input-format lines\|csv\|tsv\|nul\|inline` | Select decoding. |
| `--allow-mixed-inputs` | Allow `--list` and `--file` together, in argument order. |
| `--list-delim TEXT` | Literal inline-list delimiter; default `,`. |

```text
# Two inline lists: four records
combinator --list red,blue --list car,bike --sep -
# Repeat --list for each field; commas inside an item require a delimiter
combinator --list 'a|b' --list 'x|y' --list-delim '|'
# Files are line-oriented by default
combinator --file hosts.txt --file ports.txt --sep :
# A NUL-delimited file
combinator --file values.bin --input-format nul
# Mixed sources, explicitly enabled
combinator --allow-mixed-inputs --list a,b --file more.txt
```

CSV/TSV input accepts one field per record for list operations. `--file -`
may be used only once. Inputs are bounded by the `--max-*` options and the
compiled ceilings described below.

## Operation modes

### Product

The ordered Cartesian product; the rightmost list varies fastest.

```text
combinator product --list a,b --list 1,2 --sep -
a-1
a-2
b-1
b-2
```

`--sep TEXT` joins fields (default empty), and `--reverse-fields` makes the
leftmost field vary fastest. `--reverse` reverses the complete sequence; the
two reverse controls cannot be combined.

### Zip

Pairs positions across lists. `--on-unequal` is `error` by default, or
`truncate` (shortest length) or `cycle` (longest length, wrapping shorter
lists).

```text
combinator zip --list a,b,c --list 1,2 --on-unequal cycle --sep -
a-1
b-2
c-1
```

### Concat

Emits each list sequentially. It has no `--sep`, because each record has one
field.

```text
combinator concat --list a,b --list c,d
a
b
c
d
```

### Selection modes

These modes use one logical input pool (the supplied lists are normalized into
one pool):

```text
combinator permutations --list a,b,c
abc
acb
bac
bca
cab
cba

combinator combinations --list a,b,c --choose 2
ab
ac
bc

combinator variations --list a,b,c --length 2
ab
ac
ba
bc
ca
cb
```

`--choose 0` emits one empty selection. A selection length greater than the
pool emits no records. Duplicate values remain distinct positions.

### Join

Joins two structured inputs by named keys. Inputs are CSV, TSV, or JSONL;
join output is JSONL.

```text
combinator join --left users.csv --right accounts.csv \
  --left-key user_id --right-key user_id --type left --format jsonl
```

Required options are `--left`, `--right`, `--left-key`, and `--right-key`.
`--type` is `inner`, `left`, `full`, or `anti`; `--join-format` selects the
input format (`csv`, `tsv`, or `jsonl`, default `csv`). Join safety controls
are `--max-join-records` and `--max-join-key-fanout`.

## Rendering and templates

`--format` is `text` (default), `jsonl`, `json` (only with `--explain` or
`--dry-run`), `csv`, `tsv`, or `nul`. `--rec-sep` terminates text records;
`--lean-output` makes JSONL emit only a JSON string per line.

```text
combinator --list red,blue --list car,bike --sep - --format jsonl
{"i":0,"value":"red-car","fields":["red","car"]}
{"i":1,"value":"red-bike","fields":["red","bike"]}

combinator --list a,b --list 1,2 --sep - --format jsonl --lean-output
"a-1"
"a-2"
```

`--template TEXT` or `--template-file PATH` renders a value with `{0}`
placeholders. `{{` and `}}` produce literal braces. `--name NAME` supplies
one unique identifier per input list for named placeholders and full JSONL's
`named` object.

```text
combinator --list server1,server2 --list 80,443 \
  --template 'https://{0}:{1}'
combinator --name host --name port --list server1 --list 443 \
  --template '{host}:{port}' --format jsonl
```

Transforms are repeated and applied left-to-right per list:
`trim`, `skip-empty`, `deduplicate`, `reject-duplicates`, `sort`, `lower`,
`upper`, `filter=GLOB`, `replace=FROM=>TO`, `prefix=VALUE`, and
`suffix=VALUE`. Typed `--filter` predicates are repeated and ANDed:
`eq:N=VALUE`, `neq:N=VALUE`, `prefix:N=VALUE`, `suffix:N=VALUE`, `glob:N=PATTERN`, and
`length:N=MIN..MAX`.

```text
combinator --list ' B ,a,b ' --transform trim --transform lower \
  --transform deduplicate
combinator permutations --list aa,ab,ba \
  --filter prefix:0=a --filter length:0=2..2
```

## Paging, counts, shards, and validation

These options are shared by every generation mode:

| Option | Meaning |
|---|---|
| `--reverse` | Emit the mode's sequence backwards. |
| `--offset N` | Skip N records in selected order. |
| `--limit N` | Emit at most N records. |
| `--shard-index I --shard-count N` | Select shard I of N contiguous balanced shards. |
| `--count-only` | Print only the total count. |
| `--explain` | Print a validated execution plan; use `--format json` for machine-readable output. |
| `--dry-run` | Validate and estimate without generating or creating output. |

```text
combinator --list a,b --list 1,2,3 --format jsonl --offset 1 --limit 2
{"i":1,"value":"a2","fields":["a","2"]}
{"i":2,"value":"a3","fields":["a","3"]}
combinator --list a,b --list 1,2,3 --count-only
6
combinator --list a,b --list 1,2,3 --shard-index 1 --shard-count 2
b1
b2
b3
combinator --list a,b --limit 2 --explain --format json
```

## Output files and resource controls

`-o PATH`/`--output PATH` writes to a file. Existing files require
`--overwrite` (alias `--force`, short `-f`). `--max-file-size` limits the
pre-flight estimate; `--no-preflight` skips only that advisory check.

Limits are `--max-output-bytes`, `--max-input-bytes`, `--max-item-bytes`,
`--max-items-per-list`, `--max-lists`, `--max-total-items`,
`--max-combinations`, and `--timeout-ms`. Defaults are 1 GiB output, 64 MiB
input, 1 MiB per item, 1,000,000 items/list, 128 lists, 5,000,000 total
items, 10,000,000 combinations, and no timeout. Compiled ceilings cannot be
raised; the timeout ceiling is one hour. `--quiet` suppresses warnings,
`--warnings-as-errors` promotes them, and `--summary` reports records/bytes
on stderr.

## Errors and automation

Use JSONL for subprocess integration. Stdout contains only data; stderr
contains diagnostics. Plain errors look like:

```text
combinator
error[NO_LISTS]: no input lists were provided
```

With `--format jsonl`, the diagnostic is JSON:

```json
{"error":{"code":"NO_LISTS","context":{},"message":"no input lists were provided"}}
```

Stable codes include `NO_LISTS`, `SOURCE_CONFLICT`, `INPUT_TOO_LARGE`,
`TEMPLATE_INVALID`, `TRANSFORM_INVALID`, `COMBINATION_LIMIT_EXCEEDED`,
`ZIP_LENGTH_MISMATCH`, `JOIN_LIMIT_EXCEEDED`, `OUTPUT_LIMIT_EXCEEDED`,
`OUTPUT_EXISTS`, `UNSAFE_OUTPUT_PATH`, `FILE_UNREADABLE`, and `WRITE_FAILED`.
See the complete code table in `README.md` and always check the final exit
status after draining stdout/stderr.
