# Downstream Interpretation Risk Controls Plan

**Status:** approved for implementation

**Decision:**
[Content-preserving CSV/TSV and downstream interpretation](../security-decisions.md#content-preserving-csvtsv-and-downstream-interpretation)

## Objective

Keep CSV and TSV useful as lossless interchange formats while making it clear
that serialization does not make hostile field content trusted. Add a
conservative, testable policy for recognized formula-like fields without
claiming that Combinator can sanitize data for every database, spreadsheet,
visualization tool, template engine, shell, or custom processor.

The implementation must give a user three explicit outcomes for recognized
formula-like fields in CSV/TSV output:

- preserve the content without a warning after explicit acknowledgement;
- preserve the content and issue a safe warning; or
- reject the request before output begins.

## Existing contracts to preserve

- `csv` and `tsv` continue to encode fields using the current structural
  quoting rules.
- The default warning policy does not change generated stdout or file bytes.
- Diagnostics remain on stderr and never contaminate generated data.
- Existing files are not created, truncated, or replaced when validation or a
  warnings-as-errors policy fails.
- Raw hostile values are never included in diagnostics, logs, summaries, or
  structured error context.
- Input, item, combination, output-byte, timeout, and filesystem protections
  remain authoritative.
- Existing CLI exit-status meanings and machine-readable diagnostic framing
  remain stable.
- Formula detection does not imply that other downstream grammars have been
  validated or made safe.

## Non-goals

- Universal sanitization for unknown downstream consumers.
- Testing every spreadsheet, database, BI product, visualization tool, locale,
  plugin, or import configuration.
- Silently prefixing, quoting, deleting, or rewriting user data.
- Treating every database import as SQL injection; correct data-only loaders
  normally keep formula syntax inert.
- Adding an Excel-specific transformation or XLSX output in this change.
- Inferring whether input is trusted from its path, source type, process
  environment, or destination filename.
- Detecting arbitrary SQL, shell, HTML, template, URL, path, or expression
  injection. Those require sink-specific grammars and controls.

## Threat model and terminology

An attacker may control any field, including values extracted from HTTP logs,
headers, filenames, imported tables, service responses, or user-managed lists.
A trusted user or service then runs Combinator and sends the resulting CSV/TSV
to a different person or program. The generated file has a trusted origin but
retains attacker-controlled content.

Use these terms consistently:

- **structural quoting:** encoding delimiters, quotes, and record boundaries so
  a compatible CSV/TSV parser recovers the original fields;
- **content preservation:** retaining the field value rather than changing it
  for a selected consumer;
- **downstream interpretation:** meaning assigned to a recovered field by a
  later database, spreadsheet, visualization, template, shell, or custom tool;
- **formula-like field:** a field matching the documented conservative prefix
  detector; this is evidence of risk, not proof of execution;
- **safe destination:** a destination whose parser and subsequent use are
  known and constrained by the caller, not a property conferred by CSV/TSV.

## Proposed interface and codes

Add a CSV/TSV-only CLI option:

```text
--formula-policy allow|warn|reject
```

Resolution rules:

- Omitted: resolve to `warn` for generated CSV/TSV records.
- `allow`: preserve formula-like fields and suppress the targeted warning.
- `warn`: preserve formula-like fields and emit one
  `DOWNSTREAM_INTERPRETATION_RISK` warning before generation.
- `reject`: return `DOWNSTREAM_INTERPRETATION_RISK` before output generation or
  destination creation/replacement.
- An explicitly supplied formula policy with a non-CSV/TSV output format is a
  usage error rather than an ignored option.
- `--count-only` does not generate CSV/TSV records and therefore does not
  trigger the policy. Explain and dry-run modes report the prospective warning
  without generating output.
- `--quiet` suppresses the warning but does not weaken `reject`.
- `--warnings-as-errors` promotes the default warning through the existing
  warning path before any output is opened.

Use one warning per request, not one per item or record. Safe context may
include the selected format, policy, and a bounded count of matching input
items. It must not include values, field excerpts, templates, source paths, or
other hostile strings.

The user-facing warning should communicate the general boundary while stating
the limited detection basis, for example:

```text
warning[DOWNSTREAM_INTERPRETATION_RISK]: CSV/TSV preserves field content;
downstream software may interpret one or more formula-like fields as active
expressions. Treat output derived from untrusted input as untrusted.
```

Do not use `safe`, `sanitized`, or `neutralized` in an option or success
message.

## Formula-like detector contract

Implement one pure, reusable classifier below the frontend policy layer. The
implementation phase must freeze a documented version-1 prefix set using
current primary guidance and regression tests. At minimum, assess ASCII
`=`, `+`, `-`, and `@`, leading control whitespace called out by the selected
guidance, and relevant full-width variants.

The detector must:

- inspect individual logical fields after input decoding and configured
  normalization transforms;
- operate in linear time with constant auxiliary space and no hostile-value
  formatting;
- avoid joining records or reparsing encoded CSV;
- distinguish a field prefix from the same character appearing later in a
  field;
- document any leading-whitespace and Unicode normalization policy;
- stay independent of a particular spreadsheet brand in its API and tests;
- return only classification/count information needed by callers; and
- be conservative without claiming completeness for every consumer or locale.

The CSV writer remains responsible only for structural encoding. Do not put
destination policy into the low-level writer in a way that silently changes
library callers' bytes.

## Phase 1: Lock the contract with focused tests

1. Add codec-level tests for the version-1 formula-like prefix classifier.
2. Add regression tests proving `allow` and `warn` emit byte-identical CSV and
   TSV, including formula-like, quoted, multiline, Unicode, and ordinary
   numeric/text fields.
3. Add tests proving no hostile value appears in warning/error text, JSON
   diagnostics, logs, or contexts.
4. Add failure tests proving `reject` and `--warnings-as-errors` do not create
   a new destination or replace an existing one.
5. Add negative tests for JSONL, text, NUL, count-only, characters in the
   middle of fields, and ordinary nonmatching values.
6. Use harmless fixtures such as `=2+3`, `@example`, and synthetic full-width
   characters. Tests must not contain live URLs, external-data functions,
   shell commands, credentials, or destructive payloads.

Acceptance criteria:

- Tests define exact detection and policy behavior before production wiring.
- Raw CSV/TSV output remains byte-compatible under `allow` and `warn`.
- Every rejecting path is proven to fail before output mutation.

## Phase 2: Add the reusable classifier and shared policy model

1. Add the allocation-free classifier to `combinator-codecs` or another
   interface-neutral module with no CLI dependency.
2. Add a shared `FormulaPolicy` enum with `Allow`, `Warn`, and `Reject` values
   at the application-policy layer.
3. Extend `ProductRequest` and `ExecutionPlan` so GUI and TUI callers receive
   the same warning/rejection result after prepared-list transforms.
4. Ensure `plan()` and `stream()` share the same decision and cannot diverge.
5. Preserve fail-before-sink behavior: a rejected request must fail during
   planning, before `OutputSink::record` can be called.
6. Keep formula-policy metadata out of the raw formatter so existing direct
   codec callers retain content-preserving behavior.

Acceptance criteria:

- CLI, GUI, TUI, preview, and file generation can share one classification
  result and policy vocabulary.
- Planning remains bounded and does not expose hostile data.
- Direct codec formatting is unchanged unless a caller explicitly invokes the
  new classifier.

## Phase 3: Integrate the CLI warning lifecycle

1. Add the `--formula-policy` value enum and help text, tracking whether the
   option was explicitly supplied so non-CSV/TSV misuse can be rejected.
2. Classify prepared fields after transforms and template validation but before
   file output is opened.
3. Append the warning to the existing delayed warning collection so later
   validation failures remain authoritative and stderr ordering stays stable.
4. Route `warn`, `--quiet`, and `--warnings-as-errors` through the existing
   warning renderer, including JSON/JSONL diagnostic framing rules.
5. Route `reject` through a stable coded error with no field contents.
6. Include the prospective warning in dry-run/explain output without creating
   a destination; keep count-only exempt because it emits no delimited records.
7. Add CLI black-box tests for stdout, stderr, exit status, warning context,
   output-file identity preservation, and explicit-policy/format conflicts.

Acceptance criteria:

- Default CSV/TSV generation warns exactly once when a version-1 match exists.
- `allow` is byte-identical and quiet; `reject` writes no records.
- Structured diagnostics remain valid JSON and stdout remains data-only.

## Phase 4: Integrate GUI, TUI, and profiles

1. Add an `Allow`, `Warn`, or `Reject` control visible when CSV/TSV output is
   selected, defaulting to `Warn`.
2. Display warning details, not only a warning count, before generation or in
   the plan/preview surface. Do not display hostile field contents.
3. Make `Reject` prevent background generation before a destination is opened.
4. Add the policy to versioned profile persistence with a backward-compatible
   default for profiles that omit it.
5. Keep policy state when switching formats, but explain that it applies only
   to CSV/TSV; do not silently reinterpret it for JSONL, text, or NUL.
6. Add state-transition, persistence, plan, preview, and generation tests for
   both interfaces.

Acceptance criteria:

- All three user interfaces expose equivalent policy choices and explanations.
- Older profiles load safely with `Warn`; saved profiles round-trip the chosen
  policy.
- No GUI/TUI path bypasses the shared reject decision.

## Phase 5: Update current documentation and compatibility records

1. Qualify the README's broad "safely combines" language: safety covers
   bounded processing, filesystem behavior, and correct serialization, not
   arbitrary downstream interpretation.
2. Update CLI usage, security/deployment, library usage, and compatibility
   documentation with the structural-quoting/content-preservation distinction.
3. Document that output derived from untrusted input remains untrusted and give
   representative downstream categories without implying that every database
   or visualization tool executes formulas.
4. Add the option, warning code, contexts, exit behavior, quiet behavior, and
   warnings-as-errors interaction to the CLI and error references.
5. Add concise GUI/TUI help text at the format-selection point.
6. Explain why no universal sanitize mode exists and why default byte mutation
   would break legitimate interchange and statistics.
7. Record the detector's version-1 prefix and whitespace/Unicode policy.

Acceptance criteria:

- Documentation never describes CSV/TSV as universally safe or sanitized.
- Users can distinguish machine interchange from destination-specific safety.
- Documentation describes both the general downstream boundary and the narrow
  runtime detector without conflating them.

## Phase 6: Verification and release assessment

Run focused checks first, followed by workspace verification:

```text
cargo test -p combinator-codecs --locked
cargo test -p combinator-app --locked
cargo test -p combinator-cli --locked
cargo test -p combinator-gui --locked
cargo test -p combinator-tui --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Perform bounded manual smoke checks for CSV and TSV with `allow`, `warn`,
`reject`, `--quiet`, `--warnings-as-errors`, dry-run, explain, stdout, new file
output, and overwrite of an existing file. Inspect only synthetic output; do
not open test artifacts in spreadsheet software or trigger external content.

Review the final diff for:

- accidental field mutation;
- warning or log exposure of hostile strings;
- partial output before rejection;
- frontend or profile-policy bypasses;
- misleading claims of universal safety;
- unrelated changes or generated artifacts; and
- compatibility/documentation omissions.

Before release, assess whether new evidence changes the disclosure decision in
the accepted security record. In the absence of a demonstrated supported-flow
impact or a prior destination-safety promise, ship this as documented
security-relevant hardening through the normal release process. Record a
security advisory only if the review establishes a vulnerability within the
supported boundary.

## Completion criteria

- The accepted security decision is reflected consistently in code, tests,
  CLI help, GUI/TUI behavior, profiles, and current documentation.
- CSV/TSV remains lossless under `allow` and `warn`.
- Recognized formula-like fields warn by default and can be rejected before
  output mutation.
- Diagnostics remain bounded, structured when requested, and free of hostile
  field contents.
- The project makes no claim that CSV/TSV conversion makes untrusted data safe
  for an unknown consumer.
- Focused and workspace verification passes.
- The completed plan is moved to `docs/archive/plans/` with a status note.
