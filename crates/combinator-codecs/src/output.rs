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
) -> String {
    format_record_with(items, index, sep, rec_sep, format, lean, None, &[])
        .expect("legacy formatting cannot fail")
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
        Format::Jsonl if lean => Ok(format!("{}\n", serde_json::to_string(&value).unwrap())),
        Format::Jsonl => {
            let i = if index <= u64::MAX as u128 {
                index.to_string()
            } else {
                serde_json::to_string(&index.to_string()).unwrap()
            };
            let fields = serde_json::to_string(items).unwrap();
            let named = if names.is_empty() {
                String::new()
            } else {
                format!(
                    ",\"named\":{{{}}}",
                    names
                        .iter()
                        .zip(items)
                        .map(|(n, v)| format!(
                            "{}:{}",
                            serde_json::to_string(n).unwrap(),
                            serde_json::to_string(v).unwrap()
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            Ok(format!(
                "{{\"i\":{i},\"value\":{},\"fields\":{fields}{named}}}\n",
                serde_json::to_string(&value).unwrap()
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
    writer.write_record(items).unwrap();
    Ok(String::from_utf8(writer.into_inner().unwrap()).unwrap())
}
