//! Core events emitted by the engine

use chrono::{DateTime, Local};
use shepherd_api::{SessionEndReason, WarningSeverity};
use shepherd_util::{EntryId, SessionId};
use std::time::Duration;

/// Events emitted by the core engine
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// Session started successfully
    SessionStarted {
        session_id: SessionId,
        entry_id: EntryId,
        label: String,
        deadline: DateTime<Local>,
    },

    /// Warning threshold reached
    Warning {
        session_id: SessionId,
        threshold_seconds: u64,
        time_remaining: Duration,
        severity: WarningSeverity,
        message: Option<String>,
    },

    /// Session is expiring (termination initiated)
    ExpireDue {
        session_id: SessionId,
    },

    /// Session has ended
    SessionEnded {
        session_id: SessionId,
        entry_id: EntryId,
        reason: SessionEndReason,
        duration: Duration,
    },

    /// Entry availability changed
    EntryAvailabilityChanged {
        entry_id: EntryId,
        enabled: bool,
    },

    /// Policy was reloaded
    PolicyReloaded {
        entry_count: usize,
    },
}
