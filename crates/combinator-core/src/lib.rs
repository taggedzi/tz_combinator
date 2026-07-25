//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod estimate;
pub mod operation;
pub mod product;
pub mod zip;

pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use operation::{count as operation_count, Operation};
pub use product::{combinations, Product, ProductOptions};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
