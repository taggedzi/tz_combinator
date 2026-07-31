# Opt-in Structured Logging Plan

> Proposed implementation plan dated 2026-07-31. This document does not
> describe implemented behavior. Repository code, current manuals, and the
> compatibility policy remain authoritative until individual tasks land.

## Objective

Add low-volume, opt-in operational logging that helps diagnose large or failed
operations without contaminating generated data, exposing hostile input, or
changing default CLI behavior.

Logging is distinct from the existing diagnostics, warnings, summaries, and
`ProgressEvent` callbacks. The first implementation should instrument phase
boundaries in the CLI/application workflow; it should not log individual
records or turn the core algorithms into a global logging subsystem.

## Existing contracts to preserve

- Generated data goes to stdout unless an output file is selected.
- Diagnostics, warnings, and summaries go to stderr.
- `--quiet` suppresses non-fatal warnings, not fatal errors.
- JSON and JSONL output modes can select machine-readable error rendering on
  stderr.
- Successful default invocations commonly produce empty stderr, and contract
  tests depend on that behavior.
- GUI and TUI progress uses application-layer `ProgressEvent` values and must
  remain independent of CLI log configuration.

Logging must therefore be disabled by default and must leave existing stdout,
stderr, exit codes, and files byte-for-byte unchanged when it is not enabled.

## Terminology and ownership

Keep these channels conceptually separate:

| Channel | Purpose | Default destination | Stability |
|---|---|---|---|
| Generated output | User-requested records | stdout or selected file | Stable data contract |
| Fatal diagnostic | Explain a failed invocation | stderr | Stable codes and documented rendering |
| Warning | Non-fatal actionable condition | stderr | Controlled by `--quiet` |
| Summary | Requested final record/byte totals | stderr | Controlled by `--summary` |
| Progress | UI/user-facing work progress | sink callback/UI | Rate-limited by interface |
| Operational log | Opt-in phase and timing evidence | stderr initially | New, explicitly non-stable field set before 1.0 |

The CLI owns subscriber initialization, destination selection, and user-facing
configuration. Application code may emit a small approved set of structured
events after the first CLI-only phase proves useful. Core iteration and count
functions should remain logging-free unless later evidence identifies a
specific need that cannot be met at their call boundaries.

## Security and resource requirements

- Never log generated records, list items, join keys, template content,
  environment values, credentials, or raw stdin.
- Do not log raw attacker-controlled paths. Prefer booleans, source kinds,
  counts, format identifiers, and stable error codes. Any later path field
  requires an explicit redaction and escaping policy.
- Every text event must be one physical line with control characters escaped.
- Structured events must be valid JSON and use a documented framing rule.
- Do not emit per-record, per-field, per-byte, retry-loop, or collision-loop
  events. Event volume must be bounded by invocation phases, not input size.
- Logging failures must never panic, corrupt output, bypass limits, or change a
  successful operation into a failed operation.
- Logging must not create files, open network connections, load dynamic code,
  or invoke a shell in the initial scope.
- Disabled logging must have negligible overhead; enabled logging must avoid
  allocation in generation hot loops because no hot-loop events should exist.

## Proposed initial interface

Add explicit CLI controls after validating names against the existing help
surface:

```text
--log-level off|error|warn|info|debug|trace
--log-format text|json
```

Defaults:

- `--log-level off`;
- `--log-format text` for ordinary text-oriented invocations;
- no persistent log file;
- no timestamps in tests; human runtime logs may include a timestamp only if
  the format and determinism implications are documented.

An environment variable such as `COMBINATOR_LOG` may be added in the same or a
later phase. If added, accept only the documented level vocabulary, bound its
length before parsing, and define precedence as CLI option over environment
over `off`. Do not automatically honor a broad process-global filter grammar
until its behavior and hostile-environment implications are reviewed.

`--quiet` and logging remain orthogonal: `--quiet` controls warnings, while an
explicit non-off log level controls operational logs. This avoids a hidden
precedence rule and lets automation request logs while suppressing non-fatal
warnings.

## Structured stderr framing decision

Before implementation, lock down the behavior of opt-in logs when the CLI is
also using machine-readable diagnostics. Today, callers may parse an error on
stderr as one JSON value. Emitting additional text or JSON objects would break
that framing for invocations that explicitly enable logging.

Recommended rule:

- Logging off preserves the current single-diagnostic stderr contract exactly.
- Text logging uses one escaped line per event and is allowed only when the
  caller accepts human-readable stderr.
- JSON logging uses JSON Lines: one object per physical line.
- When logging is enabled for a JSON/JSONL data invocation, require
  `--log-format json` or return a usage error before reading input or creating
  output.
- In that explicit mode, operational logs and any final diagnostic use a
  common JSON Lines envelope with a `kind` discriminator such as `log` or
  `diagnostic`.
- Document that enabling JSON logging changes stderr framing from a possible
  single diagnostic object to an event stream; stdout and exit status remain
  unchanged.

This rule must be reviewed against the compatibility policy before coding. If
the framing change is judged too costly even when explicitly requested, defer
logs for machine-readable invocations rather than mixing incompatible output.

## Event model

Start with a small allowlist of phase-level events. Suggested events and safe
fields are:

| Event | Level | Safe fields |
|---|---|---|
| `invocation_started` | debug | operation, input format, output format, output destination kind |
| `validation_complete` | debug | operation, effective limit flags, preflight enabled |
| `input_complete` | info | source count, list/record count, aggregate input bytes |
| `estimate_complete` | info | exact/overflow/unknown status, selected record count when safe |
| `generation_started` | debug | operation, output destination kind |
| `generation_complete` | info | records, bytes, elapsed milliseconds |
| `invocation_cancelled` | warn | stable error code, elapsed milliseconds |

Fatal errors should continue through the existing diagnostic renderer. Do not
emit a second generic log event for the same error unless it adds distinct,
safe operational context. Error codes are safe; raw error contexts require a
field-by-field review before inclusion.

Event names and required fields should be centralized and tested. The initial
field schema is operational rather than a stable public API, but changes must
still be intentional because users will build troubleshooting tools around it.

## Dependency and architecture decision

Use structured instrumentation rather than ad hoc `eprintln!` calls. `tracing`
with a CLI-owned `tracing-subscriber` is the initial candidate because it can
emit typed fields and remains inert without a subscriber. Before adding direct
dependencies:

1. Confirm compatible versions support Rust 1.94.1 and locked builds.
2. Review enabled features and transitive dependencies.
3. Disable unnecessary registry/filter/ANSI features where practical.
4. Verify initialization is explicit and occurs once.
5. Confirm a library caller that installs no subscriber sees no output.

Initial placement:

- `combinator-cli`: parse logging options, initialize the subscriber, select
  text/JSON formatting, and instrument top-level phases;
- `combinator-app`: no new logging dependency in the first phase; expose or
  reuse returned counts/progress so the CLI can log summaries;
- `combinator-core` and `combinator-codecs`: no logging dependency initially;
- GUI/TUI: no behavior change and no automatic log file.

Add application-layer instrumentation later only when CLI boundary events are
insufficient for a demonstrated troubleshooting case.

## Phase 1: Contract tests before instrumentation

1. Extend black-box tests to record the existing default stdout/stderr behavior
   for successful text, JSON, and JSONL invocations.
2. Preserve the current machine-readable fatal diagnostic shape with logging
   disabled.
3. Add argument-validation tests for levels, formats, conflicts, and option
   precedence before any input or output path is touched.
4. Define whether help, version, completion, and man-page commands initialize
   logging. The recommended behavior is no operational logs for generated help
   artifacts unless explicitly required and tested.
5. Define broken-pipe behavior with logging enabled so secondary log writes do
   not obscure the primary outcome.

Acceptance criteria:

- Existing invocations remain byte-identical when logging is off.
- Invalid logging configuration fails before reading stdin or creating output.
- The stderr framing rule is documented and represented by contract tests.
- Existing error codes and exit statuses are unchanged.

## Phase 2: CLI-owned logging infrastructure

1. Add the minimum compatible structured logging dependencies to the CLI as
   locked direct dependencies.
2. Add validated CLI configuration with an explicit default of `off`.
3. Initialize one subscriber after successful argument parsing and before the
   first instrumented operation phase.
4. Implement one-line escaped text formatting and JSON Lines formatting.
5. Use a writer that targets stderr without taking ownership of generated
   stdout or application output files.
6. Handle subscriber initialization and write failures without panicking or
   changing operation success.
7. Keep test formatting deterministic by disabling timestamps, colors, thread
   IDs, and source locations unless a test explicitly requests them.

Acceptance criteria:

- Logging is silent by default.
- Explicitly enabled logs are stderr-only.
- Each text event occupies one physical line.
- Each JSON event parses independently and has a `kind`, level, event name,
  and approved fields.
- GUI, TUI, and library behavior are unchanged.

## Phase 3: Phase-level instrumentation

1. Instrument validation, bounded input completion, estimation, generation,
   and final completion at their CLI orchestration boundaries.
2. Measure elapsed time with a monotonic clock and log only completed phase
   durations.
3. Reuse `ProgressEvent` totals for final record/byte fields without logging
   each callback.
4. Instrument join and non-join workflows consistently.
5. Ensure cancellation produces at most one operational event plus the normal
   diagnostic.
6. Review every field against the safe-field allowlist.

Acceptance criteria:

- Event count is bounded by a small constant for every invocation, regardless
  of records, fields, retries, duplicates, or output size.
- Logs contain no input values, generated records, join keys, template text,
  raw environment values, or raw paths.
- Text and JSON logs represent the same event names and core fields.
- No event is emitted from a per-record generation or serialization loop.

## Phase 4: Security, compatibility, and performance tests

Add black-box and focused tests proving:

- default stdout and stderr remain unchanged;
- enabled logging never writes to stdout or the selected output file;
- text log fields containing synthetic control characters cannot inject an
  additional physical line;
- every JSON log line parses and hostile strings are escaped;
- machine-readable diagnostics retain their old shape with logging off and use
  the approved envelope/framing when logging is explicitly enabled;
- `--quiet`, `--summary`, warnings, fatal diagnostics, and logs follow their
  documented independent rules;
- broken pipes and stderr write failures do not panic or cause duplicate fatal
  diagnostics;
- log volume remains constant between small and large bounded runs;
- disabled logging does not materially regress representative benchmarks.

Use unmistakably synthetic hostile values. Never place real secrets in tests
or captured logs.

## Phase 5: Documentation and later extensions

Update current documentation when implementation lands:

- CLI usage: enabling logs, destinations, precedence, examples, and stderr
  framing;
- compatibility policy: default-off guarantee and explicit structured-stream
  behavior;
- security/deployment: sensitive-data policy and untrusted environment
  handling;
- error reference: any new usage error codes for invalid logging options;
- shell completions and man pages generated from the CLI definition.

Defer these extensions until a concrete operational need exists:

- persistent log files and rotation;
- GUI/TUI log viewers;
- application/core spans;
- source locations and thread identifiers;
- external telemetry, network exporters, or tracing collectors;
- stable public log schemas.

Any persistent logging proposal requires a separate filesystem-security design
covering path ownership, symlink/reparse attacks, permissions, atomicity,
rotation, retention, and disk limits. Network telemetry requires separate user
authorization and privacy review.

## Verification

Use focused CLI contract tests first, then:

```text
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Also run the representative performance benchmark with logging disabled and
enabled to confirm bounded overhead. Review the final diff for raw hostile
fields, duplicate diagnostics, accidental default output, unbounded event
volume, and changes to JSON stderr framing.

## Completion criteria

- Logging is disabled by default and default behavior remains byte-identical.
- Opt-in text and JSON logs are stderr-only, bounded, parseable, and safe for
  hostile inputs.
- Generated stdout and output files never contain logs.
- Existing diagnostics, warnings, summaries, progress, and exit codes retain
  their documented roles.
- No per-record logging or persistent/network destination is introduced.
- Dependency, MSRV, compatibility, security, and performance reviews pass.
- Current documentation and generated CLI materials describe the final
  interface accurately.

