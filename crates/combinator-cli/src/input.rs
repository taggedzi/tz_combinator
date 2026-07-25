//! Gathering lists from inline, file, and stdin sources; delimiter validation.

use std::io::{Cursor, Read};

use crate::cli::InputFormat;
use crate::error::AppError;

pub const MAX_DELIM_BYTES: usize = 4096;
pub const MAX_TEMPLATE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct InputLimits {
    pub max_input_bytes: usize,
    pub max_item_bytes: usize,
    pub max_items_per_list: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct InputBudget {
    remaining_bytes: usize,
    remaining_items: usize,
}

impl InputBudget {
    pub fn new(max_bytes: usize, max_items: usize) -> Self {
        Self {
            remaining_bytes: max_bytes,
            remaining_items: max_items,
        }
    }

    fn consume_bytes(&mut self, amount: usize, path: &str) -> Result<(), AppError> {
        if amount > self.remaining_bytes {
            return Err(input_limit(
                "INPUT_TOO_LARGE",
                "aggregate input exceeds the byte limit",
                amount,
            )
            .with("path", path));
        }
        self.remaining_bytes -= amount;
        Ok(())
    }

    fn consume_item(&mut self, path: &str) -> Result<(), AppError> {
        if self.remaining_items == 0 {
            return Err(input_limit(
                "TOO_MANY_ITEMS",
                "aggregate input exceeds the item limit",
                1,
            )
            .with("path", path));
        }
        self.remaining_items -= 1;
        Ok(())
    }
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 64 * 1024 * 1024,
            max_item_bytes: 1024 * 1024,
            max_items_per_list: 1_000_000,
        }
    }
}

/// Validates the three delimiters. All three respect the byte cap; the inline
/// list delimiter must additionally be non-empty.
pub fn validate_delims(field_sep: &str, rec_sep: &str, list_delim: &str) -> Result<(), AppError> {
    for (name, d) in [
        ("--sep", field_sep),
        ("--rec-sep", rec_sep),
        ("--list-delim", list_delim),
    ] {
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
pub fn split_inline_bounded(
    value: &str,
    delim: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if value.len() > limits.max_input_bytes {
        return Err(input_limit(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
            value.len(),
        ));
    }
    budget.consume_bytes(value.len(), "inline")?;
    let mut items = Vec::new();
    for part in value.split(delim) {
        if part.len() > limits.max_item_bytes {
            return Err(input_limit(
                "ITEM_TOO_LARGE",
                "list item exceeds the item byte limit",
                part.len(),
            ));
        }
        if items.len() == limits.max_items_per_list {
            return Err(input_limit(
                "TOO_MANY_ITEMS",
                "list exceeds the maximum item count",
                items.len() + 1,
            ));
        }
        budget.consume_item("inline")?;
        items.push(part.to_string());
    }
    Ok(items)
}

/// Parses an explicitly escaped inline list. Supported escapes are `\\`, `\\n`,
/// `\\r`, `\\t`, `\\0`, and `\\xNN`. A backslash before a delimiter makes
/// that delimiter literal. Unknown and incomplete escapes are rejected.
pub fn split_escaped_inline_bounded(
    value: &str,
    delim: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if value.len() > limits.max_input_bytes {
        return Err(input_limit(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
            value.len(),
        ));
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = value.chars().collect();
    let delim_chars: Vec<char> = delim.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 1;
            let escaped = *chars.get(i).ok_or_else(|| {
                AppError::usage(
                    "INLINE_ESCAPE_INVALID",
                    "inline input ends with an incomplete escape",
                )
            })?;
            match escaped {
                'n' => current.push('\n'),
                'r' => current.push('\r'),
                't' => current.push('\t'),
                '0' => current.push('\0'),
                '\\' => current.push('\\'),
                'x' => {
                    let hi = *chars.get(i + 1).ok_or_else(|| {
                        AppError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is incomplete")
                    })?;
                    let lo = *chars.get(i + 2).ok_or_else(|| {
                        AppError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is incomplete")
                    })?;
                    let value = [hi, lo].iter().collect::<String>();
                    let byte = u8::from_str_radix(&value, 16).map_err(|_| {
                        AppError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is invalid")
                    })?;
                    current.push(char::from(byte));
                    i += 2;
                }
                other => current.push(other),
            }
            i += 1;
            continue;
        }
        if !delim_chars.is_empty() && chars[i..].starts_with(&delim_chars) {
            finish_parsed_item(&mut items, &mut current, limits, budget, "inline")?;
            i += delim_chars.len();
        } else {
            current.push(ch);
            i += 1;
        }
    }
    finish_parsed_item(&mut items, &mut current, limits, budget, "inline")?;
    Ok(items)
}

/// Reads a file as a list, one item per line, stripping a trailing `\r`.
/// The path `-` reads standard input instead (explicit stdin only).
pub fn read_file_list_bounded(
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if path == "-" {
        return read_bounded(std::io::stdin().lock(), path, limits, budget);
    }
    let file = std::fs::File::open(path).map_err(|e| {
        AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
            .with("path", path)
    })?;
    read_bounded(file, path, limits, budget)
}

pub fn read_file_list_format_bounded(
    path: &str,
    format: InputFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    if format == InputFormat::Inline {
        return Err(AppError::usage(
            "INPUT_FORMAT_INVALID",
            "inline input format requires --list",
        ));
    }
    if format == InputFormat::Lines {
        return read_file_list_bounded(path, limits, budget);
    }
    let mut bytes = Vec::new();
    if path == "-" {
        read_bytes_bounded(std::io::stdin().lock(), path, limits, budget, &mut bytes)?;
    } else {
        let file = std::fs::File::open(path).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?;
        read_bytes_bounded(file, path, limits, budget, &mut bytes)?;
    }
    parse_source_bytes(&bytes, path, format, limits, budget)
}

fn read_bytes_bounded<R: Read>(
    mut reader: R,
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
    output: &mut Vec<u8>,
) -> Result<(), AppError> {
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).map_err(|e| {
            AppError::runtime(
                "FILE_UNREADABLE",
                format!("could not read list source: {e}"),
            )
            .with("path", path)
        })?;
        if read == 0 {
            break;
        }
        let next = output.len().checked_add(read).ok_or_else(|| {
            input_limit("INPUT_TOO_LARGE", "input byte count overflowed", usize::MAX)
        })?;
        if next > limits.max_input_bytes {
            return Err(input_limit(
                "INPUT_TOO_LARGE",
                "input exceeds the input byte limit",
                next,
            ));
        }
        budget.consume_bytes(read, path)?;
        output.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

fn parse_source_bytes(
    bytes: &[u8],
    path: &str,
    format: InputFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    match format {
        InputFormat::Lines => parse_separated_bytes(bytes, b'\n', path, limits, budget),
        InputFormat::Nul => parse_separated_bytes(bytes, 0, path, limits, budget),
        InputFormat::Csv => parse_csv_bytes(bytes, b',', path, limits, budget),
        InputFormat::Tsv => parse_csv_bytes(bytes, b'\t', path, limits, budget),
        InputFormat::Inline => unreachable!(),
    }
}

fn parse_separated_bytes(
    bytes: &[u8],
    separator: u8,
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    let mut items = Vec::new();
    let chunks = bytes.split(|byte| *byte == separator);
    let chunk_count = chunks.clone().count();
    for (index, raw) in chunks.enumerate() {
        let raw = if separator == b'\n' {
            raw.strip_suffix(b"\r").unwrap_or(raw)
        } else {
            raw
        };
        if raw.is_empty() && (bytes.is_empty() || index + 1 == chunk_count) {
            continue;
        }
        let value = String::from_utf8(raw.to_vec()).map_err(|_| {
            AppError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8").with("path", path)
        })?;
        finish_parsed_item(&mut items, &mut value.clone(), limits, budget, path)?;
    }
    Ok(items)
}

fn parse_csv_bytes(
    bytes: &[u8],
    separator: u8,
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(separator)
        .from_reader(Cursor::new(bytes));
    let mut items = Vec::new();
    for result in reader.byte_records() {
        let record = result.map_err(|error| {
            AppError::usage("CSV_MALFORMED", format!("malformed CSV/TSV input: {error}"))
                .with("path", path)
        })?;
        if record.len() != 1 {
            return Err(AppError::usage(
                "CSV_MULTIPLE_FIELDS",
                "CSV/TSV input records must contain one field",
            )
            .with("path", path));
        }
        let value = record.get(0).unwrap_or_default();
        if value.len() > limits.max_item_bytes {
            return Err(input_limit(
                "ITEM_TOO_LARGE",
                "input item exceeds the item byte limit",
                value.len(),
            ));
        }
        if items.len() >= limits.max_items_per_list {
            return Err(input_limit(
                "TOO_MANY_ITEMS",
                "list exceeds the maximum item count",
                items.len() + 1,
            ));
        }
        let value = String::from_utf8(value.to_vec()).map_err(|_| {
            AppError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8").with("path", path)
        })?;
        budget.consume_item(path)?;
        items.push(value);
    }
    Ok(items)
}

fn finish_parsed_item(
    items: &mut Vec<String>,
    value: &mut String,
    limits: InputLimits,
    budget: &mut InputBudget,
    path: &str,
) -> Result<(), AppError> {
    if value.len() > limits.max_item_bytes {
        return Err(input_limit(
            "ITEM_TOO_LARGE",
            "input item exceeds the item byte limit",
            value.len(),
        ));
    }
    if items.len() >= limits.max_items_per_list {
        return Err(input_limit(
            "TOO_MANY_ITEMS",
            "list exceeds the maximum item count",
            items.len() + 1,
        ));
    }
    budget.consume_item(path)?;
    items.push(std::mem::take(value));
    Ok(())
}

/// Reads a UTF-8 template file without retaining more than `max_bytes + 1`
/// bytes, so an oversized template is rejected before it can grow memory.
pub fn read_template_bounded(path: &str, max_bytes: usize) -> Result<String, AppError> {
    let file = std::fs::File::open(path).map_err(|e| {
        AppError::usage(
            "TEMPLATE_FILE_UNREADABLE",
            format!("could not read template file: {e}"),
        )
        .with("path", path)
    })?;
    let mut bytes = Vec::new();
    file.take(max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| {
            AppError::usage(
                "TEMPLATE_FILE_UNREADABLE",
                format!("could not read template file: {e}"),
            )
            .with("path", path)
        })?;
    if bytes.len() > max_bytes {
        return Err(AppError::usage(
            "TEMPLATE_TOO_LARGE",
            "template exceeds the configured template byte limit",
        )
        .with("observed", bytes.len())
        .with("limit", max_bytes)
        .with("path", path));
    }
    String::from_utf8(bytes).map_err(|_| {
        AppError::usage(
            "TEMPLATE_FILE_UNREADABLE",
            "template file is not valid UTF-8",
        )
        .with("path", path)
    })
}

fn read_bounded<R: Read>(
    mut reader: R,
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, AppError> {
    let mut output = Vec::new();
    let mut current = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut total = 0usize;

    loop {
        let read = reader.read(&mut chunk).map_err(|e| {
            AppError::runtime("FILE_UNREADABLE", format!("could not read list file: {e}"))
                .with("path", path)
        })?;
        if read == 0 {
            break;
        }
        total = total.checked_add(read).ok_or_else(|| {
            input_limit("INPUT_TOO_LARGE", "input byte count overflowed", usize::MAX)
        })?;
        if total > limits.max_input_bytes {
            return Err(input_limit(
                "INPUT_TOO_LARGE",
                "input exceeds the input byte limit",
                total,
            )
            .with("path", path));
        }
        budget.consume_bytes(read, path)?;
        for &byte in &chunk[..read] {
            if byte == b'\n' {
                finish_item(&mut output, &mut current, limits, path, budget)?;
            } else {
                if current.len() >= limits.max_item_bytes {
                    return Err(input_limit(
                        "ITEM_TOO_LARGE",
                        "list item exceeds the item byte limit",
                        current.len(),
                    )
                    .with("path", path));
                }
                current.push(byte);
            }
        }
    }
    if !current.is_empty() {
        finish_item(&mut output, &mut current, limits, path, budget)?;
    }
    Ok(output)
}

fn finish_item(
    output: &mut Vec<String>,
    current: &mut Vec<u8>,
    limits: InputLimits,
    path: &str,
    budget: &mut InputBudget,
) -> Result<(), AppError> {
    if current.last() == Some(&b'\r') {
        current.pop();
    }
    if output.len() == limits.max_items_per_list {
        return Err(input_limit(
            "TOO_MANY_ITEMS",
            "list exceeds the maximum item count",
            output.len() + 1,
        )
        .with("path", path));
    }
    budget.consume_item(path)?;
    let item = String::from_utf8(std::mem::take(current)).map_err(|_| {
        AppError::runtime("FILE_UNREADABLE", "list file is not valid UTF-8").with("path", path)
    })?;
    output.push(item);
    Ok(())
}

fn input_limit(code: &'static str, message: &'static str, value: usize) -> AppError {
    AppError::runtime(code, message).with("observed", value)
}

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
        let mut budget = InputBudget::new(100, 10);
        assert_eq!(
            split_inline_bounded("red,blue,green", ",", InputLimits::default(), &mut budget)
                .unwrap(),
            vec!["red", "blue", "green"]
        );
    }

    #[test]
    fn splits_inline_on_custom_delim() {
        let mut budget = InputBudget::new(100, 10);
        assert_eq!(
            split_inline_bounded("a::b", "::", InputLimits::default(), &mut budget).unwrap(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn read_missing_file_errors() {
        let mut budget = InputBudget::new(100, 10);
        let e = read_file_list_bounded(
            "does-not-exist-12345.txt",
            InputLimits::default(),
            &mut budget,
        )
        .unwrap_err();
        assert_eq!(e.code, "FILE_UNREADABLE");
        assert_eq!(e.exit, 1);
    }

    #[test]
    fn file_lines_strip_crlf() {
        // Written and read back via a temp file.
        let dir = std::env::temp_dir();
        let path = dir.join("combinator_test_crlf.txt");
        std::fs::write(&path, "a\r\nb\r\n").unwrap();
        let mut budget = InputBudget::new(1024, 10);
        let got =
            read_file_list_bounded(path.to_str().unwrap(), InputLimits::default(), &mut budget)
                .unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn aggregate_budget_rejects_before_second_inline_list_is_stored() {
        let mut budget = InputBudget::new(3, 10);
        let limits = InputLimits::default();
        split_inline_bounded("ab", ",", limits, &mut budget).unwrap();
        let error = split_inline_bounded("cd", ",", limits, &mut budget).unwrap_err();
        assert_eq!(error.code, "INPUT_TOO_LARGE");
    }
}
