//! Mock host adapter for testing

use async_trait::async_trait;
use shepherd_api::EntryKind;
use shepherd_util::SessionId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::{
    ExitStatus, HostAdapter, HostCapabilities, HostError, HostEvent, HostHandlePayload,
    HostResult, HostSessionHandle, SpawnOptions, StopMode,
};

/// Mock session state for testing
#[derive(Debug, Clone)]
pub struct MockSession {
    pub session_id: SessionId,
    pub mock_id: u64,
    pub running: bool,
    pub exit_delay: Option<Duration>,
}

/// Mock host adapter for unit/integration testing
pub struct MockHost {
    capabilities: HostCapabilities,
    next_id: AtomicU64,
    sessions: Arc<Mutex<HashMap<u64, MockSession>>>,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    event_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<HostEvent>>>>,

    /// Configure spawn to fail
    pub fail_spawn: Arc<Mutex<bool>>,

    /// Configure stop to fail
    pub fail_stop: Arc<Mutex<bool>>,

    /// Auto-exit delay (simulates process exiting on its own)
    pub auto_exit_delay: Arc<Mutex<Option<Duration>>>,
}

impl MockHost {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        Self {
            capabilities: HostCapabilities::minimal(),
            next_id: AtomicU64::new(1),
            sessions: Arc::new(Mutex::new(HashMap::new())),
            event_tx: tx,
            event_rx: Arc::new(Mutex::new(Some(rx))),
            fail_spawn: Arc::new(Mutex::new(false)),
            fail_stop: Arc::new(Mutex::new(false)),
            auto_exit_delay: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_capabilities(mut self, caps: HostCapabilities) -> Self {
        self.capabilities = caps;
        self
    }

    /// Get list of running sessions
    pub fn running_sessions(&self) -> Vec<SessionId> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.running)
            .map(|s| s.session_id.clone())
            .collect()
    }

    /// Simulate process exit
    pub fn simulate_exit(&self, session_id: &SessionId, status: ExitStatus) {
        let sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.values().find(|s| &s.session_id == session_id) {
            let handle = HostSessionHandle::new(
                session.session_id.clone(),
                HostHandlePayload::Mock { id: session.mock_id },
            );
            let _ = self.event_tx.send(HostEvent::Exited { handle, status });
        }
    }

    /// Set auto-exit behavior
    pub fn set_auto_exit(&self, delay: Option<Duration>) {
        *self.auto_exit_delay.lock().unwrap() = delay;
    }
}

impl Default for MockHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl HostAdapter for MockHost {
    fn capabilities(&self) -> &HostCapabilities {
        &self.capabilities
    }

    async fn spawn(
        &self,
        session_id: SessionId,
        _entry_kind: &EntryKind,
        _options: SpawnOptions,
    ) -> HostResult<HostSessionHandle> {
        if *self.fail_spawn.lock().unwrap() {
            return Err(HostError::SpawnFailed("Mock spawn failure".into()));
        }

        let mock_id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let session = MockSession {
            session_id: session_id.clone(),
            mock_id,
            running: true,
            exit_delay: *self.auto_exit_delay.lock().unwrap(),
        };

        self.sessions.lock().unwrap().insert(mock_id, session.clone());

        let handle = HostSessionHandle::new(
            session_id.clone(),
            HostHandlePayload::Mock { id: mock_id },
        );

        // If auto-exit is configured, spawn a task to send exit event
        if let Some(delay) = session.exit_delay {
            let tx = self.event_tx.clone();
            let exit_handle = handle.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(HostEvent::Exited {
                    handle: exit_handle,
                    status: ExitStatus::success(),
                });
            });
        }

        Ok(handle)
    }

    async fn stop(&self, handle: &HostSessionHandle, _mode: StopMode) -> HostResult<()> {
        if *self.fail_stop.lock().unwrap() {
            return Err(HostError::StopFailed("Mock stop failure".into()));
        }

        let mock_id = match handle.payload() {
            HostHandlePayload::Mock { id } => *id,
            _ => return Err(HostError::SessionNotFound),
        };

        let mut sessions = self.sessions.lock().unwrap();
        if let Some(session) = sessions.get_mut(&mock_id) {
            session.running = false;
            let _ = self.event_tx.send(HostEvent::Exited {
                handle: handle.clone(),
                status: ExitStatus::signaled(15), // SIGTERM
            });
            Ok(())
        } else {
            Err(HostError::SessionNotFound)
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<HostEvent> {
        self.event_rx
            .lock()
            .unwrap()
            .take()
            .expect("subscribe() can only be called once")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn mock_spawn_and_stop() {
        let host = MockHost::new();
        let _rx = host.subscribe();

        let session_id = SessionId::new();
        let entry = EntryKind::Process {
            argv: vec!["test".into()],
            env: HashMap::new(),
            cwd: None,
        };

        let handle = host
            .spawn(session_id.clone(), &entry, SpawnOptions::default())
            .await
            .unwrap();

        assert_eq!(host.running_sessions().len(), 1);

        host.stop(&handle, StopMode::Force).await.unwrap();

        // Session marked as not running
        let sessions = host.sessions.lock().unwrap();
        let session = sessions.values().next().unwrap();
        assert!(!session.running);
    }

    #[tokio::test]
    async fn mock_spawn_failure() {
        let host = MockHost::new();
        let _rx = host.subscribe();
        *host.fail_spawn.lock().unwrap() = true;

        let session_id = SessionId::new();
        let entry = EntryKind::Process {
            argv: vec!["test".into()],
            env: HashMap::new(),
            cwd: None,
        };

        let result = host
            .spawn(session_id, &entry, SpawnOptions::default())
            .await;

        assert!(result.is_err());
    }
}
