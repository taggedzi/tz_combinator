mod cli;
mod error;
mod input;
mod output;
mod output_file;
mod preflight;

use std::io::{BufWriter, Write};

use clap::Parser;
use combinator_core::{
    combinations, concat_records, estimate_jsonl_size, estimate_text_size, zip_records,
    SizeEstimate, SizeInput,
};
use combinator_core::{
    operation_count, ConcatOptions, Count, Operation, ProductOptions, Template, TemplateError,
    ZipOptions,
};

use cli::{
    Cli, CommonArgs, ConcatArgs, Mode, OutFormat, ProductArgs, ZipArgs, HARD_MAX_COMBINATIONS,
    HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST, HARD_MAX_ITEM_BYTES, HARD_MAX_LISTS,
    HARD_MAX_OUTPUT_BYTES, HARD_MAX_TOTAL_ITEMS,
};
use error::{render, render_warning, AppError};
use input::{InputBudget, InputLimits, MAX_TEMPLATE_BYTES};
use output::{format_record_with, Format};
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
    let (common, sep, op) = resolve(cli);
    let json_errors = matches!(common.format, OutFormat::Jsonl | OutFormat::Json);
    if let Err(e) = run(common, sep, op) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(e.exit);
    }
}

/// Reduces the parsed `Cli` (an explicit subcommand, or the legacy bare
/// invocation) to a clap-free `(CommonArgs, String, Operation)` triple
/// (`sep` is CLI-only — not every mode has one, so it isn't part of any
/// engine's options type). Everything past this point is clap-agnostic.
fn resolve(cli: Cli) -> (CommonArgs, String, Operation) {
    match cli.command {
        Some(Mode::Product(args)) => product_operation(args),
        Some(Mode::Zip(args)) => zip_operation(args),
        Some(Mode::Concat(args)) => concat_operation(args),
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

fn concat_operation(args: ConcatArgs) -> (CommonArgs, String, Operation) {
    let opts = ConcatOptions {
        reverse: args.common.reverse,
        offset: args.common.offset,
        limit: args.common.limit,
    };
    (args.common, String::new(), Operation::Concat(opts))
}

fn run(common: CommonArgs, sep: String, op: Operation) -> Result<(), AppError> {
    validate_resource_limits(&common)?;
    if matches!(common.format, OutFormat::Json) && !(common.explain || common.dry_run) {
        return Err(AppError::usage(
            "FORMAT_UNSUPPORTED",
            "--format json is only valid with --explain or --dry-run",
        ));
    }
    if common.count_only && (common.explain || common.dry_run) {
        return Err(AppError::usage(
            "MODE_CONFLICT",
            "--count-only cannot be combined with --explain or --dry-run",
        ));
    }
    input::validate_delims(&sep, &common.rec_sep, &common.list_delim)?;
    let template = load_template(&common, &sep)?;

    if let Operation::Product(product_opts) = &op {
        if product_opts.reverse && product_opts.reverse_fields {
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

    let empty_template = Template::parse("").expect("empty template is valid");
    let template_for_validation = template.as_ref().unwrap_or(&empty_template);
    template_for_validation
        .validate_fields(&common.names, lists.len())
        .map_err(template_error)?;

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

    if common.explain || common.dry_run {
        let estimate = bounded_size_estimate(
            &common,
            &sep,
            &op,
            &lists,
            matches!(common.format, OutFormat::Jsonl),
            template.as_ref(),
        );
        return explain(&common, &op, &lists, total_for_limits, estimate);
    }

    // Pre-flight for file output.
    if let Some(path) = &common.output {
        preflight::check_output_path(path, common.overwrite)?;
        if !common.no_preflight {
            let estimate =
                bounded_size_estimate(&common, &sep, &op, &lists, json_out, template.as_ref());
            let available = available_space(path)?;
            preflight::check_capacity(estimate, available, effective_output_limit(&common))?;
        }
    }

    stream(&common, &sep, &op, &lists, json_out, template.as_ref())
}

fn explain(
    common: &CommonArgs,
    op: &Operation,
    lists: &[Vec<String>],
    count: Count,
    estimate: SizeEstimate,
) -> Result<(), AppError> {
    let exact_count = match count {
        Count::Exact(value) => Some(value),
        Count::Overflow => None,
    };
    let records_to_emit = exact_count.map(|total| {
        let remaining = total.saturating_sub(common.offset);
        common.limit.map_or(remaining, |limit| remaining.min(limit))
    });
    let estimated_bytes = match estimate {
        SizeEstimate::Bytes(value) => Some(value),
        SizeEstimate::Overflow => None,
    };
    let operation = operation_name(op);
    let format = match common.format {
        OutFormat::Text => "text",
        OutFormat::Jsonl => "jsonl",
        OutFormat::Json => "json",
    };
    let destination = if common.output.is_some() {
        "file"
    } else {
        "stdout"
    };

    if matches!(common.format, OutFormat::Json) {
        let summary = serde_json::json!({
            "schema_version": 1,
            "operation": operation,
            "input": {
                "lists": lists.len(),
                "items_per_list": lists.iter().map(Vec::len).collect::<Vec<_>>(),
                "total_items": lists.iter().map(Vec::len).try_fold(0usize, |a, b| a.checked_add(b)).unwrap_or(usize::MAX),
            },
            "combination_count": exact_count,
            "combination_count_overflow": exact_count.is_none(),
            "offset": common.offset,
            "limit": common.limit,
            "records_to_emit": records_to_emit,
            "estimated_output_bytes": estimated_bytes,
            "estimated_output_overflow": estimated_bytes.is_none(),
            "output": destination,
            "format": format,
            "limits": {
                "max_output_bytes": common.max_output_bytes,
                "max_combinations": common.max_combinations,
                "max_input_bytes": common.max_input_bytes,
                "max_item_bytes": common.max_item_bytes,
                "max_items_per_list": common.max_items_per_list,
                "max_lists": common.max_lists,
                "max_total_items": common.max_total_items,
            }
        });
        println!("{summary}");
    } else {
        println!("schema_version=1");
        println!("operation={operation}");
        println!("input_lists={}", lists.len());
        println!(
            "items_per_list={}",
            lists
                .iter()
                .map(Vec::len)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        println!(
            "combination_count={}",
            exact_count.map_or_else(|| "overflow".to_string(), |n| n.to_string())
        );
        println!("offset={}", common.offset);
        println!(
            "limit={}",
            common
                .limit
                .map_or_else(|| "unlimited".to_string(), |n| n.to_string())
        );
        println!(
            "records_to_emit={}",
            records_to_emit.map_or_else(|| "unknown".to_string(), |n| n.to_string())
        );
        println!(
            "estimated_output_bytes={}",
            estimated_bytes.map_or_else(|| "overflow".to_string(), |n| n.to_string())
        );
        println!("output={destination}");
        println!("format={format}");
    }
    Ok(())
}

fn operation_name(op: &Operation) -> &'static str {
    match op {
        Operation::Product(_) => "product",
        Operation::Zip(_) => "zip",
        Operation::Concat(_) => "concat",
    }
}

fn load_template(common: &CommonArgs, sep: &str) -> Result<Option<Template>, AppError> {
    if common.template.is_some() && common.template_file.is_some() {
        return Err(AppError::usage(
            "TEMPLATE_CONFLICT",
            "use either --template or --template-file, not both",
        ));
    }
    if (common.template.is_some() || common.template_file.is_some()) && !sep.is_empty() {
        return Err(AppError::usage(
            "TEMPLATE_SEPARATOR_CONFLICT",
            "a template cannot be combined with a non-empty --sep",
        ));
    }
    let max_template_bytes = common.max_input_bytes.min(MAX_TEMPLATE_BYTES);
    let source = match (&common.template, &common.template_file) {
        (Some(value), None) => {
            if value.len() > max_template_bytes {
                return Err(AppError::usage(
                    "TEMPLATE_TOO_LARGE",
                    "template exceeds the configured template byte limit",
                )
                .with("observed", value.len())
                .with("limit", max_template_bytes));
            }
            value.clone()
        }
        (None, Some(path)) => input::read_template_bounded(path, max_template_bytes)?,
        (None, None) => return Ok(None),
        (Some(_), Some(_)) => unreachable!("template source conflict handled above"),
    };
    Template::parse(&source).map(Some).map_err(template_error)
}

fn template_error(error: TemplateError) -> AppError {
    let (code, message, position) = match error {
        TemplateError::InvalidSyntax { position } => (
            "TEMPLATE_INVALID",
            "template syntax is invalid",
            Some(position),
        ),
        TemplateError::InvalidReference { position } => (
            "TEMPLATE_INVALID",
            "template reference is invalid",
            Some(position),
        ),
        TemplateError::InvalidName { position } => (
            "TEMPLATE_INVALID_NAME",
            "a field name is invalid",
            Some(position),
        ),
        TemplateError::DuplicateName { position } => (
            "TEMPLATE_DUPLICATE_NAME",
            "a field name was supplied more than once",
            Some(position),
        ),
        TemplateError::NameCountMismatch { expected, actual } => {
            return AppError::usage(
                "TEMPLATE_NAMES_MISMATCH",
                "the number of field names must equal the number of input lists",
            )
            .with("expected", expected)
            .with("actual", actual);
        }
        TemplateError::UnknownField { position } => (
            "TEMPLATE_UNKNOWN_FIELD",
            "template references an unknown field",
            Some(position),
        ),
    };
    let error = AppError::usage(code, message);
    match position {
        Some(position) => error.with("position", position),
        None => error,
    }
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
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
    template: Option<&Template>,
) -> SizeEstimate {
    let input = SizeInput {
        lists,
        field_sep_bytes: sep.len() as u64,
        rec_sep_bytes: common.rec_sep.len() as u64,
    };
    let full = if template.is_some() {
        // The legacy estimator does not know about template literals or
        // repeated references. The mode-aware bounded estimate below is the
        // conservative source of truth for templated records.
        SizeEstimate::Overflow
    } else if json_out {
        estimate_jsonl_size(&input, common.lean_output)
    } else {
        estimate_text_size(&input)
    };

    // How many records will actually be written.
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

    // Per-record upper bound: format the longest-possible record once.
    let format = if json_out {
        Format::Jsonl
    } else {
        Format::Text
    };
    let bounded: Option<u128> = count.and_then(|c| {
        let longest_key = |s: &&String| {
            if json_out {
                serde_json::to_string(s)
                    .map(|v| v.len())
                    .unwrap_or(usize::MAX)
            } else {
                s.len()
            }
        };
        let max_items: Vec<&str> = match op {
            Operation::Concat(_) => {
                let longest = lists
                    .iter()
                    .flatten()
                    .max_by_key(longest_key)
                    .map(String::as_str)
                    .unwrap_or("");
                vec![longest]
            }
            Operation::Product(_) | Operation::Zip(_) => lists
                .iter()
                .map(|l| {
                    l.iter()
                        .max_by_key(longest_key)
                        .map(String::as_str)
                        .unwrap_or("")
                })
                .collect(),
        };
        let max_index = common.offset.saturating_add(c.saturating_sub(1));
        let per_record = format_record_with(
            &max_items,
            max_index,
            sep,
            &common.rec_sep,
            format,
            common.lean_output,
            template,
            &common.names,
        )
        .expect("template was validated before estimation")
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
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    json_out: bool,
    template: Option<&Template>,
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

    enum Records {
        Multi(Box<dyn Iterator<Item = Vec<usize>>>),
        Single(Box<dyn Iterator<Item = (usize, usize)>>),
    }

    let records = match op {
        Operation::Product(opts) => Records::Multi(Box::new(combinations(lists, opts.clone()))),
        Operation::Zip(opts) => Records::Multi(Box::new(
            zip_records(lists, opts.clone()).map_err(|_| {
                AppError::runtime(
                    "ZIP_LENGTH_MISMATCH",
                    "zip inputs have unequal lengths; pass --on-unequal truncate or cycle",
                )
            })?,
        )),
        Operation::Concat(opts) => {
            Records::Single(Box::new(concat_records(lists, opts.clone()).ok_or_else(
                || AppError::runtime("COUNT_OVERFLOW", "concatenated item count overflowed"),
            )?))
        }
    };

    macro_rules! emit {
        ($items:expr) => {{
            let items = $items;
            let record = format_record_with(
                &items,
                index,
                sep,
                &common.rec_sep,
                format,
                common.lean_output,
                template,
                &common.names,
            )
            .map_err(template_error)?;
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
