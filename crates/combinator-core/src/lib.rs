//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod concat;
pub mod count;
pub mod estimate;
pub mod operation;
pub mod product;
pub mod template;
pub mod zip;

pub use concat::{concat_count, concat_records, Concat, ConcatOptions};
pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use operation::{count as operation_count, Operation};
pub use product::{combinations, Product, ProductOptions};
pub use template::{validate_name, Template, TemplateError};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
