//! Interface-neutral record formatting over caller-provided values.

use crate::template::{Template, TemplateError};

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
) -> Result<String, TemplateError> {
    format_record_with(items, index, sep, rec_sep, format, lean, None, &[])
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
) -> Result<String, TemplateError> {
    let value = match template {
        Some(t) => t.render(items, names)?,
        None => items.join(sep),
    };
    match format {
        Format::Text => Ok(format!("{value}{rec_sep}")),
        Format::Nul => Ok(format!("{value}\0")),
        Format::Jsonl if lean => Ok(format!(
            "{}\n",
            serde_json::to_string(&value).map_err(|_| TemplateError::OutputEncoding)?
        )),
        Format::Jsonl => {
            let i = if index <= u64::MAX as u128 {
                index.to_string()
            } else {
                serde_json::to_string(&index.to_string())
                    .map_err(|_| TemplateError::OutputEncoding)?
            };
            let fields = serde_json::to_string(items).map_err(|_| TemplateError::OutputEncoding)?;
            let named = if names.is_empty() {
                String::new()
            } else {
                let pairs = names
                    .iter()
                    .zip(items)
                    .map(|(n, v)| {
                        Ok(format!(
                            "{}:{}",
                            serde_json::to_string(n).map_err(|_| TemplateError::OutputEncoding)?,
                            serde_json::to_string(v).map_err(|_| TemplateError::OutputEncoding)?
                        ))
                    })
                    .collect::<Result<Vec<_>, TemplateError>>()?;
                format!(",\"named\":{{{}}}", pairs.join(","))
            };
            Ok(format!(
                "{{\"i\":{i},\"value\":{},\"fields\":{fields}{named}}}\n",
                serde_json::to_string(&value).map_err(|_| TemplateError::OutputEncoding)?
            ))
        }
        Format::Csv => csv_record(items, b','),
        Format::Tsv => csv_record(items, b'\t'),
    }
}

fn csv_record(items: &[&str], sep: u8) -> Result<String, TemplateError> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .delimiter(sep)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(items)
        .map_err(|_| TemplateError::OutputEncoding)?;
    let bytes = writer
        .into_inner()
        .map_err(|_| TemplateError::OutputEncoding)?;
    String::from_utf8(bytes).map_err(|_| TemplateError::OutputEncoding)
}
