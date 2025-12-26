//! IPC server implementation

use shepherd_api::{ClientInfo, ClientRole, Event, Request, Response};
use shepherd_util::ClientId;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::{IpcError, IpcResult};

/// Message from client to server
pub enum ServerMessage {
    Request {
        client_id: ClientId,
        request: Request,
    },
    ClientConnected {
        client_id: ClientId,
        info: ClientInfo,
    },
    ClientDisconnected {
        client_id: ClientId,
    },
}

/// IPC Server
pub struct IpcServer {
    socket_path: PathBuf,
    listener: Option<UnixListener>,
    clients: Arc<RwLock<HashMap<ClientId, ClientHandle>>>,
    event_tx: broadcast::Sender<Event>,
    message_tx: mpsc::UnboundedSender<ServerMessage>,
    message_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ServerMessage>>>>,
}

struct ClientHandle {
    info: ClientInfo,
    response_tx: mpsc::UnboundedSender<String>,
    subscribed: bool,
}

impl IpcServer {
    /// Create a new IPC server
    pub fn new(socket_path: impl AsRef<Path>) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        let (message_tx, message_rx) = mpsc::unbounded_channel();

        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            listener: None,
            clients: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            message_tx,
            message_rx: Arc::new(Mutex::new(Some(message_rx))),
        }
    }

    /// Start listening
    pub async fn start(&mut self) -> IpcResult<()> {
        // Remove existing socket if present
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        // Create parent directory if needed
        if let Some(parent) = self.socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;

        // Set socket permissions (readable/writable by owner and group)
        std::fs::set_permissions(&self.socket_path, std::fs::Permissions::from_mode(0o660))?;

        info!(path = %self.socket_path.display(), "IPC server listening");

        self.listener = Some(listener);

        Ok(())
    }

    /// Get receiver for server messages
    pub async fn take_message_receiver(&self) -> Option<mpsc::UnboundedReceiver<ServerMessage>> {
        self.message_rx.lock().await.take()
    }

    /// Accept connections in a loop
    pub async fn run(&self) -> IpcResult<()> {
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| IpcError::ServerError("Server not started".into()))?;

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let client_id = ClientId::new();

                    // Get peer credentials
                    let uid = get_peer_uid(&stream);

                    // Determine role based on UID
                    let role = match uid {
                        Some(0) => ClientRole::Admin, // root
                        Some(u) if u == nix::unistd::getuid().as_raw() => ClientRole::Admin,
                        _ => ClientRole::Shell,
                    };

                    let info = ClientInfo::new(role);
                    let info = if let Some(u) = uid {
                        info.with_uid(u)
                    } else {
                        info
                    };

                    info!(client_id = %client_id, uid = ?uid, role = ?role, "Client connected");

                    self.handle_client(stream, client_id, info).await;
                }
                Err(e) => {
                    error!(error = %e, "Failed to accept connection");
                }
            }
        }
    }

    async fn handle_client(&self, stream: UnixStream, client_id: ClientId, info: ClientInfo) {
        let (read_half, write_half) = stream.into_split();
        let (response_tx, mut response_rx) = mpsc::unbounded_channel::<String>();

        // Register client
        {
            let mut clients = self.clients.write().await;
            clients.insert(
                client_id.clone(),
                ClientHandle {
                    info: info.clone(),
                    response_tx: response_tx.clone(),
                    subscribed: false,
                },
            );
        }

        // Notify of connection
        let _ = self.message_tx.send(ServerMessage::ClientConnected {
            client_id: client_id.clone(),
            info: info.clone(),
        });

        let clients = self.clients.clone();
        let message_tx = self.message_tx.clone();
        let event_tx = self.event_tx.clone();
        let client_id_clone = client_id.clone();

        // Spawn reader task
        let reader_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        debug!(client_id = %client_id_clone, "Client disconnected (EOF)");
                        break;
                    }
                    Ok(_) => {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }

                        match serde_json::from_str::<Request>(line) {
                            Ok(request) => {
                                // Check for subscribe command
                                if matches!(request.command, shepherd_api::Command::SubscribeEvents) {
                                    let mut clients = clients.write().await;
                                    if let Some(handle) = clients.get_mut(&client_id_clone) {
                                        handle.subscribed = true;
                                    }
                                }

                                let _ = message_tx.send(ServerMessage::Request {
                                    client_id: client_id_clone.clone(),
                                    request,
                                });
                            }
                            Err(e) => {
                                warn!(
                                    client_id = %client_id_clone,
                                    error = %e,
                                    "Invalid request"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        debug!(client_id = %client_id_clone, error = %e, "Read error");
                        break;
                    }
                }
            }
        });

        // Spawn writer task
        let mut event_rx = event_tx.subscribe();
        let clients_writer = self.clients.clone();
        let client_id_writer = client_id.clone();
        let message_tx_writer = self.message_tx.clone();

        tokio::spawn(async move {
            let mut writer = write_half;

            loop {
                tokio::select! {
                    // Handle responses
                    Some(response) = response_rx.recv() => {
                        let mut msg = response;
                        msg.push('\n');
                        if let Err(e) = writer.write_all(msg.as_bytes()).await {
                            debug!(client_id = %client_id_writer, error = %e, "Write error");
                            break;
                        }
                    }

                    // Handle events (for subscribed clients)
                    Ok(event) = event_rx.recv() => {
                        let is_subscribed = {
                            let clients = clients_writer.read().await;
                            clients.get(&client_id_writer).map(|h| h.subscribed).unwrap_or(false)
                        };

                        if is_subscribed {
                            if let Ok(json) = serde_json::to_string(&event) {
                                let mut msg = json;
                                msg.push('\n');
                                if let Err(e) = writer.write_all(msg.as_bytes()).await {
                                    debug!(client_id = %client_id_writer, error = %e, "Event write error");
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            // Notify of disconnection
            let _ = message_tx_writer.send(ServerMessage::ClientDisconnected {
                client_id: client_id_writer.clone(),
            });

            // Remove client
            let mut clients = clients_writer.write().await;
            clients.remove(&client_id_writer);
        });
    }

    /// Send a response to a specific client
    pub async fn send_response(&self, client_id: &ClientId, response: Response) -> IpcResult<()> {
        let json = serde_json::to_string(&response)?;

        let clients = self.clients.read().await;
        if let Some(handle) = clients.get(client_id) {
            handle
                .response_tx
                .send(json)
                .map_err(|_| IpcError::ConnectionClosed)?;
        }

        Ok(())
    }

    /// Broadcast an event to all subscribed clients
    pub fn broadcast_event(&self, event: Event) {
        let _ = self.event_tx.send(event);
    }

    /// Get client info
    pub async fn get_client_info(&self, client_id: &ClientId) -> Option<ClientInfo> {
        let clients = self.clients.read().await;
        clients.get(client_id).map(|h| h.info.clone())
    }

    /// Get connected client count
    pub async fn client_count(&self) -> usize {
        self.clients.read().await.len()
    }

    /// Shutdown the server
    pub fn shutdown(&self) {
        if self.socket_path.exists() {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Get peer UID from Unix socket
fn get_peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::unix::io::AsFd;

    // Get the borrowed file descriptor from the stream
    let fd = stream.as_fd();

    match nix::sys::socket::getsockopt(&fd, nix::sys::socket::sockopt::PeerCredentials) {
        Ok(cred) => Some(cred.uid()),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_server_start() {
        let dir = tempdir().unwrap();
        let socket_path = dir.path().join("test.sock");

        let mut server = IpcServer::new(&socket_path);
        server.start().await.unwrap();

        assert!(socket_path.exists());
    }
}
