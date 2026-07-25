//! Overflow-safe combination counting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Exact(u128),
    Overflow,
}

/// Counts the ordered Cartesian product of lists with the given lengths.
///
/// Returns `Exact(0)` if any list is empty, `Exact(1)` for no lists at all
/// (the empty product), and `Overflow` if the true count exceeds `u128`.
pub fn combination_count(list_lens: &[usize]) -> Count {
    let mut acc: u128 = 1;
    for &n in list_lens {
        if n == 0 {
            return Count::Exact(0);
        }
        match acc.checked_mul(n as u128) {
            Some(v) => acc = v,
            None => return Count::Overflow,
        }
    }
    Count::Exact(acc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_of_lengths() {
        assert_eq!(combination_count(&[2, 2]), Count::Exact(4));
        assert_eq!(combination_count(&[3, 4, 5]), Count::Exact(60));
    }

    #[test]
    fn single_list_is_its_length() {
        assert_eq!(combination_count(&[7]), Count::Exact(7));
    }

    #[test]
    fn any_empty_list_is_zero() {
        assert_eq!(combination_count(&[2, 0, 3]), Count::Exact(0));
    }

    #[test]
    fn empty_slice_is_one() {
        assert_eq!(combination_count(&[]), Count::Exact(1));
    }

    #[test]
    fn overflow_reports_overflow() {
        // 20 lists of u32::MAX length overflows u128.
        let lens = vec![u32::MAX as usize; 20];
        assert_eq!(combination_count(&lens), Count::Overflow);
    }
}
