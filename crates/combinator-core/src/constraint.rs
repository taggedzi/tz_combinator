//! Typed, bounded, side-effect-free candidate constraints.

use crate::CoreError;

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;
const MAX_LITERAL_BYTES: usize = 1 << 20;
/// Maximum cumulative pattern-by-value work charged while evaluating the glob
/// constraints for one candidate record.
pub const MAX_GLOB_WORK: usize = 16 * 1024 * 1024;
const CANCEL_POLL_INTERVAL: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    True,
    Equals {
        field: usize,
        value: String,
    },
    NotEquals {
        field: usize,
        value: String,
    },
    Prefix {
        field: usize,
        value: String,
    },
    Suffix {
        field: usize,
        value: String,
    },
    Glob {
        field: usize,
        pattern: String,
    },
    Length {
        field: usize,
        min: usize,
        max: usize,
    },
    Not(Box<Constraint>),
    All(Vec<Constraint>),
    Any(Vec<Constraint>),
}

impl Constraint {
    pub fn validate(&self) -> Result<(), CoreError> {
        self.validate_node(0, 0).map(|_| ())
    }
    fn validate_node(&self, depth: usize, nodes: usize) -> Result<usize, CoreError> {
        if depth > MAX_DEPTH {
            return Err(CoreError::usage(
                "CONSTRAINT_TOO_DEEP",
                "constraint nesting exceeds the security limit",
            ));
        }
        let nodes = nodes.checked_add(1).ok_or_else(|| {
            CoreError::usage("CONSTRAINT_TOO_LARGE", "constraint node count overflowed")
        })?;
        if nodes > MAX_NODES {
            return Err(CoreError::usage(
                "CONSTRAINT_TOO_LARGE",
                "constraint node count exceeds the security limit",
            ));
        }
        match self {
            Self::Equals { value, .. }
            | Self::NotEquals { value, .. }
            | Self::Prefix { value, .. }
            | Self::Suffix { value, .. } => {
                if value.len() > MAX_LITERAL_BYTES {
                    return Err(CoreError::usage(
                        "CONSTRAINT_TOO_LARGE",
                        "constraint literal exceeds the security limit",
                    ));
                }
            }
            Self::Glob { pattern, .. } => {
                if pattern.len() > MAX_LITERAL_BYTES {
                    return Err(CoreError::usage(
                        "CONSTRAINT_TOO_LARGE",
                        "constraint pattern exceeds the security limit",
                    ));
                }
            }
            _ => {}
        }
        let mut total = nodes;
        match self {
            Self::Not(inner) => total = inner.validate_node(depth + 1, total)?,
            Self::All(items) | Self::Any(items) => {
                if items.len() > MAX_NODES {
                    return Err(CoreError::usage(
                        "CONSTRAINT_TOO_LARGE",
                        "constraint list exceeds the security limit",
                    ));
                }
                for item in items {
                    total = item.validate_node(depth + 1, total)?;
                }
            }
            Self::Length { min, max, .. } if min > max => {
                return Err(CoreError::usage(
                    "INVALID_CONSTRAINT",
                    "constraint minimum exceeds maximum",
                ))
            }
            _ => {}
        }
        Ok(total)
    }
    pub fn matches(&self, fields: &[&str]) -> Result<bool, CoreError> {
        self.validate()?;
        ConstraintMatcher::new(None).matches(self, fields)
    }

    fn matches_validated(
        &self,
        fields: &[&str],
        matcher: &mut ConstraintMatcher<'_>,
    ) -> Result<bool, CoreError> {
        Ok(match self {
            Self::True => true,
            Self::Equals { field, value } => fields.get(*field).is_some_and(|v| *v == value),
            Self::NotEquals { field, value } => fields.get(*field).is_some_and(|v| *v != value),
            Self::Prefix { field, value } => {
                fields.get(*field).is_some_and(|v| v.starts_with(value))
            }
            Self::Suffix { field, value } => fields.get(*field).is_some_and(|v| v.ends_with(value)),
            Self::Glob { field, pattern } => match fields.get(*field) {
                Some(value) => glob_matches(value, pattern, matcher)?,
                None => false,
            },
            Self::Length { field, min, max } => fields.get(*field).is_some_and(|v| {
                let n = v.len();
                n >= *min && n <= *max
            }),
            Self::Not(inner) => !inner.matches_validated(fields, matcher)?,
            Self::All(items) => {
                for item in items {
                    if !item.matches_validated(fields, matcher)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Any(items) => {
                for item in items {
                    if item.matches_validated(fields, matcher)? {
                        return Ok(true);
                    }
                }
                false
            }
        })
    }
}

pub(crate) struct ConstraintMatcher<'a> {
    cancel: Option<&'a dyn Fn() -> bool>,
    remaining_glob_work: usize,
    steps_until_cancel_poll: usize,
}

impl<'a> ConstraintMatcher<'a> {
    pub(crate) fn new(cancel: Option<&'a dyn Fn() -> bool>) -> Self {
        Self {
            cancel,
            remaining_glob_work: MAX_GLOB_WORK,
            steps_until_cancel_poll: CANCEL_POLL_INTERVAL,
        }
    }

    pub(crate) fn matches(
        &mut self,
        constraint: &Constraint,
        fields: &[&str],
    ) -> Result<bool, CoreError> {
        constraint.matches_validated(fields, self)
    }

    fn charge_glob_work(
        &mut self,
        pattern_bytes: usize,
        value_bytes: usize,
    ) -> Result<(), CoreError> {
        // Empty inputs still require scanning the non-empty side. Count them
        // as one byte so a tree of empty-value globs cannot bypass the budget.
        let work = pattern_bytes
            .max(1)
            .checked_mul(value_bytes.max(1))
            .ok_or_else(|| {
                glob_work_error(pattern_bytes, value_bytes, None, self.remaining_glob_work)
            })?;
        if work > self.remaining_glob_work {
            return Err(glob_work_error(
                pattern_bytes,
                value_bytes,
                Some(work),
                self.remaining_glob_work,
            ));
        }
        self.remaining_glob_work -= work;
        Ok(())
    }

    fn check_cancel(&self) -> Result<(), CoreError> {
        if self.cancel.is_some_and(|cancel| cancel()) {
            Err(CoreError::runtime("CANCELLED", "execution was cancelled"))
        } else {
            Ok(())
        }
    }

    fn matcher_step(&mut self) -> Result<(), CoreError> {
        if self.steps_until_cancel_poll == 0 {
            self.check_cancel()?;
            self.steps_until_cancel_poll = CANCEL_POLL_INTERVAL;
        }
        self.steps_until_cancel_poll -= 1;
        Ok(())
    }
}

fn glob_work_error(
    pattern_bytes: usize,
    value_bytes: usize,
    work: Option<usize>,
    remaining: usize,
) -> CoreError {
    let mut error = CoreError::runtime(
        "CONSTRAINT_WORK_LIMIT_EXCEEDED",
        "constraint glob work exceeds the security limit",
    )
    .with("pattern_bytes", pattern_bytes)
    .with("value_bytes", value_bytes)
    .with("remaining", remaining)
    .with("limit", MAX_GLOB_WORK);
    if let Some(work) = work {
        error = error.with("work", work);
    }
    error
}

fn glob_matches(
    value: &str,
    pattern: &str,
    matcher: &mut ConstraintMatcher<'_>,
) -> Result<bool, CoreError> {
    matcher.charge_glob_work(pattern.len(), value.len())?;
    matcher.check_cancel()?;

    let value = value.as_bytes();
    let pattern = pattern.as_bytes();
    let Some(first_star) = pattern.iter().position(|token| *token == b'*') else {
        return fixed_tokens_match(pattern, value, matcher);
    };
    let last_star = pattern
        .iter()
        .rposition(|token| *token == b'*')
        .expect("first star proves a last star exists");
    let suffix_len = pattern.len() - last_star - 1;
    if suffix_len > value.len() || first_star > value.len() - suffix_len {
        return Ok(false);
    }

    if !fixed_tokens_match(&pattern[..first_star], &value[..first_star], matcher)? {
        return Ok(false);
    }
    if !fixed_tokens_match(
        &pattern[last_star + 1..],
        &value[value.len() - suffix_len..],
        matcher,
    )? {
        return Ok(false);
    }

    // Match the star-delimited middle after anchoring the fixed prefix and
    // suffix. Keep only the most recent star as a retry point; earlier stars
    // never need to be reconsidered because choosing an earlier occurrence
    // leaves at least as much input available to the rest of the pattern.
    let pattern_end = last_star + 1;
    let value_end = value.len() - suffix_len;
    let mut pattern_index = first_star;
    let mut value_index = first_star;
    let mut star_resume = None;
    let mut star_value_index = value_index;

    while value_index < value_end {
        matcher.matcher_step()?;
        if pattern_index < pattern_end
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern_end && pattern[pattern_index] == b'*' {
            while pattern_index < pattern_end && pattern[pattern_index] == b'*' {
                matcher.matcher_step()?;
                pattern_index += 1;
            }
            if pattern_index == pattern_end {
                return Ok(true);
            }
            star_resume = Some(pattern_index);
            star_value_index = value_index;
        } else if let Some(resume) = star_resume {
            star_value_index += 1;
            value_index = star_value_index;
            pattern_index = resume;
        } else {
            return Ok(false);
        }
    }

    while pattern_index < pattern_end && pattern[pattern_index] == b'*' {
        matcher.matcher_step()?;
        pattern_index += 1;
    }
    Ok(pattern_index == pattern_end)
}

fn fixed_tokens_match(
    pattern: &[u8],
    value: &[u8],
    matcher: &mut ConstraintMatcher<'_>,
) -> Result<bool, CoreError> {
    if pattern.len() != value.len() {
        return Ok(false);
    }
    if !pattern.contains(&b'?') {
        return Ok(pattern == value);
    }
    for (&token, &byte) in pattern.iter().zip(value) {
        matcher.matcher_step()?;
        if token != b'?' && token != byte {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_glob_matches(value: &str, pattern: &str) -> bool {
        let value = value.as_bytes();
        let pattern = pattern.as_bytes();
        let mut previous = vec![false; value.len() + 1];
        previous[0] = true;
        for &token in pattern {
            let mut current = vec![false; value.len() + 1];
            if token == b'*' {
                current[0] = previous[0];
                for index in 1..=value.len() {
                    current[index] = previous[index] || current[index - 1];
                }
            } else {
                for index in 1..=value.len() {
                    current[index] =
                        previous[index - 1] && (token == b'?' || token == value[index - 1]);
                }
            }
            previous = current;
        }
        previous[value.len()]
    }

    fn strings(alphabet: &[u8], max_len: usize) -> Vec<String> {
        let mut all = vec![String::new()];
        let mut frontier = vec![String::new()];
        for _ in 0..max_len {
            let mut next = Vec::new();
            for prefix in &frontier {
                for &byte in alphabet {
                    let mut value = prefix.clone();
                    value.push(char::from(byte));
                    next.push(value);
                }
            }
            all.extend(next.iter().cloned());
            frontier = next;
        }
        all
    }

    #[test]
    fn predicates_and_short_circuit_are_deterministic() {
        let c = Constraint::All(vec![
            Constraint::Prefix {
                field: 0,
                value: "a".into(),
            },
            Constraint::Length {
                field: 1,
                min: 2,
                max: 4,
            },
        ]);
        assert!(c.matches(&["abc", "xy"]).unwrap());
        assert!(!c.matches(&["xbc", "xy"]).unwrap());
        assert!(Constraint::Glob {
            field: 0,
            pattern: "a*".into()
        }
        .matches(&["abc"])
        .unwrap());
        assert!(Constraint::NotEquals {
            field: 0,
            value: "red".into(),
        }
        .matches(&["blue"])
        .unwrap());
        assert!(!Constraint::NotEquals {
            field: 0,
            value: "red".into(),
        }
        .matches(&["red"])
        .unwrap());
    }
    #[test]
    fn deep_constraints_are_rejected() {
        let mut c = Constraint::True;
        for _ in 0..65 {
            c = Constraint::Not(Box::new(c));
        }
        assert_eq!(c.validate().unwrap_err().code, "CONSTRAINT_TOO_DEEP");
    }

    #[test]
    fn glob_matcher_matches_the_reference_language() {
        let patterns = strings(b"ab*?", 4);
        let values = strings(b"ab", 4);
        for pattern in &patterns {
            let constraint = Constraint::Glob {
                field: 0,
                pattern: pattern.clone(),
            };
            for value in &values {
                assert_eq!(
                    constraint.matches(&[value]).unwrap(),
                    reference_glob_matches(value, pattern),
                    "pattern {pattern:?}, value {value:?}"
                );
            }
        }
    }

    #[test]
    fn glob_question_mark_preserves_byte_semantics() {
        assert!(!Constraint::Glob {
            field: 0,
            pattern: "?".into(),
        }
        .matches(&["é"])
        .unwrap());
        assert!(Constraint::Glob {
            field: 0,
            pattern: "??".into(),
        }
        .matches(&["é"])
        .unwrap());
    }

    #[test]
    fn glob_work_limit_is_checked_and_inclusive() {
        let pattern = "?".repeat(4 * 1024);
        let value = "a".repeat(4 * 1024);
        assert_eq!(pattern.len() * value.len(), MAX_GLOB_WORK);
        assert!(Constraint::Glob { field: 0, pattern }
            .matches(&[&value])
            .unwrap());

        let over_limit = Constraint::Glob {
            field: 0,
            pattern: "?".repeat(4 * 1024 + 1),
        }
        .matches(&[&value])
        .unwrap_err();
        assert_eq!(over_limit.code, "CONSTRAINT_WORK_LIMIT_EXCEEDED");

        let maximum = "a".repeat(MAX_LITERAL_BYTES);
        let advisory_case = Constraint::Glob {
            field: 0,
            pattern: maximum.clone(),
        }
        .matches(&[&maximum])
        .unwrap_err();
        assert_eq!(advisory_case.code, "CONSTRAINT_WORK_LIMIT_EXCEEDED");

        let overflow = ConstraintMatcher::new(None)
            .charge_glob_work(usize::MAX, 2)
            .unwrap_err();
        assert_eq!(overflow.code, "CONSTRAINT_WORK_LIMIT_EXCEEDED");

        let mut empty_side = ConstraintMatcher::new(None);
        empty_side.charge_glob_work(MAX_GLOB_WORK, 0).unwrap();
        assert_eq!(
            empty_side.charge_glob_work(1, 0).unwrap_err().code,
            "CONSTRAINT_WORK_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn nested_globs_share_work_and_preserve_short_circuiting() {
        const LENGTH: usize = 2896;
        let value = "a".repeat(LENGTH);
        let glob = || Constraint::Glob {
            field: 0,
            pattern: "?".repeat(LENGTH),
        };
        const {
            assert!(2 * LENGTH * LENGTH <= MAX_GLOB_WORK);
            assert!(3 * LENGTH * LENGTH > MAX_GLOB_WORK);
        }

        let error = Constraint::All(vec![glob(), glob(), glob()])
            .matches(&[&value])
            .unwrap_err();
        assert_eq!(error.code, "CONSTRAINT_WORK_LIMIT_EXCEEDED");

        assert!(Constraint::Any(vec![
            Constraint::True,
            Constraint::Glob {
                field: 0,
                pattern: "?".repeat(MAX_LITERAL_BYTES),
            },
        ])
        .matches(&[&value])
        .unwrap());
    }
}
