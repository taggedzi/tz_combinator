//! Format-neutral logical-record generation.
//!
//! This module deliberately knows nothing about bytes, encodings, writers,
//! paths, terminals, or command-line syntax. Consumers choose how to encode
//! the emitted field indices.

use crate::constraint::ConstraintMatcher;
use crate::{
    combinations, concat_records, zip_records, ConcatOptions, Constraint, CoreError, Count,
    Operation, ProductOptions, SelectionOptions, ZipOptions,
};

/// A selected item is identified by its input-list and item position.
pub type FieldIndex = (usize, usize);

/// A logical record emitted by an operation before presentation encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalRecord {
    /// The stable ordinal in the accepted output page.
    pub ordinal: u128,
    /// Input positions making up this record, in output field order.
    pub fields: Vec<FieldIndex>,
}

/// Resource controls enforced during logical generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationLimits {
    pub max_combinations: u128,
}

/// A request containing only validated domain values.
pub struct GenerationRequest<'a> {
    pub operation: &'a Operation,
    pub lists: &'a [Vec<String>],
    pub constraints: &'a [Constraint],
    pub limits: GenerationLimits,
    pub cancel: Option<&'a dyn Fn() -> bool>,
}

/// Summary of records delivered to the sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationReport {
    pub records: u128,
}

/// Consumer interface for logical records.
pub trait RecordSink {
    fn record(&mut self, record: LogicalRecord) -> Result<(), CoreError>;
}

/// Adapts a closure into a logical-record sink.
pub fn generate_with<F>(
    request: GenerationRequest<'_>,
    mut sink: F,
) -> Result<GenerationReport, CoreError>
where
    F: FnMut(LogicalRecord) -> Result<(), CoreError>,
{
    generate(request, &mut ClosureSink { sink: &mut sink })
}

struct ClosureSink<'a, F> {
    sink: &'a mut F,
}

impl<F> RecordSink for ClosureSink<'_, F>
where
    F: FnMut(LogicalRecord) -> Result<(), CoreError>,
{
    fn record(&mut self, record: LogicalRecord) -> Result<(), CoreError> {
        (self.sink)(record)
    }
}

/// Generates logical records lazily into `sink`.
pub fn generate<S: RecordSink>(
    request: GenerationRequest<'_>,
    sink: &mut S,
) -> Result<GenerationReport, CoreError> {
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
    if requested > request.limits.max_combinations {
        return Err(CoreError::runtime(
            "COMBINATION_LIMIT_EXCEEDED",
            "requested combinations exceed the configured generation limit",
        ));
    }
    if !request.constraints.is_empty() {
        match count {
            Count::Exact(total) if total > request.limits.max_combinations => {
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
    let mut window = FilterWindow::new(request.operation, request.constraints);
    let mut report = GenerationReport { records: 0 };

    match &generation_operation {
        Operation::Product(options) => {
            for indices in combinations(request.lists, options.clone()) {
                check_cancel(&request)?;
                let fields = indices.into_iter().enumerate().collect();
                deliver(&request, &mut window, &mut report, fields, sink)?;
                if window.done() {
                    break;
                }
            }
        }
        Operation::Zip(options) => {
            for indices in zip_records(request.lists, options.clone()).map_err(|_| {
                CoreError::runtime("ZIP_LENGTH_MISMATCH", "zip inputs have unequal lengths")
            })? {
                check_cancel(&request)?;
                let fields = indices.into_iter().enumerate().collect();
                deliver(&request, &mut window, &mut report, fields, sink)?;
                if window.done() {
                    break;
                }
            }
        }
        Operation::Concat(options) => {
            for (list, item) in concat_records(request.lists, options.clone()).ok_or_else(|| {
                CoreError::runtime("COUNT_OVERFLOW", "concatenated item count overflowed")
            })? {
                check_cancel(&request)?;
                deliver(&request, &mut window, &mut report, vec![(list, item)], sink)?;
                if window.done() {
                    break;
                }
            }
        }
        Operation::Permutations(options) => {
            for indices in crate::selection::permutations(request.lists[0].len(), *options)? {
                check_cancel(&request)?;
                let fields = indices.into_iter().map(|item| (0, item)).collect();
                deliver(&request, &mut window, &mut report, fields, sink)?;
                if window.done() {
                    break;
                }
            }
        }
        Operation::Combinations { choose, options } => {
            for indices in
                crate::selection::combinations(request.lists[0].len(), *choose, *options)?
            {
                check_cancel(&request)?;
                let fields = indices.into_iter().map(|item| (0, item)).collect();
                deliver(&request, &mut window, &mut report, fields, sink)?;
                if window.done() {
                    break;
                }
            }
        }
        Operation::Variations { length, options } => {
            for indices in crate::selection::variations(request.lists[0].len(), *length, *options)?
            {
                check_cancel(&request)?;
                let fields = indices.into_iter().map(|item| (0, item)).collect();
                deliver(&request, &mut window, &mut report, fields, sink)?;
                if window.done() {
                    break;
                }
            }
        }
    }
    Ok(report)
}

fn deliver<S: RecordSink>(
    request: &GenerationRequest<'_>,
    window: &mut FilterWindow<'_>,
    report: &mut GenerationReport,
    fields: Vec<FieldIndex>,
    sink: &mut S,
) -> Result<(), CoreError> {
    match window.decide(&fields, request.lists, request.cancel)? {
        FilterDecision::Emit => {
            let ordinal = offset(request.operation).saturating_add(report.records);
            sink.record(LogicalRecord { ordinal, fields })?;
            report.records = report.records.checked_add(1).ok_or_else(|| {
                CoreError::runtime("COUNT_OVERFLOW", "generated record count overflowed")
            })?;
        }
        FilterDecision::Skip | FilterDecision::Done => {}
    }
    Ok(())
}

fn check_cancel(request: &GenerationRequest<'_>) -> Result<(), CoreError> {
    if request.cancel.is_some_and(|cancel| cancel()) {
        Err(CoreError::runtime("CANCELLED", "execution was cancelled"))
    } else {
        Ok(())
    }
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
    finished: bool,
}

impl<'a> FilterWindow<'a> {
    fn new(operation: &Operation, constraints: &'a [Constraint]) -> Self {
        Self {
            constraints,
            offset: offset(operation),
            limit: limit(operation),
            matched: 0,
            emitted: 0,
            finished: false,
        }
    }

    fn decide(
        &mut self,
        fields: &[FieldIndex],
        lists: &[Vec<String>],
        cancel: Option<&dyn Fn() -> bool>,
    ) -> Result<FilterDecision, CoreError> {
        if self.finished {
            return Ok(FilterDecision::Done);
        }
        if self.constraints.is_empty() {
            return Ok(FilterDecision::Emit);
        }
        let values: Vec<&str> = fields
            .iter()
            .map(|(list, item)| lists[*list][*item].as_str())
            .collect();
        let mut matcher = ConstraintMatcher::new(cancel);
        for constraint in self.constraints {
            if !matcher.matches(constraint, &values)? {
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
            self.finished = true;
            return Ok(FilterDecision::Done);
        }
        self.emitted = self.emitted.checked_add(1).ok_or_else(|| {
            CoreError::runtime("COUNT_OVERFLOW", "filtered output count overflowed")
        })?;
        Ok(FilterDecision::Emit)
    }

    fn done(&self) -> bool {
        self.finished
    }
}

fn unpaged(operation: &Operation) -> Operation {
    match operation {
        Operation::Product(options) => Operation::Product(ProductOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Zip(options) => Operation::Zip(ZipOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Concat(options) => Operation::Concat(ConcatOptions {
            offset: 0,
            limit: None,
            ..options.clone()
        }),
        Operation::Permutations(options) => Operation::Permutations(SelectionOptions {
            offset: 0,
            limit: None,
            ..*options
        }),
        Operation::Combinations { choose, options } => Operation::Combinations {
            choose: *choose,
            options: SelectionOptions {
                offset: 0,
                limit: None,
                ..*options
            },
        },
        Operation::Variations { length, options } => Operation::Variations {
            length: *length,
            options: SelectionOptions {
                offset: 0,
                limit: None,
                ..*options
            },
        },
    }
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
    use std::cell::Cell;

    use super::*;

    fn request<'a>(operation: &'a Operation, lists: &'a [Vec<String>]) -> GenerationRequest<'a> {
        GenerationRequest {
            operation,
            lists,
            constraints: &[],
            limits: GenerationLimits {
                max_combinations: 100,
            },
            cancel: None,
        }
    }

    #[test]
    fn emits_format_neutral_product_indices() {
        let lists = vec![vec!["a".into(), "b".into()], vec!["x".into()]];
        let operation = Operation::Product(Default::default());
        let mut records = Vec::new();
        let report = generate_with(request(&operation, &lists), |record| {
            records.push(record);
            Ok(())
        })
        .unwrap();
        assert_eq!(report.records, 2);
        assert_eq!(records[0].ordinal, 0);
        assert_eq!(records[0].fields, vec![(0, 0), (1, 0)]);
        assert_eq!(records[1].fields, vec![(0, 1), (1, 0)]);
    }

    #[test]
    fn sink_errors_and_cancellation_are_interface_neutral() {
        let lists = vec![vec!["a".into()]];
        let operation = Operation::Product(Default::default());
        let error = generate_with(request(&operation, &lists), |_| {
            Err(CoreError::runtime("SINK_FAILED", "sink rejected record"))
        })
        .unwrap_err();
        assert_eq!(error.code, "SINK_FAILED");

        let cancel = || true;
        let mut request = request(&operation, &lists);
        request.cancel = Some(&cancel);
        assert_eq!(
            generate_with(request, |_| Ok(())).unwrap_err().code,
            "CANCELLED"
        );
    }

    #[test]
    fn cancellation_interrupts_the_first_glob_match() {
        let lists = vec![vec!["a".repeat(8 * 1024)]];
        let operation = Operation::Product(Default::default());
        let constraints = [Constraint::Glob {
            field: 0,
            pattern: format!("*{}b*", "a".repeat(1024)),
        }];
        let polls = Cell::new(0usize);
        let cancel = || {
            let next = polls.get() + 1;
            polls.set(next);
            next >= 3
        };
        let mut request = request(&operation, &lists);
        request.constraints = &constraints;
        request.cancel = Some(&cancel);
        let emitted = Cell::new(false);

        let error = generate_with(request, |_| {
            emitted.set(true);
            Ok(())
        })
        .unwrap_err();

        assert_eq!(error.code, "CANCELLED");
        assert!(!emitted.get());
        assert!(polls.get() >= 3);
    }
}
