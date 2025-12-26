use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command};

/// Messages that can be sent from the UI to the daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcMessage {
    Ping,
    GetStatus,
    LaunchApp { name: String, command: String },
    SpawnProcess { command: String, args: Vec<String> },
    Shutdown,
}

/// Responses sent from the daemon to the UI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IpcResponse {
    Pong,
    Status { uptime_secs: u64, apps_running: usize },
    AppLaunched { success: bool, message: String },
    ProcessSpawned { success: bool, pid: Option<u32>, message: String },
    ShuttingDown,
    Error { message: String },
}

/// Get the IPC socket path
pub fn get_socket_path() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(runtime_dir).join("shepherd-launcher.sock")
}

/// Server-side IPC handler for the daemon
pub struct IpcServer {
    listener: UnixListener,
    start_time: std::time::Instant,
    processes: HashMap<u32, Child>,
}

impl IpcServer {
    pub fn new() -> std::io::Result<Self> {
        let socket_path = get_socket_path();
        
        // Remove old socket if it exists
        let _ = std::fs::remove_file(&socket_path);
        
        let listener = UnixListener::bind(&socket_path)?;
        listener.set_nonblocking(true)?;
        
        Ok(Self {
            listener,
            start_time: std::time::Instant::now(),
            processes: HashMap::new(),
        })
    }
    
    pub fn accept_and_handle(&mut self) -> std::io::Result<bool> {
        // Clean up finished processes
        self.cleanup_processes();
        
        match self.listener.accept() {
            Ok((stream, _)) => {
                let should_shutdown = self.handle_client(stream)?;
                Ok(should_shutdown)
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
    
    fn handle_client(&mut self, mut stream: UnixStream) -> std::io::Result<bool> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        
        reader.read_line(&mut line)?;
        
        let message: IpcMessage = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(e) => {
                let response = IpcResponse::Error {
                    message: format!("Failed to parse message: {}", e),
                };
                let response_json = serde_json::to_string(&response)?;
                writeln!(stream, "{}", response_json)?;
                return Ok(false);
            }
        };
        
        let should_shutdown = matches!(message, IpcMessage::Shutdown);
        let response = self.process_message(message);
        let response_json = serde_json::to_string(&response)?;
        writeln!(stream, "{}", response_json)?;
        
        Ok(should_shutdown)
    }
    
    fn process_message(&mut self, message: IpcMessage) -> IpcResponse {
        match message {
            IpcMessage::Ping => IpcResponse::Pong,
            IpcMessage::GetStatus => {
                let uptime_secs = self.start_time.elapsed().as_secs();
                IpcResponse::Status {
                    uptime_secs,
                    apps_running: self.processes.len(),
                }
            }
            IpcMessage::LaunchApp { name, command } => {
                println!("[Daemon] Launching app: {} ({})", name, command);
                self.spawn_graphical_process(&command, &[])
            }
            IpcMessage::SpawnProcess { command, args } => {
                println!("[Daemon] Spawning process: {} {:?}", command, args);
                self.spawn_graphical_process(&command, &args)
            }
            IpcMessage::Shutdown => IpcResponse::ShuttingDown,
        }
    }
    
    fn spawn_graphical_process(&mut self, command: &str, args: &[String]) -> IpcResponse {
        // Parse command if it contains arguments and args is empty
        let (cmd, cmd_args) = if args.is_empty() {
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return IpcResponse::ProcessSpawned {
                    success: false,
                    pid: None,
                    message: "Empty command".to_string(),
                };
            }
            (parts[0], parts[1..].iter().map(|s| s.to_string()).collect())
        } else {
            (command, args.to_vec())
        };
        
        match Command::new(cmd)
            .args(&cmd_args)
            .spawn()
        {
            Ok(child) => {
                let pid = child.id();
                println!("[Daemon] Successfully spawned process PID: {}", pid);
                self.processes.insert(pid, child);
                IpcResponse::ProcessSpawned {
                    success: true,
                    pid: Some(pid),
                    message: format!("Process spawned with PID {}", pid),
                }
            }
            Err(e) => {
                eprintln!("[Daemon] Failed to spawn process '{}': {}", cmd, e);
                IpcResponse::ProcessSpawned {
                    success: false,
                    pid: None,
                    message: format!("Failed to spawn: {}", e),
                }
            }
        }
    }
    
    fn cleanup_processes(&mut self) {
        // Check for finished processes and remove them
        let mut finished = Vec::new();
        for (pid, child) in self.processes.iter_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("[Daemon] Process {} exited with status: {}", pid, status);
                    finished.push(*pid);
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("[Daemon] Error checking process {}: {}", pid, e);
                    finished.push(*pid);
                }
            }
        }
        for pid in finished {
            self.processes.remove(&pid);
        }
    }
}

/// Client-side IPC handler for the UI
pub struct IpcClient;

impl IpcClient {
    pub fn send_message(message: &IpcMessage) -> std::io::Result<IpcResponse> {
        let socket_path = get_socket_path();
        let mut stream = UnixStream::connect(&socket_path)?;
        
        let message_json = serde_json::to_string(message)?;
        writeln!(stream, "{}", message_json)?;
        
        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line)?;
        
        let response: IpcResponse = serde_json::from_str(&response_line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        
        Ok(response)
    }
}
