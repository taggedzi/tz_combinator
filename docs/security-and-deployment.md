# Security and deployment

`tz_combinator` is designed to process untrusted text without unbounded work.
Its built-in limits are a safety baseline, not a complete policy for a
multi-user service.

For the rationale behind the raw-output policy and its explicit override, see
[Security decisions](security-decisions.md).

## Resource controls

The shared application layer bounds input bytes, item bytes, items per list,
the number of lists, total items, generated records, and output bytes for the
CLI, GUI, TUI, and embedding callers. Join operations also bound records per
side and the expansion caused by duplicate keys.

The default limits are:

| Resource | Default | Compiled ceiling |
|---|---:|---:|
| Output bytes | 1 GiB | 1 GiB |
| Input byte budget | 64 MiB | 64 MiB |
| Bytes per item | 1 MiB | 1 MiB |
| Items per list | 1,000,000 | 1,000,000 |
| Input lists | 128 | 128 |
| Total items | 5,000,000 | 5,000,000 |
| Generated combinations | 10,000,000 | 10,000,000 |
| Constraint glob work per candidate | 16,777,216 byte-pairs | 16,777,216 byte-pairs |
| Join records per side | 100,000 | 250,000 |
| Duplicate-key join expansion | 10,000 | 100,000 |
| Caller timeout | None | 1 hour when provided |

Use the corresponding `--max-*` options and `--timeout-ms` to lower these
limits. The compiled ceilings cannot be raised by any first-party interface.
Requests and profiles above a ceiling fail with `RESOURCE_LIMIT_TOO_HIGH`
before input loading. The maximum accepted caller timeout is one hour.

Defaults, compiled ceilings, deployment policy, and client requests are
distinct. A service should configure deployment limits below the compiled
ceilings and permit clients only to lower them. Deserialize network input into
an untrusted transport type, validate it against the service-owned policy, and
only then construct `ProductRequest` or `JoinRequest`. Do not deserialize a
client-selected policy directly into an executable application request.

An omitted request timeout retains the local CLI/desktop behavior and is not a
service-grade deadline. A public wrapper must install a trusted finite deadline;
if a client requests a shorter timeout, use the earlier of the two.

Files and standard input are read incrementally under byte and item limits.
Each source is capped, and list operations also share the input byte budget
across all sources in one request.
Generation is streamed rather than materialized in full. Join generation is
streamed, but parsed join inputs and the right-side hash index remain in
memory: the current bounded hash-join design retains both parsed sides, an
index of right-side row positions, and (for full joins) matched-row markers.
`--limit 1` avoids retaining joined output records, and count-only computes
counts without constructing joined records, but neither mode removes the
bounded input/index residency. For representative service workloads, size
`--max-input-bytes`, `--max-item-bytes`, and `--max-join-records` together with
an external memory/concurrency quota; the CLI ceilings are safety bounds, not
a promise that the worst-case 1 MiB field limit fits in a service's memory.

Constraint glob matching uses constant auxiliary space. Before evaluating a
glob, the core multiplies pattern bytes by value bytes with checked arithmetic
and charges that estimate to a per-candidate budget shared by all evaluated
glob constraints; an empty side counts as one byte so it cannot bypass the
budget. Overflow or budget exhaustion fails closed with
`CONSTRAINT_WORK_LIMIT_EXCEEDED`. Deadline and caller cancellation checks also
run periodically inside glob matching rather than only between candidates.

## Safe file output

Without `--overwrite`, the output file is created exclusively and the
operation fails if it already exists. With `--overwrite`, output is staged in
a sibling temporary file and committed after successful generation. The
destination is rechecked immediately before commit, so a destination changed
to a symbolic link or reparse point is rejected. A failed write therefore
preserves the previous destination. Without overwrite, the final
hard-link/exclusive-create step remains authoritative if another process
creates the destination after opening.

Output paths require an existing parent directory. The writer rejects:

- a destination that is a symbolic link or reparse point;
- a parent path containing a symbolic link or reparse point; and
- parent traversal using `..`.

The writer assumes that a privileged attacker cannot concurrently replace the
approved parent directory. Portable path-based replacement cannot hold an
ancestor directory handle across Unix and Windows with identical semantics;
the preflight and commit-time checks therefore reduce races but are not a
complete defense against that privileged namespace attack. Services operating
across trust boundaries must restrict output to an application-owned directory
and deny untrusted users rename/write access to its ancestors.

## Preflight and runtime enforcement

Preflight validates the request and estimates output size before creating the
destination. Available disk space can change after this check, so the estimate
is advisory rather than a reservation. Output-byte enforcement during writing
is authoritative.

`--no-preflight` disables the early capacity check only. It does not disable
runtime input, combination, or output limits.

Use `--dry-run` for a human-readable validation summary or
`--explain --format json` for a versioned machine-readable plan. Neither mode
generates records or creates the requested output file.

## Processing untrusted input

For local automation:

- use explicit limits that match the expected workload;
- supply a finite timeout for attacker-controlled requests;
- constrain all paths to directories controlled by the application;
- prefer JSON Lines output and parse diagnostics by field name; and
- keep standard output and standard error on separate pipes.

Text and NUL output do not escape values. Their record boundaries are therefore
ambiguous when an untrusted value contains the selected record separator. CSV
and TSV quote structural delimiters, but they do not neutralize terminal
controls or semantics assigned by a later consumer. A spreadsheet, database
loader, reporting system, visualization tool, or later export may interpret a
preserved field as active syntax. Do not display any of these raw formats from
an untrusted producer by piping them through `cat`, `type`, or an equivalent
terminal-writing command; use JSON Lines and a real parser.

CSV/TSV generation defaults to `--formula-policy warn` for a documented set of
formula-like prefixes. `reject` stops recognized fields before generation opens
or changes a destination; `allow` explicitly accepts byte-identical output
without that warning. The detector is defense in depth only. It neither
sanitizes values nor recognizes every grammar, locale, consumer, or later
interpretation boundary. Select and validate for the actual destination, and
continue to treat reports derived from untrusted data as untrusted.

When the CLI itself owns an interactive stdout terminal, it scans normalized
values and other data-bearing formatting inputs before generation. It rejects
terminal controls and direct NUL output with `UNSAFE_TERMINAL_OUTPUT`, before
writing any records. `--allow-unsafe-terminal-output` is an explicit escape
hatch for trusted, intentional raw output. Redirection to a pipe or file keeps
the established byte-for-byte raw format behavior, so the receiving program
remains responsible for parsing safely and for not replaying hostile bytes to
a terminal.

Templates and filters are data-only. They do not execute commands, evaluate
general expressions, read environment variables, or load arbitrary files.
Template files are read only when explicitly selected and are subject to input
limits.

Operational logging is opt-in and disabled by default. Logs are phase-level
only and exclude generated values, list items, join keys, templates, raw paths,
environment values, and credentials. `COMBINATOR_LOG` accepts only the
documented bounded level vocabulary; a command-line level takes precedence.
The initial implementation has no persistent file, network, shell, dynamic
loading, or telemetry destination. Treat explicitly enabled stderr logs as
diagnostic data and keep stderr separate from generated stdout.

## Public-service checklist

A network-facing wrapper must impose stricter policy outside the process. At
minimum:

1. Authenticate and authorize requests before accepting paths or output
   destinations.
2. Restrict input and output to an application-owned directory, or do not
   expose arbitrary paths.
3. Set a finite `--timeout-ms` and enforce independent wall-clock and CPU
   limits.
4. Enforce memory, concurrency, request-rate, input-rate, output-rate, and disk
   quotas.
5. Run each request with a low-privilege identity and use process or container
   isolation where practical.
6. Apply both per-client and global concurrency limits.
7. Keep runtime limits enabled even when preflight is disabled.
8. Set join limits below the CLI ceilings for untrusted clients.

Preflight checks must not be used as the service's only resource-control
mechanism.
