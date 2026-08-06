use combinator_codecs::InputLimits;
use std::fmt;

pub const DEFAULT_MAX_OUTPUT_BYTES: u64 = 1_073_741_824;
pub const DEFAULT_MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
pub const DEFAULT_MAX_ITEM_BYTES: usize = 1024 * 1024;
pub const DEFAULT_MAX_ITEMS_PER_LIST: usize = 1_000_000;
pub const DEFAULT_MAX_LISTS: usize = 128;
pub const DEFAULT_MAX_TOTAL_ITEMS: usize = 5_000_000;
pub const DEFAULT_MAX_COMBINATIONS: u128 = 10_000_000;
pub const DEFAULT_MAX_JOIN_RECORDS: usize = 100_000;
pub const DEFAULT_MAX_JOIN_KEY_FANOUT: u128 = 10_000;

pub const HARD_MAX_OUTPUT_BYTES: u64 = DEFAULT_MAX_OUTPUT_BYTES;
pub const HARD_MAX_INPUT_BYTES: usize = DEFAULT_MAX_INPUT_BYTES;
pub const HARD_MAX_ITEM_BYTES: usize = DEFAULT_MAX_ITEM_BYTES;
pub const HARD_MAX_ITEMS_PER_LIST: usize = DEFAULT_MAX_ITEMS_PER_LIST;
pub const HARD_MAX_LISTS: usize = DEFAULT_MAX_LISTS;
pub const HARD_MAX_TOTAL_ITEMS: usize = DEFAULT_MAX_TOTAL_ITEMS;
pub const HARD_MAX_COMBINATIONS: u128 = DEFAULT_MAX_COMBINATIONS;
pub const HARD_MAX_JOIN_RECORDS: usize = 250_000;
pub const HARD_MAX_JOIN_KEY_FANOUT: u128 = 100_000;
pub const HARD_MAX_TIMEOUT_MS: u64 = 3_600_000;

/// Caller-selected application limits. First-party application entry points
/// reject values above [`HARD_RESOURCE_LIMITS`] before performing work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceLimits {
    pub max_output_bytes: u128,
    pub max_input_bytes: usize,
    pub max_item_bytes: usize,
    pub max_items_per_list: usize,
    pub max_lists: usize,
    pub max_total_items: usize,
    pub max_combinations: u128,
    pub max_join_records: usize,
    pub max_join_key_fanout: u128,
    /// Optional caller-requested cancellation deadline. `None` preserves the
    /// existing desktop/CLI behavior; service wrappers must supply their own
    /// trusted deadline and may let clients shorten, but never extend, it.
    pub timeout_ms: Option<u64>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: DEFAULT_MAX_OUTPUT_BYTES as u128,
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_item_bytes: DEFAULT_MAX_ITEM_BYTES,
            max_items_per_list: DEFAULT_MAX_ITEMS_PER_LIST,
            max_lists: DEFAULT_MAX_LISTS,
            max_total_items: DEFAULT_MAX_TOTAL_ITEMS,
            max_combinations: DEFAULT_MAX_COMBINATIONS,
            max_join_records: DEFAULT_MAX_JOIN_RECORDS,
            max_join_key_fanout: DEFAULT_MAX_JOIN_KEY_FANOUT,
            timeout_ms: None,
        }
    }
}

/// Immutable compiled ceilings for every first-party application surface.
pub const HARD_RESOURCE_LIMITS: ResourceLimits = ResourceLimits {
    max_output_bytes: HARD_MAX_OUTPUT_BYTES as u128,
    max_input_bytes: HARD_MAX_INPUT_BYTES,
    max_item_bytes: HARD_MAX_ITEM_BYTES,
    max_items_per_list: HARD_MAX_ITEMS_PER_LIST,
    max_lists: HARD_MAX_LISTS,
    max_total_items: HARD_MAX_TOTAL_ITEMS,
    max_combinations: HARD_MAX_COMBINATIONS,
    max_join_records: HARD_MAX_JOIN_RECORDS,
    max_join_key_fanout: HARD_MAX_JOIN_KEY_FANOUT,
    timeout_ms: Some(HARD_MAX_TIMEOUT_MS),
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitField {
    MaxOutputBytes,
    MaxInputBytes,
    MaxItemBytes,
    MaxItemsPerList,
    MaxLists,
    MaxTotalItems,
    MaxCombinations,
    MaxJoinRecords,
    MaxJoinKeyFanout,
    TimeoutMs,
}

impl LimitField {
    pub const fn name(self) -> &'static str {
        match self {
            Self::MaxOutputBytes => "max-output-bytes",
            Self::MaxInputBytes => "max-input-bytes",
            Self::MaxItemBytes => "max-item-bytes",
            Self::MaxItemsPerList => "max-items-per-list",
            Self::MaxLists => "max-lists",
            Self::MaxTotalItems => "max-total-items",
            Self::MaxCombinations => "max-combinations",
            Self::MaxJoinRecords => "max-join-records",
            Self::MaxJoinKeyFanout => "max-join-key-fanout",
            Self::TimeoutMs => "timeout-ms",
        }
    }

    pub const fn hard_limit(self) -> u128 {
        match self {
            Self::MaxOutputBytes => HARD_RESOURCE_LIMITS.max_output_bytes,
            Self::MaxInputBytes => HARD_RESOURCE_LIMITS.max_input_bytes as u128,
            Self::MaxItemBytes => HARD_RESOURCE_LIMITS.max_item_bytes as u128,
            Self::MaxItemsPerList => HARD_RESOURCE_LIMITS.max_items_per_list as u128,
            Self::MaxLists => HARD_RESOURCE_LIMITS.max_lists as u128,
            Self::MaxTotalItems => HARD_RESOURCE_LIMITS.max_total_items as u128,
            Self::MaxCombinations => HARD_RESOURCE_LIMITS.max_combinations,
            Self::MaxJoinRecords => HARD_RESOURCE_LIMITS.max_join_records as u128,
            Self::MaxJoinKeyFanout => HARD_RESOURCE_LIMITS.max_join_key_fanout,
            Self::TimeoutMs => HARD_MAX_TIMEOUT_MS as u128,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitViolation {
    pub field: LimitField,
    pub requested: u128,
    pub hard_limit: u128,
}

impl fmt::Display for LimitViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} exceeds the hard security ceiling (requested {}, hard limit {})",
            self.field.name(),
            self.requested,
            self.hard_limit
        )
    }
}

pub fn validate_limit(field: LimitField, requested: u128) -> Result<(), LimitViolation> {
    let hard_limit = field.hard_limit();
    if requested > hard_limit {
        return Err(LimitViolation {
            field,
            requested,
            hard_limit,
        });
    }
    Ok(())
}

pub fn validate_resource_limits(limits: &ResourceLimits) -> Result<(), LimitViolation> {
    let checks = [
        (LimitField::MaxOutputBytes, limits.max_output_bytes),
        (LimitField::MaxInputBytes, limits.max_input_bytes as u128),
        (LimitField::MaxItemBytes, limits.max_item_bytes as u128),
        (
            LimitField::MaxItemsPerList,
            limits.max_items_per_list as u128,
        ),
        (LimitField::MaxLists, limits.max_lists as u128),
        (LimitField::MaxTotalItems, limits.max_total_items as u128),
        (LimitField::MaxCombinations, limits.max_combinations),
        (LimitField::MaxJoinRecords, limits.max_join_records as u128),
        (LimitField::MaxJoinKeyFanout, limits.max_join_key_fanout),
    ];
    for (field, requested) in checks {
        validate_limit(field, requested)?;
    }
    if let Some(timeout_ms) = limits.timeout_ms {
        validate_limit(LimitField::TimeoutMs, timeout_ms as u128)?;
    }
    Ok(())
}

pub fn validate_input_limits(limits: InputLimits) -> Result<(), LimitViolation> {
    validate_limit(LimitField::MaxInputBytes, limits.max_input_bytes as u128)?;
    validate_limit(LimitField::MaxItemBytes, limits.max_item_bytes as u128)?;
    validate_limit(
        LimitField::MaxItemsPerList,
        limits.max_items_per_list as u128,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hard_limit_is_inclusive_and_rejects_the_next_value() {
        let fields = [
            LimitField::MaxOutputBytes,
            LimitField::MaxInputBytes,
            LimitField::MaxItemBytes,
            LimitField::MaxItemsPerList,
            LimitField::MaxLists,
            LimitField::MaxTotalItems,
            LimitField::MaxCombinations,
            LimitField::MaxJoinRecords,
            LimitField::MaxJoinKeyFanout,
            LimitField::TimeoutMs,
        ];
        for field in fields {
            let hard_limit = field.hard_limit();
            assert_eq!(validate_limit(field, hard_limit), Ok(()));
            assert_eq!(
                validate_limit(field, hard_limit + 1),
                Err(LimitViolation {
                    field,
                    requested: hard_limit + 1,
                    hard_limit,
                })
            );
        }
    }

    #[test]
    fn defaults_are_within_the_compiled_ceilings() {
        validate_resource_limits(&ResourceLimits::default()).unwrap();
    }
}
