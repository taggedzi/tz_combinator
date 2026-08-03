//! Interface-neutral record formatting over caller-provided values.

use crate::template::{Template, TemplateError};
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Jsonl,
    Csv,
    Tsv,
    Nul,
}

pub fn format_record(
    items: &[&str],
    index: u128,
    sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
    max_output_bytes: u128,
) -> Result<String, TemplateError> {
    format_record_with(
        items,
        index,
        sep,
        rec_sep,
        format,
        lean,
        None,
        &[],
        max_output_bytes,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn format_record_with(
    items: &[&str],
    index: u128,
    sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
    template: Option<&Template>,
    names: &[String],
    max_output_bytes: u128,
) -> Result<String, TemplateError> {
    if template.is_none() {
        match format {
            Format::Text => {
                return joined_record_bounded(items, sep, rec_sep, max_output_bytes);
            }
            Format::Nul => return joined_record_bounded(items, sep, "\0", max_output_bytes),
            Format::Csv => return csv_record(items, b',', max_output_bytes),
            Format::Tsv => return csv_record(items, b'\t', max_output_bytes),
            Format::Jsonl => {}
        }
    }
    let value = match template {
        Some(template) => template.render(items, names, max_output_bytes)?,
        None => join_bounded(items, sep, max_output_bytes)?,
    };
    let mut output = BoundedOutput::new(max_output_bytes);
    match format {
        Format::Text => {
            output.append(value.as_bytes())?;
            output.append(rec_sep.as_bytes())?;
        }
        Format::Nul => {
            output.append(value.as_bytes())?;
            output.append(b"\0")?;
        }
        Format::Jsonl if lean => {
            if serde_json::to_writer(&mut output, &value).is_err() {
                return Err(output.failure());
            }
            output.append(b"\n")?;
        }
        Format::Jsonl => {
            output.append(b"{\"i\":")?;
            if index <= u64::MAX as u128 {
                write!(&mut output, "{index}").map_err(|_| output.failure())?;
            } else {
                if serde_json::to_writer(&mut output, &index.to_string()).is_err() {
                    return Err(output.failure());
                }
            }
            output.append(b",\"value\":")?;
            if serde_json::to_writer(&mut output, &value).is_err() {
                return Err(output.failure());
            }
            output.append(b",\"fields\":")?;
            if serde_json::to_writer(&mut output, items).is_err() {
                return Err(output.failure());
            }
            if !names.is_empty() {
                output.append(b",\"named\":{")?;
                for (position, (name, item)) in names.iter().zip(items).enumerate() {
                    if position != 0 {
                        output.append(b",")?;
                    }
                    if serde_json::to_writer(&mut output, name).is_err() {
                        return Err(output.failure());
                    }
                    output.append(b":")?;
                    if serde_json::to_writer(&mut output, item).is_err() {
                        return Err(output.failure());
                    }
                }
                output.append(b"}")?;
            }
            output.append(b"}\n")?;
        }
        Format::Csv => return csv_record(items, b',', max_output_bytes),
        Format::Tsv => return csv_record(items, b'\t', max_output_bytes),
    }
    output.finish()
}

fn join_bounded(items: &[&str], sep: &str, max_bytes: u128) -> Result<String, TemplateError> {
    let capacity = joined_len(items, sep, max_bytes)?;
    let mut output = String::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| TemplateError::OutputEncoding)?;
    append_joined(&mut output, items, sep);
    Ok(output)
}

fn joined_record_bounded(
    items: &[&str],
    sep: &str,
    suffix: &str,
    max_bytes: u128,
) -> Result<String, TemplateError> {
    let joined_len = joined_len(items, sep, max_bytes)?;
    let output_len = (joined_len as u128)
        .checked_add(suffix.len() as u128)
        .ok_or(TemplateError::OutputTooLarge { limit: max_bytes })?;
    if output_len > max_bytes {
        return Err(TemplateError::OutputTooLarge { limit: max_bytes });
    }
    let capacity = usize::try_from(output_len)
        .map_err(|_| TemplateError::OutputTooLarge { limit: max_bytes })?;
    let mut output = String::new();
    output
        .try_reserve_exact(capacity)
        .map_err(|_| TemplateError::OutputEncoding)?;
    append_joined(&mut output, items, sep);
    output.push_str(suffix);
    Ok(output)
}

fn joined_len(items: &[&str], sep: &str, max_bytes: u128) -> Result<usize, TemplateError> {
    let separators = items.len().saturating_sub(1);
    let mut output_len = (separators as u128)
        .checked_mul(sep.len() as u128)
        .ok_or(TemplateError::OutputTooLarge { limit: max_bytes })?;
    for item in items {
        output_len = output_len
            .checked_add(item.len() as u128)
            .ok_or(TemplateError::OutputTooLarge { limit: max_bytes })?;
        if output_len > max_bytes {
            return Err(TemplateError::OutputTooLarge { limit: max_bytes });
        }
    }
    usize::try_from(output_len).map_err(|_| TemplateError::OutputTooLarge { limit: max_bytes })
}

fn append_joined(output: &mut String, items: &[&str], sep: &str) {
    for (position, item) in items.iter().enumerate() {
        if position != 0 {
            output.push_str(sep);
        }
        output.push_str(item);
    }
}

fn csv_record(items: &[&str], sep: u8, max_output_bytes: u128) -> Result<String, TemplateError> {
    if max_output_bytes == 0 {
        return Err(TemplateError::OutputTooLarge {
            limit: max_output_bytes,
        });
    }
    let buffer_capacity = usize::try_from(max_output_bytes.min(8 * 1024)).map_err(|_| {
        TemplateError::OutputTooLarge {
            limit: max_output_bytes,
        }
    })?;
    let mut output = BoundedOutput::new(max_output_bytes);
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .delimiter(sep)
        .terminator(csv::Terminator::Any(b'\n'))
        .buffer_capacity(buffer_capacity)
        .from_writer(&mut output);
    if writer.write_record(items).is_err() || writer.flush().is_err() {
        drop(writer);
        return Err(output.failure());
    }
    drop(writer);
    output.finish()
}

struct BoundedOutput {
    bytes: Vec<u8>,
    max_bytes: u128,
    limit_exceeded: bool,
}

impl BoundedOutput {
    fn new(max_bytes: u128) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes,
            limit_exceeded: false,
        }
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), TemplateError> {
        if self.write_all(bytes).is_err() {
            return Err(self.failure());
        }
        Ok(())
    }

    fn failure(&self) -> TemplateError {
        if self.limit_exceeded {
            TemplateError::OutputTooLarge {
                limit: self.max_bytes,
            }
        } else {
            TemplateError::OutputEncoding
        }
    }

    fn finish(self) -> Result<String, TemplateError> {
        String::from_utf8(self.bytes).map_err(|_| TemplateError::OutputEncoding)
    }
}

impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = (self.bytes.len() as u128)
            .checked_add(bytes.len() as u128)
            .ok_or_else(|| {
                self.limit_exceeded = true;
                io::Error::other("output size overflowed")
            })?;
        if next > self.max_bytes {
            self.limit_exceeded = true;
            return Err(io::Error::other("output limit exceeded"));
        }
        let next_len =
            usize::try_from(next).map_err(|_| io::Error::other("output allocation failed"))?;
        if next_len > self.bytes.capacity() {
            let max_capacity = usize::try_from(self.max_bytes.min(usize::MAX as u128))
                .map_err(|_| io::Error::other("output allocation failed"))?;
            let grown_capacity = self.bytes.capacity().saturating_mul(2).max(256);
            let target_capacity = next_len.max(grown_capacity.min(max_capacity));
            self.bytes
                .try_reserve_exact(target_capacity - self.bytes.len())
                .map_err(|_| io::Error::other("output allocation failed"))?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_json_expansion_is_bounded_while_encoding() {
        let template = Template::parse("{0}").unwrap();
        assert_eq!(
            format_record_with(
                &["\0"],
                0,
                "",
                "\n",
                Format::Jsonl,
                true,
                Some(&template),
                &[],
                8,
            ),
            Err(TemplateError::OutputTooLarge { limit: 8 })
        );
        assert_eq!(
            format_record_with(
                &["\0"],
                0,
                "",
                "\n",
                Format::Jsonl,
                true,
                Some(&template),
                &[],
                9,
            )
            .unwrap(),
            "\"\\u0000\"\n"
        );
    }

    #[test]
    fn joining_and_csv_escaping_respect_the_final_limit() {
        assert_eq!(
            format_record(&["aaaa", "bbbb"], 0, "-", "\n", Format::Text, false, 9),
            Err(TemplateError::OutputTooLarge { limit: 9 })
        );
        assert_eq!(
            format_record(&["a,b"], 0, "", "\n", Format::Csv, false, 5),
            Err(TemplateError::OutputTooLarge { limit: 5 })
        );
        assert_eq!(
            format_record(&[], 0, "", "\n", Format::Csv, false, 0),
            Err(TemplateError::OutputTooLarge { limit: 0 })
        );
        assert_eq!(
            format_record(&["a,b"], 0, "", "\n", Format::Csv, false, 6).unwrap(),
            "\"a,b\"\n"
        );
    }
}
