# Security decisions

This document records security decisions whose rationale is easy to lose when
the implementation or user interface changes. It is intentionally separate
from the operational security guidance so that the reasoning, trade-offs, and
responsibility boundaries remain visible.

## Raw output and terminal controls

**Status:** accepted

**Decision date:** 2026-08-06

### Context

Combinator accepts text that may be supplied by another person or system and
can emit text, CSV, TSV, NUL-delimited, or JSON Lines output. The text and NUL
formatters preserve values verbatim and append delimiters. CSV and TSV quote
their structural fields, but a quoted field can still contain bytes that a
terminal interprets as control sequences. JSON Lines serializes C0 controls
inside JSON strings.

Consequently, a hostile value can have two different effects depending on the
destination:

- In a delimiter-based consumer, a value containing the record delimiter can
  create an apparent additional record.
- On an interactive terminal, ANSI/OSC and related control characters can
  change terminal state or presentation.

The CLI is not a sandbox. A caller who controls the process environment,
command line, shell wrapper, executable, or output pipeline can already choose
another command, redirect output, or write terminal bytes directly. The
utility cannot determine whether a caller's claim that input is trusted is
correct.

### Options considered

1. **Escape every raw output value.** This would make terminal display safer,
   but would change the established text/NUL byte contract and break legitimate
   scripts that intentionally consume exact raw output.
2. **Reject raw formats for every destination.** This would prevent misuse, but
   would unnecessarily block trusted file and pipe workflows, including tools
   that deliberately use delimiter-oriented output.
3. **Rely only on documentation.** This preserves compatibility, but leaves a
   predictable terminal hazard for the common case where a user accidentally
   displays untrusted output.
4. **Guard interactive terminals by default, preserve redirected output, and
   provide an explicit opt-out.** This protects the common accidental-display
   case while retaining trusted use cases.

### Chosen policy

Combinator uses option 4:

- When raw text, CSV, or TSV output is going directly to an interactive
  terminal, the CLI scans normalized values and applicable formatting inputs
  before generation. Control characters cause `UNSAFE_TERMINAL_OUTPUT` before
  any record is written.
- NUL output is rejected for an interactive terminal by default because it is
  a machine-oriented format.
- JSONL remains the recommended format for untrusted records and subprocess
  integration.
- Output redirected to a pipe or file retains the established raw bytes. The
  receiving program is responsible for parsing them safely and must not replay
  hostile raw bytes to a terminal.
- `--allow-unsafe-terminal-output` is an explicit opt-out for trusted, local
  use cases such as terminal-control testing, reproducing an existing byte
  stream, or compatibility with a workflow that intentionally emits raw
  controls. Enabling it means the caller accepts responsibility for the input
  and destination.

### Responsibility boundary

The tool's responsibility is to provide a safe default, make the unsafe mode
deliberate and discoverable, preserve documented compatibility, and explain the
residual risk. The user's responsibility is to verify input provenance and use
JSONL or a suitable parser when processing untrusted records. A user who
intentionally enables the override, or routes raw output through a terminal
writer, has chosen to disable that protection.

This is defense in depth, not a claim that Combinator can secure an untrusted
shell or operating environment. If an attacker controls the invocation or
environment, they can bypass a terminal check with ordinary process and shell
facilities regardless of whether this option exists.

### Review trigger

Revisit this decision if a future release changes the raw output contract,
adds a structured output mode that does not escape terminal controls, or gains
an embedding/service boundary where the caller cannot reliably control the
output destination and input provenance.
