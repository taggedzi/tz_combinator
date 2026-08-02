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

fn reset_available(available: &mut Vec<usize>, n: usize) {
    if available.len() != n {
        available.clear();
        available.extend(0..n);
    } else {
        for (value, item) in available.iter_mut().enumerate() {
            *item = value;
        }
    }
}

fn unrank_permutation(
    n: usize,
    mut rank: u128,
    blocks: &[u128],
    available: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    reset_available(available, n);
    let mut output = Vec::with_capacity(n);
    for position in 0..n {
        let block = blocks[n - position - 1];
        let position = usize::try_from(rank / block).ok()?;
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

fn unrank_variation(
    n: usize,
    k: usize,
    mut rank: u128,
    blocks: &[u128],
    available: &mut Vec<usize>,
) -> Option<Vec<usize>> {
    reset_available(available, n);
    let mut output = Vec::with_capacity(k);
    for &block in blocks.iter().take(k) {
        let selected = usize::try_from(rank / block).ok()?;
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

fn factorial_blocks(n: usize) -> Option<Vec<u128>> {
    let mut blocks = Vec::with_capacity(n);
    let mut value = 1u128;
    for width in 0..n {
        if width > 0 {
            value = value.checked_mul(width as u128)?;
        }
        blocks.push(value);
    }
    Some(blocks)
}

fn variation_blocks(n: usize, k: usize) -> Option<Vec<u128>> {
    if k > n {
        return Some(Vec::new());
    }
    (0..k)
        .map(|position| falling_factorial(n - position - 1, k - position - 1).exact())
        .collect()
}

#[derive(Debug)]
pub struct Permutations {
    next: u128,
    remaining: u128,
    reverse: bool,
    n: usize,
    blocks: Vec<u128>,
    available: Vec<usize>,
}

impl Iterator for Permutations {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let rank = self.next;
        self.remaining -= 1;
        self.next = if self.reverse {
            self.next.saturating_sub(1)
        } else {
            self.next.saturating_add(1)
        };
        unrank_permutation(self.n, rank, &self.blocks, &mut self.available)
    }
}

#[derive(Debug)]
pub struct Combinations {
    next: u128,
    remaining: u128,
    reverse: bool,
    n: usize,
    k: usize,
}

impl Iterator for Combinations {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let rank = self.next;
        self.remaining -= 1;
        self.next = if self.reverse {
            self.next.saturating_sub(1)
        } else {
            self.next.saturating_add(1)
        };
        unrank_combination(self.n, self.k, rank)
    }
}

#[derive(Debug)]
pub struct Variations {
    next: u128,
    remaining: u128,
    reverse: bool,
    n: usize,
    k: usize,
    blocks: Vec<u128>,
    available: Vec<usize>,
}

impl Iterator for Variations {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let rank = self.next;
        self.remaining -= 1;
        self.next = if self.reverse {
            self.next.saturating_sub(1)
        } else {
            self.next.saturating_add(1)
        };
        unrank_variation(self.n, self.k, rank, &self.blocks, &mut self.available)
    }
}

pub fn permutations(n: usize, options: SelectionOptions) -> Result<Permutations, CoreError> {
    let (next, remaining, reverse) = window(factorial(n), options)?;
    let blocks = if remaining == 0 {
        Vec::new()
    } else {
        factorial_blocks(n).ok_or_else(|| {
            CoreError::runtime("COUNT_OVERFLOW", "permutation rank blocks overflowed")
        })?
    };
    Ok(Permutations {
        next,
        remaining,
        reverse,
        n,
        blocks,
        available: Vec::new(),
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
    let blocks = if remaining == 0 {
        Vec::new()
    } else {
        variation_blocks(n, k).ok_or_else(|| {
            CoreError::runtime("COUNT_OVERFLOW", "variation rank blocks overflowed")
        })?
    };
    Ok(Variations {
        next,
        remaining,
        reverse,
        n,
        k,
        blocks,
        available: Vec::new(),
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
    fn combinations_match_lexicographic_reference_for_small_inputs() {
        fn visit(
            next: usize,
            n: usize,
            remaining: usize,
            current: &mut Vec<usize>,
            output: &mut Vec<Vec<usize>>,
        ) {
            if remaining == 0 {
                output.push(current.clone());
                return;
            }
            for candidate in next..=n - remaining {
                current.push(candidate);
                visit(candidate + 1, n, remaining - 1, current, output);
                current.pop();
            }
        }

        for n in 0..=8 {
            for k in 0..=n {
                let mut expected = Vec::new();
                visit(0, n, k, &mut Vec::new(), &mut expected);
                assert_eq!(
                    combinations(n, k, opts()).unwrap().collect::<Vec<_>>(),
                    expected,
                    "n={n}, k={k}"
                );
            }
        }
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

    #[test]
    fn empty_windows_do_not_allocate_rank_scratch() {
        let permutations = permutations(
            8,
            SelectionOptions {
                limit: Some(0),
                ..opts()
            },
        )
        .unwrap();
        assert!(permutations.blocks.is_empty());

        let variations = variations(
            8,
            4,
            SelectionOptions {
                offset: u128::MAX,
                ..opts()
            },
        )
        .unwrap();
        assert!(variations.blocks.is_empty());
    }
}
