//! Mode-neutral operation dispatch over product/zip/concat.

use crate::concat::{concat_count, ConcatOptions};
use crate::count::{combination_count, Count};
use crate::product::ProductOptions;
use crate::selection::{binomial, factorial, falling_factorial, SelectionOptions};
use crate::zip::{zip_count, ZipLengthMismatch, ZipOptions};
use crate::CoreError;

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
    Zip(ZipOptions),
    Concat(ConcatOptions),
    Permutations(SelectionOptions),
    Combinations {
        choose: usize,
        options: SelectionOptions,
    },
    Variations {
        length: usize,
        options: SelectionOptions,
    },
}

/// Validates operation-specific input shape before counting or generation.
pub fn validate(op: &Operation, lists: &[Vec<String>]) -> Result<(), CoreError> {
    match op {
        Operation::Permutations(_)
        | Operation::Combinations { .. }
        | Operation::Variations { .. }
            if lists.len() != 1 =>
        {
            Err(CoreError::usage(
                "ONE_LIST_REQUIRED",
                "this operation requires exactly one input list",
            ))
        }
        _ => Ok(()),
    }
}

/// Counts combinations for whichever operation is selected.
///
/// Only `Zip` under `UnequalPolicy::Error` can fail (mismatched lengths).
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Result<Count, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => Ok(combination_count(&lens)),
        Operation::Zip(opts) => zip_count(&lens, opts.on_unequal),
        Operation::Concat(_opts) => Ok(concat_count(&lens)),
        Operation::Permutations(_) => one_pool_count(lens, factorial),
        Operation::Combinations { choose, .. } => {
            one_pool_count_with(lens, |n| binomial(n, *choose))
        }
        Operation::Variations { length, .. } => {
            one_pool_count_with(lens, |n| falling_factorial(n, *length))
        }
    }
}

fn one_pool_count<F>(lens: Vec<usize>, count: F) -> Result<Count, ZipLengthMismatch>
where
    F: FnOnce(usize) -> Count,
{
    one_pool_count_with(lens, count)
}
fn one_pool_count_with<F>(lens: Vec<usize>, count: F) -> Result<Count, ZipLengthMismatch>
where
    F: FnOnce(usize) -> Count,
{
    if lens.len() == 1 {
        Ok(count(lens[0]))
    } else {
        Err(ZipLengthMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::UnequalPolicy;

    fn lists() -> Vec<Vec<String>> {
        vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ]
    }

    #[test]
    fn product_dispatches_to_combination_count() {
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(4));
    }

    #[test]
    fn product_any_empty_list_is_zero() {
        let ls = vec![vec!["a".to_string()], Vec::<String>::new()];
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &ls).unwrap(), Count::Exact(0));
    }

    #[test]
    fn zip_dispatches_to_zip_count() {
        let op = Operation::Zip(ZipOptions {
            on_unequal: UnequalPolicy::Error,
            ..Default::default()
        });
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(2));
    }

    #[test]
    fn concat_dispatches_to_concat_count() {
        let op = Operation::Concat(ConcatOptions::default());
        assert_eq!(count(&op, &lists()).unwrap(), Count::Exact(4));
    }

    #[test]
    fn selection_operations_require_one_pool_and_count_checked_values() {
        let pool = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(
            count(
                &Operation::Permutations(Default::default()),
                std::slice::from_ref(&pool)
            )
            .unwrap(),
            Count::Exact(6)
        );
        assert_eq!(
            count(
                &Operation::Combinations {
                    choose: 2,
                    options: Default::default()
                },
                std::slice::from_ref(&pool)
            )
            .unwrap(),
            Count::Exact(3)
        );
        assert_eq!(
            count(
                &Operation::Variations {
                    length: 2,
                    options: Default::default()
                },
                std::slice::from_ref(&pool)
            )
            .unwrap(),
            Count::Exact(6)
        );
        assert!(count(
            &Operation::Permutations(Default::default()),
            &[pool.clone(), pool.clone()]
        )
        .is_err());
        assert_eq!(
            validate(
                &Operation::Permutations(Default::default()),
                &[pool.clone(), pool]
            )
            .unwrap_err()
            .code,
            "ONE_LIST_REQUIRED"
        );
    }
}
