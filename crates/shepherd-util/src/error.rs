//! Error types for shepherdd

use thiserror::Error;

use crate::EntryId;

/// Core error type for shepherdd operations
#[derive(Debug, Error)]
pub enum ShepherdError {
    #[error("Entry not found: {0}")]
    EntryNotFound(EntryId),

    #[error("No active session")]
    NoActiveSession,

    #[error("Session already active")]
    SessionAlreadyActive,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Host error: {0}")]
    HostError(String),

    #[error("IPC error: {0}")]
    IpcError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Rate limited")]
    RateLimited,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl ShepherdError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::ConfigError(msg.into())
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::ValidationError(msg.into())
    }

    pub fn store(msg: impl Into<String>) -> Self {
        Self::StoreError(msg.into())
    }

    pub fn host(msg: impl Into<String>) -> Self {
        Self::HostError(msg.into())
    }

    pub fn ipc(msg: impl Into<String>) -> Self {
        Self::IpcError(msg.into())
    }

    pub fn permission(msg: impl Into<String>) -> Self {
        Self::PermissionDenied(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, ShepherdError>;
