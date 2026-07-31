# tz_combinator Phase 1 (Core Engine + CLI) Implementation Plan

> Archived implementation plan. It is retained for historical engineering
> context and should not be treated as current instructions.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust engine library that streams every element of an ordered Cartesian product of text lists, wrapped by a Rust CLI that is the single, defensively-programmed interface to that engine.

**Architecture:** A `combinator-core` library crate owns the combinatorics (count, size estimate, lazy product iterator with order/reverse/offset/limit). A `combinator-cli` binary crate wraps it: it parses args, gathers lists from inline/file/stdin sources, runs pre-flight validation for file output, formats output as plain text or JSON Lines, and owns the stable error-code diagnostics and exit codes. The CLI is the only door to the engine; all future consumers spawn it as a subprocess.

**Tech Stack:** Rust (edition 2021), Cargo workspace, `clap` v4 (arg parsing), `serde_json` v1 (JSONL + JSON error output), `fs2` v0.4 (free-space query). The core crate is std-only.

## Global Constraints

- Rust edition **2021**; minimum toolchain **1.74**.
- Cargo **workspace** with two crates under `crates/`: `combinator-core` (lib) and `combinator-cli` (bin, binary name **`combinator`**).
- `combinator-core` has **no external dependencies** (std only) — keeps the engine trivially testable.
- `combinator-cli` deps: `clap = { version = "4", features = ["derive"] }`, `serde_json = "1"`, `fs2 = "0.4"`, `combinator-core = { path = "../combinator-core" }`.
- **Data → stdout only. Diagnostics (errors/warnings) → stderr only.**
- **Exit codes:** `0` success, `2` usage/argument error, `1` runtime error.
- **Stable error codes are an API contract** — the string codes below never change meaning: `NO_LISTS`, `SOURCE_CONFLICT`, `EMPTY_LIST`, `BAD_DELIMITER`, `OUTPUT_EXISTS`, `INSUFFICIENT_SPACE`, `FILE_SIZE_LIMIT`, `COUNT_OVERFLOW`, `FILE_UNREADABLE`, `WRITE_FAILED`.
- **Input is either `--list` or `--file`, never both** (mixing ⇒ `SOURCE_CONFLICT`, exit 2). Lists are consumed in argument order within the chosen source. Stdin is read **only** when a `--file -` argument is passed explicitly — never autodetected.
- **No panics on any input.** Every error path returns a typed error; a panic is a bug.
- **Delimiter maximum length: 4096 bytes** for `--sep`, `--rec-sep`, `--list-delim`. `--list-delim` must additionally be non-empty (splitting on an empty delimiter is undefined).
- **Defaults:** field separator = empty string; record separator = `\n`; inline list delimiter = `,`; existing output file fails unless `--overwrite`; pre-flight on for file output.
- Every commit message ends with:
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`

---

## File Structure

```
Cargo.toml                                  # workspace manifest
crates/
  combinator-core/
    Cargo.toml
    src/
      lib.rs                                # re-exports; crate docs
      count.rs                              # combination_count, Count
      estimate.rs                           # size estimates, SizeEstimate, SizeInput
      product.rs                            # combinations(), Product iterator, ProductOptions
  combinator-cli/
    Cargo.toml
    src/
      main.rs                               # arg parsing (clap), orchestration, exit-code mapping
      error.rs                              # AppError, codes, text/json rendering
      input.rs                              # gather lists from inline/file/stdin; delimiter validation
      preflight.rs                          # pure output-path + capacity checks
      output.rs                             # formatting (text/jsonl/lean), count-only, streaming run
    tests/
      cli.rs                                # end-to-end black-box tests invoking the binary
```

---

## Task 1: Workspace scaffolding

**Files:**
- Create: `Cargo.toml` (workspace)
- Create: `crates/combinator-core/Cargo.toml`
- Create: `crates/combinator-core/src/lib.rs`
- Create: `crates/combinator-cli/Cargo.toml`
- Create: `crates/combinator-cli/src/main.rs`
- Create: `.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: a compiling workspace; binary `combinator` prints nothing useful yet.

- [ ] **Step 1: Create the workspace manifest**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/combinator-core", "crates/combinator-cli"]
```

- [ ] **Step 2: Create the core crate manifest and lib**

`crates/combinator-core/Cargo.toml`:
```toml
[package]
name = "combinator-core"
version = "0.1.0"
edition = "2021"
rust-version = "1.74"
```

`crates/combinator-core/src/lib.rs`:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.
```

- [ ] **Step 3: Create the CLI crate manifest and main**

`crates/combinator-cli/Cargo.toml`:
```toml
[package]
name = "combinator-cli"
version = "0.1.0"
edition = "2021"
rust-version = "1.74"

[[bin]]
name = "combinator"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde_json = "1"
fs2 = "0.4"
combinator-core = { path = "../combinator-core" }
```

`crates/combinator-cli/src/main.rs`:
```rust
fn main() {
    // Replaced in Task 10 with real orchestration.
    std::process::exit(0);
}
```

- [ ] **Step 4: Create .gitignore**

`.gitignore`:
```
/target
Cargo.lock
```

(Cargo.lock is ignored because this workspace's deliverable is a library + CLI, not a reproducible-pinned application; revisit if we ship binaries.)

- [ ] **Step 5: Verify it builds**

Run: `cargo build`
Expected: compiles with no errors; produces `target/debug/combinator`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates .gitignore
git commit -m "chore: scaffold cargo workspace with core + cli crates

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Core — combination count

**Files:**
- Create: `crates/combinator-core/src/count.rs`
- Modify: `crates/combinator-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum Count { Exact(u128), Overflow }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn combination_count(list_lens: &[usize]) -> Count`
  - Contract: any zero-length list ⇒ `Exact(0)`. Empty slice ⇒ `Exact(1)` (mathematical empty product; the CLI guards `NO_LISTS` before this is reachable). Overflow of `u128` ⇒ `Overflow`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/combinator-core/src/count.rs`:
```rust
//! Overflow-safe combination counting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Exact(u128),
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_of_lengths() {
        assert_eq!(combination_count(&[2, 2]), Count::Exact(4));
        assert_eq!(combination_count(&[3, 4, 5]), Count::Exact(60));
    }

    #[test]
    fn single_list_is_its_length() {
        assert_eq!(combination_count(&[7]), Count::Exact(7));
    }

    #[test]
    fn any_empty_list_is_zero() {
        assert_eq!(combination_count(&[2, 0, 3]), Count::Exact(0));
    }

    #[test]
    fn empty_slice_is_one() {
        assert_eq!(combination_count(&[]), Count::Exact(1));
    }

    #[test]
    fn overflow_reports_overflow() {
        // 20 lists of u32::MAX length overflows u128.
        let lens = vec![u32::MAX as usize; 20];
        assert_eq!(combination_count(&lens), Count::Overflow);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-core count`
Expected: FAIL — `combination_count` not found.

- [ ] **Step 3: Implement `combination_count`**

Insert into `crates/combinator-core/src/count.rs` above the `#[cfg(test)]` block:
```rust
/// Counts the ordered Cartesian product of lists with the given lengths.
///
/// Returns `Exact(0)` if any list is empty, `Exact(1)` for no lists at all
/// (the empty product), and `Overflow` if the true count exceeds `u128`.
pub fn combination_count(list_lens: &[usize]) -> Count {
    let mut acc: u128 = 1;
    for &n in list_lens {
        if n == 0 {
            return Count::Exact(0);
        }
        match acc.checked_mul(n as u128) {
            Some(v) => acc = v,
            None => return Count::Overflow,
        }
    }
    Count::Exact(acc)
}
```

- [ ] **Step 4: Wire the module into lib.rs**

`crates/combinator-core/src/lib.rs`:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;

pub use count::{combination_count, Count};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-core count`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-core
git commit -m "feat(core): overflow-safe combination_count

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Core — product iterator (order, reverse, offset, limit)

**Files:**
- Create: `crates/combinator-core/src/product.rs`
- Modify: `crates/combinator-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing (operates on `&[Vec<String>]`).
- Produces:
  - `pub struct ProductOptions { pub reverse: bool, pub offset: u128, pub limit: Option<u128> }` (derives `Debug, Clone`; `Default` = all false/0/None).
  - `pub struct Product` implementing `Iterator<Item = Vec<usize>>` — each item is one index per list, in list order.
  - `pub fn combinations(lists: &[Vec<String>], opts: ProductOptions) -> Product`
  - Contract: default order increments the **rightmost** list fastest; `reverse` increments the **leftmost** fastest. `offset` skips that many leading combinations (resolved by mixed-radix decomposition, not iteration). `limit` caps emitted count. Any empty list or no lists ⇒ yields nothing.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-core/src/product.rs`:
```rust
//! Lazy ordered Cartesian product as an index-tuple iterator.

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    fn collect(opts: ProductOptions) -> Vec<Vec<usize>> {
        combinations(&lists(), opts).collect()
    }

    #[test]
    fn default_order_rightmost_fastest() {
        assert_eq!(
            collect(ProductOptions::default()),
            vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]]
        );
    }

    #[test]
    fn reverse_order_leftmost_fastest() {
        let opts = ProductOptions { reverse: true, ..Default::default() };
        assert_eq!(
            collect(opts),
            vec![vec![0, 0], vec![1, 0], vec![0, 1], vec![1, 1]]
        );
    }

    #[test]
    fn offset_skips_leading_combinations() {
        let opts = ProductOptions { offset: 2, ..Default::default() };
        assert_eq!(collect(opts), vec![vec![1, 0], vec![1, 1]]);
    }

    #[test]
    fn limit_caps_output() {
        let opts = ProductOptions { limit: Some(1), ..Default::default() };
        assert_eq!(collect(opts), vec![vec![0, 0]]);
    }

    #[test]
    fn offset_and_limit_paginate() {
        let opts = ProductOptions { offset: 1, limit: Some(2), ..Default::default() };
        assert_eq!(collect(opts), vec![vec![0, 1], vec![1, 0]]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let opts = ProductOptions { offset: 99, ..Default::default() };
        assert!(collect(opts).is_empty());
    }

    #[test]
    fn empty_list_yields_nothing() {
        let lists = vec![vec!["a".to_string()], Vec::<String>::new()];
        assert!(combinations(&lists, ProductOptions::default()).next().is_none());
    }

    #[test]
    fn limit_zero_yields_nothing() {
        let opts = ProductOptions { limit: Some(0), ..Default::default() };
        assert!(collect(opts).is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-core product`
Expected: FAIL — `combinations` / `ProductOptions` not found.

- [ ] **Step 3: Implement the iterator**

Insert into `crates/combinator-core/src/product.rs` above the `#[cfg(test)]` block:
```rust
/// Options controlling iteration order and windowing.
#[derive(Debug, Clone)]
pub struct ProductOptions {
    /// When true, the leftmost list varies fastest (default: rightmost fastest).
    pub reverse: bool,
    /// Number of leading combinations to skip.
    pub offset: u128,
    /// Maximum number of combinations to emit.
    pub limit: Option<u128>,
}

impl Default for ProductOptions {
    fn default() -> Self {
        Self { reverse: false, offset: 0, limit: None }
    }
}

/// Lazy iterator over index tuples of the ordered Cartesian product.
pub struct Product {
    lens: Vec<usize>,
    digits: Vec<usize>,
    /// Positions ordered least-significant first.
    lsd_order: Vec<usize>,
    remaining: Option<u128>,
    exhausted: bool,
    started: bool,
}

/// Builds a lazy product iterator over `lists` honoring `opts`.
pub fn combinations(lists: &[Vec<String>], opts: ProductOptions) -> Product {
    let lens: Vec<usize> = lists.iter().map(|l| l.len()).collect();
    let k = lens.len();

    let lsd_order: Vec<usize> = if opts.reverse {
        (0..k).collect()
    } else {
        (0..k).rev().collect()
    };

    let mut exhausted = k == 0 || lens.iter().any(|&n| n == 0);
    let mut digits = vec![0usize; k];

    if !exhausted {
        // Resolve offset by mixed-radix decomposition (no iteration).
        let mut off = opts.offset;
        for &pos in &lsd_order {
            let len = lens[pos] as u128;
            digits[pos] = (off % len) as usize;
            off /= len;
        }
        if off > 0 {
            exhausted = true; // offset past the end of the product
        }
    }

    Product {
        lens,
        digits,
        lsd_order,
        remaining: opts.limit,
        exhausted,
        started: false,
    }
}

impl Iterator for Product {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        if self.exhausted || self.remaining == Some(0) {
            return None;
        }

        if !self.started {
            self.started = true;
        } else {
            // Odometer increment, least-significant position first.
            let mut carry = true;
            for &pos in &self.lsd_order {
                self.digits[pos] += 1;
                if self.digits[pos] < self.lens[pos] {
                    carry = false;
                    break;
                }
                self.digits[pos] = 0;
            }
            if carry {
                self.exhausted = true;
                return None;
            }
        }

        if let Some(r) = self.remaining.as_mut() {
            *r -= 1;
        }
        Some(self.digits.clone())
    }
}
```

- [ ] **Step 4: Wire the module into lib.rs**

`crates/combinator-core/src/lib.rs`:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod product;

pub use count::{combination_count, Count};
pub use product::{combinations, Product, ProductOptions};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-core product`
Expected: PASS (8 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-core
git commit -m "feat(core): lazy product iterator with reverse/offset/limit

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Core — output-size estimation

**Files:**
- Create: `crates/combinator-core/src/estimate.rs`
- Modify: `crates/combinator-core/src/lib.rs`

**Interfaces:**
- Consumes: `combination_count` (Task 2).
- Produces:
  - `pub struct SizeInput<'a> { pub lists: &'a [Vec<String>], pub field_sep_bytes: u64, pub rec_sep_bytes: u64 }`
  - `pub enum SizeEstimate { Bytes(u128), Overflow }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub fn estimate_text_size(input: &SizeInput) -> SizeEstimate` — **exact** byte count of plain-text output.
  - `pub fn estimate_jsonl_size(input: &SizeInput, lean: bool) -> SizeEstimate` — **upper-bound estimate** for JSONL output (ignores content-dependent JSON escaping, so it may be slightly under actual on heavily-escaped content; documented as an estimate).
  - Contract: any empty list ⇒ `Bytes(0)`. `u128` overflow anywhere ⇒ `Overflow`.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-core/src/estimate.rs`:
```rust
//! Output-size estimation, computed from list statistics without generating.

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<Vec<String>> {
        // lens [2,2], item byte-length sums: list0 = "a"+"bb" = 3, list1 = "c"+"d" = 2
        vec![
            vec!["a".to_string(), "bb".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn text_size_is_exact() {
        // 4 combos. item bytes: list0 sum 3 * others 2 = 6; list1 sum 2 * others 2 = 4 -> 10.
        // separators: field_sep 1 byte * (k-1)=1 + rec_sep 1 byte = 2 per record * 4 = 8.
        // total = 18.
        let input = SizeInput { lists: &lists(), field_sep_bytes: 1, rec_sep_bytes: 1 };
        assert_eq!(estimate_text_size(&input), SizeEstimate::Bytes(18));
    }

    #[test]
    fn text_size_empty_list_is_zero() {
        let lists = vec![vec!["a".to_string()], Vec::<String>::new()];
        let input = SizeInput { lists: &lists, field_sep_bytes: 1, rec_sep_bytes: 1 };
        assert_eq!(estimate_text_size(&input), SizeEstimate::Bytes(0));
    }

    #[test]
    fn jsonl_size_is_at_least_text_size() {
        let ls = lists();
        let input = SizeInput { lists: &ls, field_sep_bytes: 1, rec_sep_bytes: 1 };
        let text = match estimate_text_size(&input) { SizeEstimate::Bytes(b) => b, _ => panic!() };
        let json = match estimate_jsonl_size(&input, false) { SizeEstimate::Bytes(b) => b, _ => panic!() };
        assert!(json >= text, "jsonl {json} should be >= text {text}");
    }

    #[test]
    fn overflow_propagates() {
        let lens = vec!["x".to_string(); 2];
        let big = vec![lens; 40]; // 2^40 combos, huge byte total overflow-prone via multiply chain
        let input = SizeInput { lists: &big, field_sep_bytes: 1, rec_sep_bytes: 1 };
        // 2^40 combos * bytes stays within u128, so assert it is Bytes not panic:
        assert!(matches!(estimate_text_size(&input), SizeEstimate::Bytes(_)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-core estimate`
Expected: FAIL — `SizeInput` / `estimate_text_size` not found.

- [ ] **Step 3: Implement the estimators**

Insert into `crates/combinator-core/src/estimate.rs` above the `#[cfg(test)]` block:
```rust
use crate::count::{combination_count, Count};

/// Inputs needed to estimate output size without generating it.
pub struct SizeInput<'a> {
    pub lists: &'a [Vec<String>],
    pub field_sep_bytes: u64,
    pub rec_sep_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeEstimate {
    Bytes(u128),
    Overflow,
}

/// Sum, over every combination, of the byte lengths of the chosen items.
///
/// For position `j`, each item appears in `total / len_j` combinations, so the
/// contribution is `(sum of item byte lengths in list j) * (total / len_j)`.
fn item_bytes_across_combos(lists: &[Vec<String>], total: u128) -> Option<u128> {
    let mut acc: u128 = 0;
    for list in lists {
        let sum_len: u128 = list.iter().map(|s| s.len() as u128).sum();
        let others = total / (list.len() as u128); // list.len() > 0 here (total > 0)
        let contrib = sum_len.checked_mul(others)?;
        acc = acc.checked_add(contrib)?;
    }
    Some(acc)
}

/// Exact byte count of plain-text output.
pub fn estimate_text_size(input: &SizeInput) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(|l| l.len()).collect();
    let total = match combination_count(&lens) {
        Count::Exact(t) => t,
        Count::Overflow => return SizeEstimate::Overflow,
    };
    if total == 0 {
        return SizeEstimate::Bytes(0);
    }
    let k = lens.len() as u128;

    let item_bytes = match item_bytes_across_combos(input.lists, total) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };

    let per_record_sep =
        (input.field_sep_bytes as u128) * k.saturating_sub(1) + input.rec_sep_bytes as u128;
    let sep_bytes = match total.checked_mul(per_record_sep) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };

    match item_bytes.checked_add(sep_bytes) {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

/// Upper-bound estimate for JSON Lines output. Ignores content-dependent JSON
/// string escaping, so treat it as a close estimate, not an exact figure.
pub fn estimate_jsonl_size(input: &SizeInput, lean: bool) -> SizeEstimate {
    let lens: Vec<usize> = input.lists.iter().map(|l| l.len()).collect();
    let total = match combination_count(&lens) {
        Count::Exact(t) => t,
        Count::Overflow => return SizeEstimate::Overflow,
    };
    if total == 0 {
        return SizeEstimate::Bytes(0);
    }
    let k = lens.len() as u128;

    // The assembled `value` string appears once per record; its bytes equal the
    // item bytes plus field separators. `fields` (non-lean) repeats the item
    // bytes a second time, wrapped in quotes and commas.
    let item_bytes = match item_bytes_across_combos(input.lists, total) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    let field_sep_in_value = (input.field_sep_bytes as u128) * k.saturating_sub(1);

    // Index digits: bound by the decimal width of the largest index.
    let index_digits = decimal_width(total.saturating_sub(1)) as u128;

    // Per-record fixed structural bytes.
    // lean:      {"i":<idx>,"value":"<value>"}\n  -> `{"i":`(5) + `,"value":"`(10) + `"}`(2) + `\n`(1) = 18
    // non-lean:  ... + `,"fields":[`(11) + `]`(1) = 12 more, plus 2 quotes + (k-1) commas per record
    let per_record: u128 = if lean { 18 + index_digits } else { 30 + index_digits };

    // Variable (content) bytes. The `value` string appears once (item bytes +
    // field separators). Non-lean repeats item bytes inside `fields`, wrapped in
    // 2 quotes per field and (k-1) commas per record.
    let mut variable = match item_bytes.checked_add(field_sep_in_value) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    if !lean {
        let quote_bytes = match total.checked_mul(2 * k) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        let comma_bytes = match total.checked_mul(k.saturating_sub(1)) {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
        variable = match variable
            .checked_add(item_bytes)
            .and_then(|v| v.checked_add(quote_bytes))
            .and_then(|v| v.checked_add(comma_bytes))
        {
            Some(v) => v,
            None => return SizeEstimate::Overflow,
        };
    }

    let fixed = match total.checked_mul(per_record) {
        Some(v) => v,
        None => return SizeEstimate::Overflow,
    };
    match fixed.checked_add(variable) {
        Some(v) => SizeEstimate::Bytes(v),
        None => SizeEstimate::Overflow,
    }
}

fn decimal_width(mut n: u128) -> u32 {
    if n == 0 {
        return 1;
    }
    let mut w = 0;
    while n > 0 {
        n /= 10;
        w += 1;
    }
    w
}
```

- [ ] **Step 4: Wire the module into lib.rs**

`crates/combinator-core/src/lib.rs`:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod estimate;
pub mod product;

pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use product::{combinations, Product, ProductOptions};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-core estimate`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-core
git commit -m "feat(core): exact text + upper-bound jsonl size estimation

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: CLI — error model

**Files:**
- Create: `crates/combinator-cli/src/error.rs`
- Modify: `crates/combinator-cli/src/main.rs` (declare module)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct AppError { pub code: &'static str, pub message: String, pub context: Vec<(String, String)>, pub exit: i32 }`
  - Constructors: `AppError::usage(code, message)` (exit 2) and `AppError::runtime(code, message)` (exit 1), plus `fn with(mut self, key: &str, value: impl ToString) -> Self` for context.
  - `pub fn render(err: &AppError, json: bool) -> String` — the exact stderr line(s).
  - `pub enum Diagnostic` is **not** needed; warnings reuse `render` with a `warn_*` helper. Provide `pub fn render_warning(code: &str, message: &str, context: &[(String, String)], json: bool) -> String`.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-cli/src/error.rs`:
```rust
//! Stable, machine- and human-readable diagnostics.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_error_has_exit_2() {
        let e = AppError::usage("NO_LISTS", "no input lists were provided");
        assert_eq!(e.exit, 2);
        assert_eq!(e.code, "NO_LISTS");
    }

    #[test]
    fn runtime_error_has_exit_1() {
        let e = AppError::runtime("OUTPUT_EXISTS", "output file already exists");
        assert_eq!(e.exit, 1);
    }

    #[test]
    fn text_render_is_stable() {
        let e = AppError::runtime("OUTPUT_EXISTS", "output file already exists")
            .with("path", "out.txt");
        assert_eq!(
            render(&e, false),
            "error[OUTPUT_EXISTS]: output file already exists (path=out.txt)"
        );
    }

    #[test]
    fn json_render_is_parseable() {
        let e = AppError::runtime("INSUFFICIENT_SPACE", "not enough disk space")
            .with("needed", 100u64)
            .with("available", 40u64);
        let line = render(&e, true);
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["error"]["code"], "INSUFFICIENT_SPACE");
        assert_eq!(v["error"]["context"]["needed"], "100");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli error`
Expected: FAIL — `AppError` not found. (Add `mod error;` to `main.rs` first if the crate won't compile — see Step 4.)

- [ ] **Step 3: Implement the error model**

Insert into `crates/combinator-cli/src/error.rs` above the `#[cfg(test)]` block:
```rust
#[derive(Debug)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub context: Vec<(String, String)>,
    pub exit: i32,
}

impl AppError {
    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), context: Vec::new(), exit: 2 }
    }

    pub fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), context: Vec::new(), exit: 1 }
    }

    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.context.push((key.to_string(), value.to_string()));
        self
    }
}

/// Renders a diagnostic as a single stderr line.
pub fn render(err: &AppError, json: bool) -> String {
    render_line(err.code, &err.message, &err.context, json)
}

/// Renders a non-fatal warning (exit code unaffected).
pub fn render_warning(
    code: &str,
    message: &str,
    context: &[(String, String)],
    json: bool,
) -> String {
    render_line(code, message, context, json)
}

fn render_line(code: &str, message: &str, context: &[(String, String)], json: bool) -> String {
    if json {
        let ctx: serde_json::Map<String, serde_json::Value> = context
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
            .collect();
        let obj = serde_json::json!({
            "error": { "code": code, "message": message, "context": ctx }
        });
        obj.to_string()
    } else if context.is_empty() {
        format!("error[{code}]: {message}")
    } else {
        let ctx = context
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("error[{code}]: {message} ({ctx})")
    }
}
```

- [ ] **Step 4: Declare the module in main.rs**

`crates/combinator-cli/src/main.rs`:
```rust
mod error;

fn main() {
    std::process::exit(0);
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-cli error`
Expected: PASS (4 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): stable text/json error + warning rendering

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: CLI — input gathering & delimiter validation

**Files:**
- Create: `crates/combinator-cli/src/input.rs`
- Modify: `crates/combinator-cli/src/main.rs` (declare module)

**Interfaces:**
- Consumes: `AppError` (Task 5).
- Produces:
  - `pub const MAX_DELIM_BYTES: usize = 4096;`
  - `pub fn validate_delims(field_sep: &str, rec_sep: &str, list_delim: &str) -> Result<(), AppError>` — enforces the 4096-byte cap on all three and non-empty `list_delim`; errors are `BAD_DELIMITER`.
  - `pub fn split_inline(value: &str, delim: &str) -> Vec<String>` — splits an inline `--list` value (delim guaranteed non-empty by `validate_delims`).
  - `pub fn read_file_list(path: &str) -> Result<Vec<String>, AppError>` — one item per line, trailing `\r` stripped; missing/unreadable ⇒ `FILE_UNREADABLE`. **The path `-` reads standard input** (explicit stdin) instead of a file.
  - No stdin-autodetection and no blank-line multi-list parsing: stdin is a single list obtained only via `--file -`.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-cli/src/input.rs`:
```rust
//! Gathering lists from inline, file, and stdin sources; delimiter validation.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_delims() {
        assert!(validate_delims("", "\n", ",").is_ok());
    }

    #[test]
    fn rejects_empty_list_delim() {
        let e = validate_delims("", "\n", "").unwrap_err();
        assert_eq!(e.code, "BAD_DELIMITER");
        assert_eq!(e.exit, 2);
    }

    #[test]
    fn rejects_oversized_delim() {
        let big = "x".repeat(MAX_DELIM_BYTES + 1);
        let e = validate_delims(&big, "\n", ",").unwrap_err();
        assert_eq!(e.code, "BAD_DELIMITER");
    }

    #[test]
    fn splits_inline_on_comma() {
        assert_eq!(split_inline("red,blue,green", ","), vec!["red", "blue", "green"]);
    }

    #[test]
    fn splits_inline_on_custom_delim() {
        assert_eq!(split_inline("a::b", "::"), vec!["a", "b"]);
    }

    #[test]
    fn read_missing_file_errors() {
        let e = read_file_list("does-not-exist-12345.txt").unwrap_err();
        assert_eq!(e.code, "FILE_UNREADABLE");
        assert_eq!(e.exit, 1);
    }

    #[test]
    fn file_lines_strip_crlf() {
        // Written and read back via a temp file.
        let dir = std::env::temp_dir();
        let path = dir.join("combinator_test_crlf.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let got = read_file_list(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli input`
Expected: FAIL — `validate_delims` etc. not found.

- [ ] **Step 3: Implement the input functions**

Insert into `crates/combinator-cli/src/input.rs` above the `#[cfg(test)]` block:
```rust
use crate::error::AppError;

pub const MAX_DELIM_BYTES: usize = 4096;

/// Validates the three delimiters. All three respect the byte cap; the inline
/// list delimiter must additionally be non-empty.
pub fn validate_delims(field_sep: &str, rec_sep: &str, list_delim: &str) -> Result<(), AppError> {
    for (name, d) in [("--sep", field_sep), ("--rec-sep", rec_sep), ("--list-delim", list_delim)] {
        if d.len() > MAX_DELIM_BYTES {
            return Err(AppError::usage(
                "BAD_DELIMITER",
                format!("{name} exceeds the {MAX_DELIM_BYTES}-byte limit"),
            )
            .with("flag", name)
            .with("bytes", d.len()));
        }
    }
    if list_delim.is_empty() {
        return Err(AppError::usage(
            "BAD_DELIMITER",
            "--list-delim must not be empty",
        ));
    }
    Ok(())
}

/// Splits an inline `--list` value on a non-empty delimiter.
pub fn split_inline(value: &str, delim: &str) -> Vec<String> {
    value.split(delim).map(|s| s.to_string()).collect()
}

/// Reads a file as a list, one item per line, stripping a trailing `\r`.
/// The path `-` reads standard input instead (explicit stdin only).
pub fn read_file_list(path: &str) -> Result<Vec<String>, AppError> {
    let content = if path == "-" {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read stdin: {e}"))
                .with("path", "-")
        })?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?
    };
    Ok(split_lines(&content))
}

fn split_lines(content: &str) -> Vec<String> {
    content.lines().map(|l| l.to_string()).collect()
}
```

- [ ] **Step 4: Declare the module in main.rs**

Add `mod input;` below `mod error;` in `crates/combinator-cli/src/main.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-cli input`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): input gathering + delimiter validation

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: CLI — pre-flight checks (pure logic)

**Files:**
- Create: `crates/combinator-cli/src/preflight.rs`
- Modify: `crates/combinator-cli/src/main.rs` (declare module)

**Interfaces:**
- Consumes: `AppError` (Task 5), `SizeEstimate` (Task 4).
- Produces:
  - `pub fn check_output_path(path: &str, overwrite: bool) -> Result<(), AppError>` — `OUTPUT_EXISTS` if the file exists and `overwrite` is false.
  - `pub fn check_capacity(estimate: SizeEstimate, available: u64, fs_max: Option<u64>) -> Result<(), AppError>` — `COUNT_OVERFLOW` if the estimate overflowed (size can't be verified), `INSUFFICIENT_SPACE` if it exceeds `available`, `FILE_SIZE_LIMIT` if it exceeds `fs_max`.
  - These are pure; the OS queries (existence, free space) are performed by the caller in Task 10 and passed in.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-cli/src/preflight.rs`:
```rust
//! Pure pre-flight validation for file output.

#[cfg(test)]
mod tests {
    use super::*;
    use combinator_core::SizeEstimate;

    #[test]
    fn missing_path_is_ok() {
        assert!(check_output_path("definitely-missing-98765.txt", false).is_ok());
    }

    #[test]
    fn existing_path_without_overwrite_errors() {
        let path = std::env::temp_dir().join("combinator_preflight_exists.txt");
        std::fs::write(&path, "x").unwrap();
        let res = check_output_path(path.to_str().unwrap(), false);
        let overwrite_ok = check_output_path(path.to_str().unwrap(), true).is_ok();
        std::fs::remove_file(&path).ok();
        assert_eq!(res.unwrap_err().code, "OUTPUT_EXISTS");
        assert!(overwrite_ok);
    }

    #[test]
    fn fits_when_estimate_below_available() {
        assert!(check_capacity(SizeEstimate::Bytes(100), 1000, None).is_ok());
    }

    #[test]
    fn insufficient_space_errors() {
        let e = check_capacity(SizeEstimate::Bytes(2000), 1000, None).unwrap_err();
        assert_eq!(e.code, "INSUFFICIENT_SPACE");
    }

    #[test]
    fn fs_max_exceeded_errors() {
        let e = check_capacity(SizeEstimate::Bytes(5_000_000_000), u64::MAX, Some(4_294_967_296))
            .unwrap_err();
        assert_eq!(e.code, "FILE_SIZE_LIMIT");
    }

    #[test]
    fn overflow_estimate_cannot_verify() {
        let e = check_capacity(SizeEstimate::Overflow, u64::MAX, None).unwrap_err();
        assert_eq!(e.code, "COUNT_OVERFLOW");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli preflight`
Expected: FAIL — `check_output_path` / `check_capacity` not found.

- [ ] **Step 3: Implement the checks**

Insert into `crates/combinator-cli/src/preflight.rs` above the `#[cfg(test)]` block:
```rust
use crate::error::AppError;
use combinator_core::SizeEstimate;

/// Fails if the output file exists and overwrite was not requested.
pub fn check_output_path(path: &str, overwrite: bool) -> Result<(), AppError> {
    if !overwrite && std::path::Path::new(path).exists() {
        return Err(
            AppError::runtime("OUTPUT_EXISTS", "output file already exists; pass --overwrite to replace it")
                .with("path", path),
        );
    }
    Ok(())
}

/// Verifies the estimated output fits within available space and any filesystem limit.
pub fn check_capacity(
    estimate: SizeEstimate,
    available: u64,
    fs_max: Option<u64>,
) -> Result<(), AppError> {
    let bytes = match estimate {
        SizeEstimate::Bytes(b) => b,
        SizeEstimate::Overflow => {
            return Err(AppError::runtime(
                "COUNT_OVERFLOW",
                "estimated output size is too large to represent; cannot verify capacity (use --no-preflight to bypass)",
            ));
        }
    };

    if let Some(max) = fs_max {
        if bytes > max as u128 {
            return Err(AppError::runtime(
                "FILE_SIZE_LIMIT",
                "estimated output exceeds the filesystem's maximum file size",
            )
            .with("estimated_bytes", bytes)
            .with("limit_bytes", max));
        }
    }

    if bytes > available as u128 {
        return Err(AppError::runtime(
            "INSUFFICIENT_SPACE",
            "estimated output exceeds available disk space",
        )
        .with("estimated_bytes", bytes)
        .with("available_bytes", available));
    }
    Ok(())
}
```

- [ ] **Step 4: Declare the module in main.rs**

Add `mod preflight;` below `mod input;` in `crates/combinator-cli/src/main.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-cli preflight`
Expected: PASS (6 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): pure pre-flight output-path and capacity checks

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: CLI — output formatting

**Files:**
- Create: `crates/combinator-cli/src/output.rs`
- Modify: `crates/combinator-cli/src/main.rs` (declare module)

**Interfaces:**
- Consumes: nothing from other CLI modules; takes plain data.
- Produces:
  - `pub enum Format { Text, Jsonl }`
  - `pub fn format_record(items: &[&str], index: u128, field_sep: &str, rec_sep: &str, format: Format, lean: bool) -> String` — the exact bytes emitted for one combination.
  - Contract: Text ⇒ `items.join(field_sep) + rec_sep`. Jsonl non-lean ⇒ `{"i":<index>,"value":<value>,"fields":[...]}\n`. Jsonl lean ⇒ `<value-as-json-string>\n`. `rec_sep` is ignored in Jsonl mode (records are newline-delimited JSON).

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-cli/src/output.rs`:
```rust
//! Per-record output formatting for text and JSON Lines.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_joins_with_sep_and_rec() {
        assert_eq!(
            format_record(&["red", "car"], 0, "-", "\n", Format::Text, false),
            "red-car\n"
        );
    }

    #[test]
    fn text_empty_sep_concatenates() {
        assert_eq!(
            format_record(&["a", "b"], 0, "", "\n", Format::Text, false),
            "ab\n"
        );
    }

    #[test]
    fn jsonl_full_shape() {
        let line = format_record(&["red", "car"], 3, "-", "\n", Format::Jsonl, false);
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["i"], 3);
        assert_eq!(v["value"], "red-car");
        assert_eq!(v["fields"][0], "red");
        assert_eq!(v["fields"][1], "car");
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn jsonl_lean_is_bare_string() {
        let line = format_record(&["red", "car"], 0, "-", "\n", Format::Jsonl, true);
        assert_eq!(line, "\"red-car\"\n");
    }

    #[test]
    fn jsonl_escapes_quotes() {
        let line = format_record(&["a\"b"], 0, "", "\n", Format::Jsonl, true);
        // Valid JSON string with escaped quote.
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v, "a\"b");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli output`
Expected: FAIL — `format_record` / `Format` not found.

- [ ] **Step 3: Implement formatting**

Insert into `crates/combinator-cli/src/output.rs` above the `#[cfg(test)]` block:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Jsonl,
}

/// Formats one combination into the exact bytes to emit (including the record
/// terminator).
pub fn format_record(
    items: &[&str],
    index: u128,
    field_sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
) -> String {
    let value = items.join(field_sep);
    match format {
        Format::Text => format!("{value}{rec_sep}"),
        Format::Jsonl if lean => {
            let mut s = serde_json::to_string(&value).expect("string is always serializable");
            s.push('\n');
            s
        }
        Format::Jsonl => {
            // index is u128; serde_json numbers are limited, so emit via json! with
            // a number built from string is unsafe. Instead include i as a JSON number
            // only when it fits in u64; otherwise as a string. Indices beyond u64::MAX
            // require > 1.8e19 combinations and are not practically reachable, but we
            // stay correct regardless.
            let i_value: serde_json::Value = if index <= u64::MAX as u128 {
                serde_json::Value::from(index as u64)
            } else {
                serde_json::Value::String(index.to_string())
            };
            let obj = serde_json::json!({
                "i": i_value,
                "value": value,
                "fields": items,
            });
            let mut s = obj.to_string();
            s.push('\n');
            s
        }
    }
}
```

- [ ] **Step 4: Declare the module in main.rs**

Add `mod output;` below `mod preflight;` in `crates/combinator-cli/src/main.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p combinator-cli output`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): text and jsonl record formatting

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: CLI — argument parsing

**Files:**
- Create: `crates/combinator-cli/src/cli.rs`
- Modify: `crates/combinator-cli/src/main.rs` (declare module)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct Cli` (clap `Parser`) with fields:
    - `list: Vec<String>` (`--list`, repeatable)
    - `file: Vec<String>` (`--file`, repeatable)
    - `sep: String` (`--sep`, default `""`)
    - `rec_sep: String` (`--rec-sep`, default `"\n"`)
    - `list_delim: String` (`--list-delim`, default `","`)
    - `reverse: bool` (`--reverse`)
    - `offset: u128` (`--offset`, default 0)
    - `limit: Option<u128>` (`--limit`)
    - `count_only: bool` (`--count-only`)
    - `format: OutFormat` (`--format`, enum `text|jsonl`, default `text`)
    - `lean_output: bool` (`--lean-output`)
    - `output: Option<String>` (`--output`/`-o`)
    - `overwrite: bool` (`--overwrite`, aliases `-f`, `--force`)
    - `max_file_size: Option<u64>` (`--max-file-size`)
    - `no_preflight: bool` (`--no-preflight`)
  - `pub enum OutFormat { Text, Jsonl }` (clap `ValueEnum`)

- [ ] **Step 1: Write the failing test**

Create `crates/combinator-cli/src/cli.rs`:
```rust
//! Command-line argument definitions.

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn defaults_are_sane() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b"]);
        assert_eq!(cli.sep, "");
        assert_eq!(cli.rec_sep, "\n");
        assert_eq!(cli.list_delim, ",");
        assert!(!cli.reverse);
        assert_eq!(cli.offset, 0);
        assert!(cli.limit.is_none());
        assert!(matches!(cli.format, OutFormat::Text));
    }

    #[test]
    fn overwrite_alias_force_works() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "-o", "x.txt", "-f"]);
        assert!(cli.overwrite);
    }

    #[test]
    fn parses_repeated_lists_and_files() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "--list", "b", "--file", "f.txt"]);
        assert_eq!(cli.list, vec!["a", "b"]);
        assert_eq!(cli.file, vec!["f.txt"]);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p combinator-cli cli`
Expected: FAIL — `Cli` not found.

- [ ] **Step 3: Implement the parser**

Insert into `crates/combinator-cli/src/cli.rs` above the `#[cfg(test)]` block:
```rust
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutFormat {
    Text,
    Jsonl,
}

/// Streams the ordered Cartesian product of text lists.
#[derive(Debug, Parser)]
#[command(name = "combinator", version, about)]
pub struct Cli {
    /// Inline list, split by --list-delim. Repeatable; order is field order.
    /// Mutually exclusive with --file.
    #[arg(long)]
    pub list: Vec<String>,

    /// File list, one item per line (path `-` reads stdin). Repeatable; order
    /// is field order. Mutually exclusive with --list.
    #[arg(long)]
    pub file: Vec<String>,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Record separator between combinations (text mode only).
    #[arg(long = "rec-sep", default_value = "\n")]
    pub rec_sep: String,

    /// Delimiter for splitting inline --list values.
    #[arg(long = "list-delim", default_value = ",")]
    pub list_delim: String,

    /// Vary the leftmost list fastest instead of the rightmost.
    #[arg(long)]
    pub reverse: bool,

    /// Skip this many leading combinations.
    #[arg(long, default_value_t = 0)]
    pub offset: u128,

    /// Emit at most this many combinations.
    #[arg(long)]
    pub limit: Option<u128>,

    /// Print only the total count, generating nothing.
    #[arg(long = "count-only")]
    pub count_only: bool,

    /// Output format.
    #[arg(long, value_enum, default_value_t = OutFormat::Text)]
    pub format: OutFormat,

    /// In JSONL mode, emit only the value (as a JSON string) per line.
    #[arg(long = "lean-output")]
    pub lean_output: bool,

    /// Write to this file instead of stdout.
    #[arg(long, short = 'o')]
    pub output: Option<String>,

    /// Overwrite the output file if it exists.
    #[arg(long, visible_alias = "force", short = 'f')]
    pub overwrite: bool,

    /// Optional filesystem max file size (bytes) for pre-flight.
    #[arg(long = "max-file-size")]
    pub max_file_size: Option<u64>,

    /// Skip pre-flight validation for file output.
    #[arg(long = "no-preflight")]
    pub no_preflight: bool,
}
```

- [ ] **Step 4: Declare the module in main.rs**

Add `mod cli;` at the top of `crates/combinator-cli/src/main.rs` with the other module declarations.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p combinator-cli cli`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): clap argument definitions

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 10: CLI — orchestration & end-to-end tests

**Files:**
- Modify: `crates/combinator-cli/src/main.rs` (full orchestration)
- Create: `crates/combinator-cli/tests/cli.rs` (black-box integration tests)

**Interfaces:**
- Consumes: everything from Tasks 5–9.
- Produces: the finished `combinator` binary implementing the full §4–§6 contract.

- [ ] **Step 1: Write the failing end-to-end tests**

Create `crates/combinator-cli/tests/cli.rs`:
```rust
//! Black-box tests invoking the compiled `combinator` binary.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn basic_product_to_stdout() {
    let out = bin()
        .args(["--list", "red,blue", "--list", "car,bike", "--sep", "-"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "red-car\nred-bike\nblue-car\nblue-bike\n"
    );
}

#[test]
fn count_only_prints_total() {
    let out = bin()
        .args(["--list", "a,b", "--list", "c,d,e", "--count-only"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "6");
}

#[test]
fn no_lists_is_usage_error() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("NO_LISTS"), "stderr was: {err}");
}

#[test]
fn mixing_list_and_file_is_source_conflict() {
    let out = bin()
        .args(["--list", "a,b", "--file", "some.txt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("SOURCE_CONFLICT"));
}

#[test]
fn reads_list_from_stdin_dash() {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = bin()
        .args(["--file", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"a\nb\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a\nb\n");
}

#[test]
fn empty_list_warns_and_exits_zero() {
    // An inline empty value produces a single empty item, not an empty list, so
    // use a file with no lines to get a truly empty list.
    let path = std::env::temp_dir().join("combinator_e2e_empty.txt");
    std::fs::write(&path, "").unwrap();
    let out = bin().args(["--file", path.to_str().unwrap()]).output().unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("EMPTY_LIST"));
}

#[test]
fn output_file_exists_without_overwrite_errors() {
    let path = std::env::temp_dir().join("combinator_e2e_exists.txt");
    std::fs::write(&path, "old").unwrap();
    let out = bin()
        .args(["--list", "a,b", "-o", path.to_str().unwrap()])
        .output()
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("OUTPUT_EXISTS"));
    assert_eq!(contents, "old", "existing file must be untouched");
}

#[test]
fn overwrite_writes_file() {
    let path = std::env::temp_dir().join("combinator_e2e_overwrite.txt");
    std::fs::write(&path, "old").unwrap();
    let out = bin()
        .args(["--list", "a,b", "-o", path.to_str().unwrap(), "--overwrite"])
        .output()
        .unwrap();
    let contents = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(out.status.success());
    assert_eq!(contents, "a\nb\n");
}

#[test]
fn jsonl_and_offset_limit() {
    let out = bin()
        .args(["--list", "a,b", "--list", "c,d", "--format", "jsonl", "--offset", "1", "--limit", "2"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["value"], "ad");
    assert_eq!(first["i"], 1);
}

#[test]
fn oversized_delimiter_is_usage_error() {
    let big = "x".repeat(5000);
    let out = bin().args(["--list", "a,b", "--sep", &big]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("BAD_DELIMITER"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli --test cli`
Expected: FAIL — binary still exits 0 doing nothing.

- [ ] **Step 3: Implement orchestration in main.rs**

Replace `crates/combinator-cli/src/main.rs` with:
```rust
mod cli;
mod error;
mod input;
mod output;
mod preflight;

use std::io::{BufWriter, Write};

use clap::Parser;
use combinator_core::{combination_count, combinations, Count, ProductOptions};
use combinator_core::{estimate_jsonl_size, estimate_text_size, SizeInput};

use cli::{Cli, OutFormat};
use error::{render, render_warning, AppError};
use output::{format_record, Format};

fn main() {
    let cli = Cli::parse();
    let json_errors = matches!(cli.format, OutFormat::Jsonl);
    if let Err(e) = run(cli) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(e.exit);
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    input::validate_delims(&cli.sep, &cli.rec_sep, &cli.list_delim)?;

    // Input is either --list or --file, never both. Order within the chosen
    // source is argument order (clap preserves it). `--file -` reads stdin.
    let mut lists: Vec<Vec<String>> = Vec::new();
    match (cli.list.is_empty(), cli.file.is_empty()) {
        (false, false) => {
            return Err(AppError::usage(
                "SOURCE_CONFLICT",
                "use either --list or --file, not both",
            ));
        }
        (true, true) => {
            return Err(AppError::usage("NO_LISTS", "no input lists were provided"));
        }
        (false, true) => {
            for value in &cli.list {
                lists.push(input::split_inline(value, &cli.list_delim));
            }
        }
        (true, false) => {
            for path in &cli.file {
                lists.push(input::read_file_list(path)?);
            }
        }
    }

    let json_out = matches!(cli.format, OutFormat::Jsonl);

    // Warn on any empty list (result will be empty, exit 0).
    for (i, l) in lists.iter().enumerate() {
        if l.is_empty() {
            eprintln!(
                "{}",
                render_warning(
                    "EMPTY_LIST",
                    "a list is empty; zero combinations will be produced",
                    &[("list_index".to_string(), i.to_string())],
                    json_out,
                )
            );
        }
    }

    if cli.count_only {
        let lens: Vec<usize> = lists.iter().map(|l| l.len()).collect();
        match combination_count(&lens) {
            Count::Exact(n) => {
                println!("{n}");
                return Ok(());
            }
            Count::Overflow => {
                return Err(AppError::runtime(
                    "COUNT_OVERFLOW",
                    "the total is too large to count exactly",
                ));
            }
        }
    }

    // Pre-flight for file output.
    if let Some(path) = &cli.output {
        preflight::check_output_path(path, cli.overwrite)?;
        if !cli.no_preflight {
            let estimate = if json_out {
                estimate_jsonl_size(
                    &SizeInput { lists: &lists, field_sep_bytes: cli.sep.len() as u64, rec_sep_bytes: cli.rec_sep.len() as u64 },
                    cli.lean_output,
                )
            } else {
                estimate_text_size(&SizeInput {
                    lists: &lists,
                    field_sep_bytes: cli.sep.len() as u64,
                    rec_sep_bytes: cli.rec_sep.len() as u64,
                })
            };
            let available = available_space(path);
            preflight::check_capacity(estimate, available, cli.max_file_size)?;
        }
    }

    stream(&cli, &lists, json_out)
}

fn stream(cli: &Cli, lists: &[Vec<String>], json_out: bool) -> Result<(), AppError> {
    let opts = ProductOptions { reverse: cli.reverse, offset: cli.offset, limit: cli.limit };
    let format = if json_out { Format::Jsonl } else { Format::Text };

    let mut writer: BufWriter<Box<dyn Write>> = if let Some(path) = &cli.output {
        let file = std::fs::File::create(path).map_err(|e| {
            AppError::runtime("WRITE_FAILED", format!("could not create output file: {e}")).with("path", path)
        })?;
        BufWriter::new(Box::new(file))
    } else {
        BufWriter::new(Box::new(std::io::stdout()))
    };

    for indices in combinations(lists, opts) {
        let index = current_index_placeholder(); // replaced below
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(&items, index, &cli.sep, &cli.rec_sep, format, cli.lean_output);
        writer.write_all(record.as_bytes()).map_err(write_err)?;
    }
    writer.flush().map_err(write_err)?;
    Ok(())
}

fn write_err(e: std::io::Error) -> AppError {
    AppError::runtime("WRITE_FAILED", format!("failed writing output: {e}"))
}

fn available_space(path: &str) -> u64 {
    let dir = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    fs2::available_space(&dir).unwrap_or(u64::MAX)
}

fn current_index_placeholder() -> u128 {
    0
}
```

> Implementer note: the `stream` function needs the running index. Replace the `current_index_placeholder()` call by enumerating: change the loop to `for (index, indices) in (cli.offset..).zip(combinations(lists, opts))` and use `index` directly; delete `current_index_placeholder`. The `combinations` iterator already applies the offset to *which* combinations are produced, and the zipped counter starting at `cli.offset` gives each its true global index `i`. Remove the `let index = ...` line.

- [ ] **Step 4: Apply the index fix from the implementer note**

Edit `stream` so the loop reads:
```rust
    for (index, indices) in (cli.offset..).zip(combinations(lists, opts)) {
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(&items, index, &cli.sep, &cli.rec_sep, format, cli.lean_output);
        writer.write_all(record.as_bytes()).map_err(write_err)?;
    }
```
and delete the `current_index_placeholder` function. (`cli.offset..` is a `RangeFrom<u128>`, so `index` is `u128`.)

- [ ] **Step 5: Run the full test suite to verify it passes**

Run: `cargo test`
Expected: PASS — all core unit tests, all CLI unit tests, and all 10 end-to-end tests pass.

- [ ] **Step 6: Manual smoke check (defensive behavior)**

Run: `cargo run -p combinator-cli -- --list "a,b" --list "c,d" --sep "|"`
Expected stdout:
```
a|c
a|d
b|c
b|d
```
Run: `cargo run -p combinator-cli -- --list "a,b" --count-only`
Expected stdout: `2`

- [ ] **Step 7: Commit**

```bash
git add crates/combinator-cli
git commit -m "feat(cli): full orchestration + end-to-end contract tests

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 11: No-panic guarantee & clippy gate

**Files:**
- Create: `crates/combinator-cli/tests/no_panic.rs`
- Modify: none (verification task)

**Interfaces:**
- Consumes: the finished binary.
- Produces: assurance that hostile/malformed inputs yield coded errors, never panics.

- [ ] **Step 1: Write defensive black-box tests**

Create `crates/combinator-cli/tests/no_panic.rs`:
```rust
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

/// Every listed argument set must exit with a code in {0,1,2} and never crash
/// (a panic aborts with a signal / code 101 and prints a backtrace).
#[test]
fn malformed_inputs_never_panic() {
    let cases: Vec<Vec<&str>> = vec![
        vec!["--file", "/nonexistent/path/nope.txt"],
        vec!["--list", "a,b", "--offset", "999999999999"],
        vec!["--list", "a,b", "--limit", "0"],
        vec!["--list", ""],                       // single empty item, not empty list
        vec!["--list", "a,b", "--list-delim", ""],
        vec!["--list", "a", "--sep", &"x".repeat(9000)],
    ];
    for args in cases {
        let out = bin().args(&args).output().unwrap();
        let code = out.status.code();
        assert!(
            matches!(code, Some(0) | Some(1) | Some(2)),
            "args {args:?} produced code {code:?} (stderr: {})",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains("panicked"),
            "args {args:?} panicked"
        );
    }
}
```

- [ ] **Step 2: Run the defensive tests**

Run: `cargo test -p combinator-cli --test no_panic`
Expected: PASS.

- [ ] **Step 3: Run clippy as a quality gate**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings. Fix any that appear (they are real issues), then re-run.

- [ ] **Step 4: Run the entire suite once more**

Run: `cargo test`
Expected: everything passes.

- [ ] **Step 5: Commit**

```bash
git add crates/combinator-cli
git commit -m "test(cli): no-panic defensive suite + clippy gate

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 12: README documenting the CLI contract

**Files:**
- Create: `README.md`

**Interfaces:**
- Consumes: the finished contract.
- Produces: the human-facing description of the CLI contract that downstream (Phase 2–4) consumers code against.

- [ ] **Step 1: Write the README**

Create `README.md` documenting: purpose; install/build (`cargo build --release`); every flag from Task 9 with its default; the "either `--list` or `--file`, never both" rule and explicit `--file -` stdin; the two output formats with example lines; the stable error-code table (all ten codes with meaning and exit code); the stdout/stderr channel discipline; and a "consuming from other programs" note (spawn the binary, read stdout, parse `--format jsonl`). Include the worked example:
```
$ combinator --list "red,blue" --list "car,bike" --sep "-"
red-car
red-bike
blue-car
blue-bike
```

- [ ] **Step 2: Verify examples against the binary**

Run each example command from the README with `cargo run -p combinator-cli --` and confirm the output matches what the README claims. Fix any mismatch in the README.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document the CLI contract and error codes

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review (completed during authoring)

**Spec coverage** — every spec section maps to a task:
- Combination semantics / ordered Cartesian product → Task 3.
- Streaming-first, adaptive → Task 3 (lazy iterator) + Task 10 (streamed writes, OS-pipe backpressure).
- Input sources (either `--list` or `--file`, argument order, explicit `--file -` stdin, `SOURCE_CONFLICT` on mixing) → Task 6 + Task 10.
- Separators + delimiter caps → Task 6 + Task 9 (defaults).
- `--reverse` → Task 3 + Task 9.
- Output destination + `--output`/`--overwrite` → Task 7 + Task 10.
- Formats (text/jsonl/lean) → Task 8 + Task 9.
- `--count-only`/`--limit`/`--offset` → Task 3 (offset/limit) + Task 9 + Task 10 (count-only).
- Pre-flight (exists, size estimate, disk free, fs limit, `--no-preflight`) → Task 4 + Task 7 + Task 10.
- Error/diagnostic model (codes, exit codes, text/json, stdout/stderr) → Task 5 + Task 10.
- Edge cases (empty list, no lists, count overflow) → Tasks 2, 6, 10.
- Defensive programming / no panics → Task 11.
- Testing strategy (engine unit, CLI black-box, defensive suite) → Tasks 2–11.

**Deferred (correctly out of scope):** `--progress`, permutations/choose-k modes, REST/GUI/web — matching the spec's scope boundaries.

**Placeholder scan:** the one `Implementer note` item (Task 10 index fix — Step 3 uses a placeholder index, Step 4 replaces it with the offset-aligned counter) is a deliberate write-then-fix step with the exact final code given — not an open TODO.

**Type consistency:** `Count`, `SizeEstimate`, `SizeInput`, `ProductOptions`, `AppError`, `Format`, `OutFormat`, `Cli` names and signatures are used identically across the tasks that produce and consume them.
