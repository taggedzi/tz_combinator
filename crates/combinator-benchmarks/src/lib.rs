//! Deterministic, bounded fixtures shared by the workspace benchmarks.
//!
//! This crate is deliberately non-published and has no production consumers.

use combinator_app::{AppError, OutputRecord, OutputSink};
use combinator_core::Record;

/// Exact fixture sizes used by benchmark names and the benchmark guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureSize {
    Small,
    Medium,
    Large,
}

impl FixtureSize {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub const fn records(self) -> usize {
        match self {
            Self::Small => 128,
            Self::Medium => 2_048,
            Self::Large => 8_192,
        }
    }
}

/// Builds stable ASCII values with a fixed payload width.
pub fn values(prefix: &str, count: usize, payload_width: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            let suffix = index.to_string();
            let padding = payload_width.saturating_sub(prefix.len() + suffix.len() + 1);
            format!("{prefix}-{}{suffix}", "x".repeat(padding))
        })
        .collect()
}

/// Builds named lists whose cardinalities are supplied by the caller.
pub fn lists(lengths: &[usize], payload_width: usize) -> Vec<Vec<String>> {
    lengths
        .iter()
        .enumerate()
        .map(|(index, length)| values(&format!("l{index}"), *length, payload_width))
        .collect()
}

/// Computes a fixture product with checked arithmetic.
pub fn checked_product(lengths: &[usize]) -> u128 {
    lengths.iter().fold(1u128, |product, length| {
        product
            .checked_mul(*length as u128)
            .expect("bounded benchmark product must fit in u128")
    })
}

/// A stable checksum that forces every byte through the measured path.
pub fn checksum_bytes(mut checksum: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        checksum ^= u64::from(*byte);
        checksum = checksum.wrapping_mul(1_099_511_628_211);
    }
    checksum
}

/// Application sink that consumes all encoded bytes without retaining output.
#[derive(Debug, Default)]
pub struct CountingSink {
    pub records: u128,
    pub bytes: u128,
    pub checksum: u64,
}

impl OutputSink for CountingSink {
    fn record(&mut self, record: OutputRecord) -> Result<(), AppError> {
        self.records = self
            .records
            .checked_add(1)
            .expect("bounded benchmark record count must fit in u128");
        self.bytes = self
            .bytes
            .checked_add(record.value.len() as u128)
            .expect("bounded benchmark byte count must fit in u128");
        self.checksum = checksum_bytes(self.checksum, record.value.as_bytes());
        for field in &record.fields {
            self.checksum = checksum_bytes(self.checksum, field.as_bytes());
        }
        Ok(())
    }
}

/// Builds deterministic structured records for join benchmarks.
pub fn join_records(
    side: &str,
    count: usize,
    distinct_keys: usize,
    key_prefix: &str,
) -> Vec<Record> {
    assert!(distinct_keys > 0, "join fixtures require at least one key");
    (0..count)
        .map(|index| Record {
            fields: vec![
                (
                    "id".to_string(),
                    format!("{key_prefix}{:08}", index % distinct_keys),
                ),
                ("side".to_string(), side.to_string()),
                ("payload".to_string(), format!("{side}-payload-{index:08}")),
            ],
        })
        .collect()
}
