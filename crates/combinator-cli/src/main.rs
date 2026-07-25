mod cli;
mod error;
mod input;
mod output;
mod output_file;
mod preflight;

use std::io::{BufWriter, Write};

use clap::Parser;
use combinator_core::{combination_count, combinations, Count, ProductOptions};
use combinator_core::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};

use cli::{
    Cli, OutFormat, HARD_MAX_COMBINATIONS, HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST,
    HARD_MAX_ITEM_BYTES, HARD_MAX_LISTS, HARD_MAX_OUTPUT_BYTES, HARD_MAX_TOTAL_ITEMS,
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
    let json_errors = matches!(cli.format, OutFormat::Jsonl);
    if let Err(e) = run(cli) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(e.exit);
    }
}

fn run(cli: Cli) -> Result<(), AppError> {
    validate_resource_limits(&cli)?;
    input::validate_delims(&cli.sep, &cli.rec_sep, &cli.list_delim)?;

    if cli.reverse && cli.reverse_fields {
        return Err(AppError::usage(
            "REVERSE_CONFLICT",
            "use either --reverse or --reverse-fields, not both",
        ));
    }

    // Input is either --list or --file, never both. Order within the chosen
    // source is argument order (clap preserves it). `--file -` reads stdin.
    let mut lists: Vec<Vec<String>> = Vec::new();
    if cli.list.len().max(cli.file.len()) > cli.max_lists {
        return Err(
            AppError::runtime("TOO_MANY_LISTS", "input exceeds the maximum list count")
                .with("observed", cli.list.len().max(cli.file.len()))
                .with("limit", cli.max_lists),
        );
    }
    let input_limits = InputLimits {
        max_input_bytes: cli.max_input_bytes,
        max_item_bytes: cli.max_item_bytes,
        max_items_per_list: cli.max_items_per_list,
    };
    let mut input_budget = InputBudget::new(cli.max_input_bytes, cli.max_total_items);
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
                lists.push(input::split_inline_bounded(
                    value,
                    &cli.list_delim,
                    input_limits,
                    &mut input_budget,
                )?);
            }
        }
        (true, false) => {
            for path in &cli.file {
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
    if total_items > cli.max_total_items {
        return Err(AppError::runtime(
            "TOO_MANY_ITEMS",
            "input exceeds the maximum total item count",
        )
        .with("observed", total_items)
        .with("limit", cli.max_total_items));
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

    let lens: Vec<usize> = lists.iter().map(|l| l.len()).collect();
    match combination_count(&lens) {
        Count::Exact(total) if cli.limit.unwrap_or(total) > cli.max_combinations => {
            return Err(AppError::runtime(
                "COMBINATION_LIMIT_EXCEEDED",
                "requested combinations exceed the configured generation limit",
            )
            .with("limit", cli.max_combinations));
        }
        Count::Overflow if cli.limit.is_none() || cli.limit.unwrap_or(0) > cli.max_combinations => {
            return Err(AppError::runtime(
                "COMBINATION_LIMIT_EXCEEDED",
                "the product is too large without an explicit safe limit",
            )
            .with("limit", cli.max_combinations));
        }
        _ => {}
    }

    // Pre-flight for file output.
    if let Some(path) = &cli.output {
        preflight::check_output_path(path, cli.overwrite)?;
        if !cli.no_preflight {
            let estimate = bounded_size_estimate(&cli, &lists, json_out);
            let available = available_space(path)?;
            preflight::check_capacity(estimate, available, effective_output_limit(&cli))?;
        }
    }

    stream(&cli, &lists, json_out)
}

fn validate_resource_limits(cli: &Cli) -> Result<(), AppError> {
    let checks = [
        (
            "max-output-bytes",
            cli.max_output_bytes as u128,
            HARD_MAX_OUTPUT_BYTES as u128,
        ),
        (
            "max-input-bytes",
            cli.max_input_bytes as u128,
            HARD_MAX_INPUT_BYTES as u128,
        ),
        (
            "max-item-bytes",
            cli.max_item_bytes as u128,
            HARD_MAX_ITEM_BYTES as u128,
        ),
        (
            "max-items-per-list",
            cli.max_items_per_list as u128,
            HARD_MAX_ITEMS_PER_LIST as u128,
        ),
        ("max-lists", cli.max_lists as u128, HARD_MAX_LISTS as u128),
        (
            "max-total-items",
            cli.max_total_items as u128,
            HARD_MAX_TOTAL_ITEMS as u128,
        ),
        (
            "max-combinations",
            cli.max_combinations,
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
    if let Some(file_limit) = cli.max_file_size {
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
/// not rejected on the size of the full product. Returns a safe upper bound: the
/// smaller of the full-product estimate and (records_to_write * max_record_bytes),
/// where max_record_bytes formats the longest item from each list once.
fn bounded_size_estimate(cli: &Cli, lists: &[Vec<String>], json_out: bool) -> SizeEstimate {
    let input = SizeInput {
        lists,
        field_sep_bytes: cli.sep.len() as u64,
        rec_sep_bytes: cli.rec_sep.len() as u64,
    };
    let full = if json_out {
        estimate_jsonl_size(&input, cli.lean_output)
    } else {
        estimate_text_size(&input)
    };

    // How many records will actually be written.
    let lens: Vec<usize> = lists.iter().map(|l| l.len()).collect();
    let count: Option<u128> = match combination_count(&lens) {
        Count::Exact(total) => {
            let remaining = total.saturating_sub(cli.offset);
            Some(match cli.limit {
                Some(l) => remaining.min(l),
                None => remaining,
            })
        }
        // Unbounded product is only bounded if --limit caps it.
        Count::Overflow => cli.limit,
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
        let max_index = cli.offset.saturating_add(c.saturating_sub(1));
        let per_record = format_record(
            &max_items,
            max_index,
            &cli.sep,
            &cli.rec_sep,
            format,
            cli.lean_output,
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

fn stream(cli: &Cli, lists: &[Vec<String>], json_out: bool) -> Result<(), AppError> {
    let opts = ProductOptions {
        reverse: cli.reverse,
        reverse_fields: cli.reverse_fields,
        offset: cli.offset,
        limit: cli.limit,
    };
    let format = if json_out {
        Format::Jsonl
    } else {
        Format::Text
    };

    let mut output_file = cli
        .output
        .as_deref()
        .map(|path| OutputFile::open(path, cli.overwrite))
        .transpose()?;
    let mut writer = match output_file.as_mut() {
        Some(file) => BufWriter::new(OutputWriter::File(file.file_mut())),
        None => BufWriter::new(OutputWriter::Stdout(std::io::stdout())),
    };

    let mut index: u128 = cli.offset;
    let output_limit = effective_output_limit(cli);
    let mut written: u64 = 0;
    for indices in combinations(lists, opts) {
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(
            &items,
            index,
            &cli.sep,
            &cli.rec_sep,
            format,
            cli.lean_output,
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

fn effective_output_limit(cli: &Cli) -> Option<u64> {
    let configured = Some(cli.max_output_bytes);
    match (&cli.output, cli.max_file_size) {
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
