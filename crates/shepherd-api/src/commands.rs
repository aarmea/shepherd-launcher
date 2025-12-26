//! Command types for the shepherdd protocol

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use shepherd_util::{ClientId, EntryId};
use std::time::Duration;

use crate::{ClientRole, StopMode, API_VERSION};

/// Request wrapper with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Request ID for correlation
    pub request_id: u64,
    /// API version
    pub api_version: u32,
    /// The command
    pub command: Command,
}

impl Request {
    pub fn new(request_id: u64, command: Command) -> Self {
        Self {
            request_id,
            api_version: API_VERSION,
            command,
        }
    }
}

/// Response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Corresponding request ID
    pub request_id: u64,
    /// API version
    pub api_version: u32,
    /// Response payload or error
    pub result: ResponseResult,
}

impl Response {
    pub fn success(request_id: u64, payload: ResponsePayload) -> Self {
        Self {
            request_id,
            api_version: API_VERSION,
            result: ResponseResult::Ok(payload),
        }
    }

    pub fn error(request_id: u64, error: ErrorInfo) -> Self {
        Self {
            request_id,
            api_version: API_VERSION,
            result: ResponseResult::Err(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseResult {
    Ok(ResponsePayload),
    Err(ErrorInfo),
}

/// Error information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorInfo {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Error codes for the protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    EntryNotFound,
    LaunchDenied,
    NoActiveSession,
    SessionActive,
    PermissionDenied,
    RateLimited,
    ConfigError,
    HostError,
    InternalError,
}

/// All possible commands from clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// Get current daemon state
    GetState,

    /// List available entries
    ListEntries {
        /// Optional: evaluate at a specific time (for preview)
        at_time: Option<DateTime<Local>>,
    },

    /// Request to launch an entry
    Launch { entry_id: EntryId },

    /// Stop the current session
    StopCurrent { mode: StopMode },

    /// Reload configuration
    ReloadConfig,

    /// Subscribe to events (returns immediately, events stream separately)
    SubscribeEvents,

    /// Unsubscribe from events
    UnsubscribeEvents,

    /// Get health status
    GetHealth,

    // Admin commands

    /// Extend the current session (admin only)
    ExtendCurrent { by: Duration },

    /// Ping for keepalive
    Ping,
}

/// Response payloads
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsePayload {
    State(crate::DaemonStateSnapshot),
    Entries(Vec<crate::EntryView>),
    LaunchApproved {
        session_id: shepherd_util::SessionId,
        deadline: DateTime<Local>,
    },
    LaunchDenied {
        reasons: Vec<crate::ReasonCode>,
    },
    Stopped,
    ConfigReloaded,
    Subscribed {
        client_id: ClientId,
    },
    Unsubscribed,
    Health(crate::HealthStatus),
    Extended {
        new_deadline: DateTime<Local>,
    },
    Pong,
}

/// Client connection info (set by IPC layer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub client_id: ClientId,
    pub role: ClientRole,
    /// Unix UID if available
    pub uid: Option<u32>,
    /// Process name if available
    pub process_name: Option<String>,
}

impl ClientInfo {
    pub fn new(role: ClientRole) -> Self {
        Self {
            client_id: ClientId::new(),
            role,
            uid: None,
            process_name: None,
        }
    }

    pub fn with_uid(mut self, uid: u32) -> Self {
        self.uid = Some(uid);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialization() {
        let req = Request::new(1, Command::GetState);
        let json = serde_json::to_string(&req).unwrap();
        let parsed: Request = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.request_id, 1);
        assert!(matches!(parsed.command, Command::GetState));
    }

    #[test]
    fn response_serialization() {
        let resp = Response::success(
            1,
            ResponsePayload::State(crate::DaemonStateSnapshot {
                api_version: API_VERSION,
                policy_loaded: true,
                current_session: None,
                entry_count: 5,
            }),
        );

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: Response = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.request_id, 1);
    }
}
