# tz_combinator — Phase A / F3 Implementation Plan

**Goal:** Add bounded positional templates and optional named fields to the
`product`, `zip`, and `concat` operations while preserving all existing output
and diagnostic contracts when the feature is unused.

**Design:** `docs/superpowers/specs/2026-07-25-tz-combinator-phase-a-f3-templates-design.md`

## Scope boundaries

In scope:

- `--template` and `--template-file`;
- repeatable `--name` aligned with input-list order;
- positional and named placeholders;
- escaped braces;
- additive `named` JSONL metadata;
- conservative template-aware size estimation;
- stable template validation errors;
- product, zip, and concat integration;
- unit, black-box, hostile-input, and compatibility tests.

Out of scope:

- CSV/TSV/NUL input or output;
- transformations, normalization, or escaping functions;
- conditionals, loops, arithmetic, or scripting;
- keyed joins;
- dry-run/explain;
- public API stabilization beyond the clap-free core module needed by this
  feature.

## Task 1 — Establish the template core contract

Create a clap-free `Template`, parsed piece representation, field-name
validation, and typed core errors.

Acceptance criteria:

- The grammar exactly matches the F3 design.
- Parsing is deterministic and independent of operation mode.
- Parsed templates contain no unbounded recursive structures.
- All malformed syntax produces an error, never a panic.

Suggested tests should be written before implementation for valid positional,
valid named, escaped-brace, empty, malformed, and unknown-reference cases.

## Task 2 — Implement bounded parsing and rendering

Implement `Template::parse` and rendering against borrowed selected fields.
Compile references once and render without reparsing.

Acceptance criteria:

- Rendering supports literals, positional fields, named fields, and escaped
  braces.
- Missing fields fail cleanly.
- Rendered output can be written into a caller-owned buffer or sink.
- Template and placeholder counts are bounded.
- Unicode and control characters are preserved for text rendering and escaped
  only by the JSON serializer in JSONL mode.

Add property-style tests where practical: parsing arbitrary strings never
panics, and successful rendering contains exactly the requested literal and
field pieces.

## Task 3 — Add CLI parsing and validation

Add template and name options to the three operation argument structs. Add
template-file reading and map validation failures to stable CLI errors.

Validation order should be:

1. Parse CLI arguments.
2. Reject template-source conflicts.
3. Read and bound the template source.
4. Validate template syntax and names.
5. Read input lists.
6. Validate name count against the actual list count.
7. Stage output only after all validation succeeds.

Acceptance criteria:

- Legacy invocations do not enter the template path.
- `--template` and `--template-file` conflict cleanly.
- Template files are bounded and unreadable files receive a stable error.
- Invalid names, duplicate names, and name-count mismatches exit 2.
- The same validation applies to product, zip, and concat.

## Task 4 — Integrate rendering without changing operation engines

Refactor the shared CLI stream path so each operation continues to yield its
selected fields while the formatter chooses either legacy separator rendering
or template rendering.

Acceptance criteria:

- Product ordering and paging remain unchanged.
- Zip unequal-length policies remain unchanged.
- Concat still produces one-field records.
- `--sep` remains functional when no template is supplied.
- Non-default `--sep` plus a template is rejected.
- Output limits are checked against the fully rendered record.

Run the existing F1 suite at this checkpoint before changing JSONL metadata.

## Task 5 — Add named JSONL metadata compatibly

Extend full JSONL records only when names were explicitly supplied:

```json
{"i":0,"value":"server1:80","fields":["server1","80"],"named":{"host":"server1","port":"80"}}
```

Keep the no-name JSONL branch byte-for-byte identical, including key order.
Keep lean output unchanged.

Acceptance criteria:

- Named keys appear in declared order.
- `fields` remains present and ordered.
- Structural keys cannot be overwritten by user names.
- JSON escaping is valid for hostile field values and names.

## Task 6 — Make size estimation template-aware

Add a conservative estimator for template output. It must account for literal
bytes, placeholder expansion, JSON serialization overhead, index width, and
named metadata when applicable.

Acceptance criteria:

- Estimates never fall below actual output for text or JSONL.
- Estimates honor offset and limit.
- Overflow returns `SizeEstimate::Overflow` rather than wrapping.
- A template cannot bypass `--max-output-bytes` or `--max-file-size`.

Use property tests comparing estimates to actual formatted records across
quotes, backslashes, control characters, Unicode, long literals, and named
fields.

## Task 7 — Add CLI black-box and hostile-input coverage

Add tests for every acceptance criterion in the design, including:

- all three operation modes;
- inline and template-file sources;
- positional and named templates;
- all template errors;
- legacy compatibility;
- JSONL and lean output;
- output limits and atomic file behavior;
- malformed and oversized template inputs;
- no output-file creation on validation failure.

Use unique temporary paths and clean them on success and failure. Do not make
tests depend on execution order.

## Task 8 — Documentation and help text

Update:

- `README.md` with examples, grammar, names, JSONL shape, limits, and errors;
- generated CLI help descriptions;
- the feature roadmap status, marking F3 implemented only after verification;
- examples for product, zip, and concat.

Document that templates are literal substitution only and cannot execute
commands or access the environment.

## Task 9 — Final verification and review

Run:

```text
cargo test -p combinator-core --locked
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

Review the diff for:

- accidental changes to legacy output;
- underestimated output sizes;
- unbounded parser or renderer allocations;
- diagnostics containing unescaped hostile template content;
- output-file staging before validation;
- accidental shell/environment evaluation;
- test temporary-file collisions or cleanup hazards.

## Definition of done

F3 is complete when:

- the design contract is implemented for product, zip, and concat;
- legacy no-template behavior is unchanged;
- all template errors are stable and machine-readable;
- named JSONL metadata is additive and documented;
- output estimates and runtime limits remain safe;
- black-box and hostile-input tests pass;
- full workspace formatting, tests, and Clippy pass;
- README and CLI help document the feature accurately.

