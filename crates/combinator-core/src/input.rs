//! Bounded, format-aware input parsing over generic readers.

use std::io::Read;

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Lines,
    Csv,
    Tsv,
    Nul,
}

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
    pub fn consume_bytes(&mut self, amount: usize, source: &str) -> Result<(), CoreError> {
        if amount > self.remaining_bytes {
            return Err(CoreError::runtime(
                "INPUT_TOO_LARGE",
                "aggregate input exceeds the byte limit",
            )
            .with("observed", amount)
            .with("path", source));
        }
        self.remaining_bytes -= amount;
        Ok(())
    }
    pub fn consume_item(&mut self, source: &str) -> Result<(), CoreError> {
        if self.remaining_items == 0 {
            return Err(CoreError::runtime(
                "TOO_MANY_ITEMS",
                "aggregate input exceeds the item limit",
            )
            .with("observed", 1)
            .with("path", source));
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

pub fn split_inline(
    value: &str,
    delim: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    if value.len() > limits.max_input_bytes {
        return Err(CoreError::runtime(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
        )
        .with("observed", value.len()));
    }
    budget.consume_bytes(value.len(), "inline")?;
    let mut items = Vec::new();
    for part in value.split(delim) {
        add_item(&mut items, part.to_string(), limits, budget, "inline")?;
    }
    Ok(items)
}

pub fn split_escaped_inline(
    value: &str,
    delim: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    if value.len() > limits.max_input_bytes {
        return Err(CoreError::runtime(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
        )
        .with("observed", value.len()));
    }
    let mut items = Vec::new();
    let mut current = String::new();
    let mut byte_pos = 0;
    while byte_pos < value.len() {
        let ch = value[byte_pos..]
            .chars()
            .next()
            .expect("byte position is a UTF-8 character boundary");
        if ch == '\\' {
            byte_pos += ch.len_utf8();
            let escaped = value[byte_pos..].chars().next().ok_or_else(|| {
                CoreError::usage(
                    "INLINE_ESCAPE_INVALID",
                    "inline input ends with an incomplete escape",
                )
            })?;
            byte_pos += escaped.len_utf8();
            match escaped {
                'n' => current.push('\n'),
                'r' => current.push('\r'),
                't' => current.push('\t'),
                '0' => current.push('\0'),
                '\\' => current.push('\\'),
                'x' => {
                    let hi = value[byte_pos..].chars().next().ok_or_else(|| {
                        CoreError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is incomplete")
                    })?;
                    byte_pos += hi.len_utf8();
                    let lo = value[byte_pos..].chars().next().ok_or_else(|| {
                        CoreError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is incomplete")
                    })?;
                    let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).map_err(|_| {
                        CoreError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is invalid")
                    })?;
                    current.push(char::from(byte));
                    byte_pos += lo.len_utf8();
                }
                other => {
                    current.push(other);
                }
            }
            continue;
        }
        if !delim.is_empty() && value[byte_pos..].starts_with(delim) {
            add_item(
                &mut items,
                std::mem::take(&mut current),
                limits,
                budget,
                "inline",
            )?;
            byte_pos += delim.len();
        } else {
            current.push(ch);
            byte_pos += ch.len_utf8();
        }
    }
    add_item(&mut items, current, limits, budget, "inline")?;
    Ok(items)
}

pub fn read_lines<R: Read>(
    mut reader: R,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    let mut bytes = Vec::new();
    read_bytes(
        &mut reader,
        source,
        limits.max_input_bytes,
        budget,
        &mut bytes,
    )?;
    let mut items = Vec::new();
    let chunks: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
    for (i, raw) in chunks.iter().enumerate() {
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        if raw.is_empty() && (bytes.is_empty() || i + 1 == chunks.len()) {
            continue;
        }
        let value = String::from_utf8(raw.to_vec()).map_err(|_| {
            CoreError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8").with("path", source)
        })?;
        add_item(&mut items, value, limits, budget, source)?;
    }
    Ok(items)
}

pub fn read_formatted<R: Read>(
    mut reader: R,
    source: &str,
    format: InputFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    let mut bytes = Vec::new();
    read_bytes(
        &mut reader,
        source,
        limits.max_input_bytes,
        budget,
        &mut bytes,
    )?;
    parse_bytes(&bytes, source, format, limits, budget)
}

fn read_bytes<R: Read>(
    reader: &mut R,
    source: &str,
    max: usize,
    budget: &mut InputBudget,
    out: &mut Vec<u8>,
) -> Result<(), CoreError> {
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| {
            CoreError::runtime(
                "FILE_UNREADABLE",
                format!("could not read list source: {e}"),
            )
            .with("path", source)
        })?;
        if n == 0 {
            break;
        }
        let next = out
            .len()
            .checked_add(n)
            .ok_or_else(|| CoreError::runtime("INPUT_TOO_LARGE", "input byte count overflowed"))?;
        if next > max {
            return Err(CoreError::runtime(
                "INPUT_TOO_LARGE",
                "input exceeds the input byte limit",
            )
            .with("observed", next)
            .with("path", source));
        }
        budget.consume_bytes(n, source)?;
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

fn parse_bytes(
    bytes: &[u8],
    source: &str,
    format: InputFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    match format {
        InputFormat::Lines => parse_separated(bytes, b'\n', source, limits, budget),
        InputFormat::Nul => parse_separated(bytes, 0, source, limits, budget),
        InputFormat::Csv => parse_csv(bytes, b',', source, limits, budget),
        InputFormat::Tsv => parse_csv(bytes, b'\t', source, limits, budget),
    }
}
fn parse_separated(
    bytes: &[u8],
    sep: u8,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    let mut out = Vec::new();
    let mut chunks = bytes.split(|b| *b == sep).peekable();
    while let Some(raw) = chunks.next() {
        let is_last = chunks.peek().is_none();
        let raw = if sep == b'\n' {
            raw.strip_suffix(b"\r").unwrap_or(raw)
        } else {
            raw
        };
        if raw.is_empty() && (bytes.is_empty() || is_last) {
            continue;
        }
        let s = String::from_utf8(raw.to_vec()).map_err(|_| {
            CoreError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8").with("path", source)
        })?;
        add_item(&mut out, s, limits, budget, source)?;
    }
    Ok(out)
}
fn parse_csv(
    bytes: &[u8],
    sep: u8,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CoreError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(sep)
        .from_reader(bytes);
    let mut out = Vec::new();
    for result in reader.byte_records() {
        let record = result.map_err(|e| {
            CoreError::usage("CSV_MALFORMED", format!("malformed CSV/TSV input: {e}"))
                .with("path", source)
        })?;
        if record.len() != 1 {
            return Err(CoreError::usage(
                "CSV_MULTIPLE_FIELDS",
                "CSV/TSV input records must contain one field",
            )
            .with("path", source));
        }
        let s = String::from_utf8(record.get(0).unwrap_or_default().to_vec()).map_err(|_| {
            CoreError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8").with("path", source)
        })?;
        add_item(&mut out, s, limits, budget, source)?;
    }
    Ok(out)
}
fn add_item(
    out: &mut Vec<String>,
    value: String,
    limits: InputLimits,
    budget: &mut InputBudget,
    source: &str,
) -> Result<(), CoreError> {
    if value.len() > limits.max_item_bytes {
        return Err(
            CoreError::runtime("ITEM_TOO_LARGE", "input item exceeds the item byte limit")
                .with("observed", value.len())
                .with("path", source),
        );
    }
    if out.len() >= limits.max_items_per_list {
        return Err(
            CoreError::runtime("TOO_MANY_ITEMS", "list exceeds the maximum item count")
                .with("observed", out.len() + 1)
                .with("path", source),
        );
    }
    budget.consume_item(source)?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn generic_reader_enforces_aggregate_byte_budget() {
        let limits = InputLimits {
            max_input_bytes: 16,
            ..Default::default()
        };
        let mut budget = InputBudget::new(3, 10);
        let error = read_lines(Cursor::new("ab\ncd\n"), "memory", limits, &mut budget).unwrap_err();
        assert_eq!(error.code, "INPUT_TOO_LARGE");
    }

    #[test]
    fn generic_reader_rejects_oversized_items_before_storage() {
        let limits = InputLimits {
            max_item_bytes: 2,
            ..Default::default()
        };
        let mut budget = InputBudget::new(32, 10);
        let error = read_lines(Cursor::new("abc\n"), "memory", limits, &mut budget).unwrap_err();
        assert_eq!(error.code, "ITEM_TOO_LARGE");
    }

    #[test]
    fn csv_reader_rejects_multiple_fields() {
        let mut budget = InputBudget::new(32, 10);
        let error = read_formatted(
            Cursor::new("a,b\n"),
            "memory",
            InputFormat::Csv,
            InputLimits::default(),
            &mut budget,
        )
        .unwrap_err();
        assert_eq!(error.code, "CSV_MULTIPLE_FIELDS");
    }

    #[test]
    fn escaped_inline_supports_escapes_and_rejects_invalid_hex() {
        let mut budget = InputBudget::new(64, 10);
        let values =
            split_escaped_inline(r"a\n,b\x21,c\\d", ",", InputLimits::default(), &mut budget)
                .unwrap();
        assert_eq!(values, ["a\n", "b!", "c\\d"]);

        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            split_escaped_inline(r"bad\xG0", ",", InputLimits::default(), &mut budget)
                .unwrap_err()
                .code,
            "INLINE_ESCAPE_INVALID"
        );
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            split_escaped_inline("trailing\\", ",", InputLimits::default(), &mut budget)
                .unwrap_err()
                .code,
            "INLINE_ESCAPE_INVALID"
        );
    }

    #[test]
    fn parses_nul_tsv_utf8_and_item_limits() {
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_formatted(
                Cursor::new(b"a\0b\0"),
                "memory",
                InputFormat::Nul,
                InputLimits::default(),
                &mut budget
            )
            .unwrap(),
            ["a", "b"]
        );
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_formatted(
                Cursor::new("a\tb\n"),
                "memory",
                InputFormat::Tsv,
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "CSV_MULTIPLE_FIELDS"
        );
        let mut budget = InputBudget::new(64, 1);
        assert_eq!(
            read_lines(
                Cursor::new("a\nb\n"),
                "memory",
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "TOO_MANY_ITEMS"
        );
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_lines(
                Cursor::new(vec![0xff]),
                "memory",
                InputLimits::default(),
                &mut budget
            )
            .unwrap_err()
            .code,
            "INPUT_NOT_UTF8"
        );
    }

    #[test]
    fn handles_crlf_blank_lines_and_exact_boundaries() {
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            read_lines(
                Cursor::new("a\r\n\r\n"),
                "memory",
                InputLimits::default(),
                &mut budget
            )
            .unwrap(),
            ["a", ""]
        );

        let limits = InputLimits {
            max_input_bytes: 3,
            ..Default::default()
        };
        let mut budget = InputBudget::new(3, 10);
        assert_eq!(
            read_lines(Cursor::new("a\nb"), "memory", limits, &mut budget).unwrap(),
            ["a", "b"]
        );

        let limits = InputLimits {
            max_input_bytes: 2,
            ..Default::default()
        };
        let mut budget = InputBudget::new(2, 10);
        assert_eq!(
            read_lines(Cursor::new("a\nb"), "memory", limits, &mut budget)
                .unwrap_err()
                .code,
            "INPUT_TOO_LARGE"
        );
    }

    #[test]
    fn empty_delimiter_and_empty_items_are_deterministic() {
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            split_inline("", ",", InputLimits::default(), &mut budget).unwrap(),
            [""]
        );
        let mut budget = InputBudget::new(64, 10);
        assert_eq!(
            split_inline("a,,b", ",", InputLimits::default(), &mut budget).unwrap(),
            ["a", "", "b"]
        );
    }
}
