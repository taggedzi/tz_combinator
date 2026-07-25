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
