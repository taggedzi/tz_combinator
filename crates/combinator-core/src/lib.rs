//! Reusable, CLI-independent execution core.
//!
//! This crate owns bounded parsing, normalization, operation dispatch,
//! formatting, estimates, and cancellable writer-based execution. Filesystem
//! paths, terminal behavior, diagnostics rendering, and atomic replacement
//! remain responsibilities of the CLI adapter.

pub mod concat;
pub mod count;
pub mod error;
pub mod estimate;
pub mod execute;
pub mod input;
pub mod join;
pub mod normalize;
pub mod operation;
pub mod output;
pub mod product;
pub mod template;
pub mod zip;

pub use concat::{concat_count, concat_records, Concat, ConcatOptions};
pub use count::{combination_count, Count};
pub use error::{CoreError, ErrorKind};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use execute::{execute, ExecutionRequest, ExecutionResult};
pub use join::{join, JoinType, JoinedRecord, Record};
pub use operation::{count as operation_count, Operation};
pub use output::{format_record, format_record_with, Format};
pub use product::{combinations, Product, ProductOptions};
pub use template::{validate_name, Template, TemplateError};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
