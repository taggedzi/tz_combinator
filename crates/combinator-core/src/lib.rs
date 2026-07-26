//! Reusable, CLI-independent execution core.
//!
//! This crate owns the interface-neutral domain model and bounded combination
//! algorithms. It does not parse files, format output, render diagnostics, or
//! depend on terminal/process behavior.

pub mod concat;
pub mod constraint;
pub mod count;
pub mod error;
pub mod join;
pub mod normalize;
pub mod operation;
pub mod product;
pub mod records;
pub mod selection;
pub mod sharding;
pub mod zip;

pub use concat::{concat_count, concat_records, Concat, ConcatOptions};
pub use constraint::Constraint;
pub use count::{combination_count, Count};
pub use error::{CoreError, ErrorKind};
pub use join::{
    join, join_count, join_count_with_fanout, join_each, join_each_with_fanout, JoinType,
    JoinedRecord, Record,
};
pub use normalize::{normalize_typed, Transform};
pub use operation::{count as operation_count, validate as validate_operation, Operation};
pub use product::{combinations, Product, ProductOptions};
pub use records::{
    generate, generate_with, FieldIndex, GenerationLimits, GenerationReport, GenerationRequest,
    LogicalRecord, RecordSink,
};
pub use selection::{
    binomial, combinations as select_combinations, factorial, falling_factorial, permutations,
    variations, Combinations, Permutations, SelectionOptions, Variations,
};
pub use sharding::{page as shard_page, range as shard_range, ShardError, ShardRange};
pub use zip::{zip_count, zip_records, UnequalPolicy, Zip, ZipLengthMismatch, ZipOptions};
