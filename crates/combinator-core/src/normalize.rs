//! Bounded list transformations independent of the command line.

use crate::error::CoreError;
use std::collections::HashSet;

pub const MAX_TRANSFORMS: usize = 64;

/// Interface-neutral, validated list transformation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transform {
    Trim,
    SkipEmpty,
    Deduplicate,
    RejectDuplicates,
    Sort,
    Lowercase,
    Uppercase,
    FilterGlob(String),
    Replace { from: String, to: String },
    Prefix(String),
    Suffix(String),
}

/// Applies typed transformations in order with checked resource limits.
pub fn normalize_typed(
    lists: &mut [Vec<String>],
    transforms: &[Transform],
    max_item_bytes: usize,
    max_total_items: usize,
) -> Result<(), CoreError> {
    if transforms.len() > MAX_TRANSFORMS {
        return Err(CoreError::usage(
            "TRANSFORM_LIMIT",
            "the number of transforms exceeds the security limit",
        ));
    }
    for transform in transforms {
        for (list_index, list) in lists.iter_mut().enumerate() {
            apply_typed(list, transform, max_item_bytes)
                .map_err(|error| error.with("list_index", list_index))?;
            for item in list.iter() {
                if item.len() > max_item_bytes {
                    return Err(CoreError::runtime(
                        "ITEM_TOO_LARGE",
                        "a transformed item exceeds the maximum item byte limit",
                    )
                    .with("list_index", list_index)
                    .with("observed", item.len())
                    .with("limit", max_item_bytes));
                }
            }
        }
    }
    let total = lists
        .iter()
        .try_fold(0usize, |total, list| total.checked_add(list.len()))
        .ok_or_else(|| CoreError::runtime("TOO_MANY_ITEMS", "total item count overflowed"))?;
    if total > max_total_items {
        return Err(CoreError::runtime(
            "TOO_MANY_ITEMS",
            "transformed input exceeds the maximum total item count",
        )
        .with("observed", total)
        .with("limit", max_total_items));
    }
    Ok(())
}

fn apply_typed(
    list: &mut Vec<String>,
    transform: &Transform,
    max_item_bytes: usize,
) -> Result<(), CoreError> {
    match transform {
        Transform::Trim => list
            .iter_mut()
            .for_each(|value| *value = value.trim().to_string()),
        Transform::SkipEmpty => list.retain(|value| !value.is_empty()),
        Transform::Deduplicate => {
            let mut seen = HashSet::new();
            list.retain(|value| seen.insert(value.clone()));
        }
        Transform::RejectDuplicates => {
            let mut seen = HashSet::new();
            for value in list.iter() {
                if !seen.insert(value) {
                    return Err(CoreError::runtime(
                        "DUPLICATE_ITEM",
                        "a list contains a duplicate item",
                    ));
                }
            }
        }
        Transform::Sort => list.sort(),
        Transform::Lowercase => list
            .iter_mut()
            .for_each(|value| *value = value.chars().flat_map(char::to_lowercase).collect()),
        Transform::Uppercase => list
            .iter_mut()
            .for_each(|value| *value = value.chars().flat_map(char::to_uppercase).collect()),
        Transform::FilterGlob(pattern) => {
            if pattern.len() > 4096 || pattern.contains(['[', ']']) {
                return Err(CoreError::usage(
                    "TRANSFORM_INVALID",
                    "invalid transform glob",
                ));
            }
            list.retain(|value| glob(pattern, value));
        }
        Transform::Replace { from, to } => {
            if !from.is_empty() {
                validate_replacements(list, from, to, max_item_bytes)?;
                list.iter_mut()
                    .for_each(|value| *value = value.replace(from, to));
            }
        }
        Transform::Prefix(prefix) => list.iter_mut().for_each(|value| {
            if let Some(stripped) = value.strip_prefix(prefix) {
                *value = stripped.to_string();
            }
        }),
        Transform::Suffix(suffix) => list.iter_mut().for_each(|value| {
            if let Some(stripped) = value.strip_suffix(suffix) {
                *value = stripped.to_string();
            }
        }),
    }
    Ok(())
}
pub fn normalize_lists(
    lists: &mut [Vec<String>],
    expressions: &[String],
    max_item_bytes: usize,
    max_total_items: usize,
) -> Result<(), CoreError> {
    if expressions.len() > MAX_TRANSFORMS {
        return Err(CoreError::usage(
            "TRANSFORM_LIMIT",
            "the number of transforms exceeds the security limit",
        ));
    }
    for expression in expressions {
        for (i, list) in lists.iter_mut().enumerate() {
            apply(list, expression, max_item_bytes).map_err(|e| e.with("list_index", i))?;
            for item in list.iter() {
                if item.len() > max_item_bytes {
                    return Err(CoreError::runtime(
                        "ITEM_TOO_LARGE",
                        "a transformed item exceeds the maximum item byte limit",
                    )
                    .with("list_index", i)
                    .with("observed", item.len())
                    .with("limit", max_item_bytes));
                }
            }
        }
    }
    let total = lists
        .iter()
        .try_fold(0usize, |a, l| a.checked_add(l.len()))
        .ok_or_else(|| CoreError::runtime("TOO_MANY_ITEMS", "total item count overflowed"))?;
    if total > max_total_items {
        return Err(CoreError::runtime(
            "TOO_MANY_ITEMS",
            "transformed input exceeds the maximum total item count",
        )
        .with("observed", total)
        .with("limit", max_total_items));
    }
    Ok(())
}
fn apply(list: &mut Vec<String>, e: &str, max_item_bytes: usize) -> Result<(), CoreError> {
    match e {
        "trim" => list.iter_mut().for_each(|s| *s = s.trim().to_string()),
        "skip-empty" => list.retain(|s| !s.is_empty()),
        "deduplicate" | "dedup" => {
            let mut seen = HashSet::new();
            list.retain(|s| seen.insert(s.clone()));
        }
        "reject-duplicates" => {
            let mut seen = HashSet::new();
            for s in list.iter() {
                if !seen.insert(s) {
                    return Err(CoreError::runtime(
                        "DUPLICATE_ITEM",
                        "a list contains a duplicate item",
                    ));
                }
            }
        }
        "sort" => list.sort(),
        "lower" | "case=lower" => list
            .iter_mut()
            .for_each(|s| *s = s.chars().flat_map(char::to_lowercase).collect()),
        "upper" | "case=upper" => list
            .iter_mut()
            .for_each(|s| *s = s.chars().flat_map(char::to_uppercase).collect()),
        e if e.starts_with("filter=") => {
            let p = &e[7..];
            if p.len() > 4096 || p.contains(['[', ']']) {
                return Err(invalid(e));
            }
            list.retain(|s| glob(p, s));
        }
        e if e.starts_with("replace=") => {
            let (from, to) = e[8..].split_once("=>").ok_or_else(|| invalid(e))?;
            if !from.is_empty() {
                validate_replacements(list, from, to, max_item_bytes)?;
                list.iter_mut().for_each(|s| *s = s.replace(from, to));
            }
        }
        e if e.starts_with("prefix=") => {
            let p = &e[7..];
            list.iter_mut().for_each(|s| {
                if let Some(v) = s.strip_prefix(p) {
                    *s = v.to_string()
                }
            });
        }
        e if e.starts_with("suffix=") => {
            let p = &e[7..];
            list.iter_mut().for_each(|s| {
                if let Some(v) = s.strip_suffix(p) {
                    *s = v.to_string()
                }
            });
        }
        _ => return Err(invalid(e)),
    }
    Ok(())
}

fn validate_replacements(
    list: &[String],
    from: &str,
    to: &str,
    max_item_bytes: usize,
) -> Result<(), CoreError> {
    for value in list {
        let matches = value.match_indices(from).count();
        let removed = matches
            .checked_mul(from.len())
            .ok_or_else(|| CoreError::runtime("ITEM_TOO_LARGE", "replacement size overflowed"))?;
        let added = matches
            .checked_mul(to.len())
            .ok_or_else(|| CoreError::runtime("ITEM_TOO_LARGE", "replacement size overflowed"))?;
        let output_len = value
            .len()
            .checked_sub(removed)
            .and_then(|length| length.checked_add(added))
            .ok_or_else(|| CoreError::runtime("ITEM_TOO_LARGE", "replacement size overflowed"))?;
        if output_len > max_item_bytes {
            return Err(CoreError::runtime(
                "ITEM_TOO_LARGE",
                "a transformed item exceeds the maximum item byte limit",
            )
            .with("observed", output_len)
            .with("limit", max_item_bytes));
        }
    }
    Ok(())
}
fn invalid(e: &str) -> CoreError {
    CoreError::usage("TRANSFORM_INVALID", "invalid transform").with("transform", e)
}
fn glob(p: &str, v: &str) -> bool {
    let (p, v): (Vec<_>, Vec<_>) = (p.chars().collect(), v.chars().collect());
    let (mut pi, mut vi, mut star, mut mark) = (0, 0, None, 0);
    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = vi;
            pi += 1
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            vi = mark
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_are_ordered_and_bounded() {
        let mut lists = vec![vec![" B ".into(), "a".into(), "b".into(), "a".into()]];
        normalize_lists(
            &mut lists,
            &[
                "trim".into(),
                "lower".into(),
                "deduplicate".into(),
                "sort".into(),
            ],
            16,
            8,
        )
        .unwrap();
        assert_eq!(lists[0], ["a", "b"]);
    }

    #[test]
    fn hostile_glob_syntax_is_rejected() {
        let mut lists = vec![vec!["a".into()]];
        let error = normalize_lists(&mut lists, &["filter=[a]".into()], 16, 8).unwrap_err();
        assert_eq!(error.code, "TRANSFORM_INVALID");
    }

    #[test]
    fn supports_filter_replace_prefix_suffix_and_case_aliases() {
        let mut lists = vec![vec![
            "pre-Alpha-suf".into(),
            "pre-beta-suf".into(),
            "other".into(),
        ]];
        normalize_lists(
            &mut lists,
            &[
                "filter=pre-*".into(),
                "replace=pre-=>".into(),
                "prefix=ALPHA".into(),
                "suffix=-suf".into(),
                "upper".into(),
            ],
            64,
            10,
        )
        .unwrap();
        assert_eq!(lists[0], ["ALPHA", "BETA"]);
    }

    #[test]
    fn rejects_duplicate_transform_and_limits() {
        let mut lists = vec![vec!["a".into(), "a".into()]];
        assert_eq!(
            normalize_lists(&mut lists, &["reject-duplicates".into()], 8, 8)
                .unwrap_err()
                .code,
            "DUPLICATE_ITEM"
        );
        let mut lists = vec![vec!["a".into()]];
        assert_eq!(
            normalize_lists(&mut lists, &["unknown".into()], 8, 8)
                .unwrap_err()
                .code,
            "TRANSFORM_INVALID"
        );
        let mut lists = vec![vec!["a".into(), "b".into()]];
        assert_eq!(
            normalize_lists(&mut lists, &[], 8, 1).unwrap_err().code,
            "TOO_MANY_ITEMS"
        );
    }

    #[test]
    fn empty_patterns_and_unicode_case_changes_are_safe() {
        let mut lists = vec![vec!["ß".into(), "".into()]];
        normalize_lists(
            &mut lists,
            &[
                "prefix=".into(),
                "suffix=".into(),
                "replace==>x".into(),
                "upper".into(),
            ],
            8,
            8,
        )
        .unwrap();
        assert_eq!(lists[0], ["SS", ""]);

        let mut lists = vec![vec!["a".into()]];
        assert_eq!(
            normalize_lists(&mut lists, &["filter=".into()], 8, 8).unwrap(),
            ()
        );
        assert!(lists[0].is_empty());
    }

    #[test]
    fn replacement_expansion_is_checked_before_allocation() {
        let original = vec![vec!["aaaa".to_string()]];

        let mut legacy = original.clone();
        let error = normalize_lists(&mut legacy, &["replace=a=>bbb".into()], 8, 8).unwrap_err();
        assert_eq!(error.code, "ITEM_TOO_LARGE");
        assert_eq!(legacy, original);

        let mut typed = original.clone();
        let error = normalize_typed(
            &mut typed,
            &[Transform::Replace {
                from: "a".into(),
                to: "bbb".into(),
            }],
            8,
            8,
        )
        .unwrap_err();
        assert_eq!(error.code, "ITEM_TOO_LARGE");
        assert_eq!(typed, original);
    }
}
