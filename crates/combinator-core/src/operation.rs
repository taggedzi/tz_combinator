//! Mode-neutral operation dispatch over product/zip/concat.

use crate::count::{combination_count, Count};
use crate::product::ProductOptions;

/// The operation an invocation selects, carrying that mode's options.
#[derive(Debug, Clone)]
pub enum Operation {
    Product(ProductOptions),
}

/// Counts combinations for whichever operation is selected.
pub fn count(op: &Operation, lists: &[Vec<String>]) -> Count {
    let lens: Vec<usize> = lists.iter().map(Vec::len).collect();
    match op {
        Operation::Product(_opts) => combination_count(&lens),
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

    #[test]
    fn product_dispatches_to_combination_count() {
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &lists()), Count::Exact(4));
    }

    #[test]
    fn product_any_empty_list_is_zero() {
        let ls = vec![vec!["a".to_string()], Vec::<String>::new()];
        let op = Operation::Product(ProductOptions::default());
        assert_eq!(count(&op, &ls), Count::Exact(0));
    }
}
