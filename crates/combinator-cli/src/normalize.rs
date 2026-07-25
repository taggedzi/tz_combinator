//! Bounded, deterministic per-list input transformations.

use std::collections::HashSet;

use crate::error::AppError;

pub const MAX_TRANSFORMS: usize = 64;
pub const MAX_TRANSFORM_BYTES: usize = 4096;

#[derive(Debug, Clone)]
enum Transform {
    Trim,
    SkipEmpty,
    Deduplicate,
    RejectDuplicates,
    Sort,
    Lower,
    Upper,
    Filter(String),
    Replace(String, String),
    RemovePrefix(String),
    RemoveSuffix(String),
}

pub fn normalize_lists(
    lists: &mut [Vec<String>],
    expressions: &[String],
    max_item_bytes: usize,
    max_total_items: usize,
) -> Result<(), AppError> {
    let transforms = expressions
        .iter()
        .map(|expression| parse(expression))
        .collect::<Result<Vec<_>, _>>()?;

    for transform in &transforms {
        for (list_index, list) in lists.iter_mut().enumerate() {
            apply(list, transform).map_err(|error| error.with("list_index", list_index))?;
            validate_items(list, max_item_bytes, list_index)?;
        }
    }

    let total = lists
        .iter()
        .map(Vec::len)
        .try_fold(0usize, usize::checked_add)
        .ok_or_else(|| AppError::runtime("TOO_MANY_ITEMS", "total item count overflowed"))?;
    if total > max_total_items {
        return Err(AppError::runtime(
            "TOO_MANY_ITEMS",
            "transformed input exceeds the maximum total item count",
        )
        .with("observed", total)
        .with("limit", max_total_items));
    }
    Ok(())
}

fn parse(expression: &str) -> Result<Transform, AppError> {
    if expression.is_empty() || expression.len() > MAX_TRANSFORM_BYTES {
        return Err(invalid(expression));
    }
    let transform = match expression {
        "trim" => Transform::Trim,
        "skip-empty" => Transform::SkipEmpty,
        "deduplicate" | "dedup" => Transform::Deduplicate,
        "reject-duplicates" => Transform::RejectDuplicates,
        "sort" => Transform::Sort,
        "lower" | "case=lower" => Transform::Lower,
        "upper" | "case=upper" => Transform::Upper,
        value if value.starts_with("filter=") => {
            let pattern = value[7..].to_string();
            if pattern.len() > MAX_TRANSFORM_BYTES || pattern.contains(['[', ']']) {
                return Err(invalid(expression));
            }
            Transform::Filter(pattern)
        }
        value if value.starts_with("replace=") => {
            let (from, to) = value[8..]
                .split_once("=>")
                .ok_or_else(|| invalid(expression))?;
            Transform::Replace(from.to_string(), to.to_string())
        }
        value if value.starts_with("prefix=") => Transform::RemovePrefix(value[7..].to_string()),
        value if value.starts_with("suffix=") => Transform::RemoveSuffix(value[7..].to_string()),
        _ => return Err(invalid(expression)),
    };
    Ok(transform)
}

fn invalid(expression: &str) -> AppError {
    AppError::usage(
        "TRANSFORM_INVALID",
        "invalid transform; use trim, skip-empty, deduplicate, reject-duplicates, sort, lower, upper, filter=GLOB, replace=FROM=>TO, prefix=VALUE, or suffix=VALUE",
    )
    .with("transform", expression)
}

fn apply(list: &mut Vec<String>, transform: &Transform) -> Result<(), AppError> {
    match transform {
        Transform::Trim => list
            .iter_mut()
            .for_each(|item| *item = item.trim().to_string()),
        Transform::SkipEmpty => list.retain(|item| !item.is_empty()),
        Transform::Deduplicate => deduplicate(list),
        Transform::RejectDuplicates => {
            let mut seen = HashSet::with_capacity(list.len());
            for item in list.iter() {
                if !seen.insert(item) {
                    return Err(AppError::runtime(
                        "DUPLICATE_ITEM",
                        "a list contains a duplicate item",
                    ));
                }
            }
        }
        Transform::Sort => list.sort(),
        Transform::Lower => list
            .iter_mut()
            .for_each(|item| *item = item.chars().flat_map(char::to_lowercase).collect()),
        Transform::Upper => list
            .iter_mut()
            .for_each(|item| *item = item.chars().flat_map(char::to_uppercase).collect()),
        Transform::Filter(pattern) => list.retain(|item| glob_matches(pattern, item)),
        Transform::Replace(from, to) => {
            if !from.is_empty() {
                list.iter_mut()
                    .for_each(|item| *item = item.replace(from, to));
            }
        }
        Transform::RemovePrefix(prefix) => list.iter_mut().for_each(|item| {
            if let Some(value) = item.strip_prefix(prefix) {
                *item = value.to_string();
            }
        }),
        Transform::RemoveSuffix(suffix) => list.iter_mut().for_each(|item| {
            if let Some(value) = item.strip_suffix(suffix) {
                *item = value.to_string();
            }
        }),
    }
    Ok(())
}

fn deduplicate(list: &mut Vec<String>) {
    let mut seen = HashSet::with_capacity(list.len());
    list.retain(|item| seen.insert(item.clone()));
}

fn validate_items(
    list: &[String],
    max_item_bytes: usize,
    list_index: usize,
) -> Result<(), AppError> {
    for item in list {
        if item.len() > max_item_bytes {
            return Err(AppError::runtime(
                "ITEM_TOO_LARGE",
                "a transformed item exceeds the maximum item byte limit",
            )
            .with("list_index", list_index)
            .with("observed", item.len())
            .with("limit", max_item_bytes));
        }
    }
    Ok(())
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut last_star = None;
    let mut star_match = 0;

    // Greedy matching with bounded backtracking to the most recent '*'. This
    // is linear in the pattern and value lengths and cannot exhibit regex
    // style exponential behavior.
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            last_star = Some(pattern_index);
            star_match = value_index;
            pattern_index += 1;
        } else if let Some(star) = last_star {
            pattern_index = star + 1;
            star_match += 1;
            value_index = star_match;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == '*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(values: &[&str], transforms: &[&str]) -> Result<Vec<String>, AppError> {
        let mut lists = vec![values.iter().map(|v| (*v).to_string()).collect()];
        let expressions = transforms
            .iter()
            .map(|v| (*v).to_string())
            .collect::<Vec<_>>();
        normalize_lists(&mut lists, &expressions, 1024, 100).map(|_| lists.remove(0))
    }

    #[test]
    fn applies_left_to_right_and_keeps_first_duplicate() {
        assert_eq!(
            run(
                &[" B ", "a", "b", "a"],
                &["trim", "lower", "deduplicate", "sort"]
            )
            .unwrap(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn glob_filter_and_replacement_are_deterministic() {
        assert_eq!(
            run(
                &["id-01", "id-02", "other"],
                &["filter=id-??", "replace=id-=>item-"]
            )
            .unwrap(),
            vec!["item-01", "item-02"]
        );
    }

    #[test]
    fn rejects_malformed_expression_and_duplicate() {
        assert_eq!(
            run(&["a"], &["filter=[a]"]).unwrap_err().code,
            "TRANSFORM_INVALID"
        );
        assert_eq!(
            run(&["a", "a"], &["reject-duplicates"]).unwrap_err().code,
            "DUPLICATE_ITEM"
        );
    }
}
