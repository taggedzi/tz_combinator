//! Bounded, format-aware input parsing over generic readers.

use crate::CodecError;
use std::io::Read;

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
    pub fn consume_bytes(&mut self, amount: usize, source: &str) -> Result<(), CodecError> {
        if amount > self.remaining_bytes {
            return Err(CodecError::runtime(
                "INPUT_TOO_LARGE",
                "aggregate input exceeds the byte limit",
            )
            .with("observed", amount)
            .with("path", source));
        }
        self.remaining_bytes -= amount;
        Ok(())
    }
    pub fn consume_item(&mut self, source: &str) -> Result<(), CodecError> {
        if self.remaining_items == 0 {
            return Err(CodecError::runtime(
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
) -> Result<Vec<String>, CodecError> {
    if value.len() > limits.max_input_bytes {
        return Err(CodecError::runtime(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
        )
        .with("observed", value.len()));
    }
    budget.consume_bytes(value.len(), "inline")?;
    let mut out = Vec::new();
    for part in value.split(delim) {
        add_item(&mut out, part.to_string(), limits, budget, "inline")?;
    }
    Ok(out)
}

pub fn split_escaped_inline(
    value: &str,
    delim: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CodecError> {
    if value.len() > limits.max_input_bytes {
        return Err(CodecError::runtime(
            "INPUT_TOO_LARGE",
            "inline list exceeds the input byte limit",
        )
        .with("observed", value.len()));
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut pos = 0;
    while pos < value.len() {
        let ch = value[pos..].chars().next().expect("UTF-8 boundary");
        if ch == '\\' {
            pos += 1;
            let escaped = value[pos..].chars().next().ok_or_else(|| {
                CodecError::usage(
                    "INLINE_ESCAPE_INVALID",
                    "inline input ends with an incomplete escape",
                )
            })?;
            pos += escaped.len_utf8();
            match escaped {
                'n' => current.push('\n'),
                'r' => current.push('\r'),
                't' => current.push('\t'),
                '0' => current.push('\0'),
                '\\' => current.push('\\'),
                'x' => {
                    let hi = value[pos..].chars().next().ok_or_else(|| {
                        CodecError::usage(
                            "INLINE_ESCAPE_INVALID",
                            "inline hex escape is incomplete",
                        )
                    })?;
                    pos += hi.len_utf8();
                    let lo = value[pos..].chars().next().ok_or_else(|| {
                        CodecError::usage(
                            "INLINE_ESCAPE_INVALID",
                            "inline hex escape is incomplete",
                        )
                    })?;
                    pos += lo.len_utf8();
                    let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).map_err(|_| {
                        CodecError::usage("INLINE_ESCAPE_INVALID", "inline hex escape is invalid")
                    })?;
                    current.push(char::from(byte));
                }
                other => current.push(other),
            }
        } else if !delim.is_empty() && value[pos..].starts_with(delim) {
            add_item(
                &mut out,
                std::mem::take(&mut current),
                limits,
                budget,
                "inline",
            )?;
            pos += delim.len();
        } else {
            current.push(ch);
            pos += ch.len_utf8();
        }
    }
    add_item(&mut out, current, limits, budget, "inline")?;
    Ok(out)
}

pub fn read_lines<R: Read>(
    mut reader: R,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CodecError> {
    let bytes = read_bytes(&mut reader, source, limits.max_input_bytes, budget)?;
    parse_separated(&bytes, b'\n', source, limits, budget)
}

pub fn read_formatted<R: Read>(
    mut reader: R,
    source: &str,
    format: InputFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CodecError> {
    let bytes = read_bytes(&mut reader, source, limits.max_input_bytes, budget)?;
    match format {
        InputFormat::Lines => parse_separated(&bytes, b'\n', source, limits, budget),
        InputFormat::Nul => parse_separated(&bytes, 0, source, limits, budget),
        InputFormat::Csv => parse_csv(&bytes, b',', source, limits, budget),
        InputFormat::Tsv => parse_csv(&bytes, b'\t', source, limits, budget),
    }
}

fn read_bytes<R: Read>(
    reader: &mut R,
    source: &str,
    max: usize,
    budget: &mut InputBudget,
) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = reader.read(&mut chunk).map_err(|e| {
            CodecError::runtime(
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
            .ok_or_else(|| CodecError::runtime("INPUT_TOO_LARGE", "input byte count overflowed"))?;
        if next > max {
            return Err(CodecError::runtime(
                "INPUT_TOO_LARGE",
                "input exceeds the input byte limit",
            )
            .with("observed", next)
            .with("path", source));
        }
        budget.consume_bytes(n, source)?;
        out.extend_from_slice(&chunk[..n]);
    }
    Ok(out)
}

fn parse_separated(
    bytes: &[u8],
    sep: u8,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CodecError> {
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
        let value = String::from_utf8(raw.to_vec()).map_err(|_| {
            CodecError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8")
                .with("path", source)
        })?;
        add_item(&mut out, value, limits, budget, source)?;
    }
    Ok(out)
}

fn parse_csv(
    bytes: &[u8],
    sep: u8,
    source: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<String>, CodecError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .delimiter(sep)
        .from_reader(bytes);
    let mut out = Vec::new();
    for result in reader.byte_records() {
        let record = result.map_err(|e| {
            CodecError::usage("CSV_MALFORMED", format!("malformed CSV/TSV input: {e}"))
                .with("path", source)
        })?;
        if record.len() != 1 {
            return Err(CodecError::usage(
                "CSV_MULTIPLE_FIELDS",
                "CSV/TSV input records must contain one field",
            )
            .with("path", source));
        }
        let value =
            String::from_utf8(record.get(0).unwrap_or_default().to_vec()).map_err(|_| {
                CodecError::usage("INPUT_NOT_UTF8", "text input is not valid UTF-8")
                    .with("path", source)
            })?;
        add_item(&mut out, value, limits, budget, source)?;
    }
    Ok(out)
}

fn add_item(
    out: &mut Vec<String>,
    value: String,
    limits: InputLimits,
    budget: &mut InputBudget,
    source: &str,
) -> Result<(), CodecError> {
    if value.len() > limits.max_item_bytes {
        return Err(CodecError::runtime(
            "ITEM_TOO_LARGE",
            "input item exceeds the item byte limit",
        )
        .with("observed", value.len())
        .with("path", source));
    }
    if out.len() >= limits.max_items_per_list {
        return Err(
            CodecError::runtime("TOO_MANY_ITEMS", "list exceeds the maximum item count")
                .with("observed", out.len() + 1)
                .with("path", source),
        );
    }
    budget.consume_item(source)?;
    out.push(value);
    Ok(())
}
