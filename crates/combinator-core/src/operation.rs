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

/// Fields in one output record, which is what templates and `--name` values
/// address.
///
/// This is not always the input-list count: `concat` emits one item at a time
/// regardless of how many lists it walks, and the single-pool operations draw
/// several fields from their one list.
pub fn field_count(op: &Operation, lists: &[Vec<String>]) -> usize {
    match op {
        Operation::Product(_) | Operation::Zip(_) => lists.len(),
        Operation::Concat(_) => 1,
        // Callers may not have validated the single-list shape yet, so read the
        // pool defensively rather than indexing.
        Operation::Permutations(_) => lists.first().map_or(0, Vec::len),
        Operation::Combinations { choose, .. } => *choose,
        Operation::Variations { length, .. } => *length,
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

    /// Templates and `--name` values address record fields, so this must track
    /// the record shape rather than the input-list count. Only product and zip
    /// happen to agree with the list count.
    #[test]
    fn field_count_tracks_record_shape_not_list_count() {
        let two = lists();
        assert_eq!(
            field_count(&Operation::Product(ProductOptions::default()), &two),
            2
        );
        assert_eq!(field_count(&Operation::Zip(ZipOptions::default()), &two), 2);
        // Concat emits one item at a time however many lists it walks.
        assert_eq!(
            field_count(&Operation::Concat(ConcatOptions::default()), &two),
            1
        );
        // Single-pool operations draw several fields from their one list.
        let pool = vec![vec!["a".to_string(), "b".to_string(), "c".to_string()]];
        assert_eq!(
            field_count(&Operation::Permutations(Default::default()), &pool),
            3
        );
        assert_eq!(
            field_count(
                &Operation::Combinations {
                    choose: 2,
                    options: Default::default()
                },
                &pool
            ),
            2
        );
        assert_eq!(
            field_count(
                &Operation::Variations {
                    length: 2,
                    options: Default::default()
                },
                &pool
            ),
            2
        );
    }

    /// Field validation runs before the single-list shape check, so an
    /// unvalidated permutations request must not panic here.
    #[test]
    fn field_count_of_a_pool_less_permutation_is_zero() {
        assert_eq!(
            field_count(&Operation::Permutations(Default::default()), &[]),
            0
        );
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
