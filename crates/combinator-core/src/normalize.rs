//! Bounded list transformations independent of the command line.

use crate::error::CoreError;
use std::collections::HashSet;

pub const MAX_TRANSFORMS: usize = 64;
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
            apply(list, expression).map_err(|e| e.with("list_index", i))?;
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
fn apply(list: &mut Vec<String>, e: &str) -> Result<(), CoreError> {
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
}
