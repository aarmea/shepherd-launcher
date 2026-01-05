//! shepherdd - The shepherd background service
//!
//! This is the main entry point for the shepherdd service.
//! It wires together all the components:
//! - Configuration loading
//! - Store initialization
//! - Core engine
//! - Host adapter (Linux)
//! - IPC server
//! - Volume control

use anyhow::{Context, Result};
use clap::Parser;
use shepherd_api::{
    Command, ErrorCode, ErrorInfo, Event, EventPayload, HealthStatus,
    Response, ResponsePayload, SessionEndReason, StopMode, VolumeInfo, VolumeRestrictions,
};
use shepherd_config::{load_config, VolumePolicy};
use shepherd_core::{CoreEngine, CoreEvent, LaunchDecision, StopDecision};
use shepherd_host_api::{HostAdapter, HostEvent, StopMode as HostStopMode, VolumeController};
use shepherd_host_linux::{LinuxHost, LinuxVolumeController};
use shepherd_ipc::{IpcServer, ServerMessage};
use shepherd_store::{AuditEvent, AuditEventType, SqliteStore, Store};
use shepherd_util::{default_config_path, ClientId, MonotonicInstant, RateLimiter};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use tracing_subscriber::EnvFilter;

/// shepherdd - Policy enforcement service for child-focused computing
#[derive(Parser, Debug)]
#[command(name = "shepherdd")]
#[command(about = "Policy enforcement service for child-focused computing", long_about = None)]
struct Args {
    /// Configuration file path (default: ~/.config/shepherd/config.toml)
    #[arg(short, long, default_value_os_t = default_config_path())]
    config: PathBuf,

    /// Socket path override (or set SHEPHERD_SOCKET env var)
    #[arg(short, long, env = "SHEPHERD_SOCKET")]
    socket: Option<PathBuf>,

    /// Data directory override (or set SHEPHERD_DATA_DIR env var)
    #[arg(short, long, env = "SHEPHERD_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

/// Main service state
struct Service {
    engine: CoreEngine,
    host: Arc<LinuxHost>,
    volume: Arc<LinuxVolumeController>,
    ipc: Arc<IpcServer>,
    store: Arc<dyn Store>,
    rate_limiter: RateLimiter,
}

impl Service {
    async fn new(args: &Args) -> Result<Self> {
        // Load configuration
        let policy = load_config(&args.config)
            .with_context(|| format!("Failed to load config from {:?}", args.config))?;

        info!(
            config_path = %args.config.display(),
            entry_count = policy.entries.len(),
            "Configuration loaded"
        );

        // Determine paths
        let socket_path = args
            .socket
            .clone()
            .unwrap_or_else(|| policy.service.socket_path.clone());

        let data_dir = args
            .data_dir
            .clone()
            .unwrap_or_else(|| policy.service.data_dir.clone());

        // Create data directory
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("Failed to create data directory {:?}", data_dir))?;

        // Initialize store
        let db_path = data_dir.join("shepherdd.db");
        let store: Arc<dyn Store> = Arc::new(
            SqliteStore::open(&db_path)
                .with_context(|| format!("Failed to open database {:?}", db_path))?,
        );

        info!(db_path = %db_path.display(), "Store initialized");

        // Log service start
        store.append_audit(AuditEvent::new(AuditEventType::ServiceStarted))?;

        // Initialize host adapter
        let host = Arc::new(LinuxHost::new());

        // Initialize volume controller
        let volume = Arc::new(LinuxVolumeController::new());
        if volume.capabilities().available {
            info!(
                backend = ?volume.capabilities().backend,
                "Volume controller initialized"
            );
        } else {
            warn!("No sound backend detected, volume control unavailable");
        }

        // Initialize core engine
        let engine = CoreEngine::new(policy, store.clone(), host.capabilities().clone());

        // Initialize IPC server
        let mut ipc = IpcServer::new(&socket_path);
        ipc.start().await?;

        info!(socket_path = %socket_path.display(), "IPC server started");

        // Rate limiter: 30 requests per second per client
        let rate_limiter = RateLimiter::new(30, Duration::from_secs(1));

        Ok(Self {
            engine,
            host,
            volume,
            ipc: Arc::new(ipc),
            store,
            rate_limiter,
        })
    }

    async fn run(self) -> Result<()> {
        // Start host process monitor
        let _monitor_handle = self.host.start_monitor();

        // Get channels
        let mut host_events = self.host.subscribe();
        let ipc_ref = self.ipc.clone();
        let mut ipc_messages = ipc_ref
            .take_message_receiver()
            .await
            .expect("Message receiver should be available");

        // Wrap mutable state
        let engine = Arc::new(Mutex::new(self.engine));
        let rate_limiter = Arc::new(Mutex::new(self.rate_limiter));
        let host = self.host.clone();
        let volume = self.volume.clone();
        let store = self.store.clone();

        // Spawn IPC accept task
        let ipc_accept = ipc_ref.clone();
        tokio::spawn(async move {
            if let Err(e) = ipc_accept.run().await {
                error!(error = %e, "IPC server error");
            }
        });

        // Set up signal handlers
        let mut sigterm = signal(SignalKind::terminate())
            .context("Failed to create SIGTERM handler")?;
        let mut sigint = signal(SignalKind::interrupt())
            .context("Failed to create SIGINT handler")?;
        let mut sighup = signal(SignalKind::hangup())
            .context("Failed to create SIGHUP handler")?;

        // Main event loop
        let tick_interval = Duration::from_millis(100);
        let mut tick_timer = tokio::time::interval(tick_interval);

        info!("Service running");

        loop {
            tokio::select! {
                // Signal: SIGTERM or SIGINT - graceful shutdown
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, shutting down gracefully");
                    break;
                }
                _ = sigint.recv() => {
                    info!("Received SIGINT, shutting down gracefully");
                    break;
                }

                // Signal: SIGHUP - graceful shutdown (sent by sway on exit)
                _ = sighup.recv() => {
                    info!("Received SIGHUP, shutting down gracefully");
                    break;
                }

                // Tick timer - check warnings and expiry
                _ = tick_timer.tick() => {
                    let now_mono = MonotonicInstant::now();
                    let now = shepherd_util::now();

                    let events = {
                        let mut engine = engine.lock().await;
                        engine.tick(now_mono, now)
                    };

                    for event in events {
                        Self::handle_core_event(&engine, &host, &ipc_ref, event, now_mono, now).await;
                    }
                }

                // Host events (process exit)
                Some(host_event) = host_events.recv() => {
                    Self::handle_host_event(&engine, &ipc_ref, host_event).await;
                }

                // IPC messages
                Some(msg) = ipc_messages.recv() => {
                    Self::handle_ipc_message(&engine, &host, &volume, &ipc_ref, &store, &rate_limiter, msg).await;
                }
            }
        }

        // Graceful shutdown
        info!("Shutting down shepherdd");

        // Stop all running sessions
        {
            let engine = engine.lock().await;
            if let Some(session) = engine.current_session() {
                info!(session_id = %session.plan.session_id, "Stopping active session");
                if let Some(handle) = &session.host_handle && let Err(e) = host.stop(handle, HostStopMode::Graceful {
                    timeout: Duration::from_secs(5),
                }).await {
                    warn!(error = %e, "Failed to stop session gracefully");
                }
            }
        }

        // Log shutdown
        if let Err(e) = store.append_audit(AuditEvent::new(AuditEventType::ServiceStopped)) {
            warn!(error = %e, "Failed to log service shutdown");
        }

        info!("Shutdown complete");
        Ok(())
    }

    async fn handle_core_event(
        engine: &Arc<Mutex<CoreEngine>>,
        host: &Arc<LinuxHost>,
        ipc: &Arc<IpcServer>,
        event: CoreEvent,
        _now_mono: MonotonicInstant,
        _now: chrono::DateTime<chrono::Local>,
    ) {
        match &event {
            CoreEvent::Warning {
                session_id,
                threshold_seconds,
                time_remaining,
                severity,
                message,
            } => {
                info!(
                    session_id = %session_id,
                    threshold = threshold_seconds,
                    remaining = ?time_remaining,
                    "Warning issued"
                );

                ipc.broadcast_event(Event::new(EventPayload::WarningIssued {
                    session_id: session_id.clone(),
                    threshold_seconds: *threshold_seconds,
                    time_remaining: *time_remaining,
                    severity: *severity,
                    message: message.clone(),
                }));
            }

            CoreEvent::ExpireDue { session_id } => {
                info!(session_id = %session_id, "Session expired, stopping");

                // Get the host handle and stop it
                let handle = {
                    let engine = engine.lock().await;
                    engine
                        .current_session()
                        .and_then(|s| s.host_handle.clone())
                };

                if let Some(handle) = handle
                    && let Err(e) = host
                        .stop(
                            &handle,
                            HostStopMode::Graceful {
                                timeout: Duration::from_secs(5),
                            },
                        )
                        .await
                    {
                        warn!(error = %e, "Failed to stop session gracefully, forcing");
                        let _ = host.stop(&handle, HostStopMode::Force).await;
                    }

                ipc.broadcast_event(Event::new(EventPayload::SessionExpiring {
                    session_id: session_id.clone(),
                }));
            }

            CoreEvent::SessionStarted {
                session_id,
                entry_id,
                label,
                deadline,
            } => {
                ipc.broadcast_event(Event::new(EventPayload::SessionStarted {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                    label: label.clone(),
                    deadline: *deadline,
                }));
            }

            CoreEvent::SessionEnded {
                session_id,
                entry_id,
                reason,
                duration,
            } => {
                ipc.broadcast_event(Event::new(EventPayload::SessionEnded {
                    session_id: session_id.clone(),
                    entry_id: entry_id.clone(),
                    reason: reason.clone(),
                    duration: *duration,
                }));

                // Broadcast state change
                let state = {
                    let engine = engine.lock().await;
                    engine.get_state()
                };
                ipc.broadcast_event(Event::new(EventPayload::StateChanged(state)));
            }

            CoreEvent::PolicyReloaded { entry_count } => {
                ipc.broadcast_event(Event::new(EventPayload::PolicyReloaded {
                    entry_count: *entry_count,
                }));
            }

            CoreEvent::EntryAvailabilityChanged { entry_id, enabled } => {
                ipc.broadcast_event(Event::new(EventPayload::EntryAvailabilityChanged {
                    entry_id: entry_id.clone(),
                    enabled: *enabled,
                }));
            }

            CoreEvent::AvailabilitySetChanged => {
                // Time-based availability change - broadcast updated state
                let state = {
                    let engine = engine.lock().await;
                    engine.get_state()
                };
                ipc.broadcast_event(Event::new(EventPayload::StateChanged(state)));
            }
        }
    }

    async fn handle_host_event(
        engine: &Arc<Mutex<CoreEngine>>,
        ipc: &Arc<IpcServer>,
        event: HostEvent,
    ) {
        match event {
            HostEvent::Exited { handle, status } => {
                let now_mono = MonotonicInstant::now();
                let now = shepherd_util::now();

                info!(
                    session_id = %handle.session_id,
                    status = ?status,
                    "Host process exited - will end session"
                );

                let core_event = {
                    let mut engine = engine.lock().await;
                    engine.notify_session_exited(status.code, now_mono, now)
                };

                info!(has_event = core_event.is_some(), "notify_session_exited result");

                if let Some(CoreEvent::SessionEnded {
                    session_id,
                    entry_id,
                    reason,
                    duration,
                }) = core_event
                    {
                        info!(
                            session_id = %session_id,
                            entry_id = %entry_id,
                            reason = ?reason,
                            duration_secs = duration.as_secs(),
                            "Broadcasting SessionEnded"
                        );
                        ipc.broadcast_event(Event::new(EventPayload::SessionEnded {
                            session_id,
                            entry_id,
                            reason,
                            duration,
                        }));

                        // Broadcast state change
                        let state = {
                            let engine = engine.lock().await;
                            engine.get_state()
                        };
                        info!("Broadcasting StateChanged");
                        ipc.broadcast_event(Event::new(EventPayload::StateChanged(state)));
                    }
            }

            HostEvent::WindowReady { handle } => {
                debug!(session_id = %handle.session_id, "Window ready");
            }

            HostEvent::SpawnFailed { session_id, error } => {
                error!(session_id = %session_id, error = %error, "Spawn failed");
            }
        }
    }

    async fn handle_ipc_message(
        engine: &Arc<Mutex<CoreEngine>>,
        host: &Arc<LinuxHost>,
        volume: &Arc<LinuxVolumeController>,
        ipc: &Arc<IpcServer>,
        store: &Arc<dyn Store>,
        rate_limiter: &Arc<Mutex<RateLimiter>>,
        msg: ServerMessage,
    ) {
        match msg {
            ServerMessage::Request { client_id, request } => {
                // Rate limiting
                {
                    let mut limiter = rate_limiter.lock().await;
                    if !limiter.check(&client_id) {
                        let response = Response::error(
                            request.request_id,
                            ErrorInfo::new(ErrorCode::RateLimited, "Too many requests"),
                        );
                        let _ = ipc.send_response(&client_id, response).await;
                        return;
                    }
                }

                let response =
                    Self::handle_command(engine, host, volume, ipc, store, &client_id, request.request_id, request.command)
                        .await;

                let _ = ipc.send_response(&client_id, response).await;
            }

            ServerMessage::ClientConnected { client_id, info } => {
                info!(
                    client_id = %client_id,
                    role = ?info.role,
                    uid = ?info.uid,
                    "Client connected"
                );

                let _ = store.append_audit(AuditEvent::new(
                    AuditEventType::ClientConnected {
                        client_id: client_id.to_string(),
                        role: format!("{:?}", info.role),
                        uid: info.uid,
                    },
                ));
            }

            ServerMessage::ClientDisconnected { client_id } => {
                debug!(client_id = %client_id, "Client disconnected");

                let _ = store.append_audit(AuditEvent::new(
                    AuditEventType::ClientDisconnected {
                        client_id: client_id.to_string(),
                    },
                ));

                // Clean up rate limiter
                let mut limiter = rate_limiter.lock().await;
                limiter.remove_client(&client_id);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_command(
        engine: &Arc<Mutex<CoreEngine>>,
        host: &Arc<LinuxHost>,
        volume: &Arc<LinuxVolumeController>,
        ipc: &Arc<IpcServer>,
        store: &Arc<dyn Store>,
        client_id: &ClientId,
        request_id: u64,
        command: Command,
    ) -> Response {
        let now = shepherd_util::now();
        let now_mono = MonotonicInstant::now();

        match command {
            Command::GetState => {
                let state = engine.lock().await.get_state();
                Response::success(request_id, ResponsePayload::State(state))
            }

            Command::ListEntries { at_time } => {
                let time = at_time.unwrap_or(now);
                let entries = engine.lock().await.list_entries(time);
                Response::success(request_id, ResponsePayload::Entries(entries))
            }

            Command::Launch { entry_id } => {
                let mut eng = engine.lock().await;

                match eng.request_launch(&entry_id, now) {
                    LaunchDecision::Approved(plan) => {
                        // Start the session in the engine
                        let event = eng.start_session(plan.clone(), now, now_mono);

                        // Get the entry kind for spawning
                        let entry_kind = eng
                            .policy()
                            .get_entry(&entry_id)
                            .map(|e| e.kind.clone());

                        // Build spawn options with log path if capture_child_output is enabled
                        let spawn_options = if eng.policy().service.capture_child_output {
                            let log_dir = &eng.policy().service.child_log_dir;
                            // Create log filename: <entry_id>_<session_id>_<timestamp>.log
                            let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
                            let log_filename = format!(
                                "{}_{}.log",
                                entry_id.as_str().replace(['/', '\\', ' '], "_"),
                                timestamp
                            );
                            let log_path = log_dir.join(log_filename);
                            shepherd_host_api::SpawnOptions {
                                capture_stdout: true,
                                capture_stderr: true,
                                log_path: Some(log_path),
                                ..Default::default()
                            }
                        } else {
                            shepherd_host_api::SpawnOptions::default()
                        };

                        drop(eng); // Release lock before spawning

                        if let Some(kind) = entry_kind {
                            match host
                                .spawn(
                                    plan.session_id.clone(),
                                    &kind,
                                    spawn_options,
                                )
                                .await
                            {
                                Ok(handle) => {
                                    // Attach handle to session
                                    let mut eng = engine.lock().await;
                                    eng.attach_host_handle(handle);

                                    // Broadcast session started
                                    if let CoreEvent::SessionStarted {
                                        session_id,
                                        entry_id,
                                        label,
                                        deadline,
                                    } = event
                                    {
                                        ipc.broadcast_event(Event::new(EventPayload::SessionStarted {
                                            session_id: session_id.clone(),
                                            entry_id,
                                            label,
                                            deadline,
                                        }));

                                        Response::success(
                                            request_id,
                                            ResponsePayload::LaunchApproved {
                                                session_id,
                                                deadline,
                                            },
                                        )
                                    } else {
                                        Response::error(
                                            request_id,
                                            ErrorInfo::new(ErrorCode::InternalError, "Unexpected event"),
                                        )
                                    }
                                }
                                Err(e) => {
                                    // Notify session ended with error and broadcast to subscribers
                                    let mut eng = engine.lock().await;
                                    if let Some(CoreEvent::SessionEnded {
                                        session_id,
                                        entry_id,
                                        reason,
                                        duration,
                                    }) = eng.notify_session_exited(Some(-1), now_mono, now)
                                        {
                                            ipc.broadcast_event(Event::new(EventPayload::SessionEnded {
                                                session_id,
                                                entry_id,
                                                reason,
                                                duration,
                                            }));

                                            // Broadcast state change so clients return to idle
                                            let state = eng.get_state();
                                            ipc.broadcast_event(Event::new(EventPayload::StateChanged(state)));
                                        }

                                    Response::error(
                                        request_id,
                                        ErrorInfo::new(
                                            ErrorCode::HostError,
                                            format!("Spawn failed: {}", e),
                                        ),
                                    )
                                }
                            }
                        } else {
                            Response::error(
                                request_id,
                                ErrorInfo::new(ErrorCode::EntryNotFound, "Entry not found"),
                            )
                        }
                    }
                    LaunchDecision::Denied { reasons } => {
                        Response::success(request_id, ResponsePayload::LaunchDenied { reasons })
                    }
                }
            }

            Command::StopCurrent { mode } => {
                let mut eng = engine.lock().await;

                // Get handle before stopping in engine
                let handle = eng
                    .current_session()
                    .and_then(|s| s.host_handle.clone());

                let reason = match mode {
                    StopMode::Graceful => SessionEndReason::UserStop,
                    StopMode::Force => SessionEndReason::AdminStop,
                };

                match eng.stop_current(reason.clone(), now_mono, now) {
                    StopDecision::Stopped(result) => {
                        // Broadcast SessionEnded event so UIs know to transition
                        info!(
                            session_id = %result.session_id,
                            reason = ?result.reason,
                            "Broadcasting SessionEnded from StopCurrent"
                        );
                        ipc.broadcast_event(Event::new(EventPayload::SessionEnded {
                            session_id: result.session_id,
                            entry_id: result.entry_id,
                            reason: result.reason,
                            duration: result.duration,
                        }));

                        // Also broadcast StateChanged so UIs can update their entry list
                        let snapshot = eng.get_state();
                        ipc.broadcast_event(Event::new(EventPayload::StateChanged(snapshot)));

                        drop(eng); // Release lock before host operations

                        // Stop the actual process
                        if let Some(h) = handle {
                            let host_mode = match mode {
                                StopMode::Graceful => HostStopMode::Graceful {
                                    timeout: Duration::from_secs(5),
                                },
                                StopMode::Force => HostStopMode::Force,
                            };
                            let _ = host.stop(&h, host_mode).await;
                        }

                        Response::success(request_id, ResponsePayload::Stopped)
                    }
                    StopDecision::NoActiveSession => Response::error(
                        request_id,
                        ErrorInfo::new(ErrorCode::NoActiveSession, "No active session"),
                    ),
                }
            }

            Command::ReloadConfig => {
                // Check permission
                if let Some(info) = ipc.get_client_info(client_id).await
                    && !info.role.can_reload_config() {
                        return Response::error(
                            request_id,
                            ErrorInfo::new(ErrorCode::PermissionDenied, "Admin role required"),
                        );
                    }

                // TODO: Reload from original config path
                Response::error(
                    request_id,
                    ErrorInfo::new(ErrorCode::InternalError, "Reload not yet implemented"),
                )
            }

            Command::SubscribeEvents => {
                Response::success(
                    request_id,
                    ResponsePayload::Subscribed {
                        client_id: client_id.clone(),
                    },
                )
            }

            Command::UnsubscribeEvents => {
                Response::success(request_id, ResponsePayload::Unsubscribed)
            }

            Command::GetHealth => {
                let _eng = engine.lock().await;
                let health = HealthStatus {
                    live: true,
                    ready: true,
                    policy_loaded: true,
                    host_adapter_ok: host.is_healthy(),
                    store_ok: store.is_healthy(),
                };
                Response::success(request_id, ResponsePayload::Health(health))
            }

            Command::ExtendCurrent { by } => {
                // Check permission
                if let Some(info) = ipc.get_client_info(client_id).await
                    && !info.role.can_extend() {
                        return Response::error(
                            request_id,
                            ErrorInfo::new(ErrorCode::PermissionDenied, "Admin role required"),
                        );
                    }

                let mut eng = engine.lock().await;
                match eng.extend_current(by, now_mono, now) {
                    Some(new_deadline) => {
                        Response::success(request_id, ResponsePayload::Extended { new_deadline: Some(new_deadline) })
                    }
                    None => Response::error(
                        request_id,
                        ErrorInfo::new(ErrorCode::NoActiveSession, "No active session or session is unlimited"),
                    ),
                }
            }

            Command::GetVolume => {
                let restrictions = Self::get_current_volume_restrictions(engine).await;

                match volume.get_status().await {
                    Ok(status) => {
                        let info = VolumeInfo {
                            percent: status.percent,
                            muted: status.muted,
                            available: volume.capabilities().available,
                            backend: volume.capabilities().backend.clone(),
                            restrictions,
                        };
                        Response::success(request_id, ResponsePayload::Volume(info))
                    }
                    Err(e) => {
                        let info = VolumeInfo {
                            percent: 0,
                            muted: false,
                            available: false,
                            backend: None,
                            restrictions,
                        };
                        warn!(error = %e, "Failed to get volume status");
                        Response::success(request_id, ResponsePayload::Volume(info))
                    }
                }
            }

            Command::SetVolume { percent } => {
                let restrictions = Self::get_current_volume_restrictions(engine).await;

                if !restrictions.allow_change {
                    return Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: "Volume changes are not allowed".into(),
                        },
                    );
                }

                let clamped = restrictions.clamp_volume(percent);

                match volume.set_volume(clamped).await {
                    Ok(()) => {
                        // Broadcast volume change
                        if let Ok(status) = volume.get_status().await {
                            ipc.broadcast_event(Event::new(EventPayload::VolumeChanged {
                                percent: status.percent,
                                muted: status.muted,
                            }));
                        }
                        Response::success(request_id, ResponsePayload::VolumeSet)
                    }
                    Err(e) => Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: e.to_string(),
                        },
                    ),
                }
            }

            Command::ToggleMute => {
                let restrictions = Self::get_current_volume_restrictions(engine).await;

                if !restrictions.allow_mute {
                    return Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: "Mute toggle is not allowed".into(),
                        },
                    );
                }

                match volume.toggle_mute().await {
                    Ok(()) => {
                        if let Ok(status) = volume.get_status().await {
                            ipc.broadcast_event(Event::new(EventPayload::VolumeChanged {
                                percent: status.percent,
                                muted: status.muted,
                            }));
                        }
                        Response::success(request_id, ResponsePayload::VolumeSet)
                    }
                    Err(e) => Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: e.to_string(),
                        },
                    ),
                }
            }

            Command::SetMute { muted } => {
                let restrictions = Self::get_current_volume_restrictions(engine).await;

                if !restrictions.allow_mute {
                    return Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: "Mute toggle is not allowed".into(),
                        },
                    );
                }

                match volume.set_mute(muted).await {
                    Ok(()) => {
                        if let Ok(status) = volume.get_status().await {
                            ipc.broadcast_event(Event::new(EventPayload::VolumeChanged {
                                percent: status.percent,
                                muted: status.muted,
                            }));
                        }
                        Response::success(request_id, ResponsePayload::VolumeSet)
                    }
                    Err(e) => Response::success(
                        request_id,
                        ResponsePayload::VolumeDenied {
                            reason: e.to_string(),
                        },
                    ),
                }
            }

            Command::Ping => Response::success(request_id, ResponsePayload::Pong),
        }
    }

    /// Get the current volume restrictions based on policy and active session
    async fn get_current_volume_restrictions(
        engine: &Arc<Mutex<CoreEngine>>,
    ) -> VolumeRestrictions {
        let eng = engine.lock().await;
        
        // Check if there's an active session with volume restrictions
        if let Some(session) = eng.current_session()
            && let Some(entry) = eng.policy().get_entry(&session.plan.entry_id)
            && let Some(ref vol_policy) = entry.volume {
                return Self::convert_volume_policy(vol_policy);
            }
        
        // Fall back to global policy
        Self::convert_volume_policy(&eng.policy().volume)
    }

    fn convert_volume_policy(policy: &VolumePolicy) -> VolumeRestrictions {
        VolumeRestrictions {
            max_volume: policy.max_volume,
            min_volume: policy.min_volume,
            allow_mute: policy.allow_mute,
            allow_change: policy.allow_change,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&args.log_level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "shepherdd starting"
    );

    // Create and run the service
    let service = Service::new(&args).await?;
    service.run().await
}
