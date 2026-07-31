use crate::{AppError, InputLimits, OutputRecord, OutputSink, ProgressEvent};
use combinator_codecs::InputBudget;
use combinator_core::{join_count_with_fanout, join_each_with_fanout, CoreError, JoinType, Record};
use std::io::Read;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinFormat {
    Csv,
    Tsv,
    Jsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Full,
    Anti,
}

#[derive(Debug, Clone)]
pub struct JoinRequest {
    pub left_path: String,
    pub right_path: String,
    pub left_key: String,
    pub right_key: String,
    pub format: JoinFormat,
    pub kind: JoinKind,
    pub offset: u128,
    pub limit: Option<u128>,
    pub max_join_records: usize,
    pub max_join_key_fanout: u128,
    pub max_output_bytes: u128,
    pub max_input_bytes: usize,
    pub max_item_bytes: usize,
    pub timeout_ms: Option<u64>,
}

impl Default for JoinRequest {
    fn default() -> Self {
        Self {
            left_path: String::new(),
            right_path: String::new(),
            left_key: String::new(),
            right_key: String::new(),
            format: JoinFormat::Csv,
            kind: JoinKind::Inner,
            offset: 0,
            limit: None,
            max_join_records: 100_000,
            max_join_key_fanout: 10_000,
            max_output_bytes: 1_073_741_824,
            max_input_bytes: 64 * 1024 * 1024,
            max_item_bytes: 1_048_576,
            timeout_ms: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinPlan {
    pub left_records: usize,
    pub right_records: usize,
    pub total_records: u128,
    pub records_to_emit: u128,
}

pub fn join_plan(request: &JoinRequest) -> Result<JoinPlan, AppError> {
    validate_request(request)?;
    let (left, right) = load_inputs(request)?;
    let total = join_count_with_fanout(
        &left,
        &right,
        &request.left_key,
        &request.right_key,
        request.kind.into(),
        request.max_join_records as u128,
        request.max_join_key_fanout,
    )
    .map_err(AppError::from)?;
    Ok(JoinPlan {
        left_records: left.len(),
        right_records: right.len(),
        total_records: total,
        records_to_emit: total
            .saturating_sub(request.offset)
            .min(request.limit.unwrap_or(u128::MAX)),
    })
}

pub fn join_preview(
    request: &JoinRequest,
    preview_limit: u128,
) -> Result<Vec<OutputRecord>, AppError> {
    let mut request = request.clone();
    request.offset = 0;
    request.limit = Some(preview_limit);
    let mut records = Vec::new();
    join_stream(
        &request,
        &mut VecSink {
            records: &mut records,
        },
        None,
    )?;
    Ok(records)
}

pub fn join_stream<S: OutputSink>(
    request: &JoinRequest,
    sink: &mut S,
    cancel: Option<&dyn Fn() -> bool>,
) -> Result<ProgressEvent, AppError> {
    validate_request(request)?;
    // A zero-page request is a validated no-op. Avoid opening or parsing
    // either source so preview/count callers do not incur join residency for
    // a page that cannot emit a record.
    if request.limit == Some(0) {
        return Ok(ProgressEvent {
            records: 0,
            bytes: 0,
        });
    }
    let (left, right) = load_inputs(request)?;
    let started = std::time::Instant::now();
    let timeout = request.timeout_ms;
    let cancelled = || {
        cancel.is_some_and(|cancel| cancel())
            || timeout.is_some_and(|ms| started.elapsed().as_millis() >= u128::from(ms))
    };
    let mut progress = ProgressEvent {
        records: 0,
        bytes: 0,
    };
    join_each_with_fanout(
        &left,
        &right,
        &request.left_key,
        &request.right_key,
        request.kind.into(),
        request.offset,
        request.limit,
        request.max_join_records as u128,
        request.max_join_key_fanout,
        Some(&cancelled),
        |record| {
            let object = record
                .fields
                .iter()
                .map(|(key, value)| (key, value))
                .collect::<std::collections::BTreeMap<_, _>>();
            let value = serde_json::to_string(&object)
                .map_err(|error| CoreError::runtime("JOIN_OUTPUT_INVALID", error.to_string()))?
                + "\n";
            progress.bytes = progress
                .bytes
                .checked_add(value.len() as u128)
                .ok_or_else(|| {
                    CoreError::runtime("OUTPUT_LIMIT_EXCEEDED", "output byte count overflowed")
                })?;
            if progress.bytes > request.max_output_bytes {
                return Err(CoreError::runtime(
                    "OUTPUT_LIMIT_EXCEEDED",
                    "output exceeds the configured byte limit",
                ));
            }
            let fields = record
                .fields
                .into_iter()
                .map(|(key, value)| format!("{key}={}", value.unwrap_or_default()))
                .collect();
            sink.record(OutputRecord {
                ordinal: progress.records,
                value,
                fields,
            })
            .map_err(|error| CoreError::runtime(error.code, error.message))?;
            progress.records += 1;
            sink.progress(progress)
                .map_err(|error| CoreError::runtime(error.code, error.message))?;
            Ok(())
        },
    )
    .map_err(AppError::from)?;
    Ok(progress)
}

struct VecSink<'a> {
    records: &'a mut Vec<OutputRecord>,
}

impl OutputSink for VecSink<'_> {
    fn record(&mut self, record: OutputRecord) -> Result<(), AppError> {
        self.records.push(record);
        Ok(())
    }
}

fn validate_request(request: &JoinRequest) -> Result<(), AppError> {
    if request.left_path.is_empty() || request.right_path.is_empty() {
        return Err(AppError::usage(
            "JOIN_SOURCE_INVALID",
            "both join input paths are required",
        ));
    }
    if request.left_key.is_empty() || request.right_key.is_empty() {
        return Err(AppError::usage(
            "JOIN_KEY_INVALID",
            "join keys must not be empty",
        ));
    }
    if request.left_path == "-" && request.right_path == "-" {
        return Err(AppError::usage(
            "DUPLICATE_STDIN",
            "stdin may be used for only one join input",
        ));
    }
    if request.max_join_records == 0 || request.max_join_key_fanout == 0 {
        return Err(AppError::usage(
            "JOIN_LIMIT_INVALID",
            "join limits must be positive",
        ));
    }
    Ok(())
}

fn load_inputs(request: &JoinRequest) -> Result<(Vec<Record>, Vec<Record>), AppError> {
    let limits = InputLimits {
        max_input_bytes: request.max_input_bytes,
        max_item_bytes: request.max_item_bytes,
        max_items_per_list: request.max_join_records,
    };
    let mut budget = InputBudget::new(
        request.max_input_bytes.saturating_mul(2),
        request.max_join_records.saturating_mul(2),
    );
    let left = read_records(&request.left_path, request.format, limits, &mut budget)?;
    let right = read_records(&request.right_path, request.format, limits, &mut budget)?;
    Ok((left, right))
}

fn read_records(
    path: &str,
    format: JoinFormat,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<Record>, AppError> {
    let mut bytes = Vec::new();
    let reader: Box<dyn Read> = if path == "-" {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path).map_err(|error| {
            AppError::runtime(
                "FILE_UNREADABLE",
                format!("could not read join input: {error}"),
            )
        })?)
    };
    reader
        .take((limits.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::runtime("FILE_UNREADABLE", error.to_string()))?;
    if bytes.len() > limits.max_input_bytes {
        return Err(AppError::runtime(
            "INPUT_TOO_LARGE",
            "join input exceeds the byte limit",
        ));
    }
    budget
        .consume_bytes(bytes.len(), path)
        .map_err(|error| AppError {
            code: error.code,
            message: error.message,
        })?;
    match format {
        JoinFormat::Jsonl => parse_jsonl(&bytes, path, limits, budget),
        JoinFormat::Csv => parse_csv(&bytes, path, b',', limits, budget),
        JoinFormat::Tsv => parse_csv(&bytes, path, b'\t', limits, budget),
    }
}

fn parse_jsonl(
    bytes: &[u8],
    path: &str,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<Record>, AppError> {
    let mut records = Vec::new();
    for (line_no, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(line).map_err(|error| {
            AppError::usage("JSONL_MALFORMED", format!("line {}: {error}", line_no + 1))
        })?;
        let object = value.as_object().ok_or_else(|| {
            AppError::usage("JOIN_RECORD_INVALID", "join JSONL records must be objects")
        })?;
        let fields = object
            .iter()
            .map(|(key, value)| {
                let value = value.as_str().ok_or_else(|| {
                    AppError::usage("JOIN_FIELD_INVALID", "join fields must be JSON strings")
                })?;
                if key.len() > limits.max_item_bytes || value.len() > limits.max_item_bytes {
                    return Err(AppError::runtime(
                        "ITEM_TOO_LARGE",
                        "join field exceeds the item limit",
                    ));
                }
                Ok((key.clone(), value.to_string()))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        budget.consume_item(path).map_err(|error| AppError {
            code: error.code,
            message: error.message,
        })?;
        records.push(Record { fields });
    }
    Ok(records)
}

fn parse_csv(
    bytes: &[u8],
    path: &str,
    delimiter: u8,
    limits: InputLimits,
    budget: &mut InputBudget,
) -> Result<Vec<Record>, AppError> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|error| AppError::usage("CSV_MALFORMED", error.to_string()))?
        .clone();
    if headers.is_empty() || headers.iter().any(str::is_empty) {
        return Err(AppError::usage(
            "JOIN_SCHEMA_INVALID",
            "join headers must be non-empty",
        ));
    }
    let mut records = Vec::new();
    for result in reader.records() {
        let row = result.map_err(|error| AppError::usage("CSV_MALFORMED", error.to_string()))?;
        if row.len() != headers.len() {
            return Err(AppError::usage(
                "JOIN_SCHEMA_INVALID",
                "join row does not match the header",
            ));
        }
        let fields = headers
            .iter()
            .zip(row.iter())
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<Vec<_>>();
        if fields.iter().any(|(key, value)| {
            key.len() > limits.max_item_bytes || value.len() > limits.max_item_bytes
        }) {
            return Err(AppError::runtime(
                "ITEM_TOO_LARGE",
                "join field exceeds the item limit",
            ));
        }
        budget.consume_item(path).map_err(|error| AppError {
            code: error.code,
            message: error.message,
        })?;
        records.push(Record { fields });
    }
    Ok(records)
}

impl From<JoinKind> for JoinType {
    fn from(value: JoinKind) -> Self {
        match value {
            JoinKind::Inner => Self::Inner,
            JoinKind::Left => Self::Left,
            JoinKind::Full => Self::Full,
            JoinKind::Anti => Self::Anti,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_bounded_csv_inputs() {
        let root = std::env::temp_dir();
        let left = root.join(format!("combinator-app-join-left-{}", std::process::id()));
        let right = root.join(format!("combinator-app-join-right-{}", std::process::id()));
        std::fs::write(&left, "id,name\n1,A\n2,B\n").unwrap();
        std::fs::write(&right, "id,value\n1,X\n").unwrap();
        let request = JoinRequest {
            left_path: left.to_string_lossy().into_owned(),
            right_path: right.to_string_lossy().into_owned(),
            left_key: "id".into(),
            right_key: "id".into(),
            kind: JoinKind::Left,
            ..Default::default()
        };
        let plan = join_plan(&request).unwrap();
        assert_eq!(plan.total_records, 2);
        let records = join_preview(&request, 10).unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].value.contains("value"));
        let _ = std::fs::remove_file(left);
        let _ = std::fs::remove_file(right);
    }

    #[test]
    fn zero_limit_does_not_load_join_sources() {
        let request = JoinRequest {
            left_path: "missing-left.csv".into(),
            right_path: "missing-right.csv".into(),
            left_key: "id".into(),
            right_key: "id".into(),
            limit: Some(0),
            ..Default::default()
        };
        let mut records = Vec::new();
        let mut sink = VecSink {
            records: &mut records,
        };
        let progress = join_stream(&request, &mut sink, None).unwrap();
        assert_eq!(progress.records, 0);
        assert_eq!(progress.bytes, 0);
    }
}
