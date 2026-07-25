//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;
pub mod product;

pub use count::{combination_count, Count};
pub use product::{combinations, Product, ProductOptions};
