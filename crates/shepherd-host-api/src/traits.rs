//! Host adapter traits

use async_trait::async_trait;
use shepherd_api::EntryKind;
use shepherd_util::SessionId;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc;

use crate::{ExitStatus, HostCapabilities, HostSessionHandle};

/// Errors from host adapter operations
#[derive(Debug, Error)]
pub enum HostError {
    #[error("Spawn failed: {0}")]
    SpawnFailed(String),

    #[error("Stop failed: {0}")]
    StopFailed(String),

    #[error("Unsupported entry kind")]
    UnsupportedKind,

    #[error("Session not found")]
    SessionNotFound,

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

pub type HostResult<T> = Result<T, HostError>;

/// Stop mode for session termination
#[derive(Debug, Clone, Copy)]
pub enum StopMode {
    /// Try graceful stop with timeout, then force
    Graceful { timeout: Duration },
    /// Force immediate termination
    Force,
}

impl Default for StopMode {
    fn default() -> Self {
        Self::Graceful {
            timeout: Duration::from_secs(5),
        }
    }
}

/// Options for spawning a session
#[derive(Debug, Clone, Default)]
pub struct SpawnOptions {
    /// Capture stdout to log file
    pub capture_stdout: bool,

    /// Capture stderr to log file
    pub capture_stderr: bool,

    /// Log file path (if capturing)
    pub log_path: Option<std::path::PathBuf>,

    /// Request fullscreen (if supported)
    pub fullscreen: bool,

    /// Request foreground focus (if supported)
    pub foreground: bool,
}

/// Events from the host adapter
#[derive(Debug, Clone)]
pub enum HostEvent {
    /// Process/session has exited
    Exited {
        handle: HostSessionHandle,
        status: ExitStatus,
    },

    /// Window is ready (for UI notification)
    WindowReady {
        handle: HostSessionHandle,
    },

    /// Spawn failed after handle was created
    SpawnFailed {
        session_id: SessionId,
        error: String,
    },
}

/// Host adapter trait - implemented by platform-specific adapters
#[async_trait]
pub trait HostAdapter: Send + Sync {
    /// Get the capabilities of this host adapter
    fn capabilities(&self) -> &HostCapabilities;

    /// Spawn a new session
    async fn spawn(
        &self,
        session_id: SessionId,
        entry_kind: &EntryKind,
        options: SpawnOptions,
    ) -> HostResult<HostSessionHandle>;

    /// Stop a running session
    async fn stop(&self, handle: &HostSessionHandle, mode: StopMode) -> HostResult<()>;

    /// Subscribe to host events
    fn subscribe(&self) -> mpsc::UnboundedReceiver<HostEvent>;

    /// Optional: set foreground focus (if supported)
    async fn set_foreground(&self, _handle: &HostSessionHandle) -> HostResult<()> {
        Err(HostError::Internal("Not supported".into()))
    }

    /// Optional: set fullscreen mode (if supported)
    async fn set_fullscreen(&self, _handle: &HostSessionHandle) -> HostResult<()> {
        Err(HostError::Internal("Not supported".into()))
    }

    /// Optional: ensure the shell/launcher is visible
    async fn ensure_shell_visible(&self) -> HostResult<()> {
        Ok(())
    }

    /// Optional: check if the host adapter is healthy
    fn is_healthy(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_mode_default() {
        let mode = StopMode::default();
        assert!(matches!(mode, StopMode::Graceful { timeout } if timeout == Duration::from_secs(5)));
    }
}
