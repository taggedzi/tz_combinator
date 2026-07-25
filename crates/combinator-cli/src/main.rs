mod cli;
mod error;
mod input;
mod output;
mod preflight;

use std::io::{BufWriter, Write};

use clap::Parser;
use combinator_core::{combination_count, combinations, Count, ProductOptions};
use combinator_core::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};

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
            let estimate = bounded_size_estimate(&cli, &lists, json_out);
            let available = available_space(path);
            preflight::check_capacity(estimate, available, cli.max_file_size)?;
        }
    }

    stream(&cli, &lists, json_out)
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
    let format = if json_out { Format::Jsonl } else { Format::Text };
    let bounded: Option<u128> = count.and_then(|c| {
        let max_items: Vec<&str> = lists
            .iter()
            .map(|l| l.iter().map(|s| s.as_str()).max_by_key(|s| s.len()).unwrap_or(""))
            .collect();
        let max_index = cli.offset.saturating_add(c.saturating_sub(1));
        let per_record =
            format_record(&max_items, max_index, &cli.sep, &cli.rec_sep, format, cli.lean_output)
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

    let mut index: u128 = cli.offset;
    for indices in combinations(lists, opts) {
        let items: Vec<&str> = indices
            .iter()
            .enumerate()
            .map(|(list_i, &item_i)| lists[list_i][item_i].as_str())
            .collect();
        let record = format_record(&items, index, &cli.sep, &cli.rec_sep, format, cli.lean_output);
        writer.write_all(record.as_bytes()).map_err(write_err)?;
        index = index.saturating_add(1);
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
