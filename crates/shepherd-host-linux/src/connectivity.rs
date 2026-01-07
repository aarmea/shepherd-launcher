//! Network connectivity monitoring for Linux
//!
//! This module provides:
//! - Periodic connectivity checks to a configurable URL
//! - Network interface change detection via netlink
//! - Per-entry connectivity status tracking

#![allow(dead_code)] // Methods on ConnectivityMonitor may be used for future admin commands

use chrono::{DateTime, Local};
use netlink_packet_core::{NetlinkMessage, NetlinkPayload};
use netlink_packet_route::RouteNetlinkMessage;
use netlink_sys::{protocols::NETLINK_ROUTE, Socket, SocketAddr};
use reqwest::Client;
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, RwLock};
use tracing::{debug, error, info, warn};

/// Events emitted by the connectivity monitor
#[derive(Debug, Clone)]
pub enum ConnectivityEvent {
    /// Global connectivity status changed
    StatusChanged {
        connected: bool,
        check_url: String,
    },
    /// Network interface changed (may trigger recheck)
    InterfaceChanged,
}

/// Configuration for the connectivity monitor
#[derive(Debug, Clone)]
pub struct ConnectivityConfig {
    /// URL to check for global network connectivity
    pub check_url: String,
    /// How often to perform periodic connectivity checks
    pub check_interval: Duration,
    /// Timeout for connectivity checks
    pub check_timeout: Duration,
}

/// Cached connectivity check result
#[derive(Debug, Clone)]
struct CheckResult {
    connected: bool,
    checked_at: DateTime<Local>,
}

/// Connectivity monitor that tracks network availability
pub struct ConnectivityMonitor {
    /// HTTP client for connectivity checks
    client: Client,
    /// Configuration
    config: ConnectivityConfig,
    /// Current global connectivity status
    global_status: Arc<RwLock<Option<CheckResult>>>,
    /// Cached results for specific URLs (entry-specific checks)
    url_cache: Arc<RwLock<HashMap<String, CheckResult>>>,
    /// Channel for sending events
    event_tx: mpsc::Sender<ConnectivityEvent>,
    /// Shutdown signal
    shutdown_rx: watch::Receiver<bool>,
}

impl ConnectivityMonitor {
    /// Create a new connectivity monitor
    pub fn new(
        config: ConnectivityConfig,
        shutdown_rx: watch::Receiver<bool>,
    ) -> (Self, mpsc::Receiver<ConnectivityEvent>) {
        let (event_tx, event_rx) = mpsc::channel(32);

        let client = Client::builder()
            .timeout(config.check_timeout)
            .connect_timeout(config.check_timeout)
            .build()
            .expect("Failed to create HTTP client");

        let monitor = Self {
            client,
            config,
            global_status: Arc::new(RwLock::new(None)),
            url_cache: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            shutdown_rx,
        };

        (monitor, event_rx)
    }

    /// Start the connectivity monitor (runs until shutdown)
    pub async fn run(self) {
        let check_interval = self.config.check_interval;
        let check_url = self.config.check_url.clone();

        // Spawn periodic check task
        let periodic_handle = {
            let client = self.client.clone();
            let global_status = self.global_status.clone();
            let event_tx = self.event_tx.clone();
            let check_url = check_url.clone();
            let check_timeout = self.config.check_timeout;
            let mut shutdown = self.shutdown_rx.clone();

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(check_interval);
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                // Do initial check immediately
                let connected = check_url_reachable(&client, &check_url, check_timeout).await;
                update_global_status(&global_status, &event_tx, &check_url, connected).await;

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let connected = check_url_reachable(&client, &check_url, check_timeout).await;
                            update_global_status(&global_status, &event_tx, &check_url, connected).await;
                        }
                        _ = shutdown.changed() => {
                            if *shutdown.borrow() {
                                debug!("Periodic check task shutting down");
                                break;
                            }
                        }
                    }
                }
            })
        };

        // Spawn netlink monitor task
        let netlink_handle = {
            let client = self.client.clone();
            let global_status = self.global_status.clone();
            let url_cache = self.url_cache.clone();
            let event_tx = self.event_tx.clone();
            let check_url = check_url.clone();
            let check_timeout = self.config.check_timeout;
            let mut shutdown = self.shutdown_rx.clone();

            tokio::spawn(async move {
                if let Err(e) = run_netlink_monitor(
                    &client,
                    &global_status,
                    &url_cache,
                    &event_tx,
                    &check_url,
                    check_timeout,
                    &mut shutdown,
                )
                .await
                {
                    warn!(error = %e, "Netlink monitor failed, network change detection unavailable");
                }
            })
        };

        // Wait for shutdown
        let mut shutdown = self.shutdown_rx.clone();
        let _ = shutdown.changed().await;

        // Cancel tasks
        periodic_handle.abort();
        netlink_handle.abort();

        info!("Connectivity monitor stopped");
    }

    /// Get the current global connectivity status
    pub async fn is_connected(&self) -> bool {
        self.global_status
            .read()
            .await
            .as_ref()
            .is_some_and(|r| r.connected)
    }

    /// Get the last check time
    pub async fn last_check_time(&self) -> Option<DateTime<Local>> {
        self.global_status.read().await.as_ref().map(|r| r.checked_at)
    }

    /// Check if a specific URL is reachable (with caching)
    /// Used for entry-specific network requirements
    pub async fn check_url(&self, url: &str) -> bool {
        // Check cache first
        {
            let cache = self.url_cache.read().await;
            if let Some(result) = cache.get(url) {
                // Cache valid for half the check interval
                let cache_ttl = self.config.check_interval / 2;
                let age = shepherd_util::now()
                    .signed_duration_since(result.checked_at)
                    .to_std()
                    .unwrap_or(Duration::MAX);
                if age < cache_ttl {
                    return result.connected;
                }
            }
        }

        // Perform check
        let connected = check_url_reachable(&self.client, url, self.config.check_timeout).await;

        // Update cache
        {
            let mut cache = self.url_cache.write().await;
            cache.insert(
                url.to_string(),
                CheckResult {
                    connected,
                    checked_at: shepherd_util::now(),
                },
            );
        }

        connected
    }

    /// Force an immediate connectivity recheck
    pub async fn trigger_recheck(&self) {
        let connected =
            check_url_reachable(&self.client, &self.config.check_url, self.config.check_timeout)
                .await;
        update_global_status(
            &self.global_status,
            &self.event_tx,
            &self.config.check_url,
            connected,
        )
        .await;

        // Clear URL cache to force rechecks
        self.url_cache.write().await.clear();
    }
}

/// Check if a URL is reachable
async fn check_url_reachable(client: &Client, url: &str, timeout: Duration) -> bool {
    debug!(url = %url, "Checking connectivity");

    match client
        .get(url)
        .timeout(timeout)
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            let connected = status.is_success() || status.as_u16() == 204;
            debug!(url = %url, status = %status, connected = connected, "Connectivity check complete");
            connected
        }
        Err(e) => {
            debug!(url = %url, error = %e, "Connectivity check failed");
            false
        }
    }
}

/// Update global status and emit event if changed
async fn update_global_status(
    global_status: &Arc<RwLock<Option<CheckResult>>>,
    event_tx: &mpsc::Sender<ConnectivityEvent>,
    check_url: &str,
    connected: bool,
) {
    let mut status = global_status.write().await;
    let previous = status.as_ref().map(|r| r.connected);

    *status = Some(CheckResult {
        connected,
        checked_at: shepherd_util::now(),
    });

    // Emit event if status changed
    if previous != Some(connected) {
        info!(
            connected = connected,
            url = %check_url,
            "Global connectivity status changed"
        );
        let _ = event_tx
            .send(ConnectivityEvent::StatusChanged {
                connected,
                check_url: check_url.to_string(),
            })
            .await;
    }
}

/// Run the netlink monitor to detect network interface changes
async fn run_netlink_monitor(
    client: &Client,
    global_status: &Arc<RwLock<Option<CheckResult>>>,
    url_cache: &Arc<RwLock<HashMap<String, CheckResult>>>,
    event_tx: &mpsc::Sender<ConnectivityEvent>,
    check_url: &str,
    check_timeout: Duration,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create netlink socket for route notifications
    let mut socket = Socket::new(NETLINK_ROUTE)?;

    // Bind to multicast groups for link and address changes
    // RTMGRP_LINK = 1, RTMGRP_IPV4_IFADDR = 0x10, RTMGRP_IPV6_IFADDR = 0x100
    let groups = 1 | 0x10 | 0x100;
    let addr = SocketAddr::new(0, groups);
    socket.bind(&addr)?;

    // Set non-blocking for async compatibility
    socket.set_non_blocking(true)?;

    info!("Netlink monitor started");

    let fd = socket.as_raw_fd();
    let mut buf = vec![0u8; 4096];

    loop {
        // Use tokio's async fd for the socket
        let async_fd = tokio::io::unix::AsyncFd::new(fd)?;

        tokio::select! {
            result = async_fd.readable() => {
                match result {
                    Ok(mut guard) => {
                        // Try to read from socket
                        match socket.recv(&mut buf, 0) {
                            Ok(len) if len > 0 => {
                                // Parse netlink messages
                                if has_relevant_netlink_event(&buf[..len]) {
                                    debug!("Network interface change detected");
                                    let _ = event_tx.send(ConnectivityEvent::InterfaceChanged).await;

                                    // Clear URL cache
                                    url_cache.write().await.clear();

                                    // Recheck connectivity after a short delay
                                    // (give network time to stabilize)
                                    tokio::time::sleep(Duration::from_millis(500)).await;

                                    let connected = check_url_reachable(client, check_url, check_timeout).await;
                                    update_global_status(global_status, event_tx, check_url, connected).await;
                                }
                                guard.clear_ready();
                            }
                            Ok(_) => {
                                guard.clear_ready();
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                guard.clear_ready();
                            }
                            Err(e) => {
                                error!(error = %e, "Netlink recv error");
                                guard.clear_ready();
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Async fd error");
                    }
                }
            }
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    debug!("Netlink monitor shutting down");
                    break;
                }
            }
        }
    }

    Ok(())
}

/// Check if a netlink message buffer contains relevant network events
fn has_relevant_netlink_event(buf: &[u8]) -> bool {
    let mut offset = 0;

    while offset < buf.len() {
        match NetlinkMessage::<RouteNetlinkMessage>::deserialize(&buf[offset..]) {
            Ok(msg) => {
                if let NetlinkPayload::InnerMessage(route_msg) = &msg.payload
                    && matches!(
                        route_msg,
                        // Link up/down events
                        RouteNetlinkMessage::NewLink(_)
                            | RouteNetlinkMessage::DelLink(_)
                            // Address added/removed
                            | RouteNetlinkMessage::NewAddress(_)
                            | RouteNetlinkMessage::DelAddress(_)
                            // Route changes
                            | RouteNetlinkMessage::NewRoute(_)
                            | RouteNetlinkMessage::DelRoute(_)
                    )
                {
                    return true;
                }

                // Move to next message
                let len = msg.header.length as usize;
                if len == 0 {
                    break;
                }
                offset += len;
            }
            Err(_) => break,
        }
    }

    false
}

/// Handle for accessing connectivity status from other parts of the service
#[derive(Clone)]
pub struct ConnectivityHandle {
    client: Client,
    global_status: Arc<RwLock<Option<CheckResult>>>,
    url_cache: Arc<RwLock<HashMap<String, CheckResult>>>,
    check_timeout: Duration,
    cache_ttl: Duration,
    global_check_url: String,
}

impl ConnectivityHandle {
    /// Create a handle from the monitor
    pub fn from_monitor(monitor: &ConnectivityMonitor) -> Self {
        Self {
            client: monitor.client.clone(),
            global_status: monitor.global_status.clone(),
            url_cache: monitor.url_cache.clone(),
            check_timeout: monitor.config.check_timeout,
            cache_ttl: monitor.config.check_interval / 2,
            global_check_url: monitor.config.check_url.clone(),
        }
    }

    /// Get the current global connectivity status
    pub async fn is_connected(&self) -> bool {
        self.global_status
            .read()
            .await
            .as_ref()
            .is_some_and(|r| r.connected)
    }

    /// Get the last check time
    pub async fn last_check_time(&self) -> Option<DateTime<Local>> {
        self.global_status.read().await.as_ref().map(|r| r.checked_at)
    }

    /// Get the global check URL
    pub fn global_check_url(&self) -> &str {
        &self.global_check_url
    }

    /// Check if a specific URL is reachable (with caching)
    pub async fn check_url(&self, url: &str) -> bool {
        // If it's the global URL, use global status
        if url == self.global_check_url {
            return self.is_connected().await;
        }

        // Check cache first
        {
            let cache = self.url_cache.read().await;
            if let Some(result) = cache.get(url) {
                let age = shepherd_util::now()
                    .signed_duration_since(result.checked_at)
                    .to_std()
                    .unwrap_or(Duration::MAX);
                if age < self.cache_ttl {
                    return result.connected;
                }
            }
        }

        // Perform check
        let connected = check_url_reachable(&self.client, url, self.check_timeout).await;

        // Update cache
        {
            let mut cache = self.url_cache.write().await;
            cache.insert(
                url.to_string(),
                CheckResult {
                    connected,
                    checked_at: shepherd_util::now(),
                },
            );
        }

        connected
    }
}
