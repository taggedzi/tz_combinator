# tz_combinator Phase A / F1 (product/zip/concat) Implementation Plan

> Archived implementation plan. It is retained for historical engineering
> context and should not be treated as current instructions.

## Historical status (2026-07-31)

This operation-modes checklist is superseded. Product, zip, and concat modes,
their CLI wiring, bounded counts, tests, and documentation are implemented in
the current workspace. The unchecked boxes below preserve the original
design-to-implementation sequence and are not open work items. Any remaining
join or operation work must be tracked separately from this archive.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit `zip` and `concat` operation subcommands alongside the existing (now-explicit) `product` subcommand, behind a shared, clap-free `Operation` request model in `combinator-core`, with zero behavior change to the existing bare-invocation product path.

**Architecture:** `combinator-core` gains `Operation::{Product,Zip,Concat}`, each carrying its own options struct, count function, and lazy index iterator. `combinator-cli`'s `Cli` becomes `{ command: Option<Mode>, #[flatten] product: ProductArgs }` where `Mode` is `Product|Zip|Concat`, each holding a `#[flatten] common: CommonArgs` plus mode-specific flags; mode-irrelevant flags (`--reverse-fields`, `--on-unequal`, `--sep` under `concat`) are rejected by clap itself because they simply don't exist on the irrelevant mode's struct. `main.rs`'s `run()`/`stream()`/`bounded_size_estimate()` are refactored to consume a clap-free `(Vec<Vec<String>>, Operation)` pair instead of `&Cli` directly.

**Tech Stack:** Rust (edition 2021), existing workspace (`combinator-core` std-only lib, `combinator-cli` bin using `clap` v4, `serde_json`, `fs2`, `windows-sys`).

## Global Constraints

- Archived design:
  [`../designs/2026-07-25-tz-combinator-phase-a-f1-operation-modes-design.md`](../designs/2026-07-25-tz-combinator-phase-a-f1-operation-modes-design.md).
  This plan implements that design exactly; do not add scope beyond it (no
  CSV/templates/dry-run/joins/Rust-API work here).
- **Data → stdout only. Diagnostics (errors/warnings) → stderr only.** Unchanged from Phase 1.
- **Exit codes:** `0` success, `2` usage/argument error, `1` runtime error. Unchanged.
- **Stable error codes never change meaning.** This plan adds exactly one new code: `ZIP_LENGTH_MISMATCH` (runtime, exit 1). Every other existing code (`NO_LISTS`, `SOURCE_CONFLICT`, `EMPTY_LIST`, `BAD_DELIMITER`, `RESOURCE_LIMIT_TOO_HIGH`, `OUTPUT_EXISTS`, `INSUFFICIENT_SPACE`, `FILE_SIZE_LIMIT`, `COUNT_OVERFLOW`, `FILE_UNREADABLE`, `INPUT_TOO_LARGE`, `ITEM_TOO_LARGE`, `TOO_MANY_ITEMS`, `TOO_MANY_LISTS`, `COMBINATION_LIMIT_EXCEEDED`, `OUTPUT_LIMIT_EXCEEDED`, `CAPACITY_UNKNOWN`, `UNSAFE_OUTPUT_PATH`, `WRITE_FAILED`) keeps its current meaning and trigger.
- **Bare `combinator --list ... --list ...` (no subcommand) means `product`, byte-identical to today.** `combinator product --list ... --list ...` is the same code path reached a second way.
- **No panics on any input.** Every new error path returns a typed error; `cargo test -p combinator-cli --test no_panic` must keep passing.
- **Checked/saturating arithmetic throughout** — matches the existing style in `count.rs`/`estimate.rs`/`product.rs`.
- Every commit message ends with:
  `Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>`

---

## File Structure

```
crates/
  combinator-core/
    src/
      lib.rs           # modify: re-exports grow each task
      count.rs          # unchanged
      estimate.rs        # unchanged (Task 6 adds a CLI-side branch, not here)
      product.rs         # unchanged
      operation.rs        # create (Task 1): Operation enum, count() dispatcher
      zip.rs               # create (Task 3): UnequalPolicy, ZipOptions, ZipLengthMismatch, zip_count, zip_records, Zip
      concat.rs             # create (Task 5): ConcatOptions, concat_count, concat_records, Concat
  combinator-cli/
    src/
      cli.rs             # modify (Task 2): CommonArgs/ProductArgs/Mode/Cli restructure; (Task 4) ZipArgs; (Task 6) ConcatArgs
      main.rs             # modify (Task 2): Operation-based run()/stream()/bounded_size_estimate(); (Task 4) zip wiring; (Task 6) concat wiring
    tests/
      cli.rs               # unchanged content, must keep passing throughout
      no_panic.rs           # unchanged content, must keep passing throughout
      zip.rs                 # create (Task 4): zip black-box tests
      concat.rs               # create (Task 6): concat black-box tests
README.md                # modify (Task 7): subcommand docs, ZIP_LENGTH_MISMATCH row
```

---

## Task 1: combinator-core — `Operation` enum (Product only)

**Files:**
- Create: `crates/combinator-core/src/operation.rs`
- Modify: `crates/combinator-core/src/lib.rs`

**Interfaces:**
- Consumes: `combination_count`/`Count` (`crates/combinator-core/src/count.rs`, existing), `ProductOptions` (`crates/combinator-core/src/product.rs`, existing).
- Produces:
  - `pub enum Operation { Product(ProductOptions) }` (derives `Debug, Clone`)
  - `pub fn count(op: &Operation, lists: &[Vec<String>]) -> Count`

This task is purely additive — nothing else in the workspace references `operation.rs` yet, so it cannot regress existing behavior. It exists so Task 2 has something to wire into.

- [ ] **Step 1: Write the failing test**

Create `crates/combinator-core/src/operation.rs`:
```rust
//! Mode-neutral operation dispatch over product/zip/concat.

use crate::count::{combination_count, Count};
use crate::product::ProductOptions;

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
}

/// Counts combinations for whichever operation is selected.
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Count {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => combination_count(&lens),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn product_dispatches_to_combination_count() {
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &lists()), Count::Exact(4));
    }

    #[test]
    fn product_any_empty_list_is_zero() {
        let ls = vec![vec!["a".to_string()], Vec::<String>::new()];
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &ls), Count::Exact(0));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p combinator-core operation`
Expected: FAIL — module not wired into `lib.rs` yet (`error[E0433]: failed to resolve: use of undeclared crate or module`).

- [ ] **Step 3: Wire the module into lib.rs**

`crates/combinator-core/src/lib.rs` (add to the existing file — do not remove any current line):
```rust
pub mod operation;

pub use operation::{count as operation_count, Operation};
```
(Full file after this step: the existing four `pub mod`/`pub use` pairs for `count`/`estimate`/`product`, plus these two new lines.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p combinator-core operation`
Expected: PASS (2 tests).

- [ ] **Step 5: Run the full existing test suite to confirm no regression**

Run: `cargo test --workspace --locked`
Expected: PASS, same test count as before this task plus the 2 new ones.

- [ ] **Step 6: Commit**

```bash
git add crates/combinator-core
git commit -m "$(cat <<'EOF'
feat(core): add mode-neutral Operation enum (product only)

Scaffolding for zip/concat: combinator-cli will build (lists, Operation)
instead of depending on clap types directly.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: combinator-cli — decouple from `clap` types (product only, no behavior change)

**Files:**
- Modify: `crates/combinator-cli/src/cli.rs` (full restructure)
- Modify: `crates/combinator-cli/src/main.rs` (full restructure of `run`/`stream`/`bounded_size_estimate`/`validate_resource_limits`)

**Interfaces:**
- Consumes: `Operation`, `operation_count` (Task 1).
- Produces:
  - `pub struct CommonArgs { list, file, rec_sep, list_delim, reverse, offset, limit, count_only, format, lean_output, output, overwrite, max_file_size, max_output_bytes, max_input_bytes, max_item_bytes, max_items_per_list, max_lists, max_total_items, max_combinations, no_preflight }`
  - `pub struct ProductArgs { common: CommonArgs, sep: String, reverse_fields: bool }`
  - `pub enum Mode { Product(ProductArgs) }`
  - `pub struct Cli { command: Option<Mode>, product: ProductArgs }` (`product` is `#[command(flatten)]`)
  - A new private `fn build_request(cli: Cli) -> (ProductArgs's common-shaped access, Operation)` — concretely, `fn resolve(cli: Cli) -> (CommonArgs, Operation)` in `main.rs`, matching on `cli.command` and falling back to `cli.product` when `None`.

This is the highest-risk task in the plan: it must not change any observable behavior of the existing `product` path. It is verified by the full pre-existing test suite (`crates/combinator-cli/tests/cli.rs`, `crates/combinator-cli/tests/no_panic.rs`, plus `cli.rs`'s own unit tests) passing **unchanged** — no test file in this task is edited for anything other than field-access-path fixes forced by the struct reshape.

- [ ] **Step 1: Replace `cli.rs`**

Replace `crates/combinator-cli/src/cli.rs` in full:
```rust
//! Command-line argument definitions.

use clap::{Args, Parser, Subcommand, ValueEnum};

pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1_073_741_824;
pub const DEFAULT_MAX_LISTS: usize = 128;
pub const DEFAULT_MAX_TOTAL_ITEMS: usize = 5_000_000;
pub const DEFAULT_MAX_COMBINATIONS: u128 = 10_000_000;
pub const HARD_MAX_OUTPUT_BYTES: u64 = DEFAULT_MAX_OUTPUT_BYTES;
pub const HARD_MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const HARD_MAX_ITEM_BYTES: usize = 1024 * 1024;
pub const HARD_MAX_ITEMS_PER_LIST: usize = 1_000_000;
pub const HARD_MAX_LISTS: usize = DEFAULT_MAX_LISTS;
pub const HARD_MAX_TOTAL_ITEMS: usize = DEFAULT_MAX_TOTAL_ITEMS;
pub const HARD_MAX_COMBINATIONS: u128 = DEFAULT_MAX_COMBINATIONS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutFormat {
    Text,
    Jsonl,
}

/// Flags shared by every operation mode.
#[derive(Debug, Args)]
pub struct CommonArgs {
    /// Inline list, split by --list-delim. Repeatable; order is field order.
    /// Mutually exclusive with --file.
    #[arg(long)]
    pub list: Vec<String>,

    /// File list, one item per line (path `-` reads stdin). Repeatable; order
    /// is field order. Mutually exclusive with --list.
    #[arg(long)]
    pub file: Vec<String>,

    /// Record separator between combinations (text mode only).
    #[arg(long = "rec-sep", default_value = "\n")]
    pub rec_sep: String,

    /// Delimiter for splitting inline --list values.
    #[arg(long = "list-delim", default_value = ",")]
    pub list_delim: String,

    /// Emit combinations in reverse of the default order.
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

    /// Maximum output bytes for any invocation, including stdout.
    #[arg(long = "max-output-bytes", default_value_t = DEFAULT_MAX_OUTPUT_BYTES)]
    pub max_output_bytes: u64,

    /// Maximum bytes read from each file, stdin stream, or inline list.
    #[arg(long = "max-input-bytes", default_value_t = 64 * 1024 * 1024)]
    pub max_input_bytes: usize,

    /// Maximum UTF-8 bytes in one list item.
    #[arg(long = "max-item-bytes", default_value_t = 1024 * 1024)]
    pub max_item_bytes: usize,

    /// Maximum items accepted from one list.
    #[arg(long = "max-items-per-list", default_value_t = 1_000_000)]
    pub max_items_per_list: usize,

    /// Maximum number of lists accepted.
    #[arg(long = "max-lists", default_value_t = DEFAULT_MAX_LISTS)]
    pub max_lists: usize,

    /// Maximum total items across all lists.
    #[arg(long = "max-total-items", default_value_t = DEFAULT_MAX_TOTAL_ITEMS)]
    pub max_total_items: usize,

    /// Maximum combinations generated unless --count-only is used.
    #[arg(long = "max-combinations", default_value_t = DEFAULT_MAX_COMBINATIONS)]
    pub max_combinations: u128,

    /// Skip pre-flight validation for file output.
    #[arg(long = "no-preflight")]
    pub no_preflight: bool,
}

/// Ordered Cartesian product of the input lists (the default operation).
#[derive(Debug, Args)]
pub struct ProductArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Vary the leftmost list fastest instead of the rightmost.
    #[arg(long = "reverse-fields")]
    pub reverse_fields: bool,
}

#[derive(Debug, Subcommand)]
pub enum Mode {
    /// Ordered Cartesian product (the default when no subcommand is given).
    Product(ProductArgs),
}

/// Streams combinations of text lists: product (default), zip, concat.
#[derive(Debug, Parser)]
#[command(name = "combinator", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Mode>,

    #[command(flatten)]
    pub product: ProductArgs,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn defaults_are_sane() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b"]);
        assert_eq!(cli.product.sep, "");
        assert_eq!(cli.product.common.rec_sep, "\n");
        assert_eq!(cli.product.common.list_delim, ",");
        assert!(!cli.product.common.reverse);
        assert!(!cli.product.reverse_fields);
        assert_eq!(cli.product.common.offset, 0);
        assert!(cli.product.common.limit.is_none());
        assert!(matches!(cli.product.common.format, OutFormat::Text));
        assert!(cli.command.is_none());
    }

    #[test]
    fn overwrite_alias_force_works() {
        let cli = Cli::parse_from(["combinator", "--list", "a", "-o", "x.txt", "-f"]);
        assert!(cli.product.common.overwrite);
    }

    #[test]
    fn parses_repeated_lists_and_files() {
        let cli = Cli::parse_from([
            "combinator",
            "--list",
            "a",
            "--list",
            "b",
            "--file",
            "f.txt",
        ]);
        assert_eq!(cli.product.common.list, vec!["a", "b"]);
        assert_eq!(cli.product.common.file, vec!["f.txt"]);
    }

    #[test]
    fn parses_reverse_modes() {
        let cli = Cli::parse_from(["combinator", "--list", "a,b", "--reverse-fields"]);
        assert!(!cli.product.common.reverse);
        assert!(cli.product.reverse_fields);
    }

    #[test]
    fn explicit_product_subcommand_parses_same_shape() {
        let cli = Cli::parse_from(["combinator", "product", "--list", "a,b", "--sep", "-"]);
        match cli.command {
            Some(Mode::Product(args)) => {
                assert_eq!(args.common.list, vec!["a,b"]);
                assert_eq!(args.sep, "-");
            }
            None => panic!("expected explicit product subcommand to parse"),
        }
    }
}
```

- [ ] **Step 2: Run the cli.rs unit tests to verify they compile against the new shape**

Run: `cargo test -p combinator-cli --lib cli`
Expected: FAIL to compile at first (`main.rs` still references the old flat `cli.list`/`cli.sep`/etc. fields) — this is expected; proceed to Step 3 before re-running.

- [ ] **Step 3: Replace `main.rs`**

Replace `crates/combinator-cli/src/main.rs` in full:
```rust
mod cli;
mod error;
mod input;
mod output;
mod output_file;
mod preflight;

use std::io::{BufWriter, Write};

use clap::Parser;
use combinator_core::{operation_count, Count, Operation, ProductOptions};
use combinator_core::{combinations, estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};

use cli::{
    Cli, CommonArgs, Mode, OutFormat, ProductArgs, HARD_MAX_COMBINATIONS, HARD_MAX_INPUT_BYTES,
    HARD_MAX_ITEMS_PER_LIST, HARD_MAX_ITEM_BYTES, HARD_MAX_LISTS, HARD_MAX_OUTPUT_BYTES,
    HARD_MAX_TOTAL_ITEMS,
};
use error::{render, render_warning, AppError};
use input::{InputBudget, InputLimits};
use output::{format_record, Format};
use output_file::OutputFile;

enum OutputWriter<'a> {
    File(&'a mut std::fs::File),
    Stdout(std::io::Stdout),
}

impl Write for OutputWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::File(file) => file.write(buf),
            Self::Stdout(stdout) => stdout.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::File(file) => file.flush(),
            Self::Stdout(stdout) => stdout.flush(),
        }
    }
}

fn main() {
    let cli = Cli::parse();
    let (common, op) = resolve(cli);
    let json_errors = matches!(common.format, OutFormat::Jsonl);
    if let Err(e) = run(common, op) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(e.exit);
    }
}

/// Reduces the parsed `Cli` (an explicit subcommand, or the legacy bare
/// invocation) to a clap-free `(CommonArgs, Operation)` pair. Everything past
/// this point is clap-agnostic.
fn resolve(cli: Cli) -> (CommonArgs, Operation) {
    match cli.command {
        Some(Mode::Product(args)) => product_operation(args),
        None => product_operation(cli.product),
    }
}

fn product_operation(args: ProductArgs) -> (CommonArgs, Operation) {
    let opts = ProductOptions {
        reverse: args.common.reverse,
        reverse_fields: args.reverse_fields,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, Operation::Product(opts))
}

fn run(common: CommonArgs, op: Operation) -> Result<(), AppError> {
    validate_resource_limits(&common)?;
    let sep = product_sep(&op);
    input::validate_delims(sep, &common.rec_sep, &common.list_delim)?;

    if let Operation::Product(opts) = &op {
        if opts.reverse && opts.reverse_fields {
            return Err(AppError::usage(
                "REVERSE_CONFLICT",
                "use either --reverse or --reverse-fields, not both",
            ));
        }
    }

    // Input is either --list or --file, never both. Order within the chosen
    // source is argument order (clap preserves it). `--file -` reads stdin.
    let mut lists: Vec<Vec<String>> = Vec::new();
    if common.list.len().max(common.file.len()) > common.max_lists {
        return Err(
            AppError::runtime("TOO_MANY_LISTS", "input exceeds the maximum list count")
                .with("observed", common.list.len().max(common.file.len()))
                .with("limit", common.max_lists),
        );
    }
    let input_limits = InputLimits {
        max_input_bytes: common.max_input_bytes,
        max_item_bytes: common.max_item_bytes,
        max_items_per_list: common.max_items_per_list,
    };
    let mut input_budget = InputBudget::new(common.max_input_bytes, common.max_total_items);
    match (common.list.is_empty(), common.file.is_empty()) {
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
            for value in &common.list {
                lists.push(input::split_inline_bounded(
                    value,
                    &common.list_delim,
                    input_limits,
                    &mut input_budget,
                )?);
            }
        }
        (true, false) => {
            for path in &common.file {
                lists.push(input::read_file_list_bounded(
                    path,
                    input_limits,
                    &mut input_budget,
                )?);
            }
        }
    }

    let total_items: usize = lists
        .iter()
        .map(Vec::len)
        .try_fold(0usize, |acc, n| acc.checked_add(n))
        .ok_or_else(|| AppError::runtime("TOO_MANY_ITEMS", "total item count overflowed"))?;
    if total_items > common.max_total_items {
        return Err(AppError::runtime(
            "TOO_MANY_ITEMS",
            "input exceeds the maximum total item count",
        )
        .with("observed", total_items)
        .with("limit", common.max_total_items));
    }

    let json_out = matches!(common.format, OutFormat::Jsonl);

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

    if common.count_only {
        match operation_count(&op, &lists) {
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

    match operation_count(&op, &lists) {
        Count::Exact(total) if common.limit.unwrap_or(total) > common.max_combinations => {
            return Err(AppError::runtime(
                "COMBINATION_LIMIT_EXCEEDED",
                "requested combinations exceed the configured generation limit",
            )
            .with("limit", common.max_combinations));
        }
        Count::Overflow
            if common.limit.is_none() || common.limit.unwrap_or(0) > common.max_combinations =>
        {
            return Err(AppError::runtime(
                "COMBINATION_LIMIT_EXCEEDED",
                "the product is too large without an explicit safe limit",
            )
            .with("limit", common.max_combinations));
        }
        _ => {}
    }

    // Pre-flight for file output.
    if let Some(path) = &common.output {
        preflight::check_output_path(path, common.overwrite)?;
        if !common.no_preflight {
            let estimate = bounded_size_estimate(&common, &op, &lists, json_out);
            let available = available_space(path)?;
            preflight::check_capacity(estimate, available, effective_output_limit(&common))?;
        }
    }

    stream(&common, &op, &lists, json_out)
}

fn product_sep(op: &Operation) -> &str {
    // Only Product exists so far; Task 4 adds Zip's own `sep` field here.
    let Operation::Product(_) = op;
    ""
}

fn validate_resource_limits(common: &CommonArgs) -> Result<(), AppError> {
    let checks = [
        (
            "max-output-bytes",
            common.max_output_bytes as u128,
            HARD_MAX_OUTPUT_BYTES as u128,
        ),
        (
            "max-input-bytes",
            common.max_input_bytes as u128,
            HARD_MAX_INPUT_BYTES as u128,
        ),
        (
            "max-item-bytes",
            common.max_item_bytes as u128,
            HARD_MAX_ITEM_BYTES as u128,
        ),
        (
            "max-items-per-list",
            common.max_items_per_list as u128,
            HARD_MAX_ITEMS_PER_LIST as u128,
        ),
        (
            "max-lists",
            common.max_lists as u128,
            HARD_MAX_LISTS as u128,
        ),
        (
            "max-total-items",
            common.max_total_items as u128,
            HARD_MAX_TOTAL_ITEMS as u128,
        ),
        (
            "max-combinations",
            common.max_combinations,
            HARD_MAX_COMBINATIONS,
        ),
    ];
    for (flag, requested, hard) in checks {
        if requested > hard {
            return Err(AppError::usage(
                "RESOURCE_LIMIT_TOO_HIGH",
                format!("{flag} exceeds the hard security ceiling"),
            )
            .with("flag", flag)
            .with("requested", requested)
            .with("hard_limit", hard));
        }
    }
    if let Some(file_limit) = common.max_file_size {
        if file_limit > HARD_MAX_OUTPUT_BYTES {
            return Err(AppError::usage(
                "RESOURCE_LIMIT_TOO_HIGH",
                "max-file-size exceeds the hard security ceiling",
            )
            .with("flag", "max-file-size")
            .with("requested", file_limit)
            .with("hard_limit", HARD_MAX_OUTPUT_BYTES));
        }
    }
    Ok(())
}

/// Estimates output size accounting for --offset/--limit, so a bounded write is
/// not rejected on the size of the full result. Returns a safe upper bound.
fn bounded_size_estimate(
    common: &CommonArgs,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
) -> SizeEstimate {
    let sep = product_sep(op);
    let input = SizeInput {
        lists,
        field_sep_bytes: sep.len() as u64,
        rec_sep_bytes: common.rec_sep.len() as u64,
    };
    let full = if json_out {
        estimate_jsonl_size(&input, common.lean_output)
    } else {
        estimate_text_size(&input)
    };

    // How many records will actually be written.
    let count: Option<u128> = match operation_count(op, lists) {
        Count::Exact(total) => {
            let remaining = total.saturating_sub(common.offset);
            Some(match common.limit {
                Some(l) => remaining.min(l),
                None => remaining,
            })
        }
        Count::Overflow => common.limit,
    };

    // Per-record upper bound: format the longest-possible record once.
    let format = if json_out {
        Format::Jsonl
    } else {
        Format::Text
    };
    let bounded: Option<u128> = count.and_then(|c| {
        let max_items: Vec<&str> = lists
            .iter()
            .map(|l| {
                l.iter()
                    .max_by_key(|s| {
                        if json_out {
                            serde_json::to_string(s)
                                .map(|v| v.len())
                                .unwrap_or(usize::MAX)
                        } else {
                            s.len()
                        }
                    })
                    .map(String::as_str)
                    .unwrap_or("")
            })
            .collect();
        let max_index = common.offset.saturating_add(c.saturating_sub(1));
        let per_record = format_record(
            &max_items,
            max_index,
            sep,
            &common.rec_sep,
            format,
            common.lean_output,
        )
        .len() as u128;
        c.checked_mul(per_record)
    });

    match (full, bounded) {
        (SizeEstimate::Bytes(f), Some(b)) => SizeEstimate::Bytes(f.min(b)),
        (SizeEstimate::Bytes(f), None) => SizeEstimate::Bytes(f),
        (SizeEstimate::Overflow, Some(b)) => SizeEstimate::Bytes(b),
        (SizeEstimate::Overflow, None) => SizeEstimate::Overflow,
    }
}

fn stream(
    common: &CommonArgs,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
) -> Result<(), AppError> {
    let sep = product_sep(op);
    let format = if json_out {
        Format::Jsonl
    } else {
        Format::Text
    };

    let mut output_file = common
        .output
        .as_deref()
        .map(|path| OutputFile::open(path, common.overwrite))
        .transpose()?;
    let mut writer = match output_file.as_mut() {
        Some(file) => BufWriter::new(OutputWriter::File(file.file_mut())),
        None => BufWriter::new(OutputWriter::Stdout(std::io::stdout())),
    };

    let mut index: u128 = common.offset;
    let output_limit = effective_output_limit(common);
    let mut written: u64 = 0;
    let Operation::Product(opts) = op;
    for indices in combinations(lists, opts.clone()) {
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(
            &items,
            index,
            sep,
            &common.rec_sep,
            format,
            common.lean_output,
        );
        let record_bytes = u64::try_from(record.len()).map_err(|_| {
            AppError::runtime(
                "OUTPUT_LIMIT_EXCEEDED",
                "output record is too large to write",
            )
        })?;
        if let Some(limit) = output_limit {
            let next = written.checked_add(record_bytes).ok_or_else(|| {
                AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
            })?;
            if next > limit {
                return Err(AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output exceeds the configured byte limit",
                )
                .with("written_bytes", written)
                .with("record_bytes", record_bytes)
                .with("limit_bytes", limit));
            }
            written = next;
        }
        writer.write_all(record.as_bytes()).map_err(write_err)?;
        index = index.saturating_add(1);
    }
    writer.flush().map_err(write_err)?;
    drop(writer);
    if let Some(file) = output_file {
        file.commit()?;
    }
    Ok(())
}

fn effective_output_limit(common: &CommonArgs) -> Option<u64> {
    let configured = Some(common.max_output_bytes);
    match (&common.output, common.max_file_size) {
        (Some(_), Some(file_limit)) => configured.map(|limit| limit.min(file_limit)),
        _ => configured,
    }
}

fn write_err(e: std::io::Error) -> AppError {
    AppError::runtime("WRITE_FAILED", format!("failed writing output: {e}"))
}

fn available_space(path: &str) -> Result<u64, AppError> {
    let dir = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    fs2::available_space(&dir).map_err(|e| {
        AppError::runtime(
            "CAPACITY_UNKNOWN",
            format!("could not determine available disk space: {e}"),
        )
        .with("path", dir.display())
    })
}
```

Note the `stream()` function's `let Operation::Product(opts) = op;` irrefutable-pattern line: with only one `Operation` variant it compiles today. Task 4 (adding `Zip`) turns this into a real `match`, which the compiler will force since the pattern becomes refutable — that's an intentional, compiler-enforced reminder to update `stream()` when the second variant is added.

`ProductOptions` needs `Clone` for `opts.clone()` above — confirm `crates/combinator-core/src/product.rs`'s `#[derive(Debug, Clone, Default)]` on `ProductOptions` already includes `Clone` (it does, per the existing file) — no core change needed here.

- [ ] **Step 4: Run the full existing test suite**

Run: `cargo test --workspace --locked`
Expected: PASS — every pre-existing test in `combinator-core`, `combinator-cli --lib`, `combinator-cli --test cli`, and `combinator-cli --test no_panic` passes with **no changes to those test files**, plus the new `explicit_product_subcommand_parses_same_shape` test and Task 1's 2 tests.

- [ ] **Step 5: Run fmt and clippy**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
Expected: both clean. Fix any warnings (e.g. unused imports) before proceeding — do not silence with `#[allow]`.

- [ ] **Step 6: Manually verify `--help` lists the new subcommands**

Run: `cargo run -p combinator-cli -- --help`
Expected: help text shows the legacy `ProductArgs` flags at the top level, and lists `product` as an available subcommand under a "Commands:" section (with only `product` listed for now — `zip`/`concat` appear once Tasks 4/6 land).

- [ ] **Step 7: Commit**

```bash
git add crates/combinator-cli
git commit -m "$(cat <<'EOF'
refactor(cli): decouple main.rs from clap types via Operation

Cli restructured into CommonArgs/ProductArgs/Mode so main.rs's run(),
stream(), and bounded_size_estimate() consume a clap-free
(CommonArgs, Operation) pair. No behavior change: bare invocations and
`combinator product` are the same code path. Verified by the full
pre-existing test suite passing unchanged.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

**This commit is the checkpoint the spec calls out: the refactor must land, green, before any zip/concat logic is added.** Do not start Task 3 until this task's Step 4 passes in full.

---

## Task 3: combinator-core — zip engine

**Files:**
- Create: `crates/combinator-core/src/zip.rs`
- Modify: `crates/combinator-core/src/lib.rs`
- Modify: `crates/combinator-core/src/operation.rs` (add `Zip` variant)

**Interfaces:**
- Consumes: `Count` (existing).
- Produces:
  - `pub enum UnequalPolicy { Error, Truncate, Cycle }` (derives `Debug, Clone, Copy, PartialEq, Eq`)
  - `pub struct ZipOptions { pub on_unequal: UnequalPolicy, pub reverse: bool, pub offset: u128, pub limit: Option<u128> }` (derives `Debug, Clone`; `Default` has `on_unequal: UnequalPolicy::Error`)
  - `pub struct ZipLengthMismatch;` (derives `Debug, Clone, Copy, PartialEq, Eq`) — returned when `UnequalPolicy::Error` meets mismatched non-zero lengths.
  - `pub fn zip_count(lens: &[usize], policy: UnequalPolicy) -> Result<Count, ZipLengthMismatch>`
  - `pub struct Zip` implementing `Iterator<Item = Vec<usize>>` (same shape as `Product`'s items, so `main.rs`'s existing `indices.iter().enumerate()...` record-assembly code works unchanged for zip).
  - `pub fn zip_records(lists: &[Vec<String>], opts: ZipOptions) -> Result<Zip, ZipLengthMismatch>`
  - `Operation::Zip(ZipOptions)` variant added to the enum from Task 1; `operation::count()` gains a matching arm returning `Result<Count, ZipLengthMismatch>` (signature change from Task 1 — see Step 4).

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-core/src/zip.rs`:
```rust
//! Lazy zip (positional pairing) as an index-tuple iterator.

use crate::count::Count;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnequalPolicy {
    Error,
    Truncate,
    Cycle,
}

#[derive(Debug, Clone)]
pub struct ZipOptions {
    pub on_unequal: UnequalPolicy,
    pub reverse: bool,
    pub offset: u128,
    pub limit: Option<u128>,
}

impl Default for ZipOptions {
    fn default() -> Self {
        Self {
            on_unequal: UnequalPolicy::Error,
            reverse: false,
            offset: 0,
            limit: None,
        }
    }
}

/// Returned when `UnequalPolicy::Error` is selected and list lengths differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLengthMismatch;

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(lists: &[Vec<String>], opts: ZipOptions) -> Vec<Vec<usize>> {
        zip_records(lists, opts).unwrap().collect()
    }

    fn lists3x2() -> Vec<Vec<String>> {
        vec![
            vec!["a0".into(), "a1".into()],
            vec!["b0".into(), "b1".into()],
            vec!["c0".into(), "c1".into()],
        ]
    }

    #[test]
    fn equal_lengths_pairs_positionally() {
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        };
        assert_eq!(collect(&lists3x2(), opts), vec![vec![0, 0, 0], vec![1, 1, 1]]);
    }

    #[test]
    fn error_policy_rejects_mismatched_lengths() {
        let lists = vec![vec!["a".into(), "b".into()], vec!["x".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        };
        assert_eq!(zip_records(&lists, opts).unwrap_err(), ZipLengthMismatch);
    }

    #[test]
    fn truncate_uses_shortest_length() {
        let lists = vec![
            vec!["a".into(), "b".into(), "c".into()],
            vec!["x".into(), "y".into()],
        ];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Truncate,
            ..Default::default()
        };
        assert_eq!(collect(&lists, opts), vec![vec![0, 0], vec![1, 1]]);
    }

    #[test]
    fn cycle_wraps_shorter_lists() {
        let lists = vec![
            vec!["a".into(), "b".into(), "c".into()],
            vec!["x".into(), "y".into()],
        ];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Cycle,
            ..Default::default()
        };
        assert_eq!(
            collect(&lists, opts),
            vec![vec![0, 0], vec![1, 1], vec![2, 0]]
        );
    }

    #[test]
    fn any_empty_list_forces_zero_regardless_of_policy() {
        for policy in [UnequalPolicy::Error, UnequalPolicy::Truncate, UnequalPolicy::Cycle] {
            let lists = vec![vec!["a".into()], Vec::<String>::new()];
            let opts = ZipOptions {
                on_unequal: policy,
                ..Default::default()
            };
            assert!(collect(&lists, opts).is_empty());
        }
    }

    #[test]
    fn reverse_offset_and_limit_paginate_from_end() {
        let lists = vec![vec!["a".into(), "b".into(), "c".into(), "d".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            reverse: true,
            offset: 1,
            limit: Some(2),
        };
        assert_eq!(collect(&lists, opts), vec![vec![2], vec![1]]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let lists = vec![vec!["a".into(), "b".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            offset: 99,
            ..Default::default()
        };
        assert!(collect(&lists, opts).is_empty());
    }

    #[test]
    fn zip_count_matches_effective_length() {
        let lens = [3usize, 2];
        assert_eq!(
            zip_count(&lens, UnequalPolicy::Truncate).unwrap(),
            Count::Exact(2)
        );
        assert_eq!(
            zip_count(&lens, UnequalPolicy::Cycle).unwrap(),
            Count::Exact(3)
        );
        assert_eq!(zip_count(&lens, UnequalPolicy::Error).unwrap_err(), ZipLengthMismatch);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-core zip`
Expected: FAIL — `zip_records`/`zip_count`/`Zip` not found.

- [ ] **Step 3: Implement the engine**

Insert into `crates/combinator-core/src/zip.rs`, above the `#[cfg(test)]` block:
```rust
/// The number of records `zip` will produce for `lens` under `policy`.
fn effective_len(lens: &[usize], policy: UnequalPolicy) -> Result<usize, ZipLengthMismatch> {
    if lens.iter().any(|&n| n == 0) {
        return Ok(0);
    }
    match policy {
        UnequalPolicy::Error => {
            let first = lens.first().copied().unwrap_or(0);
            if lens.iter().all(|&n| n == first) {
                Ok(first)
            } else {
                Err(ZipLengthMismatch)
            }
        }
        UnequalPolicy::Truncate => Ok(lens.iter().copied().min().unwrap_or(0)),
        UnequalPolicy::Cycle => Ok(lens.iter().copied().max().unwrap_or(0)),
    }
}

/// Counts the records `zip` will produce for `lens` under `policy`.
pub fn zip_count(lens: &[usize], policy: UnequalPolicy) -> Result<Count, ZipLengthMismatch> {
    effective_len(lens, policy).map(|n| Count::Exact(n as u128))
}

/// Lazy iterator over index tuples of the zip.
pub struct Zip {
    lens: Vec<usize>,
    next_pos: u128,
    remaining: u128,
    descending: bool,
}

/// Builds a lazy zip iterator over `lists` honoring `opts`.
///
/// Fails only under `UnequalPolicy::Error` with mismatched non-zero lengths.
pub fn zip_records(lists: &[Vec<String>], opts: ZipOptions) -> Result<Zip, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    let total = effective_len(&lens, opts.on_unequal)? as u128;

    let available = total.saturating_sub(opts.offset);
    let to_emit = match opts.limit {
        Some(l) => available.min(l),
        None => available,
    };
    let start = if opts.reverse {
        total.saturating_sub(1).saturating_sub(opts.offset)
    } else {
        opts.offset
    };

    Ok(Zip {
        lens,
        next_pos: start,
        remaining: to_emit,
        descending: opts.reverse,
    })
}

impl Iterator for Zip {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        if self.remaining == 0 {
            return None;
        }
        let pos = self.next_pos;
        // Safe: `remaining > 0` implies `total > 0`, which implies every
        // length in `self.lens` is non-zero (see `effective_len`).
        let indices: Vec<usize> = self
            .lens
            .iter()
            .map(|&len| (pos % len as u128) as usize)
            .collect();
        self.remaining -= 1;
        if self.descending {
            self.next_pos = self.next_pos.saturating_sub(1);
        } else {
            self.next_pos = self.next_pos.saturating_add(1);
        }
        Some(indices)
    }
}
```

- [ ] **Step 4: Wire `Zip` into the `Operation` enum**

Replace `crates/combinator-core/src/operation.rs` in full:
```rust
//! Mode-neutral operation dispatch over product/zip/concat.

use crate::count::{combination_count, Count};
use crate::product::ProductOptions;
use crate::zip::{zip_count, ZipLengthMismatch, ZipOptions};

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
    Zip(ZipOptions),
}

/// Counts combinations for whichever operation is selected.
///
/// Only `Zip` under `UnequalPolicy::Error` can fail (mismatched lengths).
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Result<Count, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => Ok(combination_count(&lens)),
        Operation::Zip(opts) => zip_count(&lens, opts.on_unequal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::UnequalPolicy;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn product_dispatches_to_combination_count() {
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(4));
    }

    #[test]
    fn product_any_empty_list_is_zero() {
        let ls = vec![vec!["a".to_string()], Vec::<String>::new()];
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &ls).unwrap(), Count::Exact(0));
    }

    #[test]
    fn zip_dispatches_to_zip_count() {
        let op = Operation::Zip(ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        });
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(2));
    }
}
```

This changes `count()`'s return type from `Count` to `Result<Count, ZipLengthMismatch>` — `combinator-cli` does not yet call `operation_count` with a `Zip` operation (Task 4 adds that), but it already calls `operation_count` for `Product` from Task 2, so this signature change ripples into `main.rs`. Handle that in Step 6 below.

- [ ] **Step 5: Wire the new module into `lib.rs`**

Replace `crates/combinator-core/src/lib.rs` in full:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod estimate;
pub mod operation;
pub mod product;
pub mod zip;

pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use operation::{count as operation_count, Operation};
pub use product::{combinations, Product, ProductOptions};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
```

- [ ] **Step 6: Fix `combinator-cli`'s two `operation_count` call sites for the new `Result` return**

In `crates/combinator-cli/src/main.rs`, `run()` currently has (from Task 2):
```rust
    if common.count_only {
        match operation_count(&op, &lists) {
            Count::Exact(n) => {
```
and
```rust
    match operation_count(&op, &lists) {
        Count::Exact(total) if common.limit.unwrap_or(total) > common.max_combinations => {
```

Replace both `match operation_count(&op, &lists) {` call sites so they handle the `Result`. Change the first occurrence to:
```rust
    if common.count_only {
        match operation_count(&op, &lists) {
            Ok(Count::Exact(n)) => {
                println!("{n}");
                return Ok(());
            }
            Ok(Count::Overflow) => {
                return Err(AppError::runtime(
                    "COUNT_OVERFLOW",
                    "the total is too large to count exactly",
                ));
            }
            Err(combinator_core::ZipLengthMismatch) => {
                return Err(AppError::runtime(
                    "ZIP_LENGTH_MISMATCH",
                    "zip inputs have unequal lengths; pass --on-unequal truncate or cycle",
                ));
            }
        }
    }
```

Change the second occurrence to:
```rust
    let total_for_limits = match operation_count(&op, &lists) {
        Ok(c) => c,
        Err(combinator_core::ZipLengthMismatch) => {
            return Err(AppError::runtime(
                "ZIP_LENGTH_MISMATCH",
                "zip inputs have unequal lengths; pass --on-unequal truncate or cycle",
            ));
        }
    };
    match total_for_limits {
        Count::Exact(total) if common.limit.unwrap_or(total) > common.max_combinations => {
```
(the rest of that `match` block — the `Count::Overflow if ...` arm and the `_ => {}` arm — is unchanged from Task 2, just now matching on `total_for_limits` instead of the raw `operation_count(...)` call).

Also fix `bounded_size_estimate()`'s call site — Task 2 has:
```rust
    let count: Option<u128> = match operation_count(op, lists) {
        Count::Exact(total) => {
```
Change to:
```rust
    let count: Option<u128> = match operation_count(op, lists) {
        Ok(Count::Exact(total)) => {
            let remaining = total.saturating_sub(common.offset);
            Some(match common.limit {
                Some(l) => remaining.min(l),
                None => remaining,
            })
        }
        Ok(Count::Overflow) => common.limit,
        Err(combinator_core::ZipLengthMismatch) => None,
    };
```
(A `ZipLengthMismatch` here is unreachable in practice — `run()` already returns the error before ever calling `bounded_size_estimate`, since the count/limit check above always runs first — but `bounded_size_estimate` is a free function that must still type-check for every case of the `Result`, so it degrades to "no bound available" rather than panicking or unwrapping.)

Also update the `use` line: `use combinator_core::{operation_count, Count, Operation, ProductOptions};` — no change needed to this line itself, since `ZipLengthMismatch` is referenced fully-qualified (`combinator_core::ZipLengthMismatch`) above rather than imported, to keep the diff minimal. (Either is fine; fully-qualified avoids touching the `use` line in three separate spots across this task and Task 4.)

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p combinator-core zip`
Expected: PASS (9 tests).

Run: `cargo test --workspace --locked`
Expected: PASS — all prior tests plus the new ones. `combinator-cli` must compile cleanly against the new `Result`-returning `operation_count`.

- [ ] **Step 8: fmt + clippy**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
Expected: clean.

- [ ] **Step 9: Commit**

```bash
git add crates/combinator-core crates/combinator-cli
git commit -m "$(cat <<'EOF'
feat(core): add zip engine (error/truncate/cycle policies)

zip_records/zip_count share one formula (index % list_len) across all
three unequal-length policies. Operation::Zip added; operation::count()
now returns Result to surface ZipLengthMismatch. combinator-cli's two
call sites updated to map that into the new ZIP_LENGTH_MISMATCH error
code (not yet reachable from the CLI until Task 4 wires the zip
subcommand).

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: combinator-cli — wire the `zip` subcommand

**Files:**
- Modify: `crates/combinator-cli/src/cli.rs` (add `ZipArgs`, `Mode::Zip`)
- Modify: `crates/combinator-cli/src/main.rs` (`resolve`, `product_sep` → mode-aware `sep`, `stream`'s iterator selection)
- Create: `crates/combinator-cli/tests/zip.rs`

**Interfaces:**
- Consumes: `Operation::Zip`, `ZipOptions`, `UnequalPolicy`, `zip_records` (Task 3).
- Produces:
  - `pub struct ZipArgs { common: CommonArgs, sep: String, on_unequal: UnequalPolicyArg }` in `cli.rs`, where `UnequalPolicyArg` is a clap `ValueEnum` mirroring `combinator_core::UnequalPolicy` (clap's derive can't be implemented for a foreign type, so the CLI defines its own enum and converts).
  - `Mode::Zip(ZipArgs)` variant.
  - `combinator zip --list a,b --list c,d [--sep X] [--on-unequal error|truncate|cycle]` end-to-end.

- [ ] **Step 1: Write the failing black-box tests**

Create `crates/combinator-cli/tests/zip.rs`:
```rust
//! Black-box tests for the `zip` subcommand.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn zip_pairs_positionally() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--sep", "-"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\n");
}

#[test]
fn zip_default_policy_is_error() {
    let out = bin()
        .args(["zip", "--list", "a,b,c", "--list", "x,y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ZIP_LENGTH_MISMATCH"));
}

#[test]
fn zip_truncate_uses_shortest() {
    let out = bin()
        .args([
            "zip", "--list", "a,b,c", "--list", "x,y", "--sep", "-", "--on-unequal", "truncate",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\n");
}

#[test]
fn zip_cycle_wraps_shorter_list() {
    let out = bin()
        .args([
            "zip", "--list", "a,b,c", "--list", "x,y", "--sep", "-", "--on-unequal", "cycle",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a-x\nb-y\nc-x\n");
}

#[test]
fn zip_rejects_reverse_fields() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--reverse-fields"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn zip_count_only() {
    let out = bin()
        .args([
            "zip", "--list", "a,b,c", "--list", "x,y", "--on-unequal", "truncate", "--count-only",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2");
}

#[test]
fn zip_jsonl_shape() {
    let out = bin()
        .args(["zip", "--list", "a,b", "--list", "x,y", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["fields"][0], "a");
    assert_eq!(first["fields"][1], "x");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli --test zip`
Expected: FAIL — `zip` is not a recognized subcommand yet (clap usage error, exit 2, on every case).

- [ ] **Step 3: Add `ZipArgs`/`Mode::Zip` to `cli.rs`**

In `crates/combinator-cli/src/cli.rs`, add near the top (after the `use` line, before `CommonArgs`):
```rust
/// CLI-facing mirror of `combinator_core::UnequalPolicy` (clap's `ValueEnum`
/// derive cannot target a foreign type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UnequalPolicyArg {
    Error,
    Truncate,
    Cycle,
}

impl From<UnequalPolicyArg> for combinator_core::UnequalPolicy {
    fn from(value: UnequalPolicyArg) -> Self {
        match value {
            UnequalPolicyArg::Error => combinator_core::UnequalPolicy::Error,
            UnequalPolicyArg::Truncate => combinator_core::UnequalPolicy::Truncate,
            UnequalPolicyArg::Cycle => combinator_core::UnequalPolicy::Cycle,
        }
    }
}
```

Add after `ProductArgs`, before `Mode`:
```rust
/// Positional pairing of the input lists.
#[derive(Debug, Args)]
pub struct ZipArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Field separator joining items within a combination.
    #[arg(long, default_value = "")]
    pub sep: String,

    /// Policy when input lists have unequal lengths.
    #[arg(long = "on-unequal", value_enum, default_value_t = UnequalPolicyArg::Error)]
    pub on_unequal: UnequalPolicyArg,
}
```

Change `Mode` to:
```rust
#[derive(Debug, Subcommand)]
pub enum Mode {
    /// Ordered Cartesian product (the default when no subcommand is given).
    Product(ProductArgs),
    /// Positional pairing of the input lists.
    Zip(ZipArgs),
}
```

Add a test to `cli.rs`'s existing `#[cfg(test)] mod tests` block (alongside `explicit_product_subcommand_parses_same_shape`):
```rust
    #[test]
    fn zip_subcommand_parses_with_default_policy() {
        let cli = Cli::parse_from(["combinator", "zip", "--list", "a,b", "--list", "c,d"]);
        match cli.command {
            Some(Mode::Zip(args)) => {
                assert_eq!(args.on_unequal, UnequalPolicyArg::Error);
                assert_eq!(args.common.list, vec!["a,b", "c,d"]);
            }
            other => panic!("expected zip subcommand, got {other:?}"),
        }
    }

    #[test]
    fn zip_rejects_reverse_fields_flag() {
        let result = Cli::try_parse_from([
            "combinator",
            "zip",
            "--list",
            "a,b",
            "--reverse-fields",
        ]);
        assert!(result.is_err());
    }
```

- [ ] **Step 4: Wire `Zip` into `main.rs`'s `resolve`/`sep`/`stream`**

In `crates/combinator-cli/src/main.rs`:

Change the `use cli::{...}` line to add `ZipArgs`:
```rust
use cli::{
    Cli, CommonArgs, Mode, OutFormat, ProductArgs, ZipArgs, HARD_MAX_COMBINATIONS,
    HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST, HARD_MAX_ITEM_BYTES, HARD_MAX_LISTS,
    HARD_MAX_OUTPUT_BYTES, HARD_MAX_TOTAL_ITEMS,
};
```

Change the `use combinator_core::{...}` lines to add `Zip`/`ZipOptions`/`zip_records`:
```rust
use combinator_core::{operation_count, Count, Operation, ProductOptions, ZipOptions};
use combinator_core::{
    combinations, estimate_jsonl_size, estimate_text_size, zip_records, SizeEstimate, SizeInput,
};
```

Replace `resolve` and add `zip_operation`:
```rust
fn resolve(cli: Cli) -> (CommonArgs, Operation) {
    match cli.command {
        Some(Mode::Product(args)) => product_operation(args),
        Some(Mode::Zip(args)) => zip_operation(args),
        None => product_operation(cli.product),
    }
}

fn zip_operation(args: ZipArgs) -> (CommonArgs, Operation) {
    let opts = ZipOptions {
        on_unequal: args.on_unequal.into(),
        reverse: args.common.reverse,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, Operation::Zip(opts))
}
```

Replace `product_sep`:
```rust
fn sep_for(op: &Operation) -> &str {
    // Set by main() before op is moved into run(); see the `sep` field below.
    match op {
        Operation::Product(_) | Operation::Zip(_) => "",
    }
}
```

Wait — `sep` lives on `ProductArgs`/`ZipArgs`, not inside `ProductOptions`/`ZipOptions` (per the spec's §3 struct layout, `sep` is CLI-only and not part of the engine's options types). So `product_operation`/`zip_operation` must also return the separator alongside `(CommonArgs, Operation)`. Change the tuple shape everywhere it is produced and consumed:

Replace `resolve`, `product_operation`, and `zip_operation` again (superseding the version just above) so the return type carries `sep`:
```rust
fn resolve(cli: Cli) -> (CommonArgs, String, Operation) {
    match cli.command {
        Some(Mode::Product(args)) => product_operation(args),
        Some(Mode::Zip(args)) => zip_operation(args),
        None => product_operation(cli.product),
    }
}

fn product_operation(args: ProductArgs) -> (CommonArgs, String, Operation) {
    let opts = ProductOptions {
        reverse: args.common.reverse,
        reverse_fields: args.reverse_fields,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, args.sep, Operation::Product(opts))
}

fn zip_operation(args: ZipArgs) -> (CommonArgs, String, Operation) {
    let opts = ZipOptions {
        on_unequal: args.on_unequal.into(),
        reverse: args.common.reverse,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, args.sep, Operation::Zip(opts))
}
```

Delete `sep_for` entirely (superseded by threading `sep` explicitly) and update every caller:

`main()`:
```rust
fn main() {
    let cli = Cli::parse();
    let (common, sep, op) = resolve(cli);
    let json_errors = matches!(common.format, OutFormat::Jsonl);
    if let Err(e) = run(common, sep, op) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(e.exit);
    }
}
```

`run()`'s signature and its two uses of `product_sep(&op)`/`sep`:
```rust
fn run(common: CommonArgs, sep: String, op: Operation) -> Result<(), AppError> {
    validate_resource_limits(&common)?;
    input::validate_delims(&sep, &common.rec_sep, &common.list_delim)?;
```
... (body unchanged down to the pre-flight block) ...
```rust
    if let Some(path) = &common.output {
        preflight::check_output_path(path, common.overwrite)?;
        if !common.no_preflight {
            let estimate = bounded_size_estimate(&common, &sep, &op, &lists, json_out);
            let available = available_space(path)?;
            preflight::check_capacity(estimate, available, effective_output_limit(&common))?;
        }
    }

    stream(&common, &sep, &op, &lists, json_out)
}
```

`bounded_size_estimate()`'s signature and its internal `sep` uses (replace `product_sep(op)` with the passed-in `sep: &str` parameter, twice — once building `SizeInput`, once building `max_items`'s `format_record` call):
```rust
fn bounded_size_estimate(
    common: &CommonArgs,
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
) -> SizeEstimate {
    let input = SizeInput {
        lists,
        field_sep_bytes: sep.len() as u64,
        rec_sep_bytes: common.rec_sep.len() as u64,
    };
```
... (unchanged middle) ...
```rust
        let per_record = format_record(
            &max_items,
            max_index,
            sep,
            &common.rec_sep,
            format,
            common.lean_output,
        )
        .len() as u128;
```

`stream()`'s signature, and its iterator selection (this is where `Operation` actually branches on the record-index source — `Product` and `Zip` both yield `Vec<usize>`, so the record-assembly loop body is unchanged, only the source iterator differs):
```rust
fn stream(
    common: &CommonArgs,
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
) -> Result<(), AppError> {
    let format = if json_out {
        Format::Jsonl
    } else {
        Format::Text
    };

    let mut output_file = common
        .output
        .as_deref()
        .map(|path| OutputFile::open(path, common.overwrite))
        .transpose()?;
    let mut writer = match output_file.as_mut() {
        Some(file) => BufWriter::new(OutputWriter::File(file.file_mut())),
        None => BufWriter::new(OutputWriter::Stdout(std::io::stdout())),
    };

    let mut index: u128 = common.offset;
    let output_limit = effective_output_limit(common);
    let mut written: u64 = 0;
    let index_source: Box<dyn Iterator<Item = Vec<usize>>> = match op {
        Operation::Product(opts) => Box::new(combinations(lists, opts.clone())),
        Operation::Zip(opts) => Box::new(
            zip_records(lists, opts.clone()).map_err(|_| {
                AppError::runtime(
                    "ZIP_LENGTH_MISMATCH",
                    "zip inputs have unequal lengths; pass --on-unequal truncate or cycle",
                )
            })?,
        ),
    };
    for indices in index_source {
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(&items, index, sep, &common.rec_sep, format, common.lean_output);
        let record_bytes = u64::try_from(record.len()).map_err(|_| {
            AppError::runtime(
                "OUTPUT_LIMIT_EXCEEDED",
                "output record is too large to write",
            )
        })?;
        if let Some(limit) = output_limit {
            let next = written.checked_add(record_bytes).ok_or_else(|| {
                AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
            })?;
            if next > limit {
                return Err(AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output exceeds the configured byte limit",
                )
                .with("written_bytes", written)
                .with("record_bytes", record_bytes)
                .with("limit_bytes", limit));
            }
            written = next;
        }
        writer.write_all(record.as_bytes()).map_err(write_err)?;
        index = index.saturating_add(1);
    }
    writer.flush().map_err(write_err)?;
    drop(writer);
    if let Some(file) = output_file {
        file.commit()?;
    }
    Ok(())
}
```

`ZipOptions` needs `Clone` for `opts.clone()` above — confirm Task 3's `#[derive(Debug, Clone)]` on `ZipOptions` already covers this (it does).

Also fix the `REVERSE_CONFLICT` check in `run()`, which currently pattern-matches `if let Operation::Product(opts) = &op` — this still compiles unchanged (an `if let` stays valid with more enum variants; it simply never matches `Zip`), so no edit needed there.

- [ ] **Step 5: Run tests**

Run: `cargo test -p combinator-cli --test zip`
Expected: PASS (7 tests).

Run: `cargo test --workspace --locked`
Expected: PASS — no regressions in `tests/cli.rs`, `tests/no_panic.rs`, or any `--lib` tests.

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
Expected: clean. (The `Box<dyn Iterator<...>>` in `stream()` is the simplest fix for unifying two different iterator types across a `match`; if clippy flags it, `needless_return`-style lints don't apply here — leave the boxed-iterator approach as is rather than introducing an enum-of-iterators, which is unnecessary complexity for two variants growing to three in Task 6.)

- [ ] **Step 7: Commit**

```bash
git add crates/combinator-cli
git commit -m "$(cat <<'EOF'
feat(cli): wire the zip subcommand

combinator zip --list ... --list ... [--on-unequal error|truncate|cycle]
--reverse-fields is rejected at parse time (ZipArgs has no such field).
ZIP_LENGTH_MISMATCH is now reachable end-to-end.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: combinator-core — concat engine

**Files:**
- Create: `crates/combinator-core/src/concat.rs`
- Modify: `crates/combinator-core/src/lib.rs`
- Modify: `crates/combinator-core/src/operation.rs` (add `Concat` variant)

**Interfaces:**
- Consumes: `Count` (existing).
- Produces:
  - `pub struct ConcatOptions { pub reverse: bool, pub offset: u128, pub limit: Option<u128> }` (derives `Debug, Clone`; `Default` = all false/0/None)
  - `pub fn concat_count(lens: &[usize]) -> Count`
  - `pub struct Concat` implementing `Iterator<Item = (usize, usize)>` — `(list_index, item_index)`, **not** `Vec<usize>` (arity-1 records; see spec §4.3).
  - `pub fn concat_records(lists: &[Vec<String>], opts: ConcatOptions) -> Option<Concat>` — `None` only if the checked sum of all lengths overflows `u128` (structurally unreachable given the CLI's pre-existing `max_total_items` ceiling, but kept correct per the project's checked-arithmetic discipline).
  - `Operation::Concat(ConcatOptions)` variant; `operation::count()` gains a matching arm.

- [ ] **Step 1: Write the failing tests**

Create `crates/combinator-core/src/concat.rs`:
```rust
//! Lazy concat (sequential emission) as a (list, item) index iterator.

use crate::count::Count;

#[derive(Debug, Clone)]
pub struct ConcatOptions {
    pub reverse: bool,
    pub offset: u128,
    pub limit: Option<u128>,
}

impl Default for ConcatOptions {
    fn default() -> Self {
        Self {
            reverse: false,
            offset: 0,
            limit: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(lists: &[Vec<String>], opts: ConcatOptions) -> Vec<(usize, usize)> {
        concat_records(lists, opts).unwrap().collect()
    }

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a0".into(), "a1".into()],
            vec!["b0".into()],
            vec!["c0".into(), "c1".into(), "c2".into()],
        ]
    }

    #[test]
    fn emits_every_list_in_order() {
        assert_eq!(
            collect(&lists(), ConcatOptions::default()),
            vec![
                (0, 0),
                (0, 1),
                (1, 0),
                (2, 0),
                (2, 1),
                (2, 2),
            ]
        );
    }

    #[test]
    fn empty_lists_contribute_nothing() {
        let ls = vec![Vec::<String>::new(), vec!["x".into()], Vec::<String>::new()];
        assert_eq!(collect(&ls, ConcatOptions::default()), vec![(1, 0)]);
    }

    #[test]
    fn offset_and_limit_paginate() {
        let opts = ConcatOptions {
            offset: 2,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(&lists(), opts), vec![(1, 0), (2, 0)]);
    }

    #[test]
    fn reverse_walks_from_the_end() {
        let opts = ConcatOptions {
            reverse: true,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(&lists(), opts), vec![(2, 2), (2, 1)]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let opts = ConcatOptions {
            offset: 99,
            ..Default::default()
        };
        assert!(collect(&lists(), opts).is_empty());
    }

    #[test]
    fn concat_count_is_checked_sum() {
        let lens = [2usize, 1, 3];
        assert_eq!(concat_count(&lens), Count::Exact(6));
    }

    #[test]
    fn concat_count_overflow_reports_overflow() {
        let lens = vec![usize::MAX; 3];
        assert_eq!(concat_count(&lens), Count::Overflow);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-core concat`
Expected: FAIL — `concat_records`/`concat_count`/`Concat` not found.

- [ ] **Step 3: Implement the engine**

Insert into `crates/combinator-core/src/concat.rs`, above the `#[cfg(test)]` block:
```rust
/// Counts the records `concat` will produce for `lens` (checked sum).
pub fn concat_count(lens: &[usize]) -> Count {
    let mut acc: u128 = 0;
    for &n in lens {
        match acc.checked_add(n as u128) {
            Some(v) => acc = v,
            None => return Count::Overflow,
        }
    }
    Count::Exact(acc)
}

/// Lazy iterator over `(list_index, item_index)` pairs of the concatenation.
pub struct Concat {
    /// Prefix sums: `prefix[j]` = sum of lengths of lists `0..j`. Length is
    /// `lens.len() + 1`; `prefix[lens.len()]` is the grand total.
    prefix: Vec<u128>,
    next_pos: u128,
    remaining: u128,
    descending: bool,
}

/// Builds a lazy concat iterator over `lists` honoring `opts`.
///
/// Returns `None` only if the checked sum of all list lengths overflows
/// `u128` — structurally unreachable given upstream input-size limits, but
/// checked rather than assumed.
pub fn concat_records(lists: &[Vec<String>], opts: ConcatOptions) -> Option<Concat> {
    let mut prefix = Vec::with_capacity(lists.len() + 1);
    prefix.push(0u128);
    let mut acc: u128 = 0;
    for list in lists {
        acc = acc.checked_add(list.len() as u128)?;
        prefix.push(acc);
    }
    let total = acc;

    let available = total.saturating_sub(opts.offset);
    let to_emit = match opts.limit {
        Some(l) => available.min(l),
        None => available,
    };
    let start = if opts.reverse {
        total.saturating_sub(1).saturating_sub(opts.offset)
    } else {
        opts.offset
    };

    Some(Concat {
        prefix,
        next_pos: start,
        remaining: to_emit,
        descending: opts.reverse,
    })
}

impl Iterator for Concat {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.remaining == 0 {
            return None;
        }
        let pos = self.next_pos;
        // Largest j with prefix[j] <= pos; safe because remaining > 0
        // guarantees pos < prefix[last].
        let list_idx = match self.prefix.binary_search(&pos) {
            Ok(exact) => exact,
            Err(insert_at) => insert_at - 1,
        };
        let item_idx = (pos - self.prefix[list_idx]) as usize;
        self.remaining -= 1;
        if self.descending {
            self.next_pos = self.next_pos.saturating_sub(1);
        } else {
            self.next_pos = self.next_pos.saturating_add(1);
        }
        Some((list_idx, item_idx))
    }
}
```

- [ ] **Step 4: Wire `Concat` into the `Operation` enum**

Replace `crates/combinator-core/src/operation.rs` in full:
```rust
//! Mode-neutral operation dispatch over product/zip/concat.

use crate::concat::{concat_count, ConcatOptions};
use crate::count::{combination_count, Count};
use crate::product::ProductOptions;
use crate::zip::{zip_count, ZipLengthMismatch, ZipOptions};

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
    Zip(ZipOptions),
    Concat(ConcatOptions),
}

/// Counts combinations for whichever operation is selected.
///
/// Only `Zip` under `UnequalPolicy::Error` can fail (mismatched lengths).
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Result<Count, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => Ok(combination_count(&lens)),
        Operation::Zip(opts) => zip_count(&lens, opts.on_unequal),
        Operation::Concat(_opts) => Ok(concat_count(&lens)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::UnequalPolicy;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn product_dispatches_to_combination_count() {
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(4));
    }

    #[test]
    fn zip_dispatches_to_zip_count() {
        let op = Operation::Zip(ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        });
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(2));
    }

    #[test]
    fn concat_dispatches_to_concat_count() {
        let op = Operation::Concat(ConcatOptions::default());
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(4));
    }
}
```

- [ ] **Step 5: Wire the new module into `lib.rs`**

Replace `crates/combinator-core/src/lib.rs` in full:
```rust
//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod concat;
pub mod count;
pub mod estimate;
pub mod operation;
pub mod product;
pub mod zip;

pub use concat::{concat_count, concat_records, Concat, ConcatOptions};
pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use operation::{count as operation_count, Operation};
pub use product::{combinations, Product, ProductOptions};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p combinator-core concat`
Expected: PASS (6 tests).

Run: `cargo test --workspace --locked`
Expected: PASS. `combinator-cli` still compiles (nothing there references `Concat` yet — Task 6 adds that).

- [ ] **Step 7: fmt + clippy**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/combinator-core
git commit -m "$(cat <<'EOF'
feat(core): add concat engine

concat_records yields (list_index, item_index) pairs rather than
Vec<usize> — concat records have arity 1, unlike product/zip's
one-field-per-list records. Operation::Concat added.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: combinator-cli — wire the `concat` subcommand

**Files:**
- Modify: `crates/combinator-cli/src/cli.rs` (add `ConcatArgs`, `Mode::Concat`)
- Modify: `crates/combinator-cli/src/main.rs` (`resolve`, `stream`'s arity-1 branch, `bounded_size_estimate`'s mode-aware max-item bound)
- Create: `crates/combinator-cli/tests/concat.rs`

**Interfaces:**
- Consumes: `Operation::Concat`, `ConcatOptions`, `concat_records` (Task 5).
- Produces:
  - `pub struct ConcatArgs { common: CommonArgs }` (**no `sep` field** — see spec §3/§4.3).
  - `Mode::Concat(ConcatArgs)` variant.
  - `combinator concat --list a,b --list c,d,e` end-to-end, emitting one field per record.

- [ ] **Step 1: Write the failing black-box tests**

Create `crates/combinator-cli/tests/concat.rs`:
```rust
//! Black-box tests for the `concat` subcommand.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_combinator"))
}

#[test]
fn concat_emits_every_list_in_order() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y,z"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a\nb\nx\ny\nz\n"
    );
}

#[test]
fn concat_rejects_sep() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--sep", "-"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn concat_rejects_reverse_fields() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--reverse-fields"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn concat_count_only() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y,z", "--count-only"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5");
}

#[test]
fn concat_offset_and_limit_paginate() {
    let out = bin()
        .args([
            "concat", "--list", "a,b", "--list", "x,y,z", "--offset", "1", "--limit", "2",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "b\nx\n");
}

#[test]
fn concat_jsonl_shape_has_single_element_fields() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--format", "jsonl"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["value"], "a");
    assert_eq!(first["fields"].as_array().unwrap().len(), 1);
    assert_eq!(first["fields"][0], "a");
}

#[test]
fn concat_reverse_walks_from_the_end() {
    let out = bin()
        .args(["concat", "--list", "a,b", "--list", "x,y", "--reverse"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "y\nx\nb\na\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p combinator-cli --test concat`
Expected: FAIL — `concat` is not a recognized subcommand yet.

- [ ] **Step 3: Add `ConcatArgs`/`Mode::Concat` to `cli.rs`**

Add after `ZipArgs`, before `Mode`:
```rust
/// Sequential concatenation of the input lists.
#[derive(Debug, Args)]
pub struct ConcatArgs {
    #[command(flatten)]
    pub common: CommonArgs,
}
```

Change `Mode` to:
```rust
#[derive(Debug, Subcommand)]
pub enum Mode {
    /// Ordered Cartesian product (the default when no subcommand is given).
    Product(ProductArgs),
    /// Positional pairing of the input lists.
    Zip(ZipArgs),
    /// Sequential concatenation of the input lists.
    Concat(ConcatArgs),
}
```

Add a test to `cli.rs`'s `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn concat_subcommand_has_no_sep_field() {
        let result = Cli::try_parse_from(["combinator", "concat", "--list", "a,b", "--sep", "-"]);
        assert!(result.is_err());
    }

    #[test]
    fn concat_subcommand_parses() {
        let cli = Cli::parse_from(["combinator", "concat", "--list", "a,b", "--list", "c,d"]);
        match cli.command {
            Some(Mode::Concat(args)) => {
                assert_eq!(args.common.list, vec!["a,b", "c,d"]);
            }
            other => panic!("expected concat subcommand, got {other:?}"),
        }
    }
```

- [ ] **Step 4: Wire `Concat` into `main.rs`**

Change the `use cli::{...}` line to add `ConcatArgs`:
```rust
use cli::{
    Cli, CommonArgs, ConcatArgs, Mode, OutFormat, ProductArgs, ZipArgs, HARD_MAX_COMBINATIONS,
    HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST, HARD_MAX_ITEM_BYTES, HARD_MAX_LISTS,
    HARD_MAX_OUTPUT_BYTES, HARD_MAX_TOTAL_ITEMS,
};
```

Change the `use combinator_core::{...}` lines to add `ConcatOptions`/`concat_records`:
```rust
use combinator_core::{operation_count, Count, Operation, ProductOptions, ZipOptions, ConcatOptions};
use combinator_core::{
    combinations, concat_records, estimate_jsonl_size, estimate_text_size, zip_records,
    SizeEstimate, SizeInput,
};
```

Add `Mode::Concat` to `resolve`, and a `concat_operation` function:
```rust
fn resolve(cli: Cli) -> (CommonArgs, String, Operation) {
    match cli.command {
        Some(Mode::Product(args)) => product_operation(args),
        Some(Mode::Zip(args)) => zip_operation(args),
        Some(Mode::Concat(args)) => concat_operation(args),
        None => product_operation(cli.product),
    }
}

fn concat_operation(args: ConcatArgs) -> (CommonArgs, String, Operation) {
    let opts = ConcatOptions {
        reverse: args.common.reverse,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, String::new(), Operation::Concat(opts))
}
```
(`concat` has no `--sep` flag at all, so its `sep` is always the empty string — harmless, since `stream()`'s concat branch, below, formats single-item records where a field separator is never joined against anything.)

Replace `stream()`'s record-production loop so it branches on arity. Replace the whole middle of `stream()` (from `let index_source: ...` down to the `for` loop's opening) — the surrounding setup (`format`, `output_file`, `writer`, `index`, `output_limit`, `written`) is unchanged from Task 4:
```rust
    enum Records {
        Multi(Box<dyn Iterator<Item = Vec<usize>>>),
        Single(Box<dyn Iterator<Item = (usize, usize)>>),
    }

    let records = match op {
        Operation::Product(opts) => Records::Multi(Box::new(combinations(lists, opts.clone()))),
        Operation::Zip(opts) => Records::Multi(Box::new(zip_records(lists, opts.clone()).map_err(
            |_| {
                AppError::runtime(
                    "ZIP_LENGTH_MISMATCH",
                    "zip inputs have unequal lengths; pass --on-unequal truncate or cycle",
                )
            },
        )?)),
        Operation::Concat(opts) => Records::Single(Box::new(
            concat_records(lists, opts.clone()).ok_or_else(|| {
                AppError::runtime("COUNT_OVERFLOW", "concatenated item count overflowed")
            })?,
        )),
    };

    macro_rules! emit {
        ($items:expr) => {{
            let items = $items;
            let record =
                format_record(&items, index, sep, &common.rec_sep, format, common.lean_output);
            let record_bytes = u64::try_from(record.len()).map_err(|_| {
                AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output record is too large to write",
                )
            })?;
            if let Some(limit) = output_limit {
                let next = written.checked_add(record_bytes).ok_or_else(|| {
                    AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
                })?;
                if next > limit {
                    return Err(AppError::runtime(
                        "OUTPUT_LIMIT_EXCEEDED",
                        "output exceeds the configured byte limit",
                    )
                    .with("written_bytes", written)
                    .with("record_bytes", record_bytes)
                    .with("limit_bytes", limit));
                }
                written = next;
            }
            writer.write_all(record.as_bytes()).map_err(write_err)?;
            index = index.saturating_add(1);
        }};
    }

    match records {
        Records::Multi(iter) => {
            for indices in iter {
                let items: Vec<&str> = indices
                    .iter()
                    .enumerate()
                    .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
                    .collect();
                emit!(items);
            }
        }
        Records::Single(iter) => {
            for (list_i, item_i) in iter {
                let items: Vec<&str> = vec![lists[list_i][item_i].as_str()];
                emit!(items);
            }
        }
    }
```
This replaces the `Box<dyn Iterator<Item = Vec<usize>>>`-only `index_source`/`for indices in index_source` block from Task 4 with a two-arity `Records` enum, keeping the per-record limit/write bookkeeping in one place via the `emit!` macro (declared inline, scoped to `stream()`, so it does not leak into the rest of the file — matches the file's existing style of small free functions rather than introducing a new module for two call sites).

Update `bounded_size_estimate()`'s `max_items` computation to branch on arity — replace its closure body (the `let max_items: Vec<&str> = ...` block inside `bounded.and_then`):
```rust
    let bounded: Option<u128> = count.and_then(|c| {
        let max_items: Vec<&str> = match op {
            Operation::Concat(_) => {
                let longest = lists
                    .iter()
                    .flatten()
                    .max_by_key(|s| {
                        if json_out {
                            serde_json::to_string(s)
                                .map(|v| v.len())
                                .unwrap_or(usize::MAX)
                        } else {
                            s.len()
                        }
                    })
                    .map(String::as_str)
                    .unwrap_or("");
                vec![longest]
            }
            Operation::Product(_) | Operation::Zip(_) => lists
                .iter()
                .map(|l| {
                    l.iter()
                        .max_by_key(|s| {
                            if json_out {
                                serde_json::to_string(s)
                                    .map(|v| v.len())
                                    .unwrap_or(usize::MAX)
                            } else {
                                s.len()
                            }
                        })
                        .map(String::as_str)
                        .unwrap_or("")
                })
                .collect(),
        };
        let max_index = common.offset.saturating_add(c.saturating_sub(1));
        let per_record = format_record(
            &max_items,
            max_index,
            sep,
            &common.rec_sep,
            format,
            common.lean_output,
        )
        .len() as u128;
        c.checked_mul(per_record)
    });
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p combinator-cli --test concat`
Expected: PASS (7 tests).

Run: `cargo test --workspace --locked`
Expected: PASS — every test in the workspace, across all six commits so far, green together.

- [ ] **Step 6: fmt + clippy**

Run: `cargo fmt --all -- --check`
Run: `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
Expected: clean. If clippy flags the `emit!` macro or the `Records` enum, prefer fixing the flagged issue over suppressing it; do not add `#[allow(...)]` without first trying the straightforward fix (e.g. clippy may want `Vec::from([longest])` instead of `vec![longest]` — apply whatever the lint actually suggests).

- [ ] **Step 7: Commit**

```bash
git add crates/combinator-cli
git commit -m "$(cat <<'EOF'
feat(cli): wire the concat subcommand

concat has no --sep flag (records have exactly one field, so a field
separator is inert). stream() now branches on record arity via a small
Records enum; bounded_size_estimate()'s worst-case bound branches the
same way (single longest item across all lists, not one per list).

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Documentation and final verification sweep

**Files:**
- Modify: `README.md`

**Interfaces:** none (documentation + verification only).

- [ ] **Step 1: Add subcommands to the README**

Read `README.md` in full first (`Read` it — do not guess at surrounding content) and add a new section after the existing `## Flags` section (around line 103, before `## Output formats`) titled `## Operation modes`, covering:
- `combinator [--list ...|--file ...] ...` and `combinator product ...` are identical.
- `combinator zip --list a,b --list x,y [--on-unequal error|truncate|cycle] [--sep ...]` — positional pairing; default policy `error`.
- `combinator concat --list a,b --list x,y,z` — sequential concatenation; no `--sep` (single-field records).
- A one-line note that `--reverse-fields` is `product`-only and `--on-unequal` is `zip`-only; passing either to an unsupported mode is a usage error (exit 2).

- [ ] **Step 2: Add the new error code to the README's error-code table**

In the `## Error codes` section's table (around line 235), add a row directly after the `SOURCE_CONFLICT` row (keeping the table's existing logical grouping of usage-time vs. runtime errors — `ZIP_LENGTH_MISMATCH` is runtime, exit 1, discovered after reading input, so place it among the other exit-1 rows, e.g. directly after `COMBINATION_LIMIT_EXCEEDED`):
```
| `ZIP_LENGTH_MISMATCH` | 1 | `zip` with `--on-unequal error` (the default) and input lists of different lengths. |
```

- [ ] **Step 3: Full workspace verification**

Run each of these and confirm clean output before proceeding:
```bash
cargo test -p combinator-core --locked
cargo test -p combinator-cli --locked
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
```

- [ ] **Step 4: Manual smoke test of all three modes**

Run each and eyeball the output against the spec:
```bash
cargo run -p combinator-cli -- --list a,b --list x,y --sep -
cargo run -p combinator-cli -- product --list a,b --list x,y --sep -
cargo run -p combinator-cli -- zip --list a,b --list x,y --sep -
cargo run -p combinator-cli -- zip --list a,b,c --list x,y --on-unequal cycle --sep -
cargo run -p combinator-cli -- concat --list a,b --list x,y,z
cargo run -p combinator-cli -- --help
cargo run -p combinator-cli -- zip --help
cargo run -p combinator-cli -- concat --sep - --list a,b
```
Expected: first two commands produce identical output (`a-x`, `a-y`, `b-x`, `b-y`); `zip` commands produce positionally-paired/cycled output; `concat` produces five bare lines; `--help` lists `product`, `zip`, `concat` as available subcommands; the last command fails with a clap usage error (exit 2) since `concat` has no `--sep`.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "$(cat <<'EOF'
docs: document zip/concat subcommands and ZIP_LENGTH_MISMATCH

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
```

---

## Self-review notes (for whoever executes this plan)

- **Spec coverage:** §2 (architecture/Operation) → Tasks 1, 3, 5. §3 (CLI structure) → Tasks 2, 4, 6. §4.1 (product unchanged) → Task 2. §4.2 (zip) → Tasks 3–4. §4.3 (concat) → Tasks 5–6. §5 (output formatting, arity) → Task 6. §6 (error handling) → Tasks 3–4 (`ZIP_LENGTH_MISMATCH`). §8 (testing strategy) → each task's own black-box/unit tests plus Task 2's full-suite checkpoint. §9 (scope boundaries) → respected throughout; no task touches CSV/templates/dry-run/joins/Rust-API.
- **Known risk concentration:** Task 2 and Task 6 touch `main.rs`'s `stream()`/`bounded_size_estimate()` the most. Both are followed immediately by a full-workspace test run in the same task — do not skip those steps even under time pressure.
- **Type consistency check already applied:** `(CommonArgs, String, Operation)` is the tuple shape used consistently from Task 4 onward in `resolve`/`product_operation`/`zip_operation`/`concat_operation`/`main`/`run`; `Records`/`emit!` in Task 6 is the only new abstraction introduced for arity, and it is not referenced anywhere outside `stream()`.
