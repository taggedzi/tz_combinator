//! Compatibility re-export for the CLI adapter.
//!
//! Paging is owned by `combinator-core`; this module keeps the private CLI
//! module path stable for existing tests and call sites.
pub use combinator_core::sharding::{page, range, ShardError, ShardRange};
