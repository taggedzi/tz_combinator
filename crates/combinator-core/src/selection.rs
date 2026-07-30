//! Lazy selections from one logical item pool.

use crate::{CoreError, Count};

/// Common paging controls for selection operations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SelectionOptions {
    pub reverse: bool,
    pub offset: u128,
    pub limit: Option<u128>,
}

pub fn factorial(n: usize) -> Count {
    let mut result = 1u128;
    for value in 2..=n {
        result = match result.checked_mul(value as u128) {
            Some(value) => value,
            None => return Count::Overflow,
        };
    }
    Count::Exact(result)
}

pub fn binomial(n: usize, k: usize) -> Count {
    if k > n {
        return Count::Exact(0);
    }
    let k = k.min(n - k);
    let mut result = 1u128;
    for i in 1..=k {
        let numerator = (n - k + i) as u128;
        result = match result
            .checked_mul(numerator)
            .and_then(|v| v.checked_div(i as u128))
        {
            Some(value) => value,
            None => return Count::Overflow,
        };
    }
    Count::Exact(result)
}

pub fn falling_factorial(n: usize, k: usize) -> Count {
    if k > n {
        return Count::Exact(0);
    }
    let mut result = 1u128;
    for value in (n - k + 1)..=n {
        result = match result.checked_mul(value as u128) {
            Some(value) => value,
            None => return Count::Overflow,
        };
    }
    Count::Exact(result)
}

fn window(total: Count, options: SelectionOptions) -> Result<(u128, u128, bool), CoreError> {
    let total = match total {
        Count::Exact(value) => value,
        Count::Overflow => {
            return Err(CoreError::runtime(
                "COUNT_OVERFLOW",
                "selection count overflowed",
            ))
        }
    };
    let available = total.saturating_sub(options.offset);
    let amount = available.min(options.limit.unwrap_or(u128::MAX));
    let start = if options.reverse {
        total.saturating_sub(1).saturating_sub(options.offset)
    } else {
        options.offset
    };
    Ok((start, amount, options.reverse))
}

fn unrank_permutation(n: usize, mut rank: u128) -> Option<Vec<usize>> {
    let mut available: Vec<usize> = (0..n).collect();
    let mut output = Vec::with_capacity(n);
    for width in (1..=n).rev() {
        let block = factorial(width - 1).exact()?;
        let position = (rank / block) as usize;
        rank %= block;
        output.push(available.remove(position));
    }
    Some(output)
}

fn unrank_combination(n: usize, k: usize, mut rank: u128) -> Option<Vec<usize>> {
    let mut output = Vec::with_capacity(k);
    let mut next = 0;
    for remaining in (1..=k).rev() {
        for candidate in next..=n - remaining {
            let count = binomial(n - candidate - 1, remaining - 1).exact()?;
            if rank < count {
                output.push(candidate);
                next = candidate + 1;
                break;
            }
            rank -= count;
        }
    }
    Some(output)
}

fn unrank_variation(n: usize, k: usize, mut rank: u128) -> Option<Vec<usize>> {
    let mut available: Vec<usize> = (0..n).collect();
    let mut output = Vec::with_capacity(k);
    for position in 0..k {
        let block = falling_factorial(n - position - 1, k - position - 1).exact()?;
        let selected = (rank / block) as usize;
        rank %= block;
        output.push(available.remove(selected));
    }
    Some(output)
}

trait ExactCount {
    fn exact(self) -> Option<u128>;
}
impl ExactCount for Count {
    fn exact(self) -> Option<u128> {
        match self {
            Count::Exact(value) => Some(value),
            Count::Overflow => None,
        }
    }
}

macro_rules! selection_iterator {
    ($name:ident, $rank:ident, $count:ident, $unrank:ident $(, $arg:ident)*) => {
        #[derive(Debug)]
        pub struct $name { next: u128, remaining: u128, reverse: bool, $( $arg: usize, )* }
        impl Iterator for $name {
            type Item = Vec<usize>;
            fn next(&mut self) -> Option<Self::Item> {
                if self.remaining == 0 { return None; }
                let rank = self.next;
                self.remaining -= 1;
                self.next = if self.reverse { self.next.saturating_sub(1) } else { self.next.saturating_add(1) };
                $unrank($(self.$arg,)* rank)
            }
        }
    };
}

selection_iterator!(Permutations, factorial, factorial, unrank_permutation, n);
selection_iterator!(Combinations, binomial, binomial, unrank_combination, n, k);
selection_iterator!(
    Variations,
    falling_factorial,
    falling_factorial,
    unrank_variation,
    n,
    k
);

pub fn permutations(n: usize, options: SelectionOptions) -> Result<Permutations, CoreError> {
    let (next, remaining, reverse) = window(factorial(n), options)?;
    Ok(Permutations {
        next,
        remaining,
        reverse,
        n,
    })
}
pub fn combinations(
    n: usize,
    k: usize,
    options: SelectionOptions,
) -> Result<Combinations, CoreError> {
    let (next, remaining, reverse) = window(binomial(n, k), options)?;
    Ok(Combinations {
        next,
        remaining,
        reverse,
        n,
        k,
    })
}
pub fn variations(n: usize, k: usize, options: SelectionOptions) -> Result<Variations, CoreError> {
    let (next, remaining, reverse) = window(falling_factorial(n, k), options)?;
    Ok(Variations {
        next,
        remaining,
        reverse,
        n,
        k,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn opts() -> SelectionOptions {
        SelectionOptions::default()
    }
    #[test]
    fn permutation_order_and_reverse_page() {
        assert_eq!(
            permutations(3, opts()).unwrap().collect::<Vec<_>>(),
            vec![
                vec![0, 1, 2],
                vec![0, 2, 1],
                vec![1, 0, 2],
                vec![1, 2, 0],
                vec![2, 0, 1],
                vec![2, 1, 0]
            ]
        );
        assert_eq!(
            permutations(
                3,
                SelectionOptions {
                    reverse: true,
                    offset: 1,
                    limit: Some(2)
                }
            )
            .unwrap()
            .collect::<Vec<_>>(),
            vec![vec![2, 0, 1], vec![1, 2, 0]]
        );
    }
    #[test]
    fn combination_edges() {
        assert_eq!(binomial(4, 0), Count::Exact(1));
        assert!(combinations(4, 5, opts()).unwrap().next().is_none());
        assert_eq!(
            combinations(4, 2, opts()).unwrap().collect::<Vec<_>>(),
            vec![
                vec![0, 1],
                vec![0, 2],
                vec![0, 3],
                vec![1, 2],
                vec![1, 3],
                vec![2, 3]
            ]
        );
    }
    #[test]
    fn variation_matches_permutation_at_n() {
        assert_eq!(
            variations(3, 3, opts()).unwrap().collect::<Vec<_>>(),
            permutations(3, opts()).unwrap().collect::<Vec<_>>()
        );
    }

    #[test]
    fn duplicate_values_remain_distinct_positions() {
        assert_eq!(
            permutations(2, opts()).unwrap().collect::<Vec<_>>(),
            vec![vec![0, 1], vec![1, 0]]
        );
    }

    #[test]
    fn large_factorials_fail_closed() {
        assert_eq!(factorial(35), Count::Overflow);
        assert_eq!(falling_factorial(40, 40), Count::Overflow);
    }
}
