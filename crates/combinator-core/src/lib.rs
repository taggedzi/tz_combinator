//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod estimate;
pub mod product;

pub use count::{combination_count, Count};
pub use estimate::{estimate_jsonl_size, estimate_text_size, SizeEstimate, SizeInput};
pub use product::{combinations, Product, ProductOptions};
