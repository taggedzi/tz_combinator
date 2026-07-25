//! Lazy zip (positional pairing) as an index-tuple iterator.

use crate::count::Count;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnequalPolicy {
    Error,
    Truncate,
    Cycle,
}

#[derive(Debug, Clone)]
pub struct ZipOptions {
    pub on_unequal: UnequalPolicy,
    pub reverse: bool,
    pub offset: u128,
    pub limit: Option<u128>,
}

impl Default for ZipOptions {
    fn default() -> Self {
        Self {
            on_unequal: UnequalPolicy::Error,
            reverse: false,
            offset: 0,
            limit: None,
        }
    }
}

/// Returned when `UnequalPolicy::Error` is selected and list lengths differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipLengthMismatch;

/// The number of records `zip` will produce for `lens` under `policy`.
fn effective_len(lens: &[usize], policy: UnequalPolicy) -> Result<usize, ZipLengthMismatch> {
    if lens.contains(&0) {
        return Ok(0);
    }
    match policy {
        UnequalPolicy::Error => {
            let first = lens.first().copied().unwrap_or(0);
            if lens.iter().all(|&n| n == first) {
                Ok(first)
            } else {
                Err(ZipLengthMismatch)
            }
        }
        UnequalPolicy::Truncate => Ok(lens.iter().copied().min().unwrap_or(0)),
        UnequalPolicy::Cycle => Ok(lens.iter().copied().max().unwrap_or(0)),
    }
}

/// Counts the records `zip` will produce for `lens` under `policy`.
pub fn zip_count(lens: &[usize], policy: UnequalPolicy) -> Result<Count, ZipLengthMismatch> {
    effective_len(lens, policy).map(|n| Count::Exact(n as u128))
}

/// Lazy iterator over index tuples of the zip.
#[derive(Debug)]
pub struct Zip {
    lens: Vec<usize>,
    next_pos: u128,
    remaining: u128,
    descending: bool,
}

/// Builds a lazy zip iterator over `lists` honoring `opts`.
///
/// Fails only under `UnequalPolicy::Error` with mismatched non-zero lengths.
pub fn zip_records(lists: &[Vec<String>], opts: ZipOptions) -> Result<Zip, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    let total = effective_len(&lens, opts.on_unequal)? as u128;

    let available = total.saturating_sub(opts.offset);
    let to_emit = match opts.limit {
        Some(l) => available.min(l),
        None => available,
    };
    let start = if opts.reverse {
        total.saturating_sub(1).saturating_sub(opts.offset)
    } else {
        opts.offset
    };

    Ok(Zip {
        lens,
        next_pos: start,
        remaining: to_emit,
        descending: opts.reverse,
    })
}

impl Iterator for Zip {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        if self.remaining == 0 {
            return None;
        }
        let pos = self.next_pos;
        // Safe: `remaining > 0` implies `total > 0`, which implies every
        // length in `self.lens` is non-zero (see `effective_len`).
        let indices: Vec<usize> = self
            .lens
            .iter()
            .map(|&len| (pos % len as u128) as usize)
            .collect();
        self.remaining -= 1;
        if self.descending {
            self.next_pos = self.next_pos.saturating_sub(1);
        } else {
            self.next_pos = self.next_pos.saturating_add(1);
        }
        Some(indices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(lists: &[Vec<String>], opts: ZipOptions) -> Vec<Vec<usize>> {
        zip_records(lists, opts).unwrap().collect()
    }

    fn lists3x2() -> Vec<Vec<String>> {
        vec![
            vec!["a0".into(), "a1".into()],
            vec!["b0".into(), "b1".into()],
            vec!["c0".into(), "c1".into()],
        ]
    }

    #[test]
    fn equal_lengths_pairs_positionally() {
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        };
        assert_eq!(
            collect(&lists3x2(), opts),
            vec![vec![0, 0, 0], vec![1, 1, 1]]
        );
    }

    #[test]
    fn error_policy_rejects_mismatched_lengths() {
        let lists = vec![vec!["a".into(), "b".into()], vec!["x".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        };
        assert_eq!(zip_records(&lists, opts).unwrap_err(), ZipLengthMismatch);
    }

    #[test]
    fn truncate_uses_shortest_length() {
        let lists = vec![
            vec!["a".into(), "b".into(), "c".into()],
            vec!["x".into(), "y".into()],
        ];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Truncate,
            ..Default::default()
        };
        assert_eq!(collect(&lists, opts), vec![vec![0, 0], vec![1, 1]]);
    }

    #[test]
    fn cycle_wraps_shorter_lists() {
        let lists = vec![
            vec!["a".into(), "b".into(), "c".into()],
            vec!["x".into(), "y".into()],
        ];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Cycle,
            ..Default::default()
        };
        assert_eq!(
            collect(&lists, opts),
            vec![vec![0, 0], vec![1, 1], vec![2, 0]]
        );
    }

    #[test]
    fn any_empty_list_forces_zero_regardless_of_policy() {
        for policy in [
            UnequalPolicy::Error,
            UnequalPolicy::Truncate,
            UnequalPolicy::Cycle,
        ] {
            let lists = vec![vec!["a".into()], Vec::<String>::new()];
            let opts = ZipOptions {
                on_unequal: policy,
                ..Default::default()
            };
            assert!(collect(&lists, opts).is_empty());
        }
    }

    #[test]
    fn reverse_offset_and_limit_paginate_from_end() {
        let lists = vec![vec!["a".into(), "b".into(), "c".into(), "d".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            reverse: true,
            offset: 1,
            limit: Some(2),
        };
        assert_eq!(collect(&lists, opts), vec![vec![2], vec![1]]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let lists = vec![vec!["a".into(), "b".into()]];
        let opts = ZipOptions {
            on_unequal: UnequalPolicy::Error,
            offset: 99,
            ..Default::default()
        };
        assert!(collect(&lists, opts).is_empty());
    }

    #[test]
    fn zip_count_matches_effective_length() {
        let lens = [3usize, 2];
        assert_eq!(
            zip_count(&lens, UnequalPolicy::Truncate).unwrap(),
            Count::Exact(2)
        );
        assert_eq!(
            zip_count(&lens, UnequalPolicy::Cycle).unwrap(),
            Count::Exact(3)
        );
        assert_eq!(
            zip_count(&lens, UnequalPolicy::Error).unwrap_err(),
            ZipLengthMismatch
        );
    }
}
