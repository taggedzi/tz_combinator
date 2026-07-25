mod cli;
mod error;
mod input;
mod normalize;
mod output;
mod output_file;
mod preflight;
mod sharding;

use std::io::{BufWriter, Write};

use clap::{CommandFactory, Parser};
use combinator_core::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
use combinator_core::{
    operation_count, ConcatOptions, Count, Operation, ProductOptions, Template, TemplateError,
    ZipOptions,
};

use cli::{
    Cli, CommonArgs, ConcatArgs, InputFormat, Mode, OutFormat, ProductArgs, ZipArgs,
    HARD_MAX_COMBINATIONS, HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST, HARD_MAX_ITEM_BYTES,
    HARD_MAX_LISTS, HARD_MAX_OUTPUT_BYTES, HARD_MAX_TOTAL_ITEMS,
};
use error::{exit_code, render, render_warning, AppError};
use input::{InputBudget, InputLimits, MAX_TEMPLATE_BYTES};
use normalize::MAX_TRANSFORMS;
use output::{format_record_with, Format};
use output_file::OutputFile;
use sharding::{page as shard_page, range as shard_range, ShardError};

enum OutputWriter<'a> {
    File(&'a mut std::fs::File),
    Stdout(std::io::Stdout),
}

type Warning = (&'static str, &'static str, Vec<(String, String)>);

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
    if let Some(mode) = cli.command.as_ref() {
        match mode {
            Mode::Completions { shell } => {
                if let Err(e) = generate_completions(*shell) {
                    eprintln!("{}", render(&e, false));
                    std::process::exit(exit_code(&e));
                }
                return;
            }
            Mode::Man => {
                if let Err(e) = generate_man_page() {
                    eprintln!("{}", render(&e, false));
                    std::process::exit(exit_code(&e));
                }
                return;
            }
            Mode::Product(_) | Mode::Zip(_) | Mode::Concat(_) => {}
        }
    }
    let (common, sep, op) = resolve(cli);
    let json_errors = matches!(common.format, OutFormat::Jsonl | OutFormat::Json);
    if let Err(e) = run(common, sep, op) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(exit_code(&e));
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
        Some(Mode::Completions { .. } | Mode::Man) => unreachable!("handled in main"),
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
    validate_shard_args(&common)?;
    if matches!(common.format, OutFormat::Json) && !(common.explain || common.dry_run) {
        return Err(AppError::usage(
            "FORMAT_UNSUPPORTED",
            "--format json is only valid with --explain or --dry-run",
        ));
    }
    if common
        .file
        .iter()
        .filter(|path| path.as_str() == "-")
        .count()
        > 1
    {
        return Err(AppError::usage(
            "DUPLICATE_STDIN",
            "stdin may be used as an input source only once",
        ));
    }
    if common.input_format == Some(InputFormat::Inline) && !common.file.is_empty() {
        return Err(AppError::usage(
            "INPUT_FORMAT_INVALID",
            "inline input format may only be used with --list",
        ));
    }
    if matches!(
        common.input_format,
        Some(InputFormat::Lines | InputFormat::Csv | InputFormat::Tsv | InputFormat::Nul)
    ) && !common.list.is_empty()
    {
        return Err(AppError::usage(
            "INPUT_FORMAT_INVALID",
            "this input format requires --file; use --input-format inline with --list",
        ));
    }
    if common.count_only && (common.explain || common.dry_run) {
        return Err(AppError::usage(
            "MODE_CONFLICT",
            "--count-only cannot be combined with --explain or --dry-run",
        ));
    }
    if common.transforms.len() > MAX_TRANSFORMS {
        return Err(AppError::usage(
            "TRANSFORM_LIMIT",
            "the number of transforms exceeds the security limit",
        )
        .with("observed", common.transforms.len())
        .with("limit", MAX_TRANSFORMS));
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

    // By default --list and --file remain mutually exclusive. Mixed sources
    // require an explicit opt-in because source order is part of the contract.
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
    match (
        common.list.is_empty(),
        common.file.is_empty(),
        common.allow_mixed_inputs,
    ) {
        (false, false, false) => {
            return Err(AppError::usage(
                "SOURCE_CONFLICT",
                "use either --list or --file, or pass --allow-mixed-inputs",
            ));
        }
        (true, true, _) => {
            return Err(AppError::usage("NO_LISTS", "no input lists were provided"));
        }
        (false, true, _) => {
            for value in &common.list {
                let parsed = if common.input_format == Some(InputFormat::Inline) {
                    input::split_escaped_inline_bounded(
                        value,
                        &common.list_delim,
                        input_limits,
                        &mut input_budget,
                    )?
                } else {
                    input::split_inline_bounded(
                        value,
                        &common.list_delim,
                        input_limits,
                        &mut input_budget,
                    )?
                };
                lists.push(parsed);
            }
        }
        (true, false, _) => {
            for path in &common.file {
                lists.push(input::read_file_list_format_bounded(
                    path,
                    common.input_format.unwrap_or(InputFormat::Lines),
                    input_limits,
                    &mut input_budget,
                )?);
            }
        }
        (false, false, true) => {
            for value in &common.list {
                let parsed = if common.input_format == Some(InputFormat::Inline) {
                    input::split_escaped_inline_bounded(
                        value,
                        &common.list_delim,
                        input_limits,
                        &mut input_budget,
                    )?
                } else {
                    input::split_inline_bounded(
                        value,
                        &common.list_delim,
                        input_limits,
                        &mut input_budget,
                    )?
                };
                lists.push(parsed);
            }
            for path in &common.file {
                lists.push(input::read_file_list_format_bounded(
                    path,
                    common.input_format.unwrap_or(InputFormat::Lines),
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

    normalize::normalize_lists(
        &mut lists,
        &common.transforms,
        common.max_item_bytes,
        common.max_total_items,
    )?;

    let empty_template = Template::parse("").expect("empty template is valid");
    let template_for_validation = template.as_ref().unwrap_or(&empty_template);
    template_for_validation
        .validate_fields(&common.names, lists.len())
        .map_err(template_error)?;

    let json_out = matches!(common.format, OutFormat::Jsonl);

    // Collect warnings until all validation and preflight checks have passed.
    // This prevents a warning from appearing before a later fatal diagnostic.
    let mut warnings: Vec<Warning> = Vec::new();
    for (i, l) in lists.iter().enumerate() {
        if l.is_empty() {
            warnings.push((
                "EMPTY_LIST",
                "a list is empty; zero combinations will be produced",
                vec![("list_index".to_string(), i.to_string())],
            ));
        }
    }

    if common.count_only {
        match operation_count(&op, &lists) {
            Ok(Count::Exact(n)) => {
                emit_warnings(&common, &warnings, json_out)?;
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
    let mut common = common;
    let mut op = op;
    let shard = apply_shard(&mut common, &mut op, total_for_limits)?;

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
        emit_warnings(&common, &warnings, json_out)?;
        let estimate = bounded_size_estimate(
            &common,
            &sep,
            &op,
            &lists,
            matches!(common.format, OutFormat::Jsonl),
            template.as_ref(),
        );
        return explain(&common, &op, &lists, total_for_limits, estimate, shard);
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

    emit_warnings(&common, &warnings, json_out)?;
    stream_core(&common, &sep, &op, &lists, template.as_ref())
}

fn stream_core(
    common: &CommonArgs,
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    template: Option<&Template>,
) -> Result<(), AppError> {
    let mut output_file = common
        .output
        .as_deref()
        .map(|path| OutputFile::open(path, common.overwrite))
        .transpose()?;
    let mut writer = match output_file.as_mut() {
        Some(file) => BufWriter::new(OutputWriter::File(file.file_mut())),
        None => BufWriter::new(OutputWriter::Stdout(std::io::stdout())),
    };
    let result = combinator_core::execute(
        combinator_core::ExecutionRequest {
            operation: op,
            lists,
            format: common.format.into(),
            field_sep: sep,
            record_sep: &common.rec_sep,
            lean: common.lean_output,
            template,
            names: &common.names,
            max_output_bytes: effective_output_limit(common).unwrap_or(u64::MAX),
            max_combinations: common.max_combinations,
            cancel: None,
        },
        &mut writer,
    );
    let result = match result {
        Err(error) if common.output.is_none() && error.message.contains("os error 232") => {
            return Ok(())
        }
        other => other?,
    };
    match writer.flush() {
        Ok(()) => {}
        Err(error) if common.output.is_none() && is_broken_pipe(&error) => return Ok(()),
        Err(error) => return Err(write_err(error)),
    }
    drop(writer);
    if let Some(file) = output_file {
        file.commit()?;
    }
    if common.summary {
        let _ = writeln!(
            std::io::stderr(),
            "summary[OUTPUT]: records={}, bytes={}",
            result.records,
            result.bytes
        );
    }
    Ok(())
}

fn is_broken_pipe(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::BrokenPipe || error.raw_os_error() == Some(232)
}

fn emit_warnings(common: &CommonArgs, warnings: &[Warning], json: bool) -> Result<(), AppError> {
    if let Some(&(code, message, ref context)) = warnings.first() {
        if common.warnings_as_errors {
            return Err(AppError::runtime(code, message).with_context(context));
        }
    }
    if !common.quiet {
        for (code, message, context) in warnings {
            eprintln!("{}", render_warning(code, message, context, json));
        }
    }
    Ok(())
}

fn explain(
    common: &CommonArgs,
    op: &Operation,
    lists: &[Vec<String>],
    count: Count,
    estimate: SizeEstimate,
    shard: Option<sharding::ShardRange>,
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
    let ordering = ordering_name(op);
    let format = match common.format {
        OutFormat::Text => "text",
        OutFormat::Jsonl => "jsonl",
        OutFormat::Json => "json",
        OutFormat::Csv => "csv",
        OutFormat::Tsv => "tsv",
        OutFormat::Nul => "nul",
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
            "ordering": ordering,
            "transforms": &common.transforms,
            "input": {
                "lists": lists.len(),
                "items_per_list": lists.iter().map(Vec::len).collect::<Vec<_>>(),
                "total_items": lists.iter().map(Vec::len).try_fold(0usize, |a, b| a.checked_add(b)).unwrap_or(usize::MAX),
            },
            "combination_count": exact_count,
            "combination_count_overflow": exact_count.is_none(),
            "offset": common.offset,
            "limit": common.limit,
            "shard": shard.map(|range| serde_json::json!({"start": range.start, "end": range.end})),
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
        println!("ordering={ordering}");
        println!("transforms={}", common.transforms.join(","));
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
        if let Some(range) = shard {
            println!("shard_start={}", range.start);
            println!("shard_end={}", range.end);
        }
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

fn ordering_name(op: &Operation) -> &'static str {
    match op {
        Operation::Product(options) if options.reverse_fields => "reverse-fields",
        Operation::Product(options) if options.reverse => "reverse",
        Operation::Product(_) => "forward",
        Operation::Zip(options) if options.reverse => "reverse",
        Operation::Zip(_) => "forward",
        Operation::Concat(options) if options.reverse => "reverse",
        Operation::Concat(_) => "forward",
    }
}

fn validate_shard_args(common: &CommonArgs) -> Result<(), AppError> {
    match (common.shard_index, common.shard_count) {
        (None, None) => Ok(()),
        (Some(_), Some(0)) => Err(AppError::usage(
            "SHARD_COUNT_INVALID",
            "--shard-count must be positive",
        )),
        (Some(index), Some(count)) if index >= count => Err(AppError::usage(
            "SHARD_INDEX_INVALID",
            "--shard-index must be less than --shard-count",
        )),
        (Some(_), Some(_)) => Ok(()),
        _ => Err(AppError::usage(
            "SHARD_ARGUMENTS_INCOMPLETE",
            "--shard-index and --shard-count must be provided together",
        )),
    }
}

fn apply_shard(
    common: &mut CommonArgs,
    op: &mut Operation,
    count: Count,
) -> Result<Option<sharding::ShardRange>, AppError> {
    let (index, shard_count) = match (common.shard_index, common.shard_count) {
        (Some(index), Some(count)) => (index, count),
        _ => return Ok(None),
    };
    let total = match count {
        Count::Exact(value) => value,
        Count::Overflow => {
            return Err(AppError::runtime(
                "SHARD_COUNT_OVERFLOW",
                "cannot compute a shard range for an overflowing combination count",
            ));
        }
    };
    let shard = shard_range(total, index, shard_count).map_err(|error| match error {
        ShardError::ZeroCount => {
            AppError::usage("SHARD_COUNT_INVALID", "--shard-count must be positive")
        }
        ShardError::IndexOutOfRange => AppError::usage(
            "SHARD_INDEX_INVALID",
            "--shard-index must be less than --shard-count",
        ),
        ShardError::Overflow => {
            AppError::runtime("SHARD_COUNT_OVERFLOW", "shard range arithmetic overflowed")
        }
    })?;
    let (offset, limit) = shard_page(shard, common.offset, common.limit);
    common.offset = offset;
    common.limit = Some(limit);
    match op {
        Operation::Product(options) => {
            options.offset = offset;
            options.limit = Some(limit);
        }
        Operation::Zip(options) => {
            options.offset = offset;
            options.limit = Some(limit);
        }
        Operation::Concat(options) => {
            options.offset = offset;
            options.limit = Some(limit);
        }
    }
    Ok(Some(shard))
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
    let format = match common.format {
        OutFormat::Text => Format::Text,
        OutFormat::Jsonl => Format::Jsonl,
        OutFormat::Csv => Format::Csv,
        OutFormat::Tsv => Format::Tsv,
        OutFormat::Nul => Format::Nul,
        OutFormat::Json => Format::Text,
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

fn generate_completions(shell: clap_complete::Shell) -> Result<(), AppError> {
    let mut command = Cli::command();
    let mut output = Vec::new();
    clap_complete::generate(shell, &mut command, "combinator", &mut output);
    write_auxiliary_stdout(&output)
}

fn generate_man_page() -> Result<(), AppError> {
    let command = Cli::command();
    let man = clap_mangen::Man::new(command);
    let mut output = Vec::new();
    man.render(&mut output).map_err(|e| {
        AppError::runtime("WRITE_FAILED", format!("failed generating man page: {e}"))
    })?;
    write_auxiliary_stdout(&output)
}

fn write_auxiliary_stdout(output: &[u8]) -> Result<(), AppError> {
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(output) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(write_err(e)),
    }
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
