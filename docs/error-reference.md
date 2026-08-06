# Error reference

The CLI reports stable, machine-readable codes so scripts do not need to match
human-readable messages.

## Exit statuses and output channels

| Exit status | Meaning |
|---:|---|
| `0` | Success, including a non-fatal warning |
| `1` | Runtime, I/O, capacity, or resource-limit failure |
| `2` | Invalid arguments or input |

Standard output contains generated data only. Standard error contains errors,
warnings, and the optional `--summary`. Always drain both streams and check
the final process status.

Plain-text diagnostics have this form:

```text
error[NO_LISTS]: no input lists were provided
```

With `--format jsonl`, diagnostics are JSON:

```json
{"error":{"code":"NO_LISTS","context":{},"message":"no input lists were provided"}}
```

Parse JSON diagnostics by field name; key order is not part of the contract.
Context fields may be added when they help identify the failing value or
limit.

## Warnings

`EMPTY_LIST` is non-fatal by default. It is written to standard error and the
process exits successfully. Every operation except `concat` produces no
records when any input list is empty. Under `concat`, the empty list
contributes no records and the other lists are still emitted.

`DOWNSTREAM_INTERPRETATION_RISK` is also non-fatal under the default CSV/TSV
formula policy. It reports that at least one prepared field begins with a
recognized formula-like prefix; it never includes the hostile field value.
`--formula-policy reject` instead reports the same code as a usage failure
before output is written or a destination is changed.

Use `--quiet` to suppress the warning or `--warnings-as-errors` to turn it into
a runtime error. `--warnings-as-errors` takes precedence if both options are
present.

## Usage and input codes

These conditions normally exit with status 2.

| Code | Meaning |
|---|---|
| `BAD_DELIMITER` | A delimiter is empty where prohibited or exceeds its byte limit. |
| `CSV_MALFORMED` | CSV or TSV input is malformed. |
| `CSV_MULTIPLE_FIELDS` | A list-input CSV or TSV record contains more than one field. |
| `DUPLICATE_STDIN` | Standard input was selected more than once. |
| `DOWNSTREAM_INTERPRETATION_RISK` | `--formula-policy reject` recognized a formula-like CSV/TSV field before output. |
| `FILTER_INVALID` | A typed filter has invalid syntax, field selection, or bounds. |
| `FILTER_LIMIT` | The request contains too many filters. |
| `FILTER_MODE_UNSUPPORTED` | Filters were combined with a summary mode that cannot evaluate accepted-record counts. |
| `FORMAT_UNSUPPORTED` | The selected format is not valid for the requested mode. |
| `FORMULA_POLICY_UNSUPPORTED` | An explicit formula policy was used with output other than CSV or TSV. |
| `INLINE_ESCAPE_INVALID` | Escaped inline input contains an unknown or incomplete escape. |
| `INPUT_FORMAT_INVALID` | The input format is incompatible with the selected source. |
| `INPUT_NOT_UTF8` | A text record is not valid UTF-8. |
| `JOIN_FIELD_INVALID` | A JSON Lines join field is not a string. |
| `JOIN_FORMAT_INVALID` | The requested join output format is unsupported. |
| `JOIN_KEY_INVALID` | A join key is empty or invalid. |
| `JOIN_RECORD_INVALID` | A JSON Lines join record is not an object. |
| `JOIN_SCHEMA_INVALID` | Join headers or row widths are invalid. |
| `JOIN_SOURCE_INVALID` | One or both join sources are missing or invalid. |
| `JSONL_MALFORMED` | A JSON Lines join record is not valid JSON. |
| `LOG_FORMAT_REQUIRED` | Enabled logging for machine-readable output requires JSON log framing. |
| `LOG_LEVEL_INVALID` | The logging level or `COMBINATOR_LOG` value is invalid or too long. |
| `MODE_CONFLICT` | Mutually exclusive generation or summary modes were combined. |
| `NO_LISTS` | No input list source was provided. |
| `ONE_LIST_REQUIRED` | A selection operation did not resolve to exactly one logical input pool. |
| `REVERSE_CONFLICT` | `--reverse` and `--reverse-fields` were combined. |
| `RESOURCE_LIMIT_TOO_HIGH` | A requested limit exceeds its compiled ceiling. |
| `SHARD_ARGUMENTS_INCOMPLETE` | Only one of `--shard-index` and `--shard-count` was supplied. |
| `SHARD_COUNT_INVALID` | The shard count is zero. |
| `SHARD_INDEX_INVALID` | The shard index is not less than the shard count. |
| `SOURCE_CONFLICT` | Inline and file sources were mixed without `--allow-mixed-inputs`. |
| `TEMPLATE_CONFLICT` | Both an inline template and a template file were supplied. |
| `TEMPLATE_DUPLICATE_NAME` | A field name was supplied more than once. |
| `TEMPLATE_FILE_UNREADABLE` | A template file cannot be read or is not valid UTF-8. |
| `TEMPLATE_INVALID` | Template syntax is invalid. |
| `TEMPLATE_INVALID_NAME` | A field name is not a valid identifier. |
| `TEMPLATE_NAMES_MISMATCH` | The number of field names does not match the operation's record width. |
| `TEMPLATE_SEPARATOR_CONFLICT` | A template was combined with a non-empty field separator. |
| `TEMPLATE_TOO_LARGE` | A template exceeds the configured or compiled size limit. |
| `TEMPLATE_UNKNOWN_FIELD` | A template refers to an unknown field. |
| `TRANSFORM_INVALID` | A transform expression is malformed or unsupported. |
| `TRANSFORM_LIMIT` | The request contains too many transforms. |
| `UNSAFE_TERMINAL_OUTPUT` | Raw output would write control characters or NUL records directly to an interactive terminal. Use JSONL, redirect the output, or explicitly pass `--allow-unsafe-terminal-output`. |

## Runtime and resource codes

These conditions normally exit with status 1.

| Code | Meaning |
|---|---|
| `CANCELLED` | Execution timed out or an embedding caller cancelled it. |
| `CAPACITY_UNKNOWN` | Available output capacity could not be determined during preflight. |
| `COMBINATION_LIMIT_EXCEEDED` | Generation would exceed the effective combination limit. |
| `CONSTRAINT_WORK_LIMIT_EXCEEDED` | Constraint glob evaluation would exceed the per-candidate work limit. |
| `COUNT_OVERFLOW` | A count cannot be represented exactly. |
| `DUPLICATE_ITEM` | `reject-duplicates` found a duplicate in an input list. |
| `FILE_SIZE_LIMIT` | The preflight estimate exceeds `--max-file-size`. |
| `FILE_UNREADABLE` | An input or template path could not be read. |
| `INPUT_TOO_LARGE` | An input source exceeds its byte limit. |
| `INSUFFICIENT_SPACE` | The preflight estimate exceeds available disk space. |
| `ITEM_TOO_LARGE` | One item exceeds its byte limit. |
| `JOIN_FANOUT_LIMIT_EXCEEDED` | Duplicate join keys would expand beyond the fanout limit. |
| `JOIN_LIMIT_EXCEEDED` | A join would exceed the effective result limit. |
| `JOIN_OUTPUT_INVALID` | A join result could not be encoded as valid output. |
| `OUTPUT_EXISTS` | The destination exists and overwrite was not enabled. |
| `OUTPUT_LIMIT_EXCEEDED` | Generated output exceeds the effective byte limit. |
| `SHARD_COUNT_OVERFLOW` | Shard boundaries cannot be calculated safely. |
| `TIMEOUT_INVALID` | The requested deadline cannot be represented safely. |
| `TOO_MANY_ITEMS` | A per-list or total item-count limit was exceeded. |
| `TOO_MANY_LISTS` | The input-list count exceeds its limit. |
| `UNSAFE_OUTPUT_PATH` | The destination or an ancestor fails output-path safety checks. |
| `WRITE_FAILED` | Output creation, encoding, writing, synchronization, or commit failed. |
| `ZIP_LENGTH_MISMATCH` | `zip` used the default `error` policy with unequal list lengths. |

`STDOUT_CLOSED` is handled as a normal broken-pipe termination so commands
can participate in pipelines whose downstream consumer exits early.

## Examples

No arguments prints help and exits successfully. Naming an operation without
an input is an error:

```console
$ combinator product
error[NO_LISTS]: no input lists were provided
$ echo $?
2
```

An empty input warns but succeeds:

```console
$ combinator --file empty.txt
error[EMPTY_LIST]: a list is empty; zero combinations will be produced (list_index=0)
$ echo $?
0
```
