//! CLI-owned, opt-in operational logging.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Level, Metadata, Subscriber};

use crate::cli::{CommonArgs, LogFormat, LogLevel, OutFormat};
use crate::error::AppError;

const ENV_NAME: &str = "COMBINATOR_LOG";
const MAX_ENV_BYTES: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedLogConfig {
    pub(crate) level: LogLevel,
    pub(crate) format: LogFormat,
}

impl ResolvedLogConfig {
    pub(crate) fn enabled(self) -> bool {
        self.level != LogLevel::Off
    }

    pub(crate) fn json_stderr(self, output: OutFormat) -> bool {
        self.enabled() && matches!(output, OutFormat::Json | OutFormat::Jsonl)
    }
}

pub(crate) fn resolve(common: &CommonArgs) -> Result<ResolvedLogConfig, AppError> {
    let level = match common.log_level {
        Some(level) => level,
        None => match std::env::var_os(ENV_NAME) {
            None => LogLevel::Off,
            Some(value) => parse_environment_level(value)?,
        },
    };
    let config = ResolvedLogConfig {
        level,
        format: common.log_format,
    };
    if config.json_stderr(common.format) && config.format != LogFormat::Json {
        return Err(AppError::usage(
            "LOG_FORMAT_REQUIRED",
            "machine-readable output with logging enabled requires --log-format json",
        ));
    }
    Ok(config)
}

fn parse_environment_level(value: std::ffi::OsString) -> Result<LogLevel, AppError> {
    let value = value.to_str().ok_or_else(|| {
        AppError::usage(
            "LOG_LEVEL_INVALID",
            "COMBINATOR_LOG must be a UTF-8 logging level",
        )
    })?;
    if value.len() > MAX_ENV_BYTES {
        return Err(AppError::usage(
            "LOG_LEVEL_INVALID",
            "COMBINATOR_LOG exceeds the logging level length limit",
        ));
    }
    match value {
        "off" => Ok(LogLevel::Off),
        "error" => Ok(LogLevel::Error),
        "warn" => Ok(LogLevel::Warn),
        "info" => Ok(LogLevel::Info),
        "debug" => Ok(LogLevel::Debug),
        "trace" => Ok(LogLevel::Trace),
        _ => Err(AppError::usage(
            "LOG_LEVEL_INVALID",
            "COMBINATOR_LOG must be one of off, error, warn, info, debug, trace",
        )),
    }
}

pub(crate) fn initialize(config: ResolvedLogConfig) {
    if !config.enabled() {
        return;
    }
    match config.format {
        LogFormat::Text => initialize_text(config.level),
        LogFormat::Json => initialize_json(config.level),
    }
}

fn initialize_text(level: LogLevel) {
    initialize_subscriber(level, false);
}

fn initialize_json(level: LogLevel) {
    initialize_subscriber(level, true);
}

fn initialize_subscriber(level: LogLevel, json: bool) {
    let subscriber = CliSubscriber {
        level,
        json,
        next_span: AtomicU64::new(1),
        writer_lock: Mutex::new(()),
    };
    let _ = tracing::subscriber::set_global_default(subscriber);
}

struct CliSubscriber {
    level: LogLevel,
    json: bool,
    next_span: AtomicU64,
    writer_lock: Mutex<()>,
}

impl CliSubscriber {
    fn accepts(&self, level: &Level) -> bool {
        match self.level {
            LogLevel::Off => false,
            LogLevel::Error => *level == Level::ERROR,
            LogLevel::Warn => *level <= Level::WARN,
            LogLevel::Info => *level <= Level::INFO,
            LogLevel::Debug => *level <= Level::DEBUG,
            LogLevel::Trace => true,
        }
    }

    fn write_event(&self, event: &Event<'_>) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let event_name = visitor
            .fields
            .remove("event")
            .and_then(string_value)
            .unwrap_or_else(|| event.metadata().name().to_string());
        let kind = visitor
            .fields
            .remove("kind")
            .and_then(string_value)
            .unwrap_or_else(|| "log".to_string());
        let level = event.metadata().level().to_string().to_lowercase();
        let line = if self.json {
            let mut object = serde_json::Map::new();
            object.insert("kind".to_string(), serde_json::Value::String(kind));
            object.insert("level".to_string(), serde_json::Value::String(level));
            object.insert("event".to_string(), serde_json::Value::String(event_name));
            for (name, value) in visitor.fields {
                object.insert(name, value);
            }
            serde_json::Value::Object(object).to_string()
        } else {
            let mut line = format!("{level} {event_name}");
            for (name, value) in visitor.fields {
                let value = text_value(value);
                line.push(' ');
                line.push_str(&name);
                line.push('=');
                line.push_str(&value);
            }
            line
        };
        if let Ok(_guard) = self.writer_lock.lock() {
            let mut stderr = io::stderr().lock();
            let _ = stderr.write_all(line.as_bytes());
            let _ = stderr.write_all(b"\n");
        }
    }
}

fn text_value(value: serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => {
            serde_json::to_string(&value).unwrap_or_else(|_| "\"<unprintable>\"".to_string())
        }
        value => value.to_string(),
    }
}

impl Subscriber for CliSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.accepts(metadata.level())
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.accepts(metadata.level()) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(self.next_span.fetch_add(1, Ordering::Relaxed))
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if self.accepts(event.metadata().level()) {
            self.write_event(event);
        }
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct EventVisitor {
    fields: BTreeMap<String, serde_json::Value>,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::String(format!("{value:?}")),
        );
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields.insert(
            field.name().to_string(),
            serde_json::Value::Number(value.into()),
        );
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::Value::Bool(value));
    }
}

fn string_value(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value),
        _ => None,
    }
}

pub(crate) fn event_started(
    operation: &'static str,
    input: &'static str,
    output: OutFormat,
    destination: &'static str,
) {
    tracing::debug!(
        kind = "log",
        event = "invocation_started",
        operation,
        input_format = input,
        output_format = output_name(output),
        output_destination = destination,
    );
}

pub(crate) fn input_complete(
    source_count: usize,
    list_count: usize,
    item_count: usize,
    input_bytes: usize,
) {
    tracing::info!(
        kind = "log",
        event = "input_complete",
        source_count,
        list_count,
        item_count,
        input_bytes,
    );
}

pub(crate) fn validation_complete(common: &CommonArgs) {
    tracing::debug!(
        kind = "log",
        event = "validation_complete",
        max_output_bytes = common.max_output_bytes,
        max_input_bytes = common.max_input_bytes,
        max_items_per_list = common.max_items_per_list,
        max_total_items = common.max_total_items,
        max_combinations = %common.max_combinations,
        preflight_enabled = common.output.is_some() && !common.no_preflight,
    );
}

pub(crate) fn estimate_complete(status: &'static str, selected_records: Option<u128>) {
    match selected_records {
        Some(selected_records) => tracing::info!(
            kind = "log",
            event = "estimate_complete",
            estimate_status = status,
            selected_records = %selected_records,
        ),
        None => tracing::info!(
            kind = "log",
            event = "estimate_complete",
            estimate_status = status,
        ),
    }
}

pub(crate) fn generation_started(operation: &'static str, destination: &'static str) {
    tracing::debug!(
        kind = "log",
        event = "generation_started",
        operation,
        output_destination = destination,
    );
}

pub(crate) fn generation_complete(records: u128, bytes: u64, elapsed_ms: u128) {
    tracing::info!(
        kind = "log",
        event = "generation_complete",
        records = %records,
        bytes,
        elapsed_ms = %elapsed_ms,
    );
}

pub(crate) fn invocation_cancelled(error_code: &'static str, elapsed_ms: u128) {
    tracing::warn!(
        kind = "log",
        event = "invocation_cancelled",
        error_code,
        elapsed_ms = %elapsed_ms,
    );
}

pub(crate) fn output_destination(common: &CommonArgs) -> &'static str {
    if common.output.is_some() {
        "file"
    } else {
        "stdout"
    }
}

pub(crate) fn output_name(format: OutFormat) -> &'static str {
    match format {
        OutFormat::Text => "text",
        OutFormat::Jsonl => "jsonl",
        OutFormat::Json => "json",
        OutFormat::Csv => "csv",
        OutFormat::Tsv => "tsv",
        OutFormat::Nul => "nul",
    }
}

pub(crate) fn input_name(common: &CommonArgs) -> &'static str {
    if !common.list.is_empty() && !common.file.is_empty() {
        "mixed"
    } else if !common.list.is_empty() {
        "inline"
    } else {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_vocabulary_is_strict_and_bounded() {
        assert_eq!(
            parse_environment_level("debug".into()).unwrap(),
            LogLevel::Debug
        );
        assert_eq!(
            parse_environment_level("off".into()).unwrap(),
            LogLevel::Off
        );
        assert_eq!(
            parse_environment_level("DEBUG".into()).unwrap_err().code,
            "LOG_LEVEL_INVALID"
        );
        assert_eq!(
            parse_environment_level("x".repeat(MAX_ENV_BYTES + 1).into())
                .unwrap_err()
                .code,
            "LOG_LEVEL_INVALID"
        );
    }

    #[test]
    fn text_values_escape_control_characters_into_one_line() {
        let rendered = text_value(serde_json::Value::String("synthetic\nvalue\t\"".into()));
        assert_eq!(rendered, "\"synthetic\\nvalue\\t\\\"\"");
        assert!(!rendered.contains('\n'));
        assert!(!rendered.contains('\r'));
    }
}
