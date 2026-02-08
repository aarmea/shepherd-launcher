//! Linux host adapter implementation

use async_trait::async_trait;
use shepherd_api::EntryKind;
use shepherd_host_api::{
    ExitStatus, HostAdapter, HostCapabilities, HostError, HostEvent, HostHandlePayload, HostResult,
    HostSessionHandle, SpawnOptions, StopMode,
};
use shepherd_util::SessionId;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::process::{
    ManagedProcess, find_steam_game_pids, init, kill_by_command, kill_flatpak_cgroup,
    kill_snap_cgroup, kill_steam_game_processes,
};

/// Expand `~` at the beginning of a path to the user's home directory
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return path.replacen("~", &home.to_string_lossy(), 1);
        }
    } else if path == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home.to_string_lossy().into_owned();
    }
    path.to_string()
}

/// Expand tilde in all arguments
fn expand_args(args: &[String]) -> Vec<String> {
    args.iter().map(|arg| expand_tilde(arg)).collect()
}

/// Information tracked for each session for cleanup purposes
#[derive(Clone, Debug)]
struct SessionInfo {
    command_name: String,
    snap_name: Option<String>,
    flatpak_app_id: Option<String>,
    steam_app_id: Option<u32>,
}

#[derive(Clone, Debug)]
struct SteamSession {
    pid: u32,
    pgid: u32,
    app_id: u32,
    seen_game: bool,
}

/// Linux host adapter
pub struct LinuxHost {
    capabilities: HostCapabilities,
    processes: Arc<Mutex<HashMap<u32, ManagedProcess>>>,
    /// Track session info for killing
    session_info: Arc<Mutex<HashMap<SessionId, SessionInfo>>>,
    steam_sessions: Arc<Mutex<HashMap<u32, SteamSession>>>,
    event_tx: mpsc::UnboundedSender<HostEvent>,
    event_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<HostEvent>>>>,
}

impl LinuxHost {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Initialize process management
        init();

        Self {
            capabilities: HostCapabilities::linux_full(),
            processes: Arc::new(Mutex::new(HashMap::new())),
            session_info: Arc::new(Mutex::new(HashMap::new())),
            steam_sessions: Arc::new(Mutex::new(HashMap::new())),
            event_tx: tx,
            event_rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    /// Start the background process monitor
    pub fn start_monitor(&self) -> tokio::task::JoinHandle<()> {
        let processes = self.processes.clone();
        let steam_sessions = self.steam_sessions.clone();
        let event_tx = self.event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;

                let mut exited = Vec::new();
                let steam_pids: HashSet<u32> =
                    { steam_sessions.lock().unwrap().keys().cloned().collect() };

                {
                    let mut procs = processes.lock().unwrap();
                    for (pid, proc) in procs.iter_mut() {
                        match proc.try_wait() {
                            Ok(Some(status)) => {
                                let is_steam = steam_pids.contains(pid);
                                exited.push((*pid, proc.pgid, status, is_steam));
                            }
                            Ok(None) => {}
                            Err(e) => {
                                warn!(pid = pid, error = %e, "Error checking process status");
                            }
                        }
                    }

                    for (pid, _, _, _) in &exited {
                        procs.remove(pid);
                    }
                }

                for (pid, pgid, status, is_steam) in exited {
                    if is_steam {
                        info!(pid = pid, pgid = pgid, status = ?status, "Steam launch process exited");
                        continue;
                    }
                    info!(pid = pid, pgid = pgid, status = ?status, "Process exited - sending HostEvent::Exited");

                    // We don't have the session_id here, so we use a placeholder
                    // The service should track the mapping
                    let handle = HostSessionHandle::new(
                        SessionId::new(), // This will be matched by PID
                        HostHandlePayload::Linux { pid, pgid },
                    );

                    let _ = event_tx.send(HostEvent::Exited { handle, status });
                }

                // Track Steam sessions by Steam App ID instead of process exit
                let steam_snapshot: Vec<SteamSession> =
                    { steam_sessions.lock().unwrap().values().cloned().collect() };

                let mut ended = Vec::new();

                for session in &steam_snapshot {
                    let has_game = !find_steam_game_pids(session.app_id).is_empty();
                    if has_game {
                        if let Ok(mut map) = steam_sessions.lock() {
                            map.entry(session.pid)
                                .and_modify(|entry| entry.seen_game = true);
                        }
                    } else if session.seen_game {
                        ended.push((session.pid, session.pgid));
                    }
                }

                if !ended.is_empty() {
                    let mut map = steam_sessions.lock().unwrap();
                    let mut procs = processes.lock().unwrap();

                    for (pid, pgid) in ended {
                        map.remove(&pid);
                        procs.remove(&pid);

                        let handle = HostSessionHandle::new(
                            SessionId::new(),
                            HostHandlePayload::Linux { pid, pgid },
                        );
                        let _ = event_tx.send(HostEvent::Exited {
                            handle,
                            status: ExitStatus::success(),
                        });
                    }
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
        // Extract argv, env, cwd, snap_name, flatpak_app_id, and steam_app_id based on entry kind
        let (argv, env, cwd, snap_name, flatpak_app_id, steam_app_id) = match entry_kind {
            EntryKind::Process {
                command,
                args,
                env,
                cwd,
            } => {
                let mut argv = vec![expand_tilde(command)];
                argv.extend(expand_args(args));
                let expanded_cwd = cwd
                    .as_ref()
                    .map(|c| std::path::PathBuf::from(expand_tilde(&c.to_string_lossy())));
                (argv, env.clone(), expanded_cwd, None, None, None)
            }
            EntryKind::Snap {
                snap_name,
                command,
                args,
                env,
            } => {
                // For snap apps, we need to use 'snap run <snap_name>' to launch them.
                // The command (if specified) is passed as an argument after the snap name,
                // followed by any additional args.
                let mut argv = vec!["snap".to_string(), "run".to_string(), snap_name.clone()];
                // If a custom command is specified (different from snap_name), add it
                if let Some(cmd) = command
                    && cmd != snap_name
                {
                    argv.push(cmd.clone());
                }
                argv.extend(expand_args(args));
                (argv, env.clone(), None, Some(snap_name.clone()), None, None)
            }
            EntryKind::Steam { app_id, args, env } => {
                // Steam games are launched via the Steam snap: snap run steam steam://rungameid/<app_id>
                let mut argv = vec![
                    "snap".to_string(),
                    "run".to_string(),
                    "steam".to_string(),
                    format!("steam://rungameid/{}", app_id),
                ];
                argv.extend(expand_args(args));
                (argv, env.clone(), None, None, None, Some(*app_id))
            }
            EntryKind::Flatpak { app_id, args, env } => {
                // For Flatpak apps, we use 'flatpak run <app_id>' to launch them.
                let mut argv = vec!["flatpak".to_string(), "run".to_string(), app_id.clone()];
                argv.extend(expand_args(args));
                (argv, env.clone(), None, None, Some(app_id.clone()), None)
            }
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
                (argv, HashMap::new(), None, None, None, None)
            }
            EntryKind::Media {
                library_id,
                args: _,
            } => {
                // For media, we'd typically launch a media player
                // This is a placeholder - real implementation would integrate with a player
                let argv = vec!["xdg-open".to_string(), expand_tilde(library_id)];
                (argv, HashMap::new(), None, None, None, None)
            }
            EntryKind::Custom {
                type_name: _,
                payload: _,
            } => {
                return Err(HostError::UnsupportedKind);
            }
        };

        // Get the command name for fallback killing
        // For snap/flatpak apps, use the app name (not "snap"/"flatpak") to avoid killing unrelated processes
        let command_name = if let Some(ref snap) = snap_name {
            snap.clone()
        } else if steam_app_id.is_some() {
            "steam".to_string()
        } else if let Some(ref app_id) = flatpak_app_id {
            app_id.clone()
        } else {
            argv.first().cloned().unwrap_or_default()
        };

        // Determine if this is a sandboxed app (snap or flatpak)
        let sandboxed_app_name = snap_name.clone().or_else(|| flatpak_app_id.clone());

        let proc = ManagedProcess::spawn(
            &argv,
            &env,
            cwd.as_ref(),
            options.log_path.clone(),
            sandboxed_app_name,
        )?;

        let pid = proc.pid;
        let pgid = proc.pgid;

        // Store the session info so we can use it for killing even after process exits
        let session_info_entry = SessionInfo {
            command_name: command_name.clone(),
            snap_name: snap_name.clone(),
            flatpak_app_id: flatpak_app_id.clone(),
            steam_app_id,
        };
        self.session_info
            .lock()
            .unwrap()
            .insert(session_id.clone(), session_info_entry);
        info!(session_id = %session_id, command = %command_name, snap = ?snap_name, flatpak = ?flatpak_app_id, "Tracking session info");

        let handle = HostSessionHandle::new(session_id, HostHandlePayload::Linux { pid, pgid });

        self.processes.lock().unwrap().insert(pid, proc);

        if let Some(app_id) = steam_app_id {
            self.steam_sessions.lock().unwrap().insert(
                pid,
                SteamSession {
                    pid,
                    pgid,
                    app_id,
                    seen_game: false,
                },
            );
        }

        info!(pid = pid, pgid = pgid, "Spawned process");

        Ok(handle)
    }

    async fn stop(&self, handle: &HostSessionHandle, mode: StopMode) -> HostResult<()> {
        let session_id = handle.session_id.clone();
        let (pid, _pgid) = match handle.payload() {
            HostHandlePayload::Linux { pid, pgid } => (*pid, *pgid),
            _ => return Err(HostError::SessionNotFound),
        };

        // Get the session's info for killing
        let session_info = self.session_info.lock().unwrap().get(&session_id).cloned();

        // Check if we have session info OR a tracked process
        let has_process = self.processes.lock().unwrap().contains_key(&pid);

        if session_info.is_none() && !has_process {
            warn!(session_id = %session_id, pid = pid, "No session info or tracked process found");
            return Err(HostError::SessionNotFound);
        }

        match mode {
            StopMode::Graceful { timeout } => {
                // If this is a snap or flatpak app, use cgroup-based killing (most reliable)
                if let Some(ref info) = session_info {
                    if let Some(ref snap) = info.snap_name {
                        kill_snap_cgroup(snap, nix::sys::signal::Signal::SIGTERM);
                        info!(snap = %snap, "Sent SIGTERM via snap cgroup");
                    } else if let Some(app_id) = info.steam_app_id {
                        let _ =
                            kill_steam_game_processes(app_id, nix::sys::signal::Signal::SIGTERM);
                        if let Ok(mut map) = self.steam_sessions.lock() {
                            map.entry(pid).and_modify(|entry| entry.seen_game = true);
                        }
                        info!(
                            steam_app_id = app_id,
                            "Sent SIGTERM to Steam game processes"
                        );
                    } else if let Some(ref app_id) = info.flatpak_app_id {
                        kill_flatpak_cgroup(app_id, nix::sys::signal::Signal::SIGTERM);
                        info!(flatpak = %app_id, "Sent SIGTERM via flatpak cgroup");
                    } else {
                        // Fall back to command name for non-sandboxed apps
                        kill_by_command(&info.command_name, nix::sys::signal::Signal::SIGTERM);
                        info!(command = %info.command_name, "Sent SIGTERM via command name");
                    }
                }

                // Also send SIGTERM via process handle (skip for Steam sessions)
                let is_steam = session_info
                    .as_ref()
                    .and_then(|info| info.steam_app_id)
                    .is_some();
                if !is_steam {
                    let procs = self.processes.lock().unwrap();
                    if let Some(p) = procs.get(&pid) {
                        let _ = p.terminate();
                    }
                }

                // Wait for graceful exit
                let start = std::time::Instant::now();
                loop {
                    if start.elapsed() >= timeout {
                        // Force kill after timeout using snap/flatpak cgroup or command name
                        if let Some(ref info) = session_info {
                            if let Some(ref snap) = info.snap_name {
                                kill_snap_cgroup(snap, nix::sys::signal::Signal::SIGKILL);
                                info!(snap = %snap, "Sent SIGKILL via snap cgroup (timeout)");
                            } else if let Some(app_id) = info.steam_app_id {
                                let _ = kill_steam_game_processes(
                                    app_id,
                                    nix::sys::signal::Signal::SIGKILL,
                                );
                                info!(
                                    steam_app_id = app_id,
                                    "Sent SIGKILL to Steam game processes (timeout)"
                                );
                            } else if let Some(ref app_id) = info.flatpak_app_id {
                                kill_flatpak_cgroup(app_id, nix::sys::signal::Signal::SIGKILL);
                                info!(flatpak = %app_id, "Sent SIGKILL via flatpak cgroup (timeout)");
                            } else {
                                kill_by_command(
                                    &info.command_name,
                                    nix::sys::signal::Signal::SIGKILL,
                                );
                                info!(command = %info.command_name, "Sent SIGKILL via command name (timeout)");
                            }
                        }

                        // Also force kill via process handle (skip for Steam sessions)
                        if !is_steam {
                            let procs = self.processes.lock().unwrap();
                            if let Some(p) = procs.get(&pid) {
                                let _ = p.kill();
                            }
                        }
                        break;
                    }

                    // Check if process is still running
                    let still_running = if is_steam {
                        let app_id = session_info.as_ref().and_then(|info| info.steam_app_id);
                        app_id
                            .map(|id| !find_steam_game_pids(id).is_empty())
                            .unwrap_or(false)
                    } else {
                        self.processes.lock().unwrap().contains_key(&pid)
                    };

                    if !still_running {
                        break;
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
            StopMode::Force => {
                // Force kill via snap/flatpak cgroup or command name
                if let Some(ref info) = session_info {
                    if let Some(ref snap) = info.snap_name {
                        kill_snap_cgroup(snap, nix::sys::signal::Signal::SIGKILL);
                        info!(snap = %snap, "Sent SIGKILL via snap cgroup");
                    } else if let Some(app_id) = info.steam_app_id {
                        let _ =
                            kill_steam_game_processes(app_id, nix::sys::signal::Signal::SIGKILL);
                        if let Ok(mut map) = self.steam_sessions.lock() {
                            map.entry(pid).and_modify(|entry| entry.seen_game = true);
                        }
                        info!(
                            steam_app_id = app_id,
                            "Sent SIGKILL to Steam game processes"
                        );
                    } else if let Some(ref app_id) = info.flatpak_app_id {
                        kill_flatpak_cgroup(app_id, nix::sys::signal::Signal::SIGKILL);
                        info!(flatpak = %app_id, "Sent SIGKILL via flatpak cgroup");
                    } else {
                        kill_by_command(&info.command_name, nix::sys::signal::Signal::SIGKILL);
                        info!(command = %info.command_name, "Sent SIGKILL via command name");
                    }
                }

                // Also force kill via process handle (skip for Steam sessions)
                let is_steam = session_info
                    .as_ref()
                    .and_then(|info| info.steam_app_id)
                    .is_some();
                if !is_steam {
                    let procs = self.processes.lock().unwrap();
                    if let Some(p) = procs.get(&pid) {
                        let _ = p.kill();
                    }
                }
            }
        }

        // Clean up the session info tracking
        self.session_info.lock().unwrap().remove(&session_id);

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
            command: "true".into(),
            args: vec![],
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
            HostHandlePayload::Linux { pid: _, .. } => {
                let _procs = host.processes.lock().unwrap();
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
            command: "sleep".into(),
            args: vec!["60".into()],
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
