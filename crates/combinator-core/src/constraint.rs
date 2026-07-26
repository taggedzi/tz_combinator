//! Typed, bounded, side-effect-free candidate constraints.

use crate::CoreError;

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 4096;
const MAX_LITERAL_BYTES: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Constraint {
    True,
    Equals {
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
        Ok(match self {
            Self::True => true,
            Self::Equals { field, value } => fields.get(*field).is_some_and(|v| *v == value),
            Self::Prefix { field, value } => {
                fields.get(*field).is_some_and(|v| v.starts_with(value))
            }
            Self::Suffix { field, value } => fields.get(*field).is_some_and(|v| v.ends_with(value)),
            Self::Glob { field, pattern } => fields
                .get(*field)
                .is_some_and(|value| glob_matches(value, pattern)),
            Self::Length { field, min, max } => fields.get(*field).is_some_and(|v| {
                let n = v.len();
                n >= *min && n <= *max
            }),
            Self::Not(inner) => !inner.matches(fields)?,
            Self::All(items) => {
                for item in items {
                    if !item.matches(fields)? {
                        return Ok(false);
                    }
                }
                true
            }
            Self::Any(items) => {
                for item in items {
                    if item.matches(fields)? {
                        return Ok(true);
                    }
                }
                false
            }
        })
    }
}

fn glob_matches(value: &str, pattern: &str) -> bool {
    // Iterative dynamic programming keeps matching bounded by the input and
    // pattern lengths and avoids regex/backtracking resource surprises.
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

#[cfg(test)]
mod tests {
    use super::*;
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
    }
    #[test]
    fn deep_constraints_are_rejected() {
        let mut c = Constraint::True;
        for _ in 0..65 {
            c = Constraint::Not(Box::new(c));
        }
        assert_eq!(c.validate().unwrap_err().code, "CONSTRAINT_TOO_DEEP");
    }
}
