//! Shared utilities for shepherdd
//!
//! This crate provides:
//! - ID types (EntryId, SessionId, ClientId)
//! - Time utilities (monotonic time, duration helpers)
//! - Error types
//! - Rate limiting helpers

mod error;
mod ids;
mod rate_limit;
mod time;

pub use error::*;
pub use ids::*;
pub use rate_limit::*;
pub use time::*;
