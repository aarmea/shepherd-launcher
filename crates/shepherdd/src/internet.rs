//! Internet connectivity monitoring for shepherdd.

use shepherd_config::{InternetCheckScheme, InternetCheckTarget, Policy};
use shepherd_core::CoreEngine;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time;
use tracing::{debug, warn};

pub struct InternetMonitor {
    targets: Vec<InternetCheckTarget>,
    interval: Duration,
    timeout: Duration,
}

impl InternetMonitor {
    pub fn from_policy(policy: &Policy) -> Option<Self> {
        let mut targets = Vec::new();

        if let Some(check) = policy.service.internet.check.clone() {
            targets.push(check);
        }

        for entry in &policy.entries {
            if entry.internet.required
                && let Some(check) = entry.internet.check.clone()
                && !targets.contains(&check)
            {
                targets.push(check);
            }
        }

        if targets.is_empty() {
            return None;
        }

        Some(Self {
            targets,
            interval: policy.service.internet.interval,
            timeout: policy.service.internet.timeout,
        })
    }

    pub async fn run(self, engine: Arc<Mutex<CoreEngine>>) {
        // Initial check
        self.check_all(&engine).await;

        let mut interval = time::interval(self.interval);
        loop {
            interval.tick().await;
            self.check_all(&engine).await;
        }
    }

    async fn check_all(&self, engine: &Arc<Mutex<CoreEngine>>) {
        for target in &self.targets {
            let available = check_target(target, self.timeout).await;
            let changed = {
                let mut eng = engine.lock().await;
                eng.set_internet_status(target.clone(), available)
            };

            if changed {
                debug!(
                    check = %target.original,
                    available,
                    "Internet connectivity status changed"
                );
            }
        }
    }
}

async fn check_target(target: &InternetCheckTarget, timeout: Duration) -> bool {
    match target.scheme {
        InternetCheckScheme::Tcp | InternetCheckScheme::Http | InternetCheckScheme::Https => {
            let connect = TcpStream::connect((target.host.as_str(), target.port));
            match time::timeout(timeout, connect).await {
                Ok(Ok(stream)) => {
                    drop(stream);
                    true
                }
                Ok(Err(err)) => {
                    debug!(
                        check = %target.original,
                        error = %err,
                        "Internet check failed"
                    );
                    false
                }
                Err(_) => {
                    warn!(check = %target.original, "Internet check timed out");
                    false
                }
            }
        }
    }
}
