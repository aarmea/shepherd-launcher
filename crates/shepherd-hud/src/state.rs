//! State management for the HUD
//!
//! The HUD subscribes to events from shepherdd and tracks session state.

use chrono::Local;
use shepherd_api::{Event, EventPayload, SessionEndReason, VolumeInfo, VolumeRestrictions, WarningSeverity};
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
    },

    /// Warning shown - time running low
    Warning {
        session_id: SessionId,
        entry_id: EntryId,
        entry_name: String,
        warning_issued_at: std::time::Instant,
        time_remaining_at_warning: u64,
        /// Optional custom message from configuration
        message: Option<String>,
        /// Severity level of the warning
        severity: WarningSeverity,
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
    /// Volume info sender (updated via events, not polling)
    volume_tx: Arc<watch::Sender<Option<VolumeInfo>>>,
    /// Volume info receiver
    volume_rx: watch::Receiver<Option<VolumeInfo>>,
}

impl SharedState {
    pub fn new() -> Self {
        let (session_tx, session_rx) = watch::channel(SessionState::NoSession);
        let (metrics_tx, metrics_rx) = watch::channel(SystemMetrics::default());
        let (volume_tx, volume_rx) = watch::channel(None);

        Self {
            session_tx: Arc::new(session_tx),
            session_rx,
            metrics_tx: Arc::new(metrics_tx),
            metrics_rx,
            volume_tx: Arc::new(volume_tx),
            volume_rx,
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

    /// Get current volume info (cached from events)
    pub fn volume_info(&self) -> Option<VolumeInfo> {
        self.volume_rx.borrow().clone()
    }

    /// Set initial volume info (called once on connect)
    pub fn set_initial_volume(&self, info: VolumeInfo) {
        let _ = self.volume_tx.send(Some(info));
    }

    /// Update volume from VolumeChanged event (preserves restrictions from initial fetch)
    fn update_volume(&self, percent: u8, muted: bool) {
        self.volume_tx.send_modify(|vol| {
            if let Some(v) = vol {
                v.percent = percent;
                v.muted = muted;
            } else {
                // If we don't have initial volume yet, create a basic one
                *vol = Some(VolumeInfo {
                    percent,
                    muted,
                    available: true,
                    backend: None,
                    restrictions: VolumeRestrictions::unrestricted(),
                });
            }
        });
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
                let now = shepherd_util::now();
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
                message,
                severity,
                ..
            } => {
                self.session_tx.send_modify(|state| {
                    // Handle transition from Active state
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
                                message: message.clone(),
                                severity: *severity,
                            };
                        }
                    }
                    // Handle update when already in Warning state (subsequent warnings)
                    else if let SessionState::Warning {
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
                                message: message.clone(),
                                severity: *severity,
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
                    let now = shepherd_util::now();
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
                    });
                } else {
                    self.set_session_state(SessionState::NoSession);
                }
            }

            EventPayload::VolumeChanged { percent, muted } => {
                self.update_volume(*percent, *muted);
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
