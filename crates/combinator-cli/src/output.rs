//! Per-record output formatting for text and JSON Lines.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Jsonl,
}

/// Formats one combination into the exact bytes to emit (including the record
/// terminator).
pub fn format_record(
    items: &[&str],
    index: u128,
    field_sep: &str,
    rec_sep: &str,
    format: Format,
    lean: bool,
) -> String {
    let value = items.join(field_sep);
    match format {
        Format::Text => format!("{value}{rec_sep}"),
        Format::Jsonl if lean => {
            let mut s = serde_json::to_string(&value).expect("string is always serializable");
            s.push('\n');
            s
        }
        Format::Jsonl => {
            // index is u128; serde_json numbers are limited, so emit via json! with
            // a number built from string is unsafe. Instead include i as a JSON number
            // only when it fits in u64; otherwise as a string. Indices beyond u64::MAX
            // require > 1.8e19 combinations and are not practically reachable, but we
            // stay correct regardless.
            let i_value: serde_json::Value = if index <= u64::MAX as u128 {
                serde_json::Value::from(index as u64)
            } else {
                serde_json::Value::String(index.to_string())
            };
            let obj = serde_json::json!({
                "i": i_value,
                "value": value,
                "fields": items,
            });
            let mut s = obj.to_string();
            s.push('\n');
            s
        }
    }
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
}
