//! Lazy concat (sequential emission) as a (list, item) index iterator.

use crate::count::Count;

#[derive(Debug, Clone, Default)]
pub struct ConcatOptions {
    pub reverse: bool,
    pub offset: u128,
    pub limit: Option<u128>,
}

/// Counts the records `concat` will produce for `lens` (checked sum).
pub fn concat_count(lens: &[usize]) -> Count {
    let mut acc: u128 = 0;
    for &n in lens {
        match acc.checked_add(n as u128) {
            Some(v) => acc = v,
            None => return Count::Overflow,
        }
    }
    Count::Exact(acc)
}

/// Lazy iterator over `(list_index, item_index)` pairs of the concatenation.
#[derive(Debug)]
pub struct Concat {
    /// Prefix sums: `prefix[j]` = sum of lengths of lists `0..j`. Length is
    /// `lens.len() + 1`; `prefix[lens.len()]` is the grand total.
    prefix: Vec<u128>,
    next_pos: u128,
    remaining: u128,
    descending: bool,
}

/// Builds a lazy concat iterator over `lists` honoring `opts`.
///
/// Returns `None` only if the checked sum of all list lengths overflows
/// `u128` — structurally unreachable given upstream input-size limits, but
/// checked rather than assumed.
pub fn concat_records(lists: &[Vec<String>], opts: ConcatOptions) -> Option<Concat> {
    let mut prefix = Vec::with_capacity(lists.len() + 1);
    prefix.push(0u128);
    let mut acc: u128 = 0;
    for list in lists {
        acc = acc.checked_add(list.len() as u128)?;
        prefix.push(acc);
    }
    let total = acc;

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

    Some(Concat {
        prefix,
        next_pos: start,
        remaining: to_emit,
        descending: opts.reverse,
    })
}

impl Iterator for Concat {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.remaining == 0 {
            return None;
        }
        let pos = self.next_pos;
        // Largest j with prefix[j] <= pos; safe because remaining > 0
        // guarantees pos < prefix[last].
        let list_idx = match self.prefix.binary_search(&pos) {
            Ok(exact) => exact,
            Err(insert_at) => insert_at - 1,
        };
        let item_idx = (pos - self.prefix[list_idx]) as usize;
        self.remaining -= 1;
        if self.descending {
            self.next_pos = self.next_pos.saturating_sub(1);
        } else {
            self.next_pos = self.next_pos.saturating_add(1);
        }
        Some((list_idx, item_idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(lists: &[Vec<String>], opts: ConcatOptions) -> Vec<(usize, usize)> {
        concat_records(lists, opts).unwrap().collect()
    }

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a0".into(), "a1".into()],
            vec!["b0".into()],
            vec!["c0".into(), "c1".into(), "c2".into()],
        ]
    }

    #[test]
    fn emits_every_list_in_order() {
        assert_eq!(
            collect(&lists(), ConcatOptions::default()),
            vec![(0, 0), (0, 1), (1, 0), (2, 0), (2, 1), (2, 2)]
        );
    }

    #[test]
    fn empty_lists_contribute_nothing() {
        let ls = vec![Vec::<String>::new(), vec!["x".into()], Vec::<String>::new()];
        assert_eq!(collect(&ls, ConcatOptions::default()), vec![(1, 0)]);
    }

    #[test]
    fn offset_and_limit_paginate() {
        let opts = ConcatOptions {
            offset: 2,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(&lists(), opts), vec![(1, 0), (2, 0)]);
    }

    #[test]
    fn reverse_walks_from_the_end() {
        let opts = ConcatOptions {
            reverse: true,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(&lists(), opts), vec![(2, 2), (2, 1)]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let opts = ConcatOptions {
            offset: 99,
            ..Default::default()
        };
        assert!(collect(&lists(), opts).is_empty());
    }

    #[test]
    fn concat_count_is_checked_sum() {
        let lens = [2usize, 1, 3];
        assert_eq!(concat_count(&lens), Count::Exact(6));
    }

    #[test]
    fn reverse_offset_at_and_past_end_is_empty_or_last_record() {
        let lists = vec![vec!["a".into(), "b".into()]];
        assert_eq!(
            collect(
                &lists,
                ConcatOptions {
                    reverse: true,
                    offset: 0,
                    ..Default::default()
                }
            ),
            vec![(0, 1), (0, 0)]
        );
        assert_eq!(
            collect(
                &lists,
                ConcatOptions {
                    reverse: true,
                    offset: 1,
                    ..Default::default()
                }
            ),
            vec![(0, 0)]
        );
        assert!(collect(
            &lists,
            ConcatOptions {
                reverse: true,
                offset: 2,
                ..Default::default()
            }
        )
        .is_empty());
    }
}
