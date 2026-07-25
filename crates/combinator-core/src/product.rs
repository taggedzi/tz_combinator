//! Lazy ordered Cartesian product as an index-tuple iterator.

/// Options controlling iteration order and windowing.
#[derive(Debug, Clone, Default)]
pub struct ProductOptions {
    /// Traverse the complete default product order from last to first.
    pub reverse: bool,
    /// When true, the leftmost list varies fastest (default: rightmost fastest).
    pub reverse_fields: bool,
    /// Number of leading combinations to skip.
    pub offset: u128,
    /// Maximum number of combinations to emit.
    pub limit: Option<u128>,
}

/// Lazy iterator over index tuples of the ordered Cartesian product.
pub struct Product {
    lens: Vec<usize>,
    digits: Vec<usize>,
    /// Positions ordered least-significant first.
    lsd_order: Vec<usize>,
    remaining: Option<u128>,
    exhausted: bool,
    started: bool,
    descending: bool,
}

/// Builds a lazy product iterator over `lists` honoring `opts`.
pub fn combinations(lists: &[Vec<String>], opts: ProductOptions) -> Product {
    let lens: Vec<usize> = lists.iter().map(|l| l.len()).collect();
    let k = lens.len();

    let lsd_order: Vec<usize> = if opts.reverse_fields {
        (0..k).collect()
    } else {
        (0..k).rev().collect()
    };

    let mut exhausted = k == 0 || lens.contains(&0);
    let mut digits = if opts.reverse {
        lens.iter().map(|&len| len.saturating_sub(1)).collect()
    } else {
        vec![0usize; k]
    };

    if !exhausted {
        let mut off = opts.offset;
        if opts.reverse {
            // Subtract the offset from the final tuple by mixed-radix
            // decomposition, without iterating through skipped records.
            for &pos in &lsd_order {
                let len = lens[pos] as u128;
                let delta = off % len;
                off /= len;
                if digits[pos] as u128 >= delta {
                    digits[pos] -= delta as usize;
                } else {
                    digits[pos] = (len - delta + digits[pos] as u128) as usize;
                    off = off.saturating_add(1);
                }
            }
        } else {
            // Resolve offset by mixed-radix decomposition (no iteration).
            for &pos in &lsd_order {
                let len = lens[pos] as u128;
                digits[pos] = (off % len) as usize;
                off /= len;
            }
        }
        if off > 0 {
            exhausted = true; // offset past the end of the product
        }
    }

    Product {
        lens,
        digits,
        lsd_order,
        remaining: opts.limit,
        exhausted,
        started: false,
        descending: opts.reverse,
    }
}

impl Iterator for Product {
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>> {
        if self.exhausted || self.remaining == Some(0) {
            return None;
        }

        if !self.started {
            self.started = true;
        } else {
            // Odometer step, least-significant position first.
            let mut carry = true;
            for &pos in &self.lsd_order {
                if self.descending {
                    if self.digits[pos] > 0 {
                        self.digits[pos] -= 1;
                        carry = false;
                        break;
                    }
                    self.digits[pos] = self.lens[pos] - 1;
                } else {
                    self.digits[pos] += 1;
                    if self.digits[pos] < self.lens[pos] {
                        carry = false;
                        break;
                    }
                    self.digits[pos] = 0;
                }
            }
            if carry {
                self.exhausted = true;
                return None;
            }
        }

        if let Some(r) = self.remaining.as_mut() {
            *r -= 1;
        }
        Some(self.digits.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    fn collect(opts: ProductOptions) -> Vec<Vec<usize>> {
        combinations(&lists(), opts).collect()
    }

    #[test]
    fn default_order_rightmost_fastest() {
        assert_eq!(
            collect(ProductOptions::default()),
            vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]]
        );
    }

    #[test]
    fn reverse_order_leftmost_fastest() {
        let opts = ProductOptions {
            reverse_fields: true,
            ..Default::default()
        };
        assert_eq!(
            collect(opts),
            vec![vec![0, 0], vec![1, 0], vec![0, 1], vec![1, 1]]
        );
    }

    #[test]
    fn reverse_order_reverses_complete_product() {
        let opts = ProductOptions {
            reverse: true,
            ..Default::default()
        };
        assert_eq!(
            collect(opts),
            vec![vec![1, 1], vec![1, 0], vec![0, 1], vec![0, 0]]
        );
    }

    #[test]
    fn reverse_offset_and_limit_paginate_from_end() {
        let opts = ProductOptions {
            reverse: true,
            offset: 1,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(opts), vec![vec![1, 0], vec![0, 1]]);
    }

    #[test]
    fn offset_skips_leading_combinations() {
        let opts = ProductOptions {
            offset: 2,
            ..Default::default()
        };
        assert_eq!(collect(opts), vec![vec![1, 0], vec![1, 1]]);
    }

    #[test]
    fn limit_caps_output() {
        let opts = ProductOptions {
            limit: Some(1),
            ..Default::default()
        };
        assert_eq!(collect(opts), vec![vec![0, 0]]);
    }

    #[test]
    fn offset_and_limit_paginate() {
        let opts = ProductOptions {
            offset: 1,
            limit: Some(2),
            ..Default::default()
        };
        assert_eq!(collect(opts), vec![vec![0, 1], vec![1, 0]]);
    }

    #[test]
    fn offset_past_end_yields_nothing() {
        let opts = ProductOptions {
            offset: 99,
            ..Default::default()
        };
        assert!(collect(opts).is_empty());
    }

    #[test]
    fn empty_list_yields_nothing() {
        let lists = vec![vec!["a".to_string()], Vec::<String>::new()];
        assert!(combinations(&lists, ProductOptions::default())
            .next()
            .is_none());
    }

    #[test]
    fn limit_zero_yields_nothing() {
        let opts = ProductOptions {
            limit: Some(0),
            ..Default::default()
        };
        assert!(collect(opts).is_empty());
    }
}
