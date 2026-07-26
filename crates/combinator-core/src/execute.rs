//! Bounded, cancellable streaming execution.

use std::io::Write;

use crate::{
    combinations, concat_records, format_record_with, zip_records, Constraint, CoreError, Count,
    Format, Operation, Template,
};

pub struct ExecutionRequest<'a> {
    pub operation: &'a Operation,
    pub lists: &'a [Vec<String>],
    pub format: Format,
    pub field_sep: &'a str,
    pub record_sep: &'a str,
    pub lean: bool,
    pub template: Option<&'a Template>,
    pub names: &'a [String],
    pub max_output_bytes: u64,
    pub max_combinations: u128,
    pub cancel: Option<&'a dyn Fn() -> bool>,
    pub constraints: &'a [Constraint],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionResult {
    pub records: u128,
    pub bytes: u64,
}

pub fn execute<W: Write>(
    request: ExecutionRequest<'_>,
    writer: &mut W,
) -> Result<ExecutionResult, CoreError> {
    crate::operation::validate(request.operation, request.lists)?;
    for constraint in request.constraints {
        constraint.validate()?;
    }
    let count = crate::operation::count(request.operation, request.lists).map_err(|_| {
        CoreError::runtime("ZIP_LENGTH_MISMATCH", "zip inputs have unequal lengths")
    })?;
    let requested = match count {
        Count::Exact(total) => total
            .saturating_sub(offset(request.operation))
            .min(limit(request.operation).unwrap_or(u128::MAX)),
        Count::Overflow => limit(request.operation).ok_or_else(|| {
            CoreError::runtime(
                "COMBINATION_LIMIT_EXCEEDED",
                "the operation is too large without an explicit safe limit",
            )
        })?,
    };
    if requested > request.max_combinations {
        return Err(CoreError::runtime(
            "COMBINATION_LIMIT_EXCEEDED",
            "requested combinations exceed the configured generation limit",
        ));
    }

    if !request.constraints.is_empty() {
        match count {
            Count::Exact(total) if total > request.max_combinations => {
                return Err(CoreError::runtime(
                    "COMBINATION_LIMIT_EXCEEDED",
                    "filtered generation exceeds the configured scan limit",
                ));
            }
            Count::Overflow => {
                return Err(CoreError::runtime(
                    "COMBINATION_LIMIT_EXCEEDED",
                    "filtered generation count overflowed before evaluation",
                ));
            }
            _ => {}
        }
    }

    let generation_operation = if request.constraints.is_empty() {
        request.operation.clone()
    } else {
        unpaged(request.operation)
    };
    let mut filter_window = FilterWindow::new(request.operation, request.constraints);

    let mut result = ExecutionResult {
        records: 0,
        bytes: 0,
    };
    match &generation_operation {
        Operation::Product(options) => {
            for indices in combinations(request.lists, options.clone()) {
                check_cancel(&request)?;
                let items: Vec<&str> = indices
                    .iter()
                    .enumerate()
                    .map(|(list, item)| request.lists[list][*item].as_str())
                    .collect();
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
        Operation::Zip(options) => {
            for indices in zip_records(request.lists, options.clone()).map_err(|_| {
                CoreError::runtime("ZIP_LENGTH_MISMATCH", "zip inputs have unequal lengths")
            })? {
                check_cancel(&request)?;
                let items: Vec<&str> = indices
                    .iter()
                    .enumerate()
                    .map(|(list, item)| request.lists[list][*item].as_str())
                    .collect();
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
        Operation::Concat(options) => {
            for (list, item) in concat_records(request.lists, options.clone()).ok_or_else(|| {
                CoreError::runtime("COUNT_OVERFLOW", "concatenated item count overflowed")
            })? {
                check_cancel(&request)?;
                let items = vec![request.lists[list][item].as_str()];
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
        Operation::Permutations(options) => {
            let list = one_list(&request)?;
            for indices in crate::selection::permutations(list.len(), *options)? {
                check_cancel(&request)?;
                let items: Vec<&str> = indices.iter().map(|i| list[*i].as_str()).collect();
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
        Operation::Combinations { choose, options } => {
            let list = one_list(&request)?;
            for indices in crate::selection::combinations(list.len(), *choose, *options)? {
                check_cancel(&request)?;
                let items: Vec<&str> = indices.iter().map(|i| list[*i].as_str()).collect();
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
        Operation::Variations { length, options } => {
            let list = one_list(&request)?;
            for indices in crate::selection::variations(list.len(), *length, *options)? {
                check_cancel(&request)?;
                let items: Vec<&str> = indices.iter().map(|i| list[*i].as_str()).collect();
                match filter_window.decide(&items)? {
                    FilterDecision::Emit => emit(&request, &items, &mut result, writer)?,
                    FilterDecision::Skip => {}
                    FilterDecision::Done => break,
                }
            }
        }
    }
    Ok(result)
}

fn check_cancel(request: &ExecutionRequest<'_>) -> Result<(), CoreError> {
    if request.cancel.is_some_and(|cancel| cancel()) {
        Err(CoreError::runtime("CANCELLED", "execution was cancelled"))
    } else {
        Ok(())
    }
}

fn one_list<'a>(request: &'a ExecutionRequest<'a>) -> Result<&'a Vec<String>, CoreError> {
    request
        .lists
        .first()
        .filter(|_| request.lists.len() == 1)
        .ok_or_else(|| {
            CoreError::usage(
                "ONE_LIST_REQUIRED",
                "this operation requires exactly one input list",
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilterDecision {
    Skip,
    Emit,
    Done,
}

struct FilterWindow<'a> {
    constraints: &'a [Constraint],
    offset: u128,
    limit: Option<u128>,
    matched: u128,
    emitted: u128,
}

impl<'a> FilterWindow<'a> {
    fn new(operation: &Operation, constraints: &'a [Constraint]) -> Self {
        Self {
            constraints,
            offset: offset(operation),
            limit: limit(operation),
            matched: 0,
            emitted: 0,
        }
    }

    fn decide(&mut self, items: &[&str]) -> Result<FilterDecision, CoreError> {
        if self.constraints.is_empty() {
            return Ok(FilterDecision::Emit);
        }
        for constraint in self.constraints {
            if !constraint.matches(items)? {
                return Ok(FilterDecision::Skip);
            }
        }
        self.matched = self.matched.checked_add(1).ok_or_else(|| {
            CoreError::runtime("COUNT_OVERFLOW", "filtered match count overflowed")
        })?;
        if self.matched <= self.offset {
            return Ok(FilterDecision::Skip);
        }
        if self.limit.is_some_and(|limit| self.emitted >= limit) {
            return Ok(FilterDecision::Done);
        }
        self.emitted = self.emitted.checked_add(1).ok_or_else(|| {
            CoreError::runtime("COUNT_OVERFLOW", "filtered output count overflowed")
        })?;
        Ok(FilterDecision::Emit)
    }
}

fn unpaged(operation: &Operation) -> Operation {
    match operation {
        Operation::Product(options) => Operation::Product(crate::ProductOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Zip(options) => Operation::Zip(crate::ZipOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Concat(options) => Operation::Concat(crate::ConcatOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Permutations(options) => Operation::Permutations(crate::SelectionOptions {
            offset: 0,
            limit: None,
            ..*options
        }),
        Operation::Combinations { choose, options } => Operation::Combinations {
            choose: *choose,
            options: crate::SelectionOptions {
                offset: 0,
                limit: None,
                ..*options
            },
        },
        Operation::Variations { length, options } => Operation::Variations {
            length: *length,
            options: crate::SelectionOptions {
                offset: 0,
                limit: None,
                ..*options
            },
        },
    }
}

fn emit<W: Write>(
    request: &ExecutionRequest<'_>,
    items: &[&str],
    result: &mut ExecutionResult,
    writer: &mut W,
) -> Result<(), CoreError> {
    let index = offset(request.operation).saturating_add(result.records);
    let line = format_record_with(
        items,
        index,
        request.field_sep,
        request.record_sep,
        request.format,
        request.lean,
        request.template,
        request.names,
    )
    .map_err(|_| CoreError::runtime("TEMPLATE_INVALID", "template rendering failed"))?;
    let size = u64::try_from(line.len()).map_err(|_| {
        CoreError::runtime(
            "OUTPUT_LIMIT_EXCEEDED",
            "output record is too large to write",
        )
    })?;
    let next = result.bytes.checked_add(size).ok_or_else(|| {
        CoreError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
    })?;
    if next > request.max_output_bytes {
        return Err(CoreError::runtime(
            "OUTPUT_LIMIT_EXCEEDED",
            "output exceeds the configured byte limit",
        )
        .with("written_bytes", result.bytes)
        .with("record_bytes", size)
        .with("limit_bytes", request.max_output_bytes));
    }
    writer.write_all(line.as_bytes()).map_err(CoreError::from)?;
    result.records += 1;
    result.bytes = next;
    Ok(())
}

fn offset(operation: &Operation) -> u128 {
    match operation {
        Operation::Product(options) => options.offset,
        Operation::Zip(options) => options.offset,
        Operation::Concat(options) => options.offset,
        Operation::Permutations(options) => options.offset,
        Operation::Combinations { options, .. } | Operation::Variations { options, .. } => {
            options.offset
        }
    }
}

fn limit(operation: &Operation) -> Option<u128> {
    match operation {
        Operation::Product(options) => options.limit,
        Operation::Zip(options) => options.limit,
        Operation::Concat(options) => options.limit,
        Operation::Permutations(options) => options.limit,
        Operation::Combinations { options, .. } | Operation::Variations { options, .. } => {
            options.limit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request<'a>(operation: &'a Operation, lists: &'a [Vec<String>]) -> ExecutionRequest<'a> {
        ExecutionRequest {
            operation,
            lists,
            format: Format::Text,
            field_sep: ",",
            record_sep: "\n",
            lean: false,
            template: None,
            names: &[],
            max_output_bytes: 1024,
            max_combinations: 100,
            cancel: None,
            constraints: &[],
        }
    }

    #[test]
    fn executes_product_into_generic_writer() {
        let lists = vec![vec!["a".into(), "b".into()], vec!["x".into()]];
        let operation = Operation::Product(Default::default());
        let mut output = Vec::new();
        let result = execute(request(&operation, &lists), &mut output).unwrap();
        assert_eq!(result.records, 2);
        assert_eq!(String::from_utf8(output).unwrap(), "a,x\nb,x\n");
    }

    #[test]
    fn cancellation_is_checked_before_each_record() {
        let lists = vec![vec!["a".into(), "b".into()]];
        let operation = Operation::Product(Default::default());
        let cancel = || true;
        let mut request = request(&operation, &lists);
        request.cancel = Some(&cancel);
        let error = execute(request, &mut Vec::new()).unwrap_err();
        assert_eq!(error.code, "CANCELLED");
    }

    #[test]
    #[allow(clippy::io_other_error)]
    fn writer_errors_are_reported() {
        struct FailingWriter;
        impl Write for FailingWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(std::io::ErrorKind::Other, "no"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let lists = vec![vec!["a".into()]];
        let operation = Operation::Product(Default::default());
        let error = execute(request(&operation, &lists), &mut FailingWriter).unwrap_err();
        assert_eq!(error.code, "WRITE_FAILED");
    }

    #[test]
    fn output_limit_allows_exact_boundary_and_rejects_one_byte_over() {
        let lists = vec![vec!["a".into()]];
        let operation = Operation::Product(Default::default());
        let mut exact = request(&operation, &lists);
        exact.max_output_bytes = 2; // "a\n"
        let result = execute(exact, &mut Vec::new()).unwrap();
        assert_eq!(result.bytes, 2);

        let mut over = request(&operation, &lists);
        over.max_output_bytes = 1;
        assert_eq!(
            execute(over, &mut Vec::new()).unwrap_err().code,
            "OUTPUT_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn zip_mismatch_and_overflow_without_limit_fail_closed() {
        let lists = vec![vec!["a".into()], vec!["b".into(), "c".into()]];
        let operation = Operation::Zip(Default::default());
        assert_eq!(
            execute(request(&operation, &lists), &mut Vec::new())
                .unwrap_err()
                .code,
            "ZIP_LENGTH_MISMATCH"
        );

        let lists = vec![vec!["a".into(), "b".into()]; 129];
        let operation = Operation::Product(Default::default());
        let mut request = request(&operation, &lists);
        request.max_combinations = 100;
        assert_eq!(
            execute(request, &mut Vec::new()).unwrap_err().code,
            "COMBINATION_LIMIT_EXCEEDED"
        );
    }

    #[test]
    fn executes_variations_and_applies_typed_constraints_lazily() {
        let lists = vec![vec!["a".into(), "b".into(), "c".into()]];
        let operation = Operation::Variations {
            length: 2,
            options: Default::default(),
        };
        let constraint = Constraint::Prefix {
            field: 0,
            value: "a".into(),
        };
        let mut request = request(&operation, &lists);
        request.constraints = std::slice::from_ref(&constraint);
        let mut output = Vec::new();
        let result = execute(request, &mut output).unwrap();
        assert_eq!(result.records, 2);
        assert_eq!(String::from_utf8(output).unwrap(), "a,b\na,c\n");
    }

    #[test]
    fn filtered_offset_and_limit_apply_to_accepted_records() {
        let lists = vec![vec!["a".into(), "b".into(), "c".into()]];
        let operation = Operation::Variations {
            length: 2,
            options: crate::SelectionOptions {
                offset: 1,
                limit: Some(1),
                ..Default::default()
            },
        };
        let constraint = Constraint::Prefix {
            field: 0,
            value: "a".into(),
        };
        let mut request = request(&operation, &lists);
        request.constraints = std::slice::from_ref(&constraint);
        let mut output = Vec::new();
        execute(request, &mut output).unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "a,c\n");
    }

    #[test]
    fn combinations_k_zero_emits_one_empty_record() {
        let lists = vec![vec!["a".into(), "b".into()]];
        let operation = Operation::Combinations {
            choose: 0,
            options: Default::default(),
        };
        let mut output = Vec::new();
        let result = execute(request(&operation, &lists), &mut output).unwrap();
        assert_eq!(result.records, 1);
        assert_eq!(output, b"\n");
    }
}
