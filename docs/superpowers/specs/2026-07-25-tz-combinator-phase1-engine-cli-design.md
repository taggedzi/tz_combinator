# tz_combinator — Phase 1 Design: Core Engine + CLI

**Date:** 2026-07-25
**Status:** Approved design, ready for implementation planning
**Scope:** Phase 1 only (core engine library + CLI). Phases 2–4 (REST API, GUI/TUI, web frontend) are deferred to their own spec → plan → build cycles.

---

## 1. Summary

`tz_combinator` takes any number of ordered text lists and streams every element of their **ordered Cartesian product**, joining the chosen items with a caller-designated separator (including no separator). Output goes to stdout or a file, as plain text or JSON Lines.

The system has one core and one door: a **Rust engine library** wrapped by a **Rust CLI**. The CLI is the *only* interface to the engine. Every other consumer (REST API, GUI, TUI, web frontend, other applications) is a **subprocess consumer of the CLI** — it spawns the binary and reads stdout. This makes all downstream consumers language-agnostic.

Phase 1 delivers the engine and the CLI. Freezing the CLI's input/output contract is the central goal, because that contract is the API everyone else codes against.

---

## 2. Architecture

```
        ┌─────────────────────────────┐
        │  combinator-core (Rust lib) │  ordered Cartesian product,
        │  lists → lazy stream        │  streaming, count, offset/limit,
        │                             │  size estimation
        └──────────────┬──────────────┘
                       │ in-process
        ┌──────────────┴──────────────┐
        │  combinator (Rust CLI bin)  │  THE contract: flags in,
        │  text / JSONL out, stdout   │  formatted stream out
        └──────────────┬──────────────┘
                       │ spawn subprocess, read stdout (text or JSONL)
   ┌───────────────────┼───────────────────┬──────────────────┐
 REST API         GUI / TUI            Web frontend      Other apps
 (Phase 2)        (Phase 3)            (Phase 4)         (external)
```

**Why the engine is a separate library crate** (not baked into the CLI binary): it can be unit-tested directly, and it leaves an in-process path open for any future Rust consumer without a rewrite. The CLI is a thin wrapper over it.

**Why the CLI is the single door:** the tool is stateless, batch-oriented, and streams text — the Unix-pipe sweet spot. A subprocess boundary gives backpressure for free (a slow reader blocks the CLI's next write via the OS pipe), makes cancellation trivial (kill the process), and decouples every consumer's language from the engine's.

---

## 3. Build phases

Each phase is its own spec → plan → implementation cycle. Later phases depend on the Phase 1 contract being frozen.

- **Phase 1 — Core engine + CLI.** This document.
- **Phase 2 — REST API.** Thin HTTP service; translates requests into CLI invocations and streams the CLI's stdout back as the response. Language: Python (marshals calls only).
- **Phase 3 — GUI / TUI.** Python client; spawns the CLI directly (or talks to the Phase 2 REST API). Uses `--count-only` + debounced `--limit`/`--offset` for live, responsive preview without generating everything.
- **Phase 4 — Web frontend.** Browser client of the REST API.

Windows note for later phases: process spawn is heavier on Windows than Linux, so the GUI's live preview must **debounce** and use `--count-only` rather than spawning a CLI per keystroke. Irrelevant to batch runs.

---

## 4. The CLI contract (Phase 1 deliverable)

### 4.1 Combination semantics

- **Ordered Cartesian product**: one item from each list, in list order. `A=[a1,a2]`, `B=[b1,b2]` → `a1|b1, a1|b2, a2|b1, a2|b2`. Count = product of list lengths.
- **Field-varying order**: rightmost list varies fastest by default (conventional). `--reverse` flips it so the leftmost varies fastest.
- **Streaming-first, adaptive**: combinations are generated lazily and streamed; the full set is never held in memory. The total count is closed-form math (product of lengths), so it needs no generation. Small-set conveniences may be added where the total is provably small, but streaming is always the default and the fallback.

### 4.2 Inputs — sources

Input comes from **one source type only** — either inline lists or file lists, never both in the same invocation. This keeps list order (= field order in each combination) unambiguous: lists are consumed in **argument order** within the chosen source. Mixing `--list` and `--file` is a `SOURCE_CONFLICT` usage error; a caller who needs both should convert one form to the other and pass a single homogeneous set.

- **Inline, repeatable:** `--list "red,blue"`. Item delimiter defaults to comma; override with `--list-delim`.
- **Files, repeatable:** `--file a.txt` — each file is one list, **one item per line**. Preferred for large lists (no delimiter ambiguity, pairs with streaming).
- **Stdin (explicit only):** `--file -` reads one list from standard input (one item per line), positioned in file-source order. Stdin is **never** read by autodetection — only when `-` is passed explicitly — so the tool never blocks on, or silently consumes, a caller's stdin.

### 4.3 Inputs — separators

- **Field separator** `--sep`: joins items within one combination. **Default: empty string** (direct concatenation).
- **Record separator** `--rec-sep`: between combinations. **Default: newline** (makes output streamable and pipe-friendly).
- **Inline list delimiter** `--list-delim`: splits `--list` values. **Default: comma.**
- **All delimiters** accept 0…N characters (empty string up to a full string), capped at a practical maximum (a few KB) so a pathological separator cannot exhaust memory. Exceeding the cap → `BAD_DELIMITER` error.

### 4.4 Outputs — destination

- **stdout by default** (streamable, pipe-friendly).
- `--output <file>` / `-o`: stream to a file instead. Never buffers the whole set.
  - **Fails by default if the file exists** → `OUTPUT_EXISTS`, nothing written.
  - **`--overwrite`** (aliases `-f`, `--force`) opts in to replacing it.

### 4.5 Outputs — format

- **Plain text (default):** one combination per record, using the field/record separators.
- **`--format jsonl`:** JSON Lines, one object per line (streams). Default shape:
  ```json
  {"i": 0, "value": "red-car", "fields": ["red", "car"]}
  ```
  `i` = zero-based index, `value` = assembled string, `fields` = the raw picked items.
- **`--lean-output`:** in JSONL mode, emit the value only — leanest/fastest for massive runs.

### 4.6 Outputs — preview & pagination control

- **`--count-only`:** compute and emit only the total (instant, closed-form). Generates nothing. Enables live GUI preview.
- **`--limit N`:** emit at most N combinations.
- **`--offset N`:** skip the first N combinations. `--offset` + `--limit` give pagination over an arbitrarily large set without materializing it.

### 4.7 Pre-flight validation (before any generation to a file)

Runs before streaming starts, so failures are early and cheap. Skipped for stdout output (no file). Bypassable with `--no-preflight`.

- **Output-size estimate**, computed without generating — a closed-form sum over item lengths + separators × combination counts. **Exact for plain-text output.** For JSONL, computed as an estimate that accounts for structural overhead (braces, keys, quotes, indices); it may differ slightly from actual because JSON string escaping is content-dependent, so it is treated as a close upper-bound rather than an exact figure.
- **Disk capacity check:** estimate > free space on target drive → `INSUFFICIENT_SPACE` (reports needed vs. available).
- **Filesystem file-size limit check** (best-effort, e.g. FAT32's 4 GB ceiling): estimate > known limit → `FILE_SIZE_LIMIT`.

---

## 5. Error handling & diagnostics

**Channels.** Data → **stdout only**. Errors, warnings, progress → **stderr only**. stdout stays a clean, parseable data stream.

**Exit codes** (coarse; specifics live in error codes):
- `0` success
- `2` usage / argument error (bad flag, oversized delimiter, no lists)
- `1` runtime error (file not found/unreadable, write failure, insufficient space, etc.)

**Every failure carries three parts**, so both humans and programs can diagnose:
1. **Stable machine-readable error code** — e.g. `OUTPUT_EXISTS`, `EMPTY_LIST`, `BAD_DELIMITER`, `INSUFFICIENT_SPACE`, `FILE_SIZE_LIMIT`, `COUNT_OVERFLOW`, `FILE_UNREADABLE`, `NO_LISTS`, `SOURCE_CONFLICT`. Codes never silently change meaning — they are part of the API.
2. **Plain-language message** — what happened and why.
3. **Context** — which file, which list index, the relevant limits/numbers.

In `--format jsonl`, errors are emitted as a JSON object on stderr so consumers parse them; in text mode, a clean single human-readable line.

**Edge cases**
- **Empty list** (a source with zero items): the Cartesian product is empty → emit nothing, **exit 0**, warn on stderr (`EMPTY_LIST`, naming the list). It is a valid answer, not a crash.
- **No lists at all** → `NO_LISTS`, exit 2.
- **Both `--list` and `--file` given** → `SOURCE_CONFLICT`, exit 2.
- **Count overflow:** compute counts and size estimates in a wide/checked integer; if the total genuinely cannot be represented, `--count-only` reports `COUNT_OVERFLOW` ("too large to count exactly") rather than lying or panicking. Streaming still works — the total is never required to stream.

---

## 6. Defensive programming (cross-cutting quality bar)

Assume deployment in hostile or careless environments.

- **No panics on any input.** Every error path is handled and reported through the model in §5. A panic is a bug; tests assert its absence.
- **Bounded resources.** Streaming means output size never dictates memory. Delimiter length caps and checked/saturating integer arithmetic (counts, size estimates) prevent overflow-driven blowups.
- **Validate at the boundary.** Unreadable/missing files, permission errors, oversized delimiters, and malformed input become clean coded errors, not crashes.
- **Sane defaults throughout.** Empty field separator, newline records, comma inline delimiter, fail-safe on existing output files, pre-flight on by default. The tool does the safe thing unless told otherwise.

---

## 7. Component breakdown

- **`combinator-core` (lib crate)**
  - *Does:* validate parsed lists/separators/options; compute count (closed-form, overflow-safe); estimate output size; produce a lazy iterator/stream of combinations honoring order, `--reverse`, `--offset`, `--limit`.
  - *Interface:* a small public API taking validated inputs and options, returning an iterator plus count/size helpers and a typed error enum.
  - *Depends on:* nothing beyond the standard library and minimal utility crates.
- **`combinator` (CLI bin crate)**
  - *Does:* parse args; gather lists from inline/file/stdin sources; run pre-flight for file output; call the core; format output (text / JSONL / lean); own stdout/stderr channel discipline, exit codes, and the stable error-code mapping.
  - *Interface:* the command-line contract in §4–§5 — the public API for all downstream consumers.
  - *Depends on:* `combinator-core`, an argument parser, filesystem/free-space queries.

Boundary check: a consumer understands the CLI without reading the engine internals; the engine's internals can change freely as long as the CLI contract holds.

---

## 8. Testing strategy

- **Engine unit tests (against the library directly):** product correctness; field-varying order and `--reverse`; `--offset`/`--limit` slicing; count correctness including overflow; exact size estimate; empty-list behavior.
- **CLI black-box tests (invoking the binary):** argument parsing; each input source (inline/file/stdin) and mixing; each output format (text/JSONL/lean); stdout-vs-stderr separation; exit codes; streaming behavior (assert it does not buffer the whole set).
- **Defensive / edge suite:** nonexistent input file; permission-denied output path; existing output file without `--overwrite` (and success with it); oversized delimiter; both `--list` and `--file` given (`SOURCE_CONFLICT`); count-overflow inputs; simulated insufficient space; malformed input — each asserts a clean coded error and a non-panic exit.

---

## 9. Scope boundaries

**In scope (Phase 1):** everything in §4–§8, including `--reverse` (cheap).

**Deferred / out of scope:**
- `--progress` to stderr — nice-to-have, not critical path (consumers can derive progress from `--count-only` + a running record count).
- Permutations and combinations-choose-k modes — Cartesian product only.
- REST API, GUI, TUI, web frontend — Phases 2–4, separate specs.
