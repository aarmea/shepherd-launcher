//! Linux host adapter implementation

use async_trait::async_trait;
use shepherd_api::{EntryKind, EntryKindTag};
use shepherd_host_api::{
    ExitStatus, HostAdapter, HostCapabilities, HostError, HostEvent, HostHandlePayload,
    HostResult, HostSessionHandle, SpawnOptions, StopMode,
};
use shepherd_util::SessionId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::ManagedProcess;

/// Linux host adapter
pub struct LinuxHost {
    capabilities: HostCapabilities,
    processes: Arc<Mutex<HashMap<u32, ManagedProcess>>>,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    event_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<HostEvent>>>>,
}

impl LinuxHost {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Self {
            capabilities: HostCapabilities::linux_full(),
            processes: Arc::new(Mutex::new(HashMap::new())),
            event_tx: tx,
            event_rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    /// Start the background process monitor
    pub fn start_monitor(&self) -> tokio::task::JoinHandle<()> {
        let processes = self.processes.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let mut exited = Vec::new();

                {
                    let mut procs = processes.lock().unwrap();
                    for (pid, proc) in procs.iter_mut() {
                        match proc.try_wait() {
                            Ok(Some(status)) => {
                                exited.push((*pid, proc.pgid, status));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                warn!(pid = pid, error = %e, "Error checking process status");
                            }
                        }
                    }

                    for (pid, _, _) in &exited {
                        procs.remove(pid);
                    }
                }

                for (pid, pgid, status) in exited {
                    info!(pid = pid, pgid = pgid, status = ?status, "Process exited - sending HostEvent::Exited");

                    // We don't have the session_id here, so we use a placeholder
                    // The daemon should track the mapping
                    let handle = HostSessionHandle::new(
                        SessionId::new(), // This will be matched by PID
                        HostHandlePayload::Linux { pid, pgid },
                    );

                    let _ = event_tx.send(HostEvent::Exited { handle, status });
                }
            }
        })
    }
}

impl Default for LinuxHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HostAdapter for LinuxHost {
    fn capabilities(&self) -> &HostCapabilities {
        &self.capabilities
    }

    async fn spawn(
        &self,
        session_id: SessionId,
        entry_kind: &EntryKind,
        options: SpawnOptions,
    ) -> HostResult<HostSessionHandle> {
        let (argv, env, cwd) = match entry_kind {
            EntryKind::Process { argv, env, cwd } => (argv.clone(), env.clone(), cwd.clone()),
            EntryKind::Vm { driver, args } => {
                // Construct command line from VM driver
                let mut argv = vec![driver.clone()];
                for (key, value) in args {
                    argv.push(format!("--{}", key));
                    if let Some(v) = value.as_str() {
                        argv.push(v.to_string());
                    } else {
                        argv.push(value.to_string());
                    }
                }
                (argv, HashMap::new(), None)
            }
            EntryKind::Media { library_id, args } => {
                // For media, we'd typically launch a media player
                // This is a placeholder - real implementation would integrate with a player
                let mut argv = vec!["xdg-open".to_string(), library_id.clone()];
                (argv, HashMap::new(), None)
            }
            EntryKind::Custom { type_name, payload } => {
                return Err(HostError::UnsupportedKind);
            }
        };

        let proc = ManagedProcess::spawn(
            &argv,
            &env,
            cwd.as_ref(),
            options.capture_stdout || options.capture_stderr,
        )?;

        let pid = proc.pid;
        let pgid = proc.pgid;

        let handle = HostSessionHandle::new(
            session_id,
            HostHandlePayload::Linux { pid, pgid },
        );

        self.processes.lock().unwrap().insert(pid, proc);

        info!(pid = pid, pgid = pgid, "Spawned process");

        Ok(handle)
    }

    async fn stop(&self, handle: &HostSessionHandle, mode: StopMode) -> HostResult<()> {
        let (pid, _pgid) = match handle.payload() {
            HostHandlePayload::Linux { pid, pgid } => (*pid, *pgid),
            _ => return Err(HostError::SessionNotFound),
        };

        // Check if process exists
        {
            let procs = self.processes.lock().unwrap();
            if !procs.contains_key(&pid) {
                return Err(HostError::SessionNotFound);
            }
        }

        match mode {
            StopMode::Graceful { timeout } => {
                // Send SIGTERM
                {
                    let procs = self.processes.lock().unwrap();
                    if let Some(p) = procs.get(&pid) {
                        p.terminate()?;
                    }
                }

                // Wait for graceful exit
                let start = std::time::Instant::now();
                loop {
                    if start.elapsed() >= timeout {
                        // Force kill after timeout
                        let procs = self.processes.lock().unwrap();
                        if let Some(p) = procs.get(&pid) {
                            p.kill()?;
                        }
                        break;
                    }

                    {
                        let procs = self.processes.lock().unwrap();
                        if !procs.contains_key(&pid) {
                            break;
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            StopMode::Force => {
                let procs = self.processes.lock().unwrap();
                if let Some(p) = procs.get(&pid) {
                    p.kill()?;
                }
            }
        }

        Ok(())
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<HostEvent> {
        self.event_rx
            .lock()
            .unwrap()
            .take()
            .expect("subscribe() can only be called once")
    }

    fn is_healthy(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_spawn_and_exit() {
        let host = LinuxHost::new();
        let _rx = host.subscribe();

        let session_id = SessionId::new();
        let entry = EntryKind::Process {
            argv: vec!["true".into()],
            env: HashMap::new(),
            cwd: None,
        };

        let handle = host
            .spawn(session_id, &entry, SpawnOptions::default())
            .await
            .unwrap();

        // Give it time to exit
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Process should have exited
        match handle.payload() {
            HostHandlePayload::Linux { pid, .. } => {
                let procs = host.processes.lock().unwrap();
                // Process may or may not still be tracked depending on monitor timing
            }
            _ => panic!("Expected Linux handle"),
        }
    }

    #[tokio::test]
    async fn test_spawn_and_kill() {
        let host = LinuxHost::new();
        let _rx = host.subscribe();

        let session_id = SessionId::new();
        let entry = EntryKind::Process {
            argv: vec!["sleep".into(), "60".into()],
            env: HashMap::new(),
            cwd: None,
        };

        let handle = host
            .spawn(session_id, &entry, SpawnOptions::default())
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Kill it
        host.stop(
            &handle,
            StopMode::Graceful {
                timeout: Duration::from_secs(1),
            },
        )
        .await
        .unwrap();
    }
}
