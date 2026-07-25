//! Mode-neutral operation dispatch over product/zip/concat.

use crate::count::{combination_count, Count};
use crate::product::ProductOptions;
use crate::zip::{zip_count, ZipLengthMismatch, ZipOptions};

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
    Zip(ZipOptions),
}

/// Counts combinations for whichever operation is selected.
///
/// Only `Zip` under `UnequalPolicy::Error` can fail (mismatched lengths).
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Result<Count, ZipLengthMismatch> {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => Ok(combination_count(&lens)),
        Operation::Zip(opts) => zip_count(&lens, opts.on_unequal),
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
}
