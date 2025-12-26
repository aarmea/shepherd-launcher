//! Audit event types

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use shepherd_api::SessionEndReason;
use shepherd_util::{EntryId, SessionId};
use std::time::Duration;

/// Types of audit events
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditEventType {
    /// Daemon started
    DaemonStarted,

    /// Daemon stopped
    DaemonStopped,

    /// Policy loaded/reloaded
    PolicyLoaded { entry_count: usize },

    /// Session started
    SessionStarted {
        session_id: SessionId,
        entry_id: EntryId,
        label: String,
        deadline: DateTime<Local>,
    },

    /// Warning issued
    WarningIssued {
        session_id: SessionId,
        threshold_seconds: u64,
    },

    /// Session ended
    SessionEnded {
        session_id: SessionId,
        entry_id: EntryId,
        reason: SessionEndReason,
        duration: Duration,
    },

    /// Launch denied
    LaunchDenied {
        entry_id: EntryId,
        reasons: Vec<String>,
    },

    /// Session extended (admin action)
    SessionExtended {
        session_id: SessionId,
        extended_by: Duration,
        new_deadline: DateTime<Local>,
    },

    /// Config reload requested
    ConfigReloaded { success: bool },

    /// Client connected
    ClientConnected {
        client_id: String,
        role: String,
        uid: Option<u32>,
    },

    /// Client disconnected
    ClientDisconnected { client_id: String },
}

/// Full audit event with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID
    pub id: i64,

    /// Event timestamp
    pub timestamp: DateTime<Local>,

    /// Event type and details
    pub event: AuditEventType,
}

impl AuditEvent {
    pub fn new(event: AuditEventType) -> Self {
        Self {
            id: 0, // Will be set by store
            timestamp: Local::now(),
            event,
        }
    }
}
