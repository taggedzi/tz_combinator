# tz_combinator — Phase A / F1 Design: Explicit Operation Modes (product, zip, concat)

**Date:** 2026-07-25
**Status:** Approved design, ready for implementation planning
**Scope:** F1 from `docs/feature-roadmap.md` only — the `product`/`zip`/`concat` subcommands,
their shared request model, and the CLI restructuring needed to support them. Uses the
*existing* line/inline list reading only (no CSV/TSV, no templates, no dry-run/explain
integration — those are F2/F3/F5, separate specs). `join` (F8) and the public Rust API
(F9) are explicitly out of scope for this document; F9 is out of scope for the project
at this time.

---

## 1. Summary

Phase 1 shipped a single, implicit operation: the ordered Cartesian product. This spec
adds two more explicit operations — `zip` (positional pairing) and `concat` (sequential
concatenation) — as CLI subcommands, alongside an explicit `product` subcommand. Bare
invocations with no subcommand (`combinator --list a --list b`) continue to mean
`product`, identically to today.

Because this project has exactly one user (the author) as of this writing, backward
compatibility with any external caller is not a constraint on *how* this is built — only
on making sure the resulting `product` behavior is unchanged, since it's the baseline
every later feature builds against.

The central engineering move is introducing a mode-neutral request model in
`combinator-core`, per the roadmap's cross-cutting rule that "argument parsing should
produce this model; operation engines should not depend directly on `clap` types." This
requires refactoring the existing `main.rs` request-handling path (currently coupled
directly to the `clap`-derived `Cli` struct) before adding new modes — a real, if
mechanical, change to Phase 1 code, not purely additive work.

---

## 2. Architecture

```
        ┌───────────────────────────────────────────┐
        │  combinator-core (Rust lib)                │
        │  Operation::{Product, Zip, Concat}          │
        │  per-mode: Options, count fn, iterator       │
        └──────────────────────┬──────────────────────┘
                                │ in-process, clap-free
        ┌──────────────────────┴──────────────────────┐
        │  combinator (Rust CLI bin)                   │
        │  clap parses Cli/Mode → (lists, Operation)   │
        │  run()/stream()/estimate() take that pair    │
        └───────────────────────────────────────────────┘
```

`combinator-core` gains a mode-neutral layer sitting alongside the existing
`Product`/`ProductOptions`:

- `enum Operation { Product(ProductOptions), Zip(ZipOptions), Concat(ConcatOptions) }`
- Each mode keeps its own options struct, its own counting function, and its own lazy
  iterator (`ProductOptions`/`combination_count`/`combinations` already exist for
  product; `ZipOptions`/`zip_count`/`zip_records` and `ConcatOptions`/`concat_count`/
  `concat_records` are new).
- A mode-neutral `count(&Operation, &[Vec<String>]) -> Count` and streaming entry point
  dispatch on the `Operation` variant.

`combinator-cli` gains a translation step: parsed CLI args (subcommand + shared
`CommonArgs`) are validated and converted into a plain `(Vec<Vec<String>>, Operation)`
pair — no `clap` types — before calling into `combinator-core`. `main.rs`'s `run()`,
`stream()`, and `bounded_size_estimate()` are refactored to take this pair instead of
`&Cli` directly; only the CLI-args-to-request translation step touches `clap` types.

**Risk note:** this refactor must preserve `product`'s existing behavior exactly. It
lands as its own implementation step, verified by the full existing test suite passing
unchanged, *before* any zip/concat logic is added — so a refactor regression is caught
independently of new-mode bugs.

---

## 3. CLI argument structure

```rust
struct Cli {
    #[command(subcommand)]
    command: Option<Mode>,
    #[command(flatten)]
    product: ProductArgs,   // legacy no-subcommand path == `product`
}

enum Mode {
    Product(ProductArgs),
    Zip(ZipArgs),
    Concat(ConcatArgs),
}

struct CommonArgs {
    list: Vec<String>,
    file: Vec<String>,
    rec_sep: String,
    list_delim: String,
    reverse: bool,
    offset: u128,
    limit: Option<u128>,
    count_only: bool,
    format: OutFormat,
    lean_output: bool,
    output: Option<String>,
    overwrite: bool,
    max_file_size: Option<u64>,
    max_output_bytes: u64,
    max_input_bytes: usize,
    max_item_bytes: usize,
    max_items_per_list: usize,
    max_lists: usize,
    max_total_items: usize,
    max_combinations: u128,
    no_preflight: bool,
}

struct ProductArgs { common: CommonArgs, sep: String, reverse_fields: bool }
struct ZipArgs     { common: CommonArgs, sep: String, on_unequal: UnequalPolicy } // default: Error
struct ConcatArgs  { common: CommonArgs }                                        // no --sep
```

`combinator --list ... --list ...` (no subcommand) and `combinator product --list ...
--list ...` (explicit) use the identical `ProductArgs` struct and produce identical
behavior — there is exactly one code path for product, reached two ways.

**Mode-irrelevant flags are usage errors, enforced by construction.** `--reverse-fields`
only exists on `ProductArgs`; `--on-unequal` only exists on `ZipArgs`; `--sep` exists on
`ProductArgs`/`ZipArgs` but not `ConcatArgs`. Passing any of these to a mode that doesn't
define them is a standard clap "unrecognized argument" usage error (exit 2) — no custom
validation code is needed; the struct shape does the enforcement.

`--sep` is excluded from `concat` specifically because concat records have exactly one
field (see §4) — `--sep` joins fields *within* a record, so it would be silently inert
for concat no matter what value it's given. Excluding it is consistent with treating
mode-irrelevant flags as errors rather than accepting-and-ignoring them.

`--list`/`--file` validation (`SOURCE_CONFLICT`, `NO_LISTS`, `TOO_MANY_LISTS`) remains one
shared function, called identically regardless of mode, since `CommonArgs` carries those
fields the same way for all three subcommands.

---

## 4. Operation semantics

### 4.1 `product` (existing, unchanged)

No behavior change. `ProductOptions { reverse, reverse_fields, offset, limit }`,
`combination_count`, and `combinations`/`Product` are unchanged by this spec; only their
callers move to go through `Operation::Product(...)`.

### 4.2 `zip`

`ZipOptions { on_unequal: UnequalPolicy, reverse: bool, offset: u128, limit: Option<u128> }`

`enum UnequalPolicy { Error, Truncate, Cycle }` — default `Error` when `--on-unequal` is
omitted, per the roadmap's anti-silent-drop principle (`zip` "cannot silently discard
data unless the user selects truncate").

Define `effective_len` over the input list lengths `lens`:
- Any list of length 0 ⇒ `effective_len = 0` regardless of policy (cycling or truncating
  against nothing isn't meaningful) — reuses the existing per-list `EMPTY_LIST` warning.
- `Error`: all lengths must be equal; if not, fail with `ZIP_LENGTH_MISMATCH` (runtime
  class, exit 1 — discovered after reading input, not a static usage error). If equal,
  `effective_len` = that common length.
- `Truncate`: `effective_len = min(lens)`.
- `Cycle`: `effective_len = max(lens)`.

The record at position `i` (`0 <= i < effective_len`) has, for each list `j`, index
`i % lens[j]`. This single formula covers all three policies: under `Error`/`Truncate`,
`i` never reaches `lens[j]`, so the modulo is a no-op; under `Cycle`, it wraps naturally.
`reverse`/`offset`/`limit` window the `0..effective_len` range — simpler than product's
odometer stepping, since it's one linear counter rather than N nested digits.

`zip_count(lens, policy) -> Result<Count, ...>` computes `effective_len` (surfacing the
`Error`-policy mismatch as a distinct outcome from a normal `Count`).

### 4.3 `concat`

`ConcatOptions { reverse: bool, offset: u128, limit: Option<u128> }`

`concat_count(lens)` = checked sum of all list lengths (`Count::Overflow` on overflow,
consistent with product's overflow handling).

The record at global position `i` (`0 <= i < total`) maps to `(list_idx, item_idx)` via a
prefix-sum table over `lens`. Same `reverse`/`offset`/`limit` windowing over `0..total`.

**Arity difference from product/zip:** a concat record contains exactly one field — the
single item at `(list_idx, item_idx)` — not one field per list. This is the reason
`--sep` is excluded (§3) and shapes the output format (§5).

Empty input lists simply contribute 0 items to the concatenated stream (no special-casing
needed beyond what `concat_count`/the prefix-sum table already do naturally); the
existing per-list `EMPTY_LIST` warning still fires.

---

## 5. Output formatting

**JSONL shape stays structurally consistent across modes:**
`{"i": index, "value": <assembled string>, "fields": [...]}`.
- `product`/`zip`: `fields` has one entry per list (as today).
- `concat`: `fields` is a 1-element array containing the single item; `value` equals that
  item (no `--sep` join is ever performed, since there's nothing to join).

**Text mode:** `product`/`zip` join `fields` with `--sep` as today. `concat` emits the
item itself, since a 1-element join with any separator is just the item.

**Estimate-path nuance:** `bounded_size_estimate`'s current worst-case bound formats the
longest item *from each list* (one per list), because a product/zip record draws one
field per list. A concat record only ever contains one field from one list, so its
worst-case bound is the single longest item across *all* input lists, not one-per-list.
The estimator needs a mode-aware branch here — this is a real behavioral difference in
the existing estimate code, not just new code for new modes.

---

## 6. Error handling

**One new error code:** `ZIP_LENGTH_MISMATCH` (runtime class, exit 1) — `zip` under
`on_unequal = Error` with mismatched list lengths.

Everything else reuses existing generic codes (`SOURCE_CONFLICT`, `NO_LISTS`,
`TOO_MANY_LISTS`, `TOO_MANY_ITEMS`, `COMBINATION_LIMIT_EXCEEDED`, `COUNT_OVERFLOW`, etc.),
since those already operate on list counts/lengths generically and just need the new
mode-aware `count()` feeding them. No code's existing meaning changes.

Mode-irrelevant-flag rejection uses clap's standard unrecognized-argument usage error
(exit 2) rather than a custom code, per §3.

---

## 7. Component breakdown

- **`combinator-core`**
  - Adds `Operation`, `ZipOptions`/`zip_count`/`zip_records`, `ConcatOptions`/
    `concat_count`/`concat_records`, alongside the existing `Product` types (unchanged).
  - Still depends on nothing beyond the standard library and minimal utility crates;
    still `clap`-free.
- **`combinator` (CLI bin)**
  - `cli.rs` gains `Mode`, `CommonArgs`, `ProductArgs`, `ZipArgs`, `ConcatArgs` alongside
    (eventually replacing) the current flat `Cli` fields.
  - `main.rs` gains a CLI-args → `(Vec<Vec<String>>, Operation)` translation step;
    `run()`/`stream()`/`bounded_size_estimate()` are refactored to consume that pair.

Boundary check unchanged from Phase 1: a consumer understands the CLI without reading
`combinator-core` internals; the engine's internals can change freely as long as the CLI
contract holds.

---

## 8. Testing strategy

1. **Refactor isolation:** after decoupling `main.rs` from `&Cli` (§2) but before adding
   any zip/concat code, the full existing test suite must pass unchanged. This step is
   verified in isolation so a refactor regression is never conflated with a new-mode bug.
2. **Per-mode black-box tests:** ordering (forward/reverse), empty lists, offset/limit
   windowing, JSONL shape (including concat's 1-element `fields`), output-byte limits.
3. **Zip-specific:** all three `on_unequal` policies, including the empty-list-forces-zero
   case and the cycle-wraps-correctly case (verified against the `i % lens[j]` formula
   directly, not just end-to-end).
4. **Invalid-combination tests:** mode-irrelevant flags rejected per subcommand,
   `SOURCE_CONFLICT`/`NO_LISTS` still enforced per-mode, `ZIP_LENGTH_MISMATCH` triggers
   correctly and only under `Error` policy with actual mismatches.

---

## 9. Scope boundaries

**In scope:** everything in §2–§8 — `product`/`zip`/`concat` subcommands, the shared
request model, the `main.rs` refactor, and the `main.rs`/`cli.rs` restructuring needed to
support it.

**Deferred / out of scope (separate specs):**
- CSV/TSV/NUL/escaped-inline input formats (F2).
- Templates and named-field output (F3).
- Dry-run / explain (F5).
- Pipeline ergonomics: quiet/warnings-as-errors/broken-pipe/completions (F6).
- Distribution/release packaging (F10).
- Keyed relational joins (F8) — deprioritized as a heavy lift beyond current scope, per
  the feature-roadmap discussion preceding this spec; still a desired eventual feature.
- Public in-process Rust API (F9) — explicitly out of the project's current scope; the
  CLI remains the single door to the engine for every consumer.
