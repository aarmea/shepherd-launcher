//! State management for the HUD
//!
//! The HUD subscribes to events from shepherdd and tracks session state.

use chrono::Local;
use shepherd_api::{Event, EventPayload, SessionEndReason};
use shepherd_util::{EntryId, SessionId};
use std::sync::Arc;
use tokio::sync::watch;

/// The current state of the session as seen by the HUD
#[derive(Debug, Clone)]
pub enum SessionState {
    /// No active session - HUD should be hidden
    NoSession,

    /// Session is active
    Active {
        session_id: SessionId,
        entry_id: EntryId,
        entry_name: String,
        started_at: std::time::Instant,
        time_limit_secs: Option<u64>,
        time_remaining_secs: Option<u64>,
        paused: bool,
    },

    /// Warning shown - time running low
    Warning {
        session_id: SessionId,
        entry_id: EntryId,
        entry_name: String,
        warning_issued_at: std::time::Instant,
        time_remaining_at_warning: u64,
    },

    /// Session is ending
    Ending {
        session_id: SessionId,
        reason: String,
    },
}

impl SessionState {
    /// Check if the HUD should be visible
    /// The HUD is always visible - it shows session info when active,
    /// or a minimal bar when no session
    pub fn is_visible(&self) -> bool {
        // Always show the HUD
        true
    }

    /// Get the current session ID if any
    pub fn session_id(&self) -> Option<&SessionId> {
        match self {
            SessionState::NoSession => None,
            SessionState::Active { session_id, .. } => Some(session_id),
            SessionState::Warning { session_id, .. } => Some(session_id),
            SessionState::Ending { session_id, .. } => Some(session_id),
        }
    }
}

/// System metrics for display
#[derive(Debug, Clone, Default)]
pub struct SystemMetrics {
    /// Battery percentage (0-100)
    pub battery_percent: Option<u8>,
    /// Whether battery is charging
    pub battery_charging: bool,
    /// Volume percentage (0-100)
    pub volume_percent: Option<u8>,
    /// Whether volume is muted
    pub volume_muted: bool,
}

/// Shared state for the HUD
#[derive(Clone)]
pub struct SharedState {
    /// Session state sender
    session_tx: Arc<watch::Sender<SessionState>>,
    /// Session state receiver
    session_rx: watch::Receiver<SessionState>,
    /// System metrics sender
    metrics_tx: Arc<watch::Sender<SystemMetrics>>,
    /// System metrics receiver
    metrics_rx: watch::Receiver<SystemMetrics>,
}

impl SharedState {
    pub fn new() -> Self {
        let (session_tx, session_rx) = watch::channel(SessionState::NoSession);
        let (metrics_tx, metrics_rx) = watch::channel(SystemMetrics::default());

        Self {
            session_tx: Arc::new(session_tx),
            session_rx,
            metrics_tx: Arc::new(metrics_tx),
            metrics_rx,
        }
    }

    /// Get the current session state
    pub fn session_state(&self) -> SessionState {
        self.session_rx.borrow().clone()
    }

    /// Subscribe to session state changes
    pub fn subscribe_session(&self) -> watch::Receiver<SessionState> {
        self.session_rx.clone()
    }

    /// Subscribe to metrics changes
    pub fn subscribe_metrics(&self) -> watch::Receiver<SystemMetrics> {
        self.metrics_rx.clone()
    }

    /// Update session state
    pub fn set_session_state(&self, state: SessionState) {
        let _ = self.session_tx.send(state);
    }

    /// Update system metrics
    pub fn set_metrics(&self, metrics: SystemMetrics) {
        let _ = self.metrics_tx.send(metrics);
    }

    /// Update time remaining for current session
    pub fn update_time_remaining(&self, remaining_secs: u64) {
        self.session_tx.send_modify(|state| {
            if let SessionState::Active {
                time_remaining_secs,
                ..
            } = state
            {
                *time_remaining_secs = Some(remaining_secs);
            }
        });
    }

    /// Handle an event from shepherdd
    pub fn handle_event(&self, event: &Event) {
        match &event.payload {
            EventPayload::SessionStarted {
                session_id,
                entry_id,
                label,
                deadline,
            } => {
                let now = chrono::Local::now();
                // For unlimited sessions (deadline=None), time_remaining is None
                let time_remaining = deadline.and_then(|d| {
                    if d > now {
                        Some((d - now).num_seconds().max(0) as u64)
                    } else {
                        Some(0)
                    }
                });
                self.set_session_state(SessionState::Active {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                    entry_name: label.clone(),
                    started_at: std::time::Instant::now(),
                    time_limit_secs: time_remaining,
                    time_remaining_secs: time_remaining,
                    paused: false,
                });
            }

            EventPayload::SessionEnded { session_id, .. } => {
                if self.session_state().session_id() == Some(session_id) {
                    self.set_session_state(SessionState::NoSession);
                }
            }

            EventPayload::WarningIssued {
                session_id,
                time_remaining,
                ..
            } => {
                self.session_tx.send_modify(|state| {
                    if let SessionState::Active {
                        session_id: sid,
                        entry_id,
                        entry_name,
                        ..
                    } = state
                    {
                        if sid == session_id {
                            *state = SessionState::Warning {
                                session_id: session_id.clone(),
                                entry_id: entry_id.clone(),
                                entry_name: entry_name.clone(),
                                warning_issued_at: std::time::Instant::now(),
                                time_remaining_at_warning: time_remaining.as_secs(),
                            };
                        }
                    }
                });
            }

            EventPayload::SessionExpiring { session_id } => {
                if self.session_state().session_id() == Some(session_id) {
                    self.set_session_state(SessionState::Ending {
                        session_id: session_id.clone(),
                        reason: "Time expired".to_string(),
                    });
                }
            }

            EventPayload::StateChanged(snapshot) => {
                if let Some(session) = &snapshot.current_session {
                    let now = chrono::Local::now();
                    // For unlimited sessions (deadline=None), time_remaining is None
                    let time_remaining = session.deadline.and_then(|d| {
                        if d > now {
                            Some((d - now).num_seconds().max(0) as u64)
                        } else {
                            Some(0)
                        }
                    });
                    self.set_session_state(SessionState::Active {
                        session_id: session.session_id.clone(),
                        entry_id: session.entry_id.clone(),
                        entry_name: session.label.clone(),
                        started_at: std::time::Instant::now(),
                        time_limit_secs: time_remaining,
                        time_remaining_secs: time_remaining,
                        paused: false,
                    });
                } else {
                    self.set_session_state(SessionState::NoSession);
                }
            }

            _ => {}
        }
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}
