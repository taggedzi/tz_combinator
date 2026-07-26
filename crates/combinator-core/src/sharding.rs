//! Checked, interface-neutral contiguous work partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShardRange {
    pub start: u128,
    pub end: u128,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShardError {
    ZeroCount,
    IndexOutOfRange,
    Overflow,
}
pub fn range(total: u128, index: u128, count: u128) -> Result<ShardRange, ShardError> {
    if count == 0 {
        return Err(ShardError::ZeroCount);
    }
    if index >= count {
        return Err(ShardError::IndexOutOfRange);
    }
    let base = total / count;
    let rem = total % count;
    let start = index
        .checked_mul(base)
        .and_then(|v| v.checked_add(index.min(rem)))
        .ok_or(ShardError::Overflow)?;
    let len = base
        .checked_add(u128::from(index < rem))
        .ok_or(ShardError::Overflow)?;
    let end = start.checked_add(len).ok_or(ShardError::Overflow)?;
    Ok(ShardRange { start, end })
}
pub fn page(range: ShardRange, offset: u128, limit: Option<u128>) -> (u128, u128) {
    let start = range.start.max(offset).min(range.end);
    let end = range
        .end
        .min(
            limit
                .and_then(|v| offset.checked_add(v))
                .unwrap_or(u128::MAX),
        )
        .max(start);
    (start, end - start)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partitions_are_contiguous_and_checked() {
        let ranges: Vec<_> = (0..4).map(|index| range(10, index, 4).unwrap()).collect();
        assert_eq!(ranges[0], ShardRange { start: 0, end: 3 });
        assert_eq!(ranges[3], ShardRange { start: 8, end: 10 });
        assert_eq!(range(1, 0, 0), Err(ShardError::ZeroCount));
        assert_eq!(range(1, 1, 1), Err(ShardError::IndexOutOfRange));
    }
}
