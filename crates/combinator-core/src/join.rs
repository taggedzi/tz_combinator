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
    let mut output = Vec::new();
    join_each(
        left,
        right,
        left_key,
        right_key,
        kind,
        0,
        None,
        max_output_records,
        None,
        |record| {
            output.push(record);
            Ok(())
        },
    )?;
    Ok(output)
}

/// Streams joined records without retaining the complete result set.
///
/// `max_output_records` applies to the complete logical join before paging;
/// `offset` and `limit` only control which records are passed to `callback`.
/// The callback is responsible for serialization and byte limits.
#[allow(clippy::too_many_arguments)]
pub fn join_each<F>(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    offset: u128,
    limit: Option<u128>,
    max_output_records: u128,
    cancel: Option<&dyn Fn() -> bool>,
    callback: F,
) -> Result<u128, CoreError>
where
    F: FnMut(JoinedRecord) -> Result<(), CoreError>,
{
    join_each_with_fanout(
        left,
        right,
        left_key,
        right_key,
        kind,
        offset,
        limit,
        max_output_records,
        u128::MAX,
        cancel,
        callback,
    )
}

/// Like [`join_each`], but also rejects a single duplicate-key expansion
/// before constructing the right-side index or emitting output.
#[allow(clippy::too_many_arguments)]
pub fn join_each_with_fanout<F>(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    offset: u128,
    limit: Option<u128>,
    max_output_records: u128,
    max_key_fanout: u128,
    cancel: Option<&dyn Fn() -> bool>,
    mut callback: F,
) -> Result<u128, CoreError>
where
    F: FnMut(JoinedRecord) -> Result<(), CoreError>,
{
    validate_keys(left_key, right_key)?;
    if limit == Some(0) {
        return Ok(0);
    }
    validate_key_fanout(left, right, left_key, right_key, kind, max_key_fanout)?;
    let mut index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, record) in right.iter().enumerate() {
        if let Some(key) = field(record, right_key).filter(|value| !value.is_empty()) {
            index.entry(key).or_default().push(i);
        }
    }
    // Joined field names depend only on the schemas, not on the values.  Only
    // prepare the collision mapping when a key expands to multiple outputs;
    // for one-to-one joins the setup cost is not repaid by avoiding a single
    // HashSet construction.
    let prepared_names =
        if kind != JoinType::Anti && index.values().any(|indices| indices.len() > 1) {
            prepare_names(left, right)
        } else {
            None
        };
    let mut matched_right = HashSet::new();
    let mut produced = 0u128;
    let mut selected = 0u128;
    let mut emit = |record: JoinedRecord| -> Result<bool, CoreError> {
        produced = produced.checked_add(1).ok_or_else(|| {
            CoreError::runtime("JOIN_LIMIT_EXCEEDED", "join output count overflowed")
        })?;
        if produced > max_output_records {
            return Err(CoreError::runtime(
                "JOIN_LIMIT_EXCEEDED",
                "join output exceeds the configured record limit",
            )
            .with("observed", produced)
            .with("limit", max_output_records));
        }
        let in_page = produced > offset
            && limit
                .map(|page_limit| selected < page_limit)
                .unwrap_or(true);
        if in_page {
            callback(record)?;
            selected = selected.checked_add(1).ok_or_else(|| {
                CoreError::runtime("JOIN_LIMIT_EXCEEDED", "join page count overflowed")
            })?;
        }
        Ok(page_complete(selected, limit))
    };
    for left_record in left {
        check_cancel(cancel)?;
        let matches = field(left_record, left_key)
            .filter(|value| !value.is_empty())
            .and_then(|key| index.get(key));
        match (kind, matches) {
            (JoinType::Anti, Some(_)) => continue,
            (JoinType::Anti, None) => {
                if emit(combine_left(left_record))? {
                    return Ok(selected);
                }
            }
            (_, Some(indices)) => {
                for &right_index in indices {
                    matched_right.insert(right_index);
                    if emit(combine(
                        left_record,
                        Some(right_index),
                        right,
                        right_key,
                        prepared_names.as_ref(),
                    ))? {
                        return Ok(selected);
                    }
                }
            }
            (JoinType::Left | JoinType::Full, None) => {
                if emit(combine(
                    left_record,
                    None,
                    right,
                    right_key,
                    prepared_names.as_ref(),
                ))? {
                    return Ok(selected);
                }
            }
            (JoinType::Inner, None) => {}
        }
    }
    if kind == JoinType::Full {
        for (i, right_record) in right.iter().enumerate() {
            check_cancel(cancel)?;
            if !matched_right.contains(&i)
                && emit(combine_right(
                    right_record,
                    i,
                    left,
                    prepared_names.as_ref(),
                ))?
            {
                return Ok(selected);
            }
        }
    }
    Ok(selected)
}

fn page_complete(selected: u128, limit: Option<u128>) -> bool {
    limit.is_some_and(|limit| selected >= limit)
}

/// Counts a join without constructing joined records.
pub fn join_count(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    max_output_records: u128,
) -> Result<u128, CoreError> {
    join_count_with_fanout(
        left,
        right,
        left_key,
        right_key,
        kind,
        max_output_records,
        u128::MAX,
    )
}

/// Counts a join with a bound on duplicate-key expansion.
pub fn join_count_with_fanout(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    max_output_records: u128,
    max_key_fanout: u128,
) -> Result<u128, CoreError> {
    validate_keys(left_key, right_key)?;
    validate_key_fanout(left, right, left_key, right_key, kind, max_key_fanout)?;
    let mut index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, record) in right.iter().enumerate() {
        if let Some(key) = field(record, right_key).filter(|value| !value.is_empty()) {
            index.entry(key).or_default().push(i);
        }
    }
    let mut matched_right = HashSet::new();
    let mut count = 0u128;
    let mut add = |amount: u128| -> Result<(), CoreError> {
        count = count.checked_add(amount).ok_or_else(|| {
            CoreError::runtime("JOIN_LIMIT_EXCEEDED", "join output count overflowed")
        })?;
        if count > max_output_records {
            return Err(CoreError::runtime(
                "JOIN_LIMIT_EXCEEDED",
                "join output exceeds the configured record limit",
            )
            .with("observed", count)
            .with("limit", max_output_records));
        }
        Ok(())
    };
    for left_record in left {
        let matches = field(left_record, left_key)
            .filter(|value| !value.is_empty())
            .and_then(|key| index.get(key));
        match (kind, matches) {
            (JoinType::Anti, Some(_)) | (JoinType::Inner, None) => {}
            (JoinType::Anti, None) | (JoinType::Left | JoinType::Full, None) => add(1)?,
            (_, Some(indices)) => {
                for &right_index in indices {
                    matched_right.insert(right_index);
                }
                add(indices.len() as u128)?;
            }
        }
    }
    if kind == JoinType::Full {
        for (i, _) in right.iter().enumerate() {
            if !matched_right.contains(&i) {
                add(1)?;
            }
        }
    }
    Ok(count)
}

fn validate_key_fanout(
    left: &[Record],
    right: &[Record],
    left_key: &str,
    right_key: &str,
    kind: JoinType,
    max_key_fanout: u128,
) -> Result<(), CoreError> {
    if matches!(kind, JoinType::Anti) {
        return Ok(());
    }
    let mut left_counts: HashMap<&str, u128> = HashMap::new();
    let mut right_counts: HashMap<&str, u128> = HashMap::new();
    for record in left {
        if let Some(key) = field(record, left_key).filter(|value| !value.is_empty()) {
            let count = left_counts.entry(key).or_default();
            *count = count.checked_add(1).ok_or_else(|| {
                CoreError::runtime("JOIN_FANOUT_LIMIT_EXCEEDED", "join key count overflowed")
            })?;
        }
    }
    for record in right {
        if let Some(key) = field(record, right_key).filter(|value| !value.is_empty()) {
            let count = right_counts.entry(key).or_default();
            *count = count.checked_add(1).ok_or_else(|| {
                CoreError::runtime("JOIN_FANOUT_LIMIT_EXCEEDED", "join key count overflowed")
            })?;
        }
    }
    for (key, right_count) in right_counts {
        if let Some(left_count) = left_counts.get(key) {
            let fanout = left_count.checked_mul(right_count).ok_or_else(|| {
                CoreError::runtime("JOIN_FANOUT_LIMIT_EXCEEDED", "join key fanout overflowed")
            })?;
            if fanout > max_key_fanout {
                return Err(CoreError::runtime(
                    "JOIN_FANOUT_LIMIT_EXCEEDED",
                    "duplicate-key join expansion exceeds the configured limit",
                )
                .with("observed", fanout)
                .with("limit", max_key_fanout));
            }
        }
    }
    Ok(())
}

fn validate_keys(left_key: &str, right_key: &str) -> Result<(), CoreError> {
    if left_key.is_empty() || right_key.is_empty() {
        return Err(CoreError::usage(
            "JOIN_KEY_INVALID",
            "join keys must not be empty",
        ));
    }
    Ok(())
}

fn check_cancel(cancel: Option<&dyn Fn() -> bool>) -> Result<(), CoreError> {
    if cancel.is_some_and(|cancel| cancel()) {
        Err(CoreError::runtime("CANCELLED", "execution was cancelled"))
    } else {
        Ok(())
    }
}

fn field<'a>(record: &'a Record, name: &str) -> Option<&'a str> {
    record
        .fields
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

struct PreparedNames {
    right: Vec<Vec<String>>,
    missing_right: Vec<String>,
}

fn prepare_names(left: &[Record], right: &[Record]) -> Option<PreparedNames> {
    let left_schema: Vec<&str> = left
        .first()
        .map(|record| record.fields.iter().map(|(key, _)| key.as_str()).collect())
        .unwrap_or_default();
    if !left.iter().all(|record| {
        record
            .fields
            .iter()
            .map(|(key, _)| key.as_str())
            .eq(left_schema.iter().copied())
    }) {
        return None;
    }

    let right_names = right
        .iter()
        .map(|record| unique_right_names(&left_schema, &record.fields))
        .collect::<Vec<_>>();
    let missing_right = right_names.first().cloned().unwrap_or_default();
    Some(PreparedNames {
        right: right_names,
        missing_right,
    })
}

fn unique_right_names(left_schema: &[&str], right_fields: &[(String, String)]) -> Vec<String> {
    let mut names: HashSet<String> = left_schema.iter().map(|key| (*key).to_string()).collect();
    right_fields
        .iter()
        .map(|(key, _)| {
            let name = unique_name(key, &names);
            names.insert(name.clone());
            name
        })
        .collect()
}

fn combine(
    left: &Record,
    right_index: Option<usize>,
    right: &[Record],
    right_key: &str,
    prepared: Option<&PreparedNames>,
) -> JoinedRecord {
    let mut fields = left
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect::<Vec<_>>();
    if let Some(index) = right_index {
        if let Some(prepared) = prepared {
            append_named_right(&mut fields, &right[index], &prepared.right[index]);
        } else {
            append_right(&mut fields, &right[index], right_key);
        }
    } else {
        if let Some(prepared) = prepared {
            append_named_missing(&mut fields, right, &prepared.missing_right);
        } else {
            append_missing_right(&mut fields, right, right_key);
        }
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

fn combine_right(
    right: &Record,
    right_index: usize,
    left: &[Record],
    prepared: Option<&PreparedNames>,
) -> JoinedRecord {
    let mut fields = left.first().map_or_else(Vec::new, |record| {
        record
            .fields
            .iter()
            .map(|(k, _)| (k.clone(), None))
            .collect()
    });
    if let Some(prepared) = prepared {
        append_named_right(&mut fields, right, &prepared.right[right_index]);
    } else {
        let mut names: HashSet<String> = fields.iter().map(|(k, _)| k.clone()).collect();
        for (key, value) in &right.fields {
            let name = unique_name(key, &names);
            names.insert(name.clone());
            fields.push((name, Some(value.clone())));
        }
    }
    JoinedRecord { fields }
}

fn append_named_right(
    fields: &mut Vec<(String, Option<String>)>,
    right: &Record,
    names: &[String],
) {
    fields.extend(
        right
            .fields
            .iter()
            .zip(names)
            .map(|((_, value), name)| (name.clone(), Some(value.clone()))),
    );
}

fn append_named_missing(
    fields: &mut Vec<(String, Option<String>)>,
    right: &[Record],
    names: &[String],
) {
    if let Some(record) = right.first() {
        fields.extend(
            record
                .fields
                .iter()
                .zip(names)
                .map(|((_, _), name)| (name.clone(), None)),
        );
    }
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
    fn supports_all_join_kinds_and_empty_keys() {
        let left = [r(&[("id", "1"), ("l", "a")]), r(&[("id", "2")])];
        let right = [r(&[("id", "1"), ("r", "x")]), r(&[("id", "3")])];
        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Inner, 8).unwrap(),
            1
        );
        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Left, 8).unwrap(),
            2
        );
        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Full, 8).unwrap(),
            3
        );
        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Anti, 8).unwrap(),
            1
        );
        assert_eq!(
            join(&left, &right, "", "id", JoinType::Inner, 8)
                .unwrap_err()
                .code,
            "JOIN_KEY_INVALID"
        );
    }

    #[test]
    fn cancellation_and_zero_page_are_honored() {
        let left = [r(&[("id", "1")])];
        let right = [r(&[("id", "1")])];
        let mut called = false;
        let cancel = || true;
        assert_eq!(
            join_each(
                &left,
                &right,
                "id",
                "id",
                JoinType::Inner,
                0,
                None,
                8,
                Some(&cancel),
                |_| {
                    called = true;
                    Ok(())
                }
            )
            .unwrap_err()
            .code,
            "CANCELLED"
        );
        assert!(!called);
        assert_eq!(
            join_each(
                &left,
                &right,
                "id",
                "id",
                JoinType::Inner,
                0,
                Some(0),
                8,
                None,
                |_| { panic!("zero page must not invoke callback") }
            )
            .unwrap(),
            0
        );
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

    #[test]
    fn streaming_join_pages_without_materializing_the_prefix() {
        let left = [r(&[("id", "1")])];
        let right = [r(&[("id", "1"), ("v", "a")]), r(&[("id", "1"), ("v", "b")])];
        let mut values = Vec::new();
        let selected = join_each(
            &left,
            &right,
            "id",
            "id",
            JoinType::Inner,
            1,
            Some(1),
            2,
            None,
            |record| {
                values.push(record.fields[2].1.clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(selected, 1);
        assert_eq!(values, [Some("b".into())]);
    }

    #[test]
    fn join_count_does_not_require_joined_records() {
        let left = [r(&[("id", "1")])];
        let right = [r(&[("id", "1")]), r(&[("id", "1")])];
        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Inner, 2).unwrap(),
            2
        );
    }

    #[test]
    fn duplicate_key_fanout_is_rejected_before_output() {
        let left = [r(&[("id", "1")]), r(&[("id", "1")])];
        let right = [r(&[("id", "1")]), r(&[("id", "1")])];
        let err = join_each_with_fanout(
            &left,
            &right,
            "id",
            "id",
            JoinType::Inner,
            0,
            None,
            100,
            3,
            None,
            |_| panic!("fanout must be checked before output"),
        )
        .unwrap_err();
        assert_eq!(err.code, "JOIN_FANOUT_LIMIT_EXCEEDED");
    }

    #[test]
    fn prepared_names_preserve_full_join_collision_order() {
        let left = [r(&[("id", "1"), ("value", "left")])];
        let right = [
            r(&[("id", "1"), ("value", "matched")]),
            r(&[("id", "2"), ("value", "unmatched")]),
        ];
        let out = join(&left, &right, "id", "id", JoinType::Full, 4).unwrap();

        assert_eq!(out.len(), 2);
        assert_eq!(out[0].fields[2].0, "id_right");
        assert_eq!(out[0].fields[3].0, "value_right");
        assert_eq!(out[1].fields[2].0, "id_right");
        assert_eq!(out[1].fields[3].1.as_deref(), Some("unmatched"));
    }

    #[test]
    fn varying_left_schemas_use_the_join_name_fallback() {
        let left = [
            r(&[("id", "1"), ("left", "a")]),
            r(&[("left", "b"), ("id", "2")]),
        ];
        let right = [
            r(&[("id", "1"), ("right", "x")]),
            r(&[("id", "2"), ("right", "y")]),
        ];

        assert_eq!(
            join_count(&left, &right, "id", "id", JoinType::Inner, 4).unwrap(),
            2
        );
        let out = join(&left, &right, "id", "id", JoinType::Inner, 4).unwrap();
        assert_eq!(out[1].fields[0].1.as_deref(), Some("b"));
        assert_eq!(out[1].fields[3].1.as_deref(), Some("y"));
    }
}
