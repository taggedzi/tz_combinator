//! Checked, deterministic contiguous work partitioning.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardRange {
    pub start: u128,
    pub end: u128,
}

/// Returns a half-open range. Earlier shards receive one extra record.
pub fn range(total: u128, index: u128, count: u128) -> Result<ShardRange, ShardError> {
    if count == 0 {
        return Err(ShardError::ZeroCount);
    }
    if index >= count {
        return Err(ShardError::IndexOutOfRange);
    }
    let base = total / count;
    let remainder = total % count;
    let extra_before = index.min(remainder);
    let start = index
        .checked_mul(base)
        .and_then(|value| value.checked_add(extra_before))
        .ok_or(ShardError::Overflow)?;
    let length = base
        .checked_add(u128::from(index < remainder))
        .ok_or(ShardError::Overflow)?;
    let end = start.checked_add(length).ok_or(ShardError::Overflow)?;
    Ok(ShardRange { start, end })
}

/// Intersects a shard with the caller's existing global page.
pub fn page(range: ShardRange, offset: u128, limit: Option<u128>) -> (u128, u128) {
    let start = range.start.max(offset).min(range.end);
    let requested_end = limit
        .and_then(|value| offset.checked_add(value))
        .unwrap_or(u128::MAX);
    let end = range.end.min(requested_end).max(start);
    (start, end - start)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardError {
    ZeroCount,
    IndexOutOfRange,
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_without_gaps_or_duplicates() {
        let ranges: Vec<_> = (0..4).map(|i| range(10, i, 4).unwrap()).collect();
        assert_eq!(
            ranges,
            vec![
                ShardRange { start: 0, end: 3 },
                ShardRange { start: 3, end: 6 },
                ShardRange { start: 6, end: 8 },
                ShardRange { start: 8, end: 10 },
            ]
        );
    }

    #[test]
    fn page_intersection_is_bounded() {
        let shard = ShardRange { start: 3, end: 6 };
        assert_eq!(page(shard, 0, None), (3, 3));
        assert_eq!(page(shard, 4, Some(99)), (4, 2));
        assert_eq!(page(shard, 99, Some(1)), (6, 0));
    }

    #[test]
    fn rejects_invalid_and_overflowing_ranges() {
        assert_eq!(range(1, 0, 0), Err(ShardError::ZeroCount));
        assert_eq!(range(1, 1, 1), Err(ShardError::IndexOutOfRange));
        assert_eq!(
            range(u128::MAX, 0, 1),
            Ok(ShardRange {
                start: 0,
                end: u128::MAX,
            })
        );
    }
}
