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
            format!("{{\"i\":{i_json},\"value\":{value_json},\"fields\":{fields_json}}}\n")
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
}
