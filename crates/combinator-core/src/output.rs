//! Core record formatting.

use crate::{Template, TemplateError};
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
    let mut w = csv::WriterBuilder::new()
        .has_headers(false)
        .delimiter(sep)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(Vec::new());
    w.write_record(items).unwrap();
    Ok(String::from_utf8(w.into_inner().unwrap()).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_all_record_variants() {
        assert_eq!(
            format_record(&["a", "b"], 0, "-", "\n", Format::Text, false),
            "a-b\n"
        );
        assert_eq!(
            format_record(&["a", "b"], 0, "-", "\n", Format::Nul, false),
            "a-b\0"
        );
        assert_eq!(
            format_record(&["a,b", "x"], 0, "-", "\n", Format::Csv, false),
            "\"a,b\",x\n"
        );
        assert_eq!(
            format_record(&["a\tb", "x"], 0, "-", "\n", Format::Tsv, false),
            "\"a\tb\"\tx\n"
        );
        assert_eq!(
            format_record(&["a", "b"], 0, "-", "\n", Format::Jsonl, true),
            "\"a-b\"\n"
        );
    }

    #[test]
    fn jsonl_includes_names_and_handles_large_indices() {
        let names = vec!["left".to_string(), "right".to_string()];
        let output = format_record_with(
            &["a", "b"],
            u64::MAX as u128 + 1,
            "-",
            "\n",
            Format::Jsonl,
            false,
            None,
            &names,
        )
        .unwrap();
        assert!(output.contains("\"i\":\"18446744073709551616\""));
        assert!(output.contains("\"named\":{"));
    }

    #[test]
    fn template_errors_are_propagated() {
        let template = Template::parse("{missing}").unwrap();
        let names = vec!["known".to_string()];
        assert!(format_record_with(
            &["value"],
            0,
            "",
            "\n",
            Format::Text,
            false,
            Some(&template),
            &names
        )
        .is_err());
    }
}
