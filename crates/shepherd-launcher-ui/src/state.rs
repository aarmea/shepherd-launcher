//! Launcher application state management

use shepherd_api::{DaemonStateSnapshot, EntryView, Event, EventPayload};
use shepherd_util::SessionId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

/// Current state of the launcher UI
#[derive(Debug, Clone)]
pub enum LauncherState {
    /// Not connected to daemon
    Disconnected,
    /// Connected, waiting for initial state
    Connecting,
    /// Connected, no session running - show grid
    Idle { entries: Vec<EntryView> },
    /// Launch requested, waiting for response
    Launching { entry_id: String },
    /// Session is running
    SessionActive {
        session_id: SessionId,
        entry_label: String,
        time_remaining: Option<Duration>,
    },
    /// Error state
    Error { message: String },
}

impl Default for LauncherState {
    fn default() -> Self {
        Self::Disconnected
    }
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

    /// Update state from daemon event
    pub fn handle_event(&self, event: Event) {
        match event.payload {
            EventPayload::StateChanged(snapshot) => {
                self.apply_snapshot(snapshot);
            }
            EventPayload::SessionStarted {
                session_id,
                entry_id: _,
                label,
                deadline,
            } => {
                let now = chrono::Local::now();
                let time_remaining = if deadline > now {
                    (deadline - now).to_std().ok()
                } else {
                    Some(Duration::ZERO)
                };
                self.set(LauncherState::SessionActive {
                    session_id,
                    entry_label: label,
                    time_remaining,
                });
            }
            EventPayload::SessionEnded { .. } => {
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
                // Daemon is shutting down
                self.set(LauncherState::Disconnected);
            }
            EventPayload::AuditEntry { .. } => {
                // Audit events are for admin clients, ignore
            }
        }
    }

    fn apply_snapshot(&self, snapshot: DaemonStateSnapshot) {
        if let Some(session) = snapshot.current_session {
            let now = chrono::Local::now();
            let time_remaining = if session.deadline > now {
                (session.deadline - now).to_std().ok()
            } else {
                Some(Duration::ZERO)
            };
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
