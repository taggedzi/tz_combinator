//! Per-record output formatting for text and JSON Lines.

use combinator_core::{Template, TemplateError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Jsonl,
    Csv,
    Tsv,
    Nul,
}

/// Formats one combination into the exact bytes to emit (including the record
/// terminator).
#[allow(dead_code)]
pub fn format_record(
    items: &[&str],
    index: u128,
    field_sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
) -> String {
    format_record_with(items, index, field_sep, rec_sep, format, lean, None, &[])
        .expect("legacy formatting has no fallible template references")
}

/// Formats one record with an optional compiled template and field names.
#[allow(clippy::too_many_arguments)]
pub fn format_record_with(
    items: &[&str],
    index: u128,
    field_sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
    template: Option<&Template>,
    names: &[String],
) -> Result<String, TemplateError> {
    let value = match template {
        Some(template) => template.render(items, names)?,
        None => items.join(field_sep),
    };
    match format {
        Format::Text => Ok(format!("{value}{rec_sep}")),
        Format::Jsonl if lean => {
            let mut s = serde_json::to_string(&value).expect("string is always serializable");
            s.push('\n');
            Ok(s)
        }
        Format::Jsonl => {
            // Build with explicit key order (i, value, fields). serde_json's
            // json! macro sorts keys alphabetically without the preserve_order
            // feature, so assemble the line by hand while still delegating all
            // escaping to serde_json::to_string. The index is a JSON number when
            // it fits in u64, otherwise a JSON string (unreachable in practice,
            // but kept correct).
            let i_json = if index <= u64::MAX as u128 {
                (index as u64).to_string()
            } else {
                serde_json::to_string(&index.to_string()).expect("string is always serializable")
            };
            let value_json = serde_json::to_string(&value).expect("string is always serializable");
            let fields_json =
                serde_json::to_string(items).expect("string slice is always serializable");
            let named_json = if names.is_empty() {
                String::new()
            } else {
                let entries = names
                    .iter()
                    .zip(items.iter())
                    .map(|(name, value)| {
                        let name_json =
                            serde_json::to_string(name).expect("string is always serializable");
                        let value_json =
                            serde_json::to_string(value).expect("string is always serializable");
                        format!("{name_json}:{value_json}")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(",\"named\":{{{entries}}}")
            };
            Ok(format!(
                "{{\"i\":{i_json},\"value\":{value_json},\"fields\":{fields_json}{named_json}}}\n"
            ))
        }
        Format::Csv => Ok(csv_record(items, b',')),
        Format::Tsv => Ok(csv_record(items, b'\t')),
        Format::Nul => Ok(format!("{value}\0")),
    }
}

fn csv_record(items: &[&str], separator: u8) -> String {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .delimiter(separator)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    writer
        .write_record(items)
        .expect("writing a CSV record to memory cannot fail");
    String::from_utf8(
        writer
            .into_inner()
            .expect("flushing a CSV record in memory cannot fail"),
    )
    .expect("CSV output is valid UTF-8 because input fields are UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_joins_with_sep_and_rec() {
        assert_eq!(
            format_record(&["red", "car"], 0, "-", "\n", Format::Text, false),
            "red-car\n"
        );
    }

    #[test]
    fn text_empty_sep_concatenates() {
        assert_eq!(
            format_record(&["a", "b"], 0, "", "\n", Format::Text, false),
            "ab\n"
        );
    }

    #[test]
    fn jsonl_full_shape() {
        let line = format_record(&["red", "car"], 3, "-", "\n", Format::Jsonl, false);
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["i"], 3);
        assert_eq!(v["value"], "red-car");
        assert_eq!(v["fields"][0], "red");
        assert_eq!(v["fields"][1], "car");
        assert!(line.ends_with('\n'));
    }

    #[test]
    fn jsonl_full_shape_exact_key_order() {
        assert_eq!(
            format_record(&["red", "car"], 3, "-", "\n", Format::Jsonl, false),
            "{\"i\":3,\"value\":\"red-car\",\"fields\":[\"red\",\"car\"]}\n"
        );
    }

    #[test]
    fn jsonl_lean_is_bare_string() {
        let line = format_record(&["red", "car"], 0, "-", "\n", Format::Jsonl, true);
        assert_eq!(line, "\"red-car\"\n");
    }

    #[test]
    fn jsonl_escapes_quotes() {
        let line = format_record(&["a\"b"], 0, "", "\n", Format::Jsonl, true);
        // Valid JSON string with escaped quote.
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v, "a\"b");
    }

    #[test]
    fn template_renders_text() {
        let template = Template::parse("{1}@{0}").unwrap();
        let line = format_record_with(
            &["host", "port"],
            0,
            "-",
            "\n",
            Format::Text,
            false,
            Some(&template),
            &[],
        )
        .unwrap();
        assert_eq!(line, "port@host\n");
    }

    #[test]
    fn named_jsonl_metadata_is_additive() {
        let template = Template::parse("{host}:{port}").unwrap();
        let names = vec!["host".to_string(), "port".to_string()];
        let line = format_record_with(
            &["server", "443"],
            0,
            "-",
            "\n",
            Format::Jsonl,
            false,
            Some(&template),
            &names,
        )
        .unwrap();
        assert_eq!(
            line,
            "{\"i\":0,\"value\":\"server:443\",\"fields\":[\"server\",\"443\"],\"named\":{\"host\":\"server\",\"port\":\"443\"}}\n"
        );
    }
}
