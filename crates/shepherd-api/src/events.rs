//! Event types for shepherdd -> client streaming

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use shepherd_util::{EntryId, SessionId};
use std::time::Duration;

use crate::{ServiceStateSnapshot, SessionEndReason, WarningSeverity, API_VERSION};

/// Event envelope
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub api_version: u32,
    pub timestamp: DateTime<Local>,
    pub payload: EventPayload,
}

impl Event {
    pub fn new(payload: EventPayload) -> Self {
        Self {
            api_version: API_VERSION,
            timestamp: shepherd_util::now(),
            payload,
        }
    }
}

/// All possible events from the service to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    /// Full state snapshot (sent on subscribe and major changes)
    StateChanged(ServiceStateSnapshot),

    /// Session has started
    SessionStarted {
        session_id: SessionId,
        entry_id: EntryId,
        label: String,
        /// Deadline for session. None means unlimited.
        deadline: Option<DateTime<Local>>,
    },

    /// Warning issued for current session
    WarningIssued {
        session_id: SessionId,
        threshold_seconds: u64,
        time_remaining: Duration,
        severity: WarningSeverity,
        message: Option<String>,
    },

    /// Session is expiring (termination initiated)
    SessionExpiring {
        session_id: SessionId,
    },

    /// Session has ended
    SessionEnded {
        session_id: SessionId,
        entry_id: EntryId,
        reason: SessionEndReason,
        duration: Duration,
    },

    /// Policy was reloaded
    PolicyReloaded {
        entry_count: usize,
    },

    /// Entry availability changed (for UI updates)
    EntryAvailabilityChanged {
        entry_id: EntryId,
        enabled: bool,
    },

    /// Volume status changed
    VolumeChanged {
        percent: u8,
        muted: bool,
    },

    /// Service is shutting down
    Shutdown,

    /// Audit event (for admin clients)
    AuditEntry {
        event_type: String,
        details: serde_json::Value,
    },

    /// Network connectivity status changed
    ConnectivityChanged {
        /// Whether global connectivity check now passes
        connected: bool,
        /// The URL that was checked
        check_url: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serialization() {
        let event = Event::new(EventPayload::SessionStarted {
            session_id: SessionId::new(),
            entry_id: EntryId::new("game-1"),
            label: "Test Game".into(),
            deadline: Some(shepherd_util::now()),
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.api_version, API_VERSION);
        assert!(matches!(parsed.payload, EventPayload::SessionStarted { .. }));
    }

    #[test]
    fn event_serialization_unlimited() {
        // Test with unlimited session (deadline=None)
        let event = Event::new(EventPayload::SessionStarted {
            session_id: SessionId::new(),
            entry_id: EntryId::new("game-1"),
            label: "Unlimited Game".into(),
            deadline: None,
        });

        let json = serde_json::to_string(&event).unwrap();
        let parsed: Event = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.api_version, API_VERSION);
        if let EventPayload::SessionStarted { deadline, .. } = parsed.payload {
            assert!(deadline.is_none());
        } else {
            panic!("Expected SessionStarted");
        }
    }
}
