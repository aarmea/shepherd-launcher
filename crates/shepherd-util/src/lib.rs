//! Shared utilities for shepherdd
//!
//! This crate provides:
//! - ID types (EntryId, SessionId, ClientId)
//! - Time utilities (monotonic time, duration helpers)
//! - Error types
//! - Rate limiting helpers
//! - Default paths for socket, data, and log directories

mod error;
mod ids;
mod paths;
mod rate_limit;
mod time;

pub use error::*;
pub use ids::*;
pub use paths::*;
pub use rate_limit::*;
pub use time::*;
