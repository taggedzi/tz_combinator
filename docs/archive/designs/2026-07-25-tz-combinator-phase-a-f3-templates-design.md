# tz_combinator — Phase A / F3 Design: Templates and Named Fields

> Archived design record. It is retained for historical engineering context,
> not as the current behavior reference.

**Date:** 2026-07-25

**Status:** Proposed design, ready for implementation planning
**Scope:** F3 from the archived
[`feature-roadmap.md`](../plans/feature-roadmap.md) only. This feature adds bounded
templates and optional field names to the existing `product`, `zip`, and
`concat` operations.

## 1. Summary

F3 lets callers control the rendered value of each record without shell glue:

```text
combinator product --list red,blue --list car,bike \
  --template 'https://{0}/{1}'
```

It also lets callers assign names to input fields:

```text
combinator product \
  --name host --name port \
  --list server1,server2 --list 80,443 \
  --template '{host}:{port}' --format jsonl
```

The first implementation deliberately uses a small, non-Turing-complete
grammar. Templates contain literal text and positional or named placeholders;
there are no expressions, loops, conditionals, functions, shell commands,
file reads, or scripting hooks.

Existing invocations without `--template` or `--name` retain their exact text
and JSONL behavior.

## 2. Goals and non-goals

### Goals

- Render values such as URLs, paths, host/port pairs, configuration lines, and
  test-case identifiers directly.
- Support all current operation modes.
- Preserve the existing `fields` array and default JSONL shape.
- Add optional named-field data without breaking existing JSONL consumers.
- Validate templates before input processing or output-file creation.
- Keep rendering deterministic, bounded, and streaming-compatible.

### Non-goals

- General-purpose programming or expression evaluation.
- Regex, conditionals, loops, arithmetic, environment expansion, or command
  substitution.
- CSV/TSV/NUL input and output; those belong to F2.
- A full templating language or compatibility with external template engines.
- Implicit trimming, case conversion, escaping, or normalization; those belong
  to F2/F7.

## 3. CLI contract

Add these options to `product`, `zip`, and `concat`:

```text
--template <TEMPLATE>
--template-file <PATH>
--name <NAME>                 # repeatable, aligned with input-list order
```

Rules:

- `--template` and `--template-file` are mutually exclusive.
- `--template-file` is read as UTF-8 text with the existing input-size and
  item-size security limits applied to the template itself.
- If neither template option is provided, rendering remains unchanged.
- `--name` may be repeated once per input list.
- Names must be valid identifiers: ASCII letter or `_` first, followed by ASCII
  letters, digits, `_`, `-`, or `.`.
- Names must be unique.
- Supplying any names requires exactly one name per input list.
- Supplying no names permits only positional placeholders.
- `--template` and `--template-file` replace the value assembled by `--sep`.
  To avoid silently ignored configuration, passing a non-default `--sep` with
  a template is a usage error (`TEMPLATE_SEPARATOR_CONFLICT`).
- `concat` has one field per record, so `{0}` or its assigned name is valid;
  references to other fields are template errors.

The existing `--sep` remains available when no template is selected. The
existing `--lean-output` behavior remains available in JSONL mode.

## 4. Template grammar

The grammar is intentionally small:

```text
template := piece*
piece    := literal | placeholder
literal  := any character except `{` or `}`
placeholder := `{` reference `}`
reference  := digits | identifier
```

Escapes:

- `{{` emits `{`.
- `}}` emits `}`.

Examples:

```text
{0}@{1}:{2}
https://{host}/{path}
literal {{ brace }}
```

Invalid forms include unmatched braces, empty placeholders, negative
indices, non-decimal positional references, and references to unknown names or
indices.

The parser should produce an immutable compiled template. Rendering walks the
compiled pieces and writes directly to the output record buffer; it should not
reparse the template for every combination.

## 5. Field naming and JSONL output

Names are aligned with list order. For example:

```text
--name host --name port --list server1,server2 --list 80,443
```

Names affect template lookup and, only when explicitly supplied, add a
machine-readable object to full JSONL records:

```json
{"i":0,"value":"server1:80","fields":["server1","80"],"named":{"host":"server1","port":"80"}}
```

Compatibility rules:

- Without names, the existing JSONL object is byte-for-byte unchanged.
- With names, `fields` remains present and ordered exactly as before.
- `named` is emitted in declared name order.
- `--lean-output` continues to emit only the rendered JSON string; it does not
  emit `named` metadata.
- JSON serialization, not the template engine, owns JSON escaping.
- Names such as `i`, `value`, `fields`, and `named` are allowed because they
  live under the `named` object and cannot overwrite structural keys.

Named JSON output is intentionally additive rather than dynamic top-level
keys. This keeps the record schema stable for consumers that parse known
top-level fields.

## 6. Rendering semantics by operation

- `product`: references address the selected item from each product list.
- `zip`: references address the selected item from each positional list.
- `concat`: each record has one field, index `0`; named lookup addresses the
  sole input field name.

The template produces the `value` string. The `fields` array continues to
contain raw selected input items. A template never changes combination count,
ordering, paging, or operation semantics.

## 7. Limits and security

Templates are untrusted input. The implementation must enforce:

- maximum template bytes, using the existing delimiter/item hard ceiling or a
  dedicated lower template ceiling if needed;
- maximum number of parsed pieces/placeholders;
- maximum rendered record bytes through the existing output limit;
- checked arithmetic for estimated output size;
- no recursion, allocation proportional to generated combinations, or dynamic
  code execution.

Template files must be validated before output-file staging. A malformed
template must not create or replace an output file.

The renderer must not use shell expansion, environment variables, filesystem
access, unsafe Rust, dynamic loading, or user-supplied format strings in a
secondary formatter.

## 8. Errors

Add stable usage errors:

| Code | Meaning |
|---|---|
| `TEMPLATE_CONFLICT` | Both `--template` and `--template-file` were provided. |
| `TEMPLATE_SEPARATOR_CONFLICT` | A non-default `--sep` was combined with a template. |
| `TEMPLATE_INVALID` | Template syntax is malformed. |
| `TEMPLATE_UNKNOWN_FIELD` | A placeholder references an unknown index or name. |
| `TEMPLATE_NAMES_MISMATCH` | The number of names does not equal the number of input lists. |
| `TEMPLATE_DUPLICATE_NAME` | A field name was supplied more than once. |
| `TEMPLATE_INVALID_NAME` | A field name does not satisfy the identifier grammar. |
| `TEMPLATE_TOO_LARGE` | The template exceeds its configured security ceiling. |
| `TEMPLATE_FILE_UNREADABLE` | The template file could not be read. |

The exact split between `TEMPLATE_INVALID` and the more specific codes should
be preserved consistently in text and JSON diagnostics. All are usage errors
and exit 2 because they can be discovered before generation.

## 9. Architecture

Add a clap-free template module in `combinator-core`:

```text
Template::parse(source) -> Result<Template, TemplateError>
Template::render(fields, names, output) -> Result<(), TemplateRenderError>
```

The CLI owns:

- parsing `--template`, `--template-file`, and `--name`;
- reading and bounding a template file;
- mapping core template errors to stable diagnostic codes;
- deciding whether the legacy separator path or template path is active.

The formatter should accept a rendering plan so text and JSONL share the same
rendered value. JSONL named metadata should be added to the output formatter
without changing the legacy no-name branch.

Size estimation must use the compiled template and a conservative maximum
rendered record bound. If an exact estimate is impractical, return a safe upper
bound or `Overflow`; never underestimate output.

## 10. Testing strategy

### Core unit tests

- positional and named placeholder parsing;
- escaped braces;
- empty templates and literal-only templates;
- malformed braces and references;
- duplicate/invalid names;
- rendering with Unicode and JSON-hostile characters;
- piece and rendered-size limits;
- no panics on arbitrary template strings.

### CLI black-box tests

- template output for product, zip, and concat;
- template-file output;
- names aligned with fields;
- named JSONL shape and legacy JSONL byte-for-byte compatibility;
- lean JSONL remains a bare rendered string;
- template/separator conflict;
- missing, duplicate, invalid, and mismatched names;
- unknown positional and named references;
- output byte limits after template expansion;
- file-output failure leaves the destination untouched;
- malformed template fails before creating a new output file;
- bare product and explicit product remain equivalent with templates.

### Verification

```text
cargo test -p combinator-core --locked
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```
