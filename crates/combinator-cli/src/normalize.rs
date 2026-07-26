//! CLI transform grammar translated into typed core transforms.

use crate::error::AppError;
use combinator_core::{normalize_typed, Transform};

pub use combinator_core::normalize::MAX_TRANSFORMS;

pub fn normalize_lists(
    lists: &mut [Vec<String>],
    expressions: &[String],
    max_item_bytes: usize,
    max_total_items: usize,
) -> Result<(), AppError> {
    let transforms = expressions
        .iter()
        .map(|expression| parse_transform(expression))
        .collect::<Result<Vec<_>, _>>()?;
    normalize_typed(lists, &transforms, max_item_bytes, max_total_items)
}

pub fn parse_transform(expression: &str) -> Result<Transform, AppError> {
    let transform = match expression {
        "trim" => Transform::Trim,
        "skip-empty" => Transform::SkipEmpty,
        "deduplicate" | "dedup" => Transform::Deduplicate,
        "reject-duplicates" => Transform::RejectDuplicates,
        "sort" => Transform::Sort,
        "lower" | "case=lower" => Transform::Lowercase,
        "upper" | "case=upper" => Transform::Uppercase,
        value if value.starts_with("filter=") => Transform::FilterGlob(value[7..].to_string()),
        value if value.starts_with("replace=") => {
            let (from, to) = value[8..].split_once("=>").ok_or_else(|| invalid(value))?;
            Transform::Replace {
                from: from.to_string(),
                to: to.to_string(),
            }
        }
        value if value.starts_with("prefix=") => Transform::Prefix(value[7..].to_string()),
        value if value.starts_with("suffix=") => Transform::Suffix(value[7..].to_string()),
        _ => return Err(invalid(expression)),
    };
    Ok(transform)
}

fn invalid(expression: &str) -> AppError {
    AppError::usage("TRANSFORM_INVALID", "invalid transform").with("transform", expression)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_syntax_into_typed_transforms() {
        assert_eq!(parse_transform("trim").unwrap(), Transform::Trim);
        assert_eq!(
            parse_transform("replace=a=>b").unwrap(),
            Transform::Replace {
                from: "a".into(),
                to: "b".into()
            }
        );
        assert_eq!(
            parse_transform("filter=pre-*").unwrap(),
            Transform::FilterGlob("pre-*".into())
        );
    }

    #[test]
    fn malformed_syntax_remains_a_cli_error() {
        assert_eq!(
            parse_transform("replace=missing").unwrap_err().code,
            "TRANSFORM_INVALID"
        );
    }
}
