//! Ordered Cartesian-product engine: counting, size estimation, and lazy streaming.

pub mod count;

pub use count::{combination_count, Count};
