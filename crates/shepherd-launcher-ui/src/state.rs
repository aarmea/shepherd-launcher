//! Launcher application state management

use shepherd_api::{ServiceStateSnapshot, EntryView, Event, EventPayload};
use shepherd_util::SessionId;
use std::time::Duration;
use tokio::sync::watch;

/// Current state of the launcher UI
#[derive(Debug, Clone, Default)]
pub enum LauncherState {
    /// Not connected to shepherdd
    #[default]
    Disconnected,
    /// Connected, waiting for initial state
    Connecting,
    /// Connected, no session running - show grid
    Idle { entries: Vec<EntryView> },
    /// Launch requested, waiting for response
    Launching {
        #[allow(dead_code)]
        entry_id: String
    },
    /// Session is running
    SessionActive {
        #[allow(dead_code)]
        session_id: SessionId,
        entry_label: String,
        #[allow(dead_code)]
        time_remaining: Option<Duration>,
    },
    /// Error state
    Error { message: String },
}

/// Shared state container
#[derive(Clone)]
pub struct SharedState {
    sender: watch::Sender<LauncherState>,
    receiver: watch::Receiver<LauncherState>,
}

impl SharedState {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(LauncherState::default());
        Self { sender, receiver }
    }

    pub fn set(&self, state: LauncherState) {
        let _ = self.sender.send(state);
    }

    pub fn get(&self) -> LauncherState {
        self.receiver.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<LauncherState> {
        self.receiver.clone()
    }

    /// Update state from shepherdd event
    pub fn handle_event(&self, event: Event) {
        tracing::info!(event = ?event.payload, "Received event from shepherdd");
        match event.payload {
            EventPayload::StateChanged(snapshot) => {
                tracing::info!(has_session = snapshot.current_session.is_some(), "Applying state snapshot");
                self.apply_snapshot(snapshot);
            }
            EventPayload::SessionStarted {
                session_id,
                entry_id: _,
                label,
                deadline,
            } => {
                tracing::info!(session_id = %session_id, label = %label, "Session started event");
                let now = shepherd_util::now();
                // For unlimited sessions (deadline=None), time_remaining is None
                let time_remaining = deadline.and_then(|d| {
                    if d > now {
                        (d - now).to_std().ok()
                    } else {
                        Some(Duration::ZERO)
                    }
                });
                self.set(LauncherState::SessionActive {
                    session_id,
                    entry_label: label,
                    time_remaining,
                });
            }
            EventPayload::SessionEnded { session_id, entry_id, reason, .. } => {
                tracing::info!(session_id = %session_id, entry_id = %entry_id, reason = ?reason, "Session ended event - setting Connecting");
                // Will be followed by StateChanged, but set to connecting
                // to ensure grid reloads
                self.set(LauncherState::Connecting);
            }
            EventPayload::SessionExpiring { .. } => {
                // Time's up indicator handled by HUD
            }
            EventPayload::WarningIssued { .. } => {
                // Warnings handled by HUD
            }
            EventPayload::PolicyReloaded { .. } => {
                // Request fresh state
                self.set(LauncherState::Connecting);
            }
            EventPayload::EntryAvailabilityChanged { .. } => {
                // Request fresh state
                self.set(LauncherState::Connecting);
            }
            EventPayload::Shutdown => {
                // Service is shutting down
                self.set(LauncherState::Disconnected);
            }
            EventPayload::AuditEntry { .. } => {
                // Audit events are for admin clients, ignore
            }
            EventPayload::VolumeChanged { .. } => {
                // Volume events are handled by HUD
            }
            EventPayload::ConnectivityChanged { .. } => {
                // Connectivity changes may affect entry availability - request fresh state
                self.set(LauncherState::Connecting);
            }
        }
    }

    fn apply_snapshot(&self, snapshot: ServiceStateSnapshot) {
        if let Some(session) = snapshot.current_session {
            let now = shepherd_util::now();
            // For unlimited sessions (deadline=None), time_remaining is None
            let time_remaining = session.deadline.and_then(|d| {
                if d > now {
                    (d - now).to_std().ok()
                } else {
                    Some(Duration::ZERO)
                }
            });
            self.set(LauncherState::SessionActive {
                session_id: session.session_id,
                entry_label: session.label,
                time_remaining,
            });
        } else {
            self.set(LauncherState::Idle {
                entries: snapshot.entries,
            });
        }
    }
}

impl Default for SharedState {
    fn default() -> Self {
        Self::new()
    }
}
