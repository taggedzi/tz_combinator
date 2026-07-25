//! Bounded, deterministic keyed joins over structured records.

use std::collections::{HashMap, HashSet};

use crate::error::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Inner,
    Left,
    Full,
    Anti,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinedRecord {
    pub fields: Vec<(String, Option<String>)>,
}

/// Performs a deterministic hash join. The right side is indexed, and the
/// caller must bound both input sizes before calling this function.
pub fn join(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    max_output_records: u128,
) -> Result<Vec<JoinedRecord>, CoreError> {
    if left_key.is_empty() || right_key.is_empty() {
        return Err(CoreError::usage(
            "JOIN_KEY_INVALID",
            "join keys must not be empty",
        ));
    }
    let mut index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, record) in right.iter().enumerate() {
        if let Some(key) = field(record, right_key).filter(|value| !value.is_empty()) {
            index.entry(key).or_default().push(i);
        }
    }
    let mut matched_right = HashSet::new();
    let mut output = Vec::new();
    for left_record in left {
        let matches = field(left_record, left_key)
            .filter(|value| !value.is_empty())
            .and_then(|key| index.get(key));
        match (kind, matches) {
            (JoinType::Anti, Some(_)) => continue,
            (JoinType::Anti, None) => {
                push(&mut output, combine_left(left_record), max_output_records)?
            }
            (_, Some(indices)) => {
                for &right_index in indices {
                    matched_right.insert(right_index);
                    push(
                        &mut output,
                        combine(left_record, Some(right_index), right, right_key),
                        max_output_records,
                    )?;
                }
            }
            (JoinType::Left | JoinType::Full, None) => {
                push(
                    &mut output,
                    combine(left_record, None, right, right_key),
                    max_output_records,
                )?;
            }
            (JoinType::Inner, None) => {}
        }
    }
    if kind == JoinType::Full {
        for (i, right_record) in right.iter().enumerate() {
            if !matched_right.contains(&i) {
                push(
                    &mut output,
                    combine_right(right_record, left),
                    max_output_records,
                )?;
            }
        }
    }
    Ok(output)
}

fn push(
    output: &mut Vec<JoinedRecord>,
    record: JoinedRecord,
    limit: u128,
) -> Result<(), CoreError> {
    let next = u128::try_from(output.len())
        .ok()
        .and_then(|n| n.checked_add(1))
        .unwrap_or(u128::MAX);
    if next > limit {
        return Err(CoreError::runtime(
            "JOIN_LIMIT_EXCEEDED",
            "join output exceeds the configured record limit",
        )
        .with("observed", next)
        .with("limit", limit));
    }
    output.push(record);
    Ok(())
}

fn field<'a>(record: &'a Record, name: &str) -> Option<&'a str> {
    record
        .fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn combine(
    left: &Record,
    right_index: Option<usize>,
    right: &[Record],
    right_key: &str,
) -> JoinedRecord {
    let mut fields = left
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect::<Vec<_>>();
    if let Some(index) = right_index {
        append_right(&mut fields, &right[index], right_key);
    } else {
        append_missing_right(&mut fields, right, right_key);
    }
    JoinedRecord { fields }
}

fn combine_left(left: &Record) -> JoinedRecord {
    JoinedRecord {
        fields: left
            .fields
            .iter()
            .map(|(k, v)| (k.clone(), Some(v.clone())))
            .collect(),
    }
}

fn combine_right(right: &Record, left: &[Record]) -> JoinedRecord {
    let mut fields = left.first().map_or_else(Vec::new, |record| {
        record
            .fields
            .iter()
            .map(|(k, _)| (k.clone(), None))
            .collect()
    });
    let mut names: HashSet<String> = fields.iter().map(|(k, _)| k.clone()).collect();
    for (key, value) in &right.fields {
        let name = unique_name(key, &names);
        names.insert(name.clone());
        fields.push((name, Some(value.clone())));
    }
    JoinedRecord { fields }
}

fn append_right(fields: &mut Vec<(String, Option<String>)>, right: &Record, _right_key: &str) {
    let mut names: HashSet<String> = fields.iter().map(|(k, _)| k.clone()).collect();
    for (key, value) in &right.fields {
        let name = unique_name(key, &names);
        names.insert(name.clone());
        fields.push((name, Some(value.clone())));
    }
}

fn append_missing_right(
    fields: &mut Vec<(String, Option<String>)>,
    right: &[Record],
    _right_key: &str,
) {
    let mut names: HashSet<String> = fields.iter().map(|(k, _)| k.clone()).collect();
    let schema = right
        .first()
        .map(|record| record.fields.iter().map(|(k, _)| k));
    if let Some(schema) = schema {
        for key in schema {
            let name = unique_name(key, &names);
            names.insert(name.clone());
            fields.push((name, None));
        }
    }
}

fn unique_name(name: &str, existing: &HashSet<String>) -> String {
    if !existing.contains(name) {
        return name.to_string();
    }
    let mut candidate = format!("{name}_right");
    let mut n = 2;
    while existing.contains(&candidate) {
        candidate = format!("{name}_right{n}");
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(fields: &[(&str, &str)]) -> Record {
        Record {
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        }
    }

    #[test]
    fn duplicates_expand_in_input_order_and_collisions_are_renamed() {
        let out = join(
            &[r(&[("id", "1"), ("name", "a")])],
            &[
                r(&[("id", "1"), ("name", "x")]),
                r(&[("id", "1"), ("name", "y")]),
            ],
            "id",
            "id",
            JoinType::Inner,
            10,
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].fields[2].0, "id_right");
        assert_eq!(out[1].fields[3].1.as_deref(), Some("y"));
    }

    #[test]
    fn missing_keys_do_not_match_and_limit_is_fail_closed() {
        let err = join(
            &[r(&[("id", "")])],
            &[r(&[("id", "1")])],
            "id",
            "id",
            JoinType::Left,
            0,
        )
        .unwrap_err();
        assert_eq!(err.code, "JOIN_LIMIT_EXCEEDED");
    }
}
