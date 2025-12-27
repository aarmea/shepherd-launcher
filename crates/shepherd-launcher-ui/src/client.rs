//! IPC client wrapper for the launcher UI

use anyhow::{Context, Result};
use shepherd_api::{Command, Event, Response, ResponsePayload, ResponseResult};
use shepherd_ipc::IpcClient;
use shepherd_util::EntryId;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::state::{LauncherState, SharedState};

/// Messages from UI to client task
#[derive(Debug)]
pub enum ClientCommand {
    /// Request to launch an entry
    Launch(EntryId),
    /// Request to stop current session
    StopCurrent,
    /// Request fresh state
    RefreshState,
    /// Shutdown the client
    Shutdown,
}

/// Client connection manager
pub struct DaemonClient {
    socket_path: std::path::PathBuf,
    state: SharedState,
    command_rx: mpsc::UnboundedReceiver<ClientCommand>,
}

impl DaemonClient {
    pub fn new(
        socket_path: impl AsRef<Path>,
        state: SharedState,
        command_rx: mpsc::UnboundedReceiver<ClientCommand>,
    ) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            state,
            command_rx,
        }
    }

    /// Run the client connection loop
    pub async fn run(mut self) {
        loop {
            match self.connect_and_run().await {
                Ok(()) => {
                    info!("Client loop exited normally");
                    break;
                }
                Err(e) => {
                    error!(error = %e, "Connection error");
                    self.state.set(LauncherState::Disconnected);
                    
                    // Wait before reconnecting
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn connect_and_run(&mut self) -> Result<()> {
        self.state.set(LauncherState::Connecting);

        info!(path = %self.socket_path.display(), "Connecting to daemon");
        
        let mut client = IpcClient::connect(&self.socket_path)
            .await
            .context("Failed to connect to daemon")?;

        info!("Connected to daemon");

        // Get initial state
        let response = client.send(Command::GetState).await?;
        self.handle_response(response)?;

        // Subscribe to events
        let response = client.send(Command::SubscribeEvents).await?;
        if let ResponseResult::Err(e) = response.result {
            warn!(error = %e.message, "Failed to subscribe to events");
        }

        // Get entries list
        let response = client.send(Command::ListEntries { at_time: None }).await?;
        self.handle_response(response)?;

        // Now consume client for event stream
        let mut events = client.subscribe().await?;

        // Main event loop
        loop {
            tokio::select! {
                // Handle commands from UI
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        ClientCommand::Shutdown => {
                            info!("Shutdown requested");
                            return Ok(());
                        }
                        ClientCommand::Launch(entry_id) => {
                            // We can't send commands after subscribing since client is consumed
                            // Need to reconnect for commands
                            warn!("Launch command received but cannot send after subscribe");
                            // For now, trigger a reconnect
                            return Ok(());
                        }
                        ClientCommand::StopCurrent => {
                            warn!("Stop command received but cannot send after subscribe");
                            return Ok(());
                        }
                        ClientCommand::RefreshState => {
                            // Trigger reconnect to refresh
                            return Ok(());
                        }
                    }
                }

                // Handle events from daemon
                event_result = events.next() => {
                    match event_result {
                        Ok(event) => {
                            debug!(event = ?event, "Received event");
                            self.state.handle_event(event);
                        }
                        Err(e) => {
                            error!(error = %e, "Event stream error");
                            return Err(e.into());
                        }
                    }
                }
            }
        }
    }

    fn handle_response(&self, response: Response) -> Result<()> {
        match response.result {
            ResponseResult::Ok(payload) => {
                match payload {
                    ResponsePayload::State(snapshot) => {
                        if let Some(session) = snapshot.current_session {
                            let now = chrono::Local::now();
                            let time_remaining = if session.deadline > now {
                                (session.deadline - now).to_std().ok()
                            } else {
                                Some(Duration::ZERO)
                            };
                            self.state.set(LauncherState::SessionActive {
                                session_id: session.session_id,
                                entry_label: session.label,
                                time_remaining,
                            });
                        } else {
                            self.state.set(LauncherState::Idle {
                                entries: snapshot.entries,
                            });
                        }
                    }
                    ResponsePayload::Entries(entries) => {
                        // Only update if we're in idle state
                        if matches!(self.state.get(), LauncherState::Idle { .. } | LauncherState::Connecting) {
                            self.state.set(LauncherState::Idle { entries });
                        }
                    }
                    ResponsePayload::LaunchApproved { session_id, deadline } => {
                        let now = chrono::Local::now();
                        let time_remaining = if deadline > now {
                            (deadline - now).to_std().ok()
                        } else {
                            Some(Duration::ZERO)
                        };
                        self.state.set(LauncherState::SessionActive {
                            session_id,
                            entry_label: "Starting...".into(),
                            time_remaining,
                        });
                    }
                    ResponsePayload::LaunchDenied { reasons } => {
                        let message = reasons
                            .iter()
                            .map(|r| r.message.as_deref().unwrap_or("Denied"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        self.state.set(LauncherState::Error { message });
                    }
                    _ => {}
                }
                Ok(())
            }
            ResponseResult::Err(e) => {
                self.state.set(LauncherState::Error {
                    message: e.message,
                });
                Ok(())
            }
        }
    }
}

/// Separate command client for sending commands (not subscribed)
pub struct CommandClient {
    socket_path: std::path::PathBuf,
}

impl CommandClient {
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
        }
    }

    pub async fn launch(&self, entry_id: &EntryId) -> Result<Response> {
        let mut client = IpcClient::connect(&self.socket_path).await?;
        client.send(Command::Launch {
            entry_id: entry_id.clone(),
        }).await.map_err(Into::into)
    }

    pub async fn stop_current(&self) -> Result<Response> {
        let mut client = IpcClient::connect(&self.socket_path).await?;
        client.send(Command::StopCurrent {
            mode: shepherd_api::StopMode::Graceful,
        }).await.map_err(Into::into)
    }

    pub async fn get_state(&self) -> Result<Response> {
        let mut client = IpcClient::connect(&self.socket_path).await?;
        client.send(Command::GetState).await.map_err(Into::into)
    }

    pub async fn list_entries(&self) -> Result<Response> {
        let mut client = IpcClient::connect(&self.socket_path).await?;
        client.send(Command::ListEntries { at_time: None }).await.map_err(Into::into)
    }
}
