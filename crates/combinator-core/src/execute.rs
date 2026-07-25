//! Bounded, cancellable streaming execution.

use std::io::Write;

use crate::{
    combinations, concat_records, format_record_with, zip_records, CoreError, Count, Format,
    Operation, Template,
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

    let mut result = ExecutionResult {
        records: 0,
        bytes: 0,
    };
    match request.operation {
        Operation::Product(options) => {
            for indices in combinations(request.lists, options.clone()) {
                check_cancel(&request)?;
                let items: Vec<&str> = indices
                    .iter()
                    .enumerate()
                    .map(|(list, item)| request.lists[list][*item].as_str())
                    .collect();
                emit(&request, &items, &mut result, writer)?;
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
                emit(&request, &items, &mut result, writer)?;
            }
        }
        Operation::Concat(options) => {
            for (list, item) in concat_records(request.lists, options.clone()).ok_or_else(|| {
                CoreError::runtime("COUNT_OVERFLOW", "concatenated item count overflowed")
            })? {
                check_cancel(&request)?;
                let items = vec![request.lists[list][item].as_str()];
                emit(&request, &items, &mut result, writer)?;
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
    }
}

fn limit(operation: &Operation) -> Option<u128> {
    match operation {
        Operation::Product(options) => options.limit,
        Operation::Zip(options) => options.limit,
        Operation::Concat(options) => options.limit,
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
}
