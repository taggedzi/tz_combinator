//! Reusable bounded codecs over caller-supplied values and streams.
//!
//! This initial adapter preserves the established codec behavior while the
//! implementation is being relocated out of `combinator-core`. It deliberately
//! exposes no paths, terminals, process state, or CLI argument types.
//!
//! The `combinator` CLI is the supported stable integration boundary. This
//! crate's Rust API is a reusable 0.x workspace API, not a semver-stable public
//! API. See the repository compatibility policy before depending on it.

pub mod estimate;
pub mod input;
pub mod output;
pub mod template;

#[derive(Debug)]
pub struct CodecError {
    pub code: &'static str,
    pub message: String,
    pub context: Vec<(String, String)>,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Runtime,
}

impl CodecError {
    pub fn usage(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            kind: ErrorKind::Usage,
        }
    }
    pub fn runtime(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            context: Vec::new(),
            kind: ErrorKind::Runtime,
        }
    }
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.context.push((key.to_string(), value.to_string()));
        self
    }
}

impl From<std::io::Error> for CodecError {
    fn from(error: std::io::Error) -> Self {
        Self::runtime("WRITE_FAILED", format!("failed writing output: {error}"))
    }
}
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use input::{InputBudget, InputFormat, InputLimits};
pub use output::{format_record, format_record_with, Format};
pub use template::{validate_name, Template, TemplateError};
