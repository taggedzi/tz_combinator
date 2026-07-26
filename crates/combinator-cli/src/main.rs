mod cli;
mod error;
mod input;
mod normalize;
mod output;
mod output_file;
mod preflight;
mod sharding;

use std::io::{BufWriter, Read, Write};
use std::time::{Duration, Instant};

use clap::{CommandFactory, Parser};
use combinator_codecs::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
use combinator_codecs::{Template, TemplateError};
use combinator_core::{
    generate_with, operation_count, ConcatOptions, Constraint, Count, GenerationLimits,
    GenerationRequest, Operation, ProductOptions, SelectionOptions, ZipOptions,
};

use cli::{
    Cli, CommonArgs, ConcatArgs, InputFormat, JoinArgs, JoinFormat, JoinTypeArg, Mode, OutFormat,
    ProductArgs, ZipArgs, HARD_MAX_COMBINATIONS, HARD_MAX_INPUT_BYTES, HARD_MAX_ITEMS_PER_LIST,
    HARD_MAX_ITEM_BYTES, HARD_MAX_JOIN_KEY_FANOUT, HARD_MAX_JOIN_RECORDS, HARD_MAX_LISTS,
    HARD_MAX_OUTPUT_BYTES, HARD_MAX_TIMEOUT_MS, HARD_MAX_TOTAL_ITEMS,
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
            Mode::Join(args) => {
                if let Err(e) = run_join(args) {
                    eprintln!("{}", render(&e, false));
                    std::process::exit(exit_code(&e));
                }
                return;
            }
            Mode::Product(_)
            | Mode::Zip(_)
            | Mode::Concat(_)
            | Mode::Permutations(_)
            | Mode::Combinations(_)
            | Mode::Variations(_) => {}
        }
    }
    let (common, sep, op) = resolve(cli);
    let json_errors = matches!(common.format, OutFormat::Jsonl | OutFormat::Json);
    if let Err(e) = run(common, sep, op) {
        eprintln!("{}", render(&e, json_errors));
        std::process::exit(exit_code(&e));
    }
}

fn run_join(args: &JoinArgs) -> Result<(), AppError> {
    let common = &args.common;
    validate_resource_limits(common)?;
    if args.max_join_records > HARD_MAX_JOIN_RECORDS
        || args.max_join_key_fanout > HARD_MAX_JOIN_KEY_FANOUT
    {
        return Err(AppError::usage(
            "RESOURCE_LIMIT_TOO_HIGH",
            "join resource limit exceeds the compiled security ceiling",
        ));
    }
    let deadline = execution_deadline(common.timeout_ms)?;
    if !common.list.is_empty() || !common.file.is_empty() {
        return Err(AppError::usage(
            "JOIN_SOURCE_INVALID",
            "join uses --left and --right instead of --list or --file",
        ));
    }
    if common.format != OutFormat::Jsonl {
        return Err(AppError::usage(
            "JOIN_FORMAT_INVALID",
            "joins require --format jsonl",
        ));
    }
    if args.left_key.is_empty() || args.right_key.is_empty() {
        return Err(AppError::usage(
            "JOIN_KEY_INVALID",
            "join keys must not be empty",
        ));
    }
    let mut budget = InputBudget::new(
        common.max_input_bytes.saturating_mul(2),
        common
            .max_total_items
            .min(args.max_join_records.saturating_mul(2)),
    );
    let limits = InputLimits {
        max_input_bytes: common.max_input_bytes,
        max_item_bytes: common.max_item_bytes,
        max_items_per_list: common.max_items_per_list.min(args.max_join_records),
    };
    let left = read_join_records(&args.left, args.join_format, limits, &mut budget)?;
    let right = read_join_records(&args.right, args.join_format, limits, &mut budget)?;
    let kind = match args.join_type {
        JoinTypeArg::Inner => combinator_core::JoinType::Inner,
        JoinTypeArg::Left => combinator_core::JoinType::Left,
        JoinTypeArg::Full => combinator_core::JoinType::Full,
        JoinTypeArg::Anti => combinator_core::JoinType::Anti,
    };
    if common.count_only {
        let count = combinator_core::join_count_with_fanout(
            &left,
            &right,
            &args.left_key,
            &args.right_key,
            kind,
            common.max_combinations,
            args.max_join_key_fanout,
        )?;
        println!("{count}");
        return Ok(());
    }
    let mut output_file = common
        .output
        .as_deref()
        .map(|path| OutputFile::open(path, common.overwrite))
        .transpose()?;
    let mut writer = match output_file.as_mut() {
        Some(file) => BufWriter::new(OutputWriter::File(file.file_mut())),
        None => BufWriter::new(OutputWriter::Stdout(std::io::stdout())),
    };
    let mut bytes = 0u64;
    combinator_core::join_each_with_fanout(
        &left,
        &right,
        &args.left_key,
        &args.right_key,
        kind,
        common.offset,
        common.limit,
        common.max_combinations,
        args.max_join_key_fanout,
        Some(&|| deadline_expired(deadline)),
        |record| {
            let object = record
                .fields
                .iter()
                .map(|(key, value)| (key, value))
                .collect::<std::collections::BTreeMap<_, _>>();
            let line = serde_json::to_string(&object)
                .map_err(|e| AppError::runtime("JOIN_OUTPUT_INVALID", e.to_string()))?;
            let size = u64::try_from(line.len() + 1).map_err(|_| {
                AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
            })?;
            bytes = bytes.checked_add(size).ok_or_else(|| {
                AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
            })?;
            if bytes > common.max_output_bytes {
                return Err(AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output exceeds the configured byte limit",
                ));
            }
            writeln!(writer, "{line}")
                .map_err(|e| AppError::runtime("WRITE_FAILED", e.to_string()))?;
            Ok(())
        },
    )?;
    writer
        .flush()
        .map_err(|e| AppError::runtime("WRITE_FAILED", e.to_string()))?;
    drop(writer);
    if let Some(file) = output_file {
        file.commit()?;
    }
    Ok(())
}

fn read_join_records(
    path: &str,
    format: JoinFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<combinator_core::Record>, AppError> {
    let mut bytes = Vec::new();
    if path == "-" {
        std::io::stdin()
            .take((limits.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| AppError::runtime("FILE_UNREADABLE", e.to_string()))?;
    } else {
        std::fs::File::open(path)
            .map_err(|e| AppError::runtime("FILE_UNREADABLE", e.to_string()).with("path", path))?
            .take((limits.max_input_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| AppError::runtime("FILE_UNREADABLE", e.to_string()).with("path", path))?;
    }
    if bytes.len() > limits.max_input_bytes {
        return Err(AppError::runtime(
            "INPUT_TOO_LARGE",
            "join input exceeds the input byte limit",
        )
        .with("path", path));
    }
    budget
        .consume_bytes(bytes.len(), path)
        .map_err(crate::error::from_codec)?;
    match format {
        JoinFormat::Jsonl => parse_join_jsonl(&bytes, path, limits, budget),
        JoinFormat::Csv | JoinFormat::Tsv => parse_join_csv(&bytes, path, format, limits, budget),
    }
}

fn parse_join_jsonl(
    bytes: &[u8],
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<combinator_core::Record>, AppError> {
    let mut out = Vec::new();
    for (line_no, line) in bytes.split(|b| *b == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|e| {
            AppError::usage("JSONL_MALFORMED", e.to_string())
                .with("path", path)
                .with("line", line_no + 1)
        })?;
        let object = value.as_object().ok_or_else(|| {
            AppError::usage("JOIN_RECORD_INVALID", "JSONL join records must be objects")
                .with("path", path)
                .with("line", line_no + 1)
        })?;
        let mut fields = Vec::new();
        for (key, value) in object {
            let value = value.as_str().ok_or_else(|| {
                AppError::usage("JOIN_FIELD_INVALID", "join fields must be JSON strings")
                    .with("path", path)
                    .with("field", key)
            })?;
            if key.len() > limits.max_item_bytes || value.len() > limits.max_item_bytes {
                return Err(AppError::runtime(
                    "ITEM_TOO_LARGE",
                    "join field exceeds the item byte limit",
                )
                .with("path", path));
            }
            fields.push((key.clone(), value.to_string()));
        }
        if out.len() >= limits.max_items_per_list {
            return Err(
                AppError::runtime("TOO_MANY_ITEMS", "join input exceeds the item limit")
                    .with("path", path),
            );
        }
        budget
            .consume_item(path)
            .map_err(crate::error::from_codec)?;
        out.push(combinator_core::Record { fields });
    }
    Ok(out)
}

fn parse_join_csv(
    bytes: &[u8],
    path: &str,
    format: JoinFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<combinator_core::Record>, AppError> {
    let delimiter = if format == JoinFormat::Tsv {
        b'\t'
    } else {
        b','
    };
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|e| AppError::usage("CSV_MALFORMED", e.to_string()).with("path", path))?
        .clone();
    if headers.is_empty() || headers.iter().any(|h| h.is_empty()) {
        return Err(
            AppError::usage("JOIN_SCHEMA_INVALID", "join headers must be non-empty")
                .with("path", path),
        );
    }
    let mut out = Vec::new();
    for result in reader.records() {
        let record = result
            .map_err(|e| AppError::usage("CSV_MALFORMED", e.to_string()).with("path", path))?;
        if record.len() != headers.len() {
            return Err(AppError::usage(
                "JOIN_SCHEMA_INVALID",
                "join row does not match the header",
            )
            .with("path", path));
        }
        let fields = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        if fields.iter().any(|(key, value)| {
            key.len() > limits.max_item_bytes || value.len() > limits.max_item_bytes
        }) {
            return Err(AppError::runtime(
                "ITEM_TOO_LARGE",
                "join field exceeds the item byte limit",
            )
            .with("path", path));
        }
        if out.len() >= limits.max_items_per_list {
            return Err(
                AppError::runtime("TOO_MANY_ITEMS", "join input exceeds the item limit")
                    .with("path", path),
            );
        }
        budget
            .consume_item(path)
            .map_err(crate::error::from_codec)?;
        out.push(combinator_core::Record { fields });
    }
    Ok(out)
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
        Some(Mode::Permutations(args)) => {
            let common = args.common;
            let options = selection_options(&common);
            selection_operation(common, Operation::Permutations(options), "")
        }
        Some(Mode::Combinations(args)) => {
            let common = args.common;
            let options = selection_options(&common);
            selection_operation(
                common,
                Operation::Combinations {
                    choose: args.choose,
                    options,
                },
                "",
            )
        }
        Some(Mode::Variations(args)) => {
            let common = args.common;
            let options = selection_options(&common);
            selection_operation(
                common,
                Operation::Variations {
                    length: args.length,
                    options,
                },
                "",
            )
        }
        Some(Mode::Join(_)) => unreachable!("handled in main"),
        Some(Mode::Completions { .. } | Mode::Man) => unreachable!("handled in main"),
        None => product_operation(cli.product),
    }
}

fn selection_options(common: &CommonArgs) -> SelectionOptions {
    SelectionOptions {
        reverse: common.reverse,
        offset: common.offset,
        limit: common.limit,
    }
}

fn selection_operation(
    common: CommonArgs,
    operation: Operation,
    sep: &str,
) -> (CommonArgs, String, Operation) {
    (common, sep.to_string(), operation)
}

const MAX_FILTER_BYTES: usize = 4096;

fn parse_filters(expressions: &[String]) -> Result<Vec<Constraint>, AppError> {
    if expressions.len() > MAX_TRANSFORMS {
        return Err(AppError::usage(
            "FILTER_LIMIT",
            "the number of filters exceeds the security limit",
        )
        .with("observed", expressions.len())
        .with("limit", MAX_TRANSFORMS));
    }
    expressions
        .iter()
        .map(|expression| parse_filter(expression))
        .collect()
}

fn parse_filter(expression: &str) -> Result<Constraint, AppError> {
    if expression.len() > MAX_FILTER_BYTES {
        return Err(AppError::usage(
            "FILTER_INVALID",
            "filter expression exceeds the byte limit",
        ));
    }
    let (kind, body) = expression.split_once(':').ok_or_else(|| {
        AppError::usage("FILTER_INVALID", "filter must use KIND:FIELD=VALUE syntax")
    })?;
    let (field, value) = body.split_once('=').ok_or_else(|| {
        AppError::usage("FILTER_INVALID", "filter must use KIND:FIELD=VALUE syntax")
    })?;
    let field = field.parse::<usize>().map_err(|_| {
        AppError::usage(
            "FILTER_INVALID",
            "filter field must be a non-negative integer",
        )
    })?;
    let constraint = match kind {
        "eq" | "equals" => Constraint::Equals {
            field,
            value: value.to_string(),
        },
        "prefix" => Constraint::Prefix {
            field,
            value: value.to_string(),
        },
        "suffix" => Constraint::Suffix {
            field,
            value: value.to_string(),
        },
        "glob" => Constraint::Glob {
            field,
            pattern: value.to_string(),
        },
        "length" => {
            let (min, max) = value.split_once("..").ok_or_else(|| {
                AppError::usage("FILTER_INVALID", "length filter must use MIN..MAX")
            })?;
            let min = min.parse::<usize>().map_err(|_| {
                AppError::usage(
                    "FILTER_INVALID",
                    "length minimum must be a non-negative integer",
                )
            })?;
            let max = max.parse::<usize>().map_err(|_| {
                AppError::usage(
                    "FILTER_INVALID",
                    "length maximum must be a non-negative integer",
                )
            })?;
            Constraint::Length { field, min, max }
        }
        _ => {
            return Err(AppError::usage(
                "FILTER_INVALID",
                "unsupported filter kind; use eq, prefix, suffix, glob, or length",
            ))
        }
    };
    constraint.validate()?;
    Ok(constraint)
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
    let deadline = execution_deadline(common.timeout_ms)?;
    check_deadline(deadline)?;
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
    let constraints = parse_filters(&common.filters)?;
    if !constraints.is_empty() && (common.count_only || common.explain || common.dry_run) {
        return Err(AppError::usage(
            "FILTER_MODE_UNSUPPORTED",
            "--filter cannot be combined with --count-only, --explain, or --dry-run",
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
    check_deadline(deadline)?;
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
    check_deadline(deadline)?;

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

    combinator_core::validate_operation(&op, &lists)?;
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
    stream_core(
        &common,
        &sep,
        &op,
        &lists,
        template.as_ref(),
        deadline,
        &constraints,
    )
}

#[derive(Debug, Default)]
struct OutputSummary {
    records: u128,
    bytes: u64,
}

fn stream_core(
    common: &CommonArgs,
    sep: &str,
    op: &Operation,
    lists: &[Vec<String>],
    template: Option<&Template>,
    deadline: Option<Instant>,
    constraints: &[Constraint],
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
    let cancel = || deadline_expired(deadline);
    let mut summary = OutputSummary {
        records: 0,
        bytes: 0,
    };
    let generation = generate_with(
        GenerationRequest {
            operation: op,
            lists,
            constraints,
            limits: GenerationLimits {
                max_combinations: common.max_combinations,
            },
            cancel: Some(&cancel),
        },
        |record| {
            let items: Vec<&str> = record
                .fields
                .iter()
                .map(|(list, item)| lists[*list][*item].as_str())
                .collect();
            let line = format_record_with(
                &items,
                record.ordinal,
                sep,
                &common.rec_sep,
                common.format.into(),
                common.lean_output,
                template,
                &common.names,
            )
            .map_err(|_| AppError::runtime("TEMPLATE_INVALID", "template rendering failed"))?;
            let size = u64::try_from(line.len()).map_err(|_| {
                AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output record is too large to write",
                )
            })?;
            let next = summary.bytes.checked_add(size).ok_or_else(|| {
                AppError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
            })?;
            let output_limit = effective_output_limit(common).unwrap_or(u64::MAX);
            if next > output_limit {
                return Err(AppError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output exceeds the configured byte limit",
                )
                .with("written_bytes", summary.bytes)
                .with("record_bytes", size)
                .with("limit_bytes", output_limit));
            }
            writer.write_all(line.as_bytes()).map_err(write_err)?;
            summary.records = summary.records.checked_add(1).ok_or_else(|| {
                AppError::runtime("COUNT_OVERFLOW", "written record count overflowed")
            })?;
            summary.bytes = next;
            Ok(())
        },
    );
    match generation {
        Err(error) if common.output.is_none() && error.message.contains("os error 232") => {
            return Ok(())
        }
        Ok(_) => {}
        Err(error) => return Err(error),
    }
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
            summary.records,
            summary.bytes
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
        Operation::Permutations(_) => "permutations",
        Operation::Combinations { .. } => "combinations",
        Operation::Variations { .. } => "variations",
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
        Operation::Permutations(options) if options.reverse => "reverse",
        Operation::Permutations(_) => "forward",
        Operation::Combinations { options, .. } if options.reverse => "reverse",
        Operation::Combinations { .. } => "forward",
        Operation::Variations { options, .. } if options.reverse => "reverse",
        Operation::Variations { .. } => "forward",
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
        Operation::Permutations(options) => {
            options.offset = offset;
            options.limit = Some(limit);
        }
        Operation::Combinations { options, .. } => {
            options.offset = offset;
            options.limit = Some(limit);
        }
        Operation::Variations { options, .. } => {
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
    if let Some(timeout) = common.timeout_ms {
        if timeout > HARD_MAX_TIMEOUT_MS {
            return Err(AppError::usage(
                "RESOURCE_LIMIT_TOO_HIGH",
                "timeout-ms exceeds the hard security ceiling",
            )
            .with("flag", "timeout-ms")
            .with("requested", timeout)
            .with("hard_limit", HARD_MAX_TIMEOUT_MS));
        }
    }
    Ok(())
}

fn execution_deadline(timeout_ms: Option<u64>) -> Result<Option<Instant>, AppError> {
    timeout_ms
        .map(|milliseconds| {
            Instant::now()
                .checked_add(Duration::from_millis(milliseconds))
                .ok_or_else(|| {
                    AppError::runtime("TIMEOUT_INVALID", "execution deadline overflowed")
                })
        })
        .transpose()
}

fn deadline_expired(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

fn check_deadline(deadline: Option<Instant>) -> Result<(), AppError> {
    if deadline_expired(deadline) {
        Err(AppError::runtime("CANCELLED", "execution was cancelled"))
    } else {
        Ok(())
    }
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
            Operation::Product(_)
            | Operation::Zip(_)
            | Operation::Permutations(_)
            | Operation::Combinations { .. }
            | Operation::Variations { .. } => lists
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
    // Treat Windows drive-qualified paths consistently when tests or callers
    // pass them to a build running on another host OS. On Unix, `Z:\\...`
    // would otherwise be treated as an ordinary relative filename and the
    // current directory would yield an incorrect capacity result.
    let bytes = path.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return Err(AppError::runtime(
            "CAPACITY_UNKNOWN",
            format!("could not determine available space for output path {path}"),
        ));
    }
    let parent = std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // `fs2::available_space` may report the drive's capacity for a path whose
    // intermediate directories do not exist (notably on Windows). Walk to an
    // existing ancestor first so a missing drive or root is reported as an
    // unknown capacity instead of being mistaken for a valid destination.
    let dir = parent
        .ancestors()
        .find(|candidate| candidate.exists())
        .ok_or_else(|| {
            AppError::runtime(
                "CAPACITY_UNKNOWN",
                format!("could not determine available space for output path {path}"),
            )
        })?;
    fs2::available_space(&dir).map_err(|e| {
        AppError::runtime(
            "CAPACITY_UNKNOWN",
            format!("could not determine available disk space: {e}"),
        )
        .with("path", dir.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn common() -> CommonArgs {
        Cli::parse_from(["combinator", "--list", "a,b"])
            .product
            .common
    }

    fn lists() -> Vec<Vec<String>> {
        vec![vec!["a".into(), "bb".into()], vec!["x".into(), "y".into()]]
    }

    #[test]
    fn resolves_all_operation_modes_and_order_names() {
        let (_, sep, product) = resolve(Cli::parse_from([
            "combinator",
            "--list",
            "a",
            "--sep",
            "-",
            "--reverse-fields",
        ]));
        assert_eq!(sep, "-");
        assert_eq!(operation_name(&product), "product");
        assert_eq!(ordering_name(&product), "reverse-fields");

        let cli = Cli::parse_from([
            "combinator",
            "zip",
            "--list",
            "a",
            "--list",
            "b",
            "--reverse",
        ]);
        let (_, _, zip) = resolve(cli);
        assert_eq!(operation_name(&zip), "zip");
        assert_eq!(ordering_name(&zip), "reverse");

        let cli = Cli::parse_from(["combinator", "concat", "--list", "a", "--reverse"]);
        let (_, _, concat) = resolve(cli);
        assert_eq!(operation_name(&concat), "concat");
        assert_eq!(ordering_name(&concat), "reverse");

        let (_, _, forward) = resolve(Cli::parse_from(["combinator", "--list", "a"]));
        assert_eq!(ordering_name(&forward), "forward");
    }

    #[test]
    fn validates_all_shard_argument_shapes_and_applies_each_operation() {
        let mut missing = common();
        missing.shard_index = Some(0);
        assert_eq!(
            validate_shard_args(&missing).unwrap_err().code,
            "SHARD_ARGUMENTS_INCOMPLETE"
        );

        let mut zero = common();
        zero.shard_index = Some(0);
        zero.shard_count = Some(0);
        assert_eq!(
            validate_shard_args(&zero).unwrap_err().code,
            "SHARD_COUNT_INVALID"
        );

        let mut out_of_range = common();
        out_of_range.shard_index = Some(2);
        out_of_range.shard_count = Some(2);
        assert_eq!(
            validate_shard_args(&out_of_range).unwrap_err().code,
            "SHARD_INDEX_INVALID"
        );

        for operation in [
            Operation::Product(ProductOptions::default()),
            Operation::Zip(ZipOptions::default()),
            Operation::Concat(ConcatOptions::default()),
        ] {
            let mut args = common();
            args.shard_index = Some(1);
            args.shard_count = Some(2);
            let range = apply_shard(&mut args, &mut operation.clone(), Count::Exact(6)).unwrap();
            assert_eq!(range.unwrap().start, 3);
            assert_eq!(args.offset, 3);
            assert_eq!(args.limit, Some(3));
        }

        let mut args = common();
        args.shard_index = Some(0);
        args.shard_count = Some(2);
        let mut operation = Operation::Product(ProductOptions::default());
        assert_eq!(
            apply_shard(&mut args, &mut operation, Count::Overflow)
                .unwrap_err()
                .code,
            "SHARD_COUNT_OVERFLOW"
        );
    }

    #[test]
    fn parses_join_jsonl_csv_and_tsv_records() {
        let limits = InputLimits::default();
        let mut budget = InputBudget::new(256, 10);
        let json = parse_join_jsonl(
            b"\r\n{\"id\":\"1\",\"name\":\"A\"}\r\n",
            "memory",
            limits,
            &mut budget,
        )
        .unwrap();
        assert_eq!(json.len(), 1);
        assert_eq!(json[0].fields[0].0, "id");

        let mut budget = InputBudget::new(256, 10);
        let csv = parse_join_csv(
            b"id,name\n1,A\n",
            "memory",
            JoinFormat::Csv,
            limits,
            &mut budget,
        )
        .unwrap();
        assert_eq!(
            csv[0].fields,
            [("id".into(), "1".into()), ("name".into(), "A".into())]
        );

        let mut budget = InputBudget::new(256, 10);
        let tsv = parse_join_csv(
            b"id\tname\n1\tA\n",
            "memory",
            JoinFormat::Tsv,
            limits,
            &mut budget,
        )
        .unwrap();
        assert_eq!(tsv.len(), 1);
    }

    #[test]
    fn template_loading_and_error_mapping_are_stable() {
        let args = common();
        assert!(load_template(&args, "").unwrap().is_none());

        let mut conflict = common();
        conflict.template = Some("{0}".into());
        conflict.template_file = Some("unused".into());
        assert_eq!(
            load_template(&conflict, "").unwrap_err().code,
            "TEMPLATE_CONFLICT"
        );

        let mut separator = common();
        separator.template = Some("{0}".into());
        assert_eq!(
            load_template(&separator, "-").unwrap_err().code,
            "TEMPLATE_SEPARATOR_CONFLICT"
        );

        let mut too_large = common();
        too_large.max_input_bytes = 2;
        too_large.template = Some("{0}".into());
        assert_eq!(
            load_template(&too_large, "").unwrap_err().code,
            "TEMPLATE_TOO_LARGE"
        );

        for error in [
            TemplateError::InvalidSyntax { position: 1 },
            TemplateError::InvalidReference { position: 2 },
            TemplateError::InvalidName { position: 3 },
            TemplateError::DuplicateName { position: 4 },
            TemplateError::UnknownField { position: 5 },
        ] {
            assert!(template_error(error)
                .context
                .iter()
                .any(|(key, _)| key == "position"));
        }
        let mismatch = template_error(TemplateError::NameCountMismatch {
            expected: 2,
            actual: 1,
        });
        assert_eq!(mismatch.code, "TEMPLATE_NAMES_MISMATCH");
        assert_eq!(mismatch.context.len(), 2);
    }

    #[test]
    fn validates_resource_limits_and_deadline_boundaries() {
        let mut args = common();
        args.max_output_bytes = HARD_MAX_OUTPUT_BYTES + 1;
        assert_eq!(
            validate_resource_limits(&args).unwrap_err().code,
            "RESOURCE_LIMIT_TOO_HIGH"
        );

        let mut args = common();
        args.max_file_size = Some(HARD_MAX_OUTPUT_BYTES + 1);
        assert_eq!(
            validate_resource_limits(&args).unwrap_err().code,
            "RESOURCE_LIMIT_TOO_HIGH"
        );

        let mut args = common();
        args.timeout_ms = Some(HARD_MAX_TIMEOUT_MS + 1);
        assert_eq!(
            validate_resource_limits(&args).unwrap_err().code,
            "RESOURCE_LIMIT_TOO_HIGH"
        );

        assert!(execution_deadline(None).unwrap().is_none());
        assert!(execution_deadline(Some(1)).unwrap().is_some());
        assert!(!deadline_expired(None));
        assert!(check_deadline(Some(Instant::now() - Duration::from_secs(1))).is_err());
        assert!(check_deadline(None).is_ok());
    }

    #[test]
    fn bounded_estimates_cover_formats_templates_and_windows() {
        let mut args = common();
        args.format = OutFormat::Jsonl;
        args.lean_output = true;
        args.limit = Some(1);
        let operation = Operation::Product(ProductOptions::default());
        assert!(matches!(
            bounded_size_estimate(&args, "-", &operation, &lists(), true, None),
            SizeEstimate::Bytes(_)
        ));

        args.format = OutFormat::Csv;
        args.limit = None;
        assert!(matches!(
            bounded_size_estimate(&args, "", &operation, &lists(), false, None),
            SizeEstimate::Bytes(_)
        ));

        let template = Template::parse("prefix-{0}-{0}").unwrap();
        assert!(matches!(
            bounded_size_estimate(&args, "", &operation, &lists(), false, Some(&template)),
            SizeEstimate::Bytes(_)
        ));

        let mut offset = common();
        offset.offset = 99;
        assert_eq!(
            bounded_size_estimate(&offset, "", &operation, &lists(), false, None),
            SizeEstimate::Bytes(0)
        );

        assert_eq!(
            effective_output_limit(&common()),
            Some(cli::DEFAULT_MAX_OUTPUT_BYTES)
        );
        let mut file_limit = common();
        file_limit.output = Some("out".into());
        file_limit.max_file_size = Some(7);
        assert_eq!(effective_output_limit(&file_limit), Some(7));
    }

    #[test]
    fn warning_and_error_helpers_preserve_contracts() {
        let mut args = common();
        args.warnings_as_errors = true;
        let warnings = vec![("EMPTY_LIST", "empty", vec![("list".into(), "0".into())])];
        assert_eq!(
            emit_warnings(&args, &warnings, false).unwrap_err().code,
            "EMPTY_LIST"
        );

        assert!(is_broken_pipe(&std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe"
        )));
        assert!(!is_broken_pipe(&std::io::Error::other("other")));
        assert_eq!(write_err(std::io::Error::other("x")).code, "WRITE_FAILED");
        assert!(available_space(".").unwrap() > 0);
        assert!(available_space("Z:\\missing\\directory\\output").is_err());
    }

    #[test]
    fn run_rejects_invalid_mode_combinations_before_touching_inputs() {
        let cases = [
            {
                let mut args = common();
                args.format = OutFormat::Json;
                (args, String::new(), "FORMAT_UNSUPPORTED")
            },
            {
                let mut args = common();
                args.file = vec!["-".into(), "-".into()];
                (args, String::new(), "DUPLICATE_STDIN")
            },
            {
                let mut args = common();
                args.input_format = Some(InputFormat::Inline);
                args.file = vec!["missing".into()];
                (args, String::new(), "INPUT_FORMAT_INVALID")
            },
            {
                let mut args = common();
                args.input_format = Some(InputFormat::Csv);
                (args, String::new(), "INPUT_FORMAT_INVALID")
            },
            {
                let mut args = common();
                args.count_only = true;
                args.explain = true;
                (args, String::new(), "MODE_CONFLICT")
            },
            {
                let mut args = common();
                args.transforms = (0..=MAX_TRANSFORMS).map(|_| "trim".into()).collect();
                (args, String::new(), "TRANSFORM_LIMIT")
            },
            {
                let mut args = common();
                args.list_delim.clear();
                (args, String::new(), "BAD_DELIMITER")
            },
            {
                let mut args = common();
                args.max_lists = 0;
                (args, String::new(), "TOO_MANY_LISTS")
            },
            {
                let mut args = common();
                args.file = vec!["missing".into()];
                (args, String::new(), "SOURCE_CONFLICT")
            },
            {
                let mut args = common();
                args.list.clear();
                (args, String::new(), "NO_LISTS")
            },
        ];
        for (args, sep, code) in cases {
            assert_eq!(
                run(args, sep, Operation::Product(ProductOptions::default()))
                    .unwrap_err()
                    .code,
                code
            );
        }

        let mut reverse = common();
        reverse.reverse = true;
        let operation = Operation::Product(ProductOptions {
            reverse: true,
            reverse_fields: true,
            ..Default::default()
        });
        assert_eq!(
            run(reverse, String::new(), operation).unwrap_err().code,
            "REVERSE_CONFLICT"
        );
    }

    #[test]
    fn run_reads_inline_file_and_mixed_sources_in_dry_run_mode() {
        let path = std::env::temp_dir().join(format!(
            "combinator_main_sources_{}.txt",
            std::process::id()
        ));
        std::fs::write(&path, "x\ny\n").unwrap();
        let mut inline = common();
        inline.dry_run = true;
        inline.input_format = Some(InputFormat::Inline);
        run(
            inline,
            String::new(),
            Operation::Product(ProductOptions::default()),
        )
        .unwrap();

        let mut lines = common();
        lines.dry_run = true;
        lines.list.clear();
        lines.file = vec![path.to_str().unwrap().into()];
        lines.input_format = Some(InputFormat::Lines);
        run(
            lines,
            String::new(),
            Operation::Product(ProductOptions::default()),
        )
        .unwrap();

        let csv_path = path.with_extension("csv");
        std::fs::write(&csv_path, "x\ny\n").unwrap();
        let mut csv = common();
        csv.dry_run = true;
        csv.list.clear();
        csv.file = vec![csv_path.to_str().unwrap().into()];
        csv.input_format = Some(InputFormat::Csv);
        run(
            csv,
            String::new(),
            Operation::Product(ProductOptions::default()),
        )
        .unwrap();

        let nul_path = path.with_extension("nul");
        std::fs::write(&nul_path, b"x\0y\0").unwrap();
        let mut nul = common();
        nul.dry_run = true;
        nul.list.clear();
        nul.file = vec![nul_path.to_str().unwrap().into()];
        nul.input_format = Some(InputFormat::Nul);
        run(
            nul,
            String::new(),
            Operation::Product(ProductOptions::default()),
        )
        .unwrap();

        let mut mixed = common();
        mixed.dry_run = true;
        mixed.allow_mixed_inputs = true;
        mixed.file = vec![path.to_str().unwrap().into()];
        run(
            mixed,
            String::new(),
            Operation::Product(ProductOptions::default()),
        )
        .unwrap();
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(csv_path);
        let _ = std::fs::remove_file(nul_path);
    }

    #[test]
    fn join_reader_reports_file_and_record_limits() {
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_join_records(
                "missing",
                JoinFormat::Jsonl,
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "FILE_UNREADABLE"
        );

        let path = std::env::temp_dir().join(format!(
            "combinator_join_limits_{}.jsonl",
            std::process::id()
        ));
        std::fs::write(&path, "[]\n").unwrap();
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_join_records(
                path.to_str().unwrap(),
                JoinFormat::Jsonl,
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "JOIN_RECORD_INVALID"
        );
        std::fs::write(&path, "{\"id\":\"long\"}\n").unwrap();
        let limits = InputLimits {
            max_item_bytes: 2,
            ..Default::default()
        };
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_join_records(
                path.to_str().unwrap(),
                JoinFormat::Jsonl,
                limits,
                &mut budget
            )
            .unwrap_err()
            .code,
            "ITEM_TOO_LARGE"
        );
        std::fs::write(&path, ",value\n1,x\n").unwrap();
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_join_records(
                path.to_str().unwrap(),
                JoinFormat::Csv,
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "JOIN_SCHEMA_INVALID"
        );
        let _ = std::fs::remove_file(path);
    }
}
