//! Persistence layer for shepherdd
//!
//! Provides:
//! - Audit log (append-only)
//! - Usage accounting (per entry/day)
//! - Cooldown tracking
//! - State snapshot for recovery

mod audit;
mod sqlite;
mod traits;

pub use audit::*;
pub use sqlite::*;
pub use traits::*;

use thiserror::Error;

/// Store errors
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(e: serde_json::Error) -> Self {
        StoreError::Serialization(e.to_string())
    }
}

pub type StoreResult<T> = Result<T, StoreError>;
