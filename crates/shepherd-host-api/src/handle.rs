//! Session handle abstraction

use serde::{Deserialize, Serialize};
use shepherd_util::SessionId;

/// Opaque handle to a running session on the host
///
/// This contains platform-specific identifiers and is created by the
/// host adapter when a session is spawned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSessionHandle {
    /// Session ID from the core
    pub session_id: SessionId,

    /// Platform-specific payload (opaque to core)
    payload: HostHandlePayload,
}

impl HostSessionHandle {
    pub fn new(session_id: SessionId, payload: HostHandlePayload) -> Self {
        Self { session_id, payload }
    }

    pub fn payload(&self) -> &HostHandlePayload {
        &self.payload
    }
}

/// Platform-specific handle payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum HostHandlePayload {
    /// Linux: process group ID
    Linux {
        pid: u32,
        pgid: u32,
    },

    /// Windows: job object handle (serialized as name/id)
    Windows {
        job_name: String,
        process_id: u32,
    },

    /// macOS: bundle or process identifier
    MacOs {
        pid: u32,
        bundle_id: Option<String>,
    },

    /// Mock for testing
    Mock {
        id: u64,
    },
}

impl HostHandlePayload {
    /// Get the process ID if applicable
    pub fn pid(&self) -> Option<u32> {
        match self {
            HostHandlePayload::Linux { pid, .. } => Some(*pid),
            HostHandlePayload::Windows { process_id, .. } => Some(*process_id),
            HostHandlePayload::MacOs { pid, .. } => Some(*pid),
            HostHandlePayload::Mock { .. } => None,
        }
    }
}

/// Exit status from a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitStatus {
    /// Exit code if the process exited normally
    pub code: Option<i32>,

    /// Whether the process was signaled
    pub signaled: bool,

    /// Signal number if signaled (Unix)
    pub signal: Option<i32>,
}

impl ExitStatus {
    pub fn success() -> Self {
        Self {
            code: Some(0),
            signaled: false,
            signal: None,
        }
    }

    pub fn with_code(code: i32) -> Self {
        Self {
            code: Some(code),
            signaled: false,
            signal: None,
        }
    }

    pub fn signaled(signal: i32) -> Self {
        Self {
            code: None,
            signaled: true,
            signal: Some(signal),
        }
    }

    pub fn is_success(&self) -> bool {
        self.code == Some(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_serialization() {
        let handle = HostSessionHandle::new(
            SessionId::new(),
            HostHandlePayload::Linux { pid: 1234, pgid: 1234 },
        );

        let json = serde_json::to_string(&handle).unwrap();
        let parsed: HostSessionHandle = serde_json::from_str(&json).unwrap();

        assert_eq!(handle.payload().pid(), parsed.payload().pid());
    }

    #[test]
    fn exit_status() {
        assert!(ExitStatus::success().is_success());
        assert!(!ExitStatus::with_code(1).is_success());
        assert!(!ExitStatus::signaled(9).is_success());
    }
}
