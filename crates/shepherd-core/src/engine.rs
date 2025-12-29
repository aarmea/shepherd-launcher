//! Core policy engine

use chrono::{DateTime, Local};
use shepherd_api::{
    ServiceStateSnapshot, EntryKindTag, EntryView, ReasonCode, SessionEndReason,
    WarningSeverity, API_VERSION,
};
use shepherd_config::{Entry, Policy};
use shepherd_host_api::{HostCapabilities, HostSessionHandle};
use shepherd_store::{AuditEvent, AuditEventType, Store};
use shepherd_util::{EntryId, MonotonicInstant, SessionId};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::{ActiveSession, CoreEvent, SessionPlan, StopResult};

/// Launch decision from the core engine
#[derive(Debug)]
pub enum LaunchDecision {
    Approved(SessionPlan),
    Denied { reasons: Vec<ReasonCode> },
}

/// Stop decision from the core engine
#[derive(Debug)]
pub enum StopDecision {
    Stopped(StopResult),
    NoActiveSession,
}

/// The core policy engine
pub struct CoreEngine {
    policy: Policy,
    store: Arc<dyn Store>,
    capabilities: HostCapabilities,
    current_session: Option<ActiveSession>,
}

impl CoreEngine {
    /// Create a new core engine
    pub fn new(
        policy: Policy,
        store: Arc<dyn Store>,
        capabilities: HostCapabilities,
    ) -> Self {
        info!(
            entry_count = policy.entries.len(),
            "Core engine initialized"
        );

        // Log policy load
        let _ = store.append_audit(AuditEvent::new(AuditEventType::PolicyLoaded {
            entry_count: policy.entries.len(),
        }));

        Self {
            policy,
            store,
            capabilities,
            current_session: None,
        }
    }

    /// Get current policy
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Reload policy
    pub fn reload_policy(&mut self, policy: Policy) -> CoreEvent {
        let entry_count = policy.entries.len();
        self.policy = policy;

        let _ = self.store.append_audit(AuditEvent::new(AuditEventType::PolicyLoaded {
            entry_count,
        }));

        info!(entry_count, "Policy reloaded");

        CoreEvent::PolicyReloaded { entry_count }
    }

    /// List all entries with availability status
    pub fn list_entries(&self, now: DateTime<Local>) -> Vec<EntryView> {
        self.policy
            .entries
            .iter()
            .map(|entry| self.evaluate_entry(entry, now))
            .collect()
    }

    /// Evaluate a single entry for availability
    fn evaluate_entry(&self, entry: &Entry, now: DateTime<Local>) -> EntryView {
        let mut reasons = Vec::new();
        let mut enabled = true;

        // Check if explicitly disabled
        if entry.disabled {
            enabled = false;
            reasons.push(ReasonCode::Disabled {
                reason: entry.disabled_reason.clone(),
            });
        }

        // Check host capabilities
        let kind_tag = entry.kind.tag();
        if !self.capabilities.supports_kind(kind_tag) {
            enabled = false;
            reasons.push(ReasonCode::UnsupportedKind { kind: kind_tag });
        }

        // Check availability window
        if !entry.availability.is_available(&now) {
            enabled = false;
            reasons.push(ReasonCode::OutsideTimeWindow {
                next_window_start: None, // TODO: compute next window
            });
        }

        // Check if another session is active
        if let Some(session) = &self.current_session {
            enabled = false;
            reasons.push(ReasonCode::SessionActive {
                entry_id: session.plan.entry_id.clone(),
                remaining: session.time_remaining(MonotonicInstant::now()),
            });
        }

        // Check cooldown
        if let Ok(Some(until)) = self.store.get_cooldown_until(&entry.id) {
            if until > now {
                enabled = false;
                reasons.push(ReasonCode::CooldownActive { available_at: until });
            }
        }

        // Check daily quota
        if let Some(quota) = entry.limits.daily_quota {
            let today = now.date_naive();
            if let Ok(used) = self.store.get_usage(&entry.id, today) {
                if used >= quota {
                    enabled = false;
                    reasons.push(ReasonCode::QuotaExhausted { used, quota });
                }
            }
        }

        // Calculate max run if enabled (None when disabled, Some(None) flattened for unlimited)
        let max_run_if_started_now = if enabled {
            self.compute_max_duration(entry, now)
        } else {
            None
        };

        EntryView {
            entry_id: entry.id.clone(),
            label: entry.label.clone(),
            icon_ref: entry.icon_ref.clone(),
            kind_tag,
            enabled,
            reasons,
            max_run_if_started_now,
        }
    }

    /// Compute maximum duration for an entry if started now.
    /// Returns None if the entry has no time limit (unlimited).
    fn compute_max_duration(&self, entry: &Entry, now: DateTime<Local>) -> Option<Duration> {
        let mut max = entry.limits.max_run;

        // Limit by time window remaining
        if let Some(window_remaining) = entry.availability.remaining_in_window(&now) {
            max = Some(match max {
                Some(m) => m.min(window_remaining),
                None => window_remaining,
            });
        }

        // Limit by daily quota remaining
        if let Some(quota) = entry.limits.daily_quota {
            let today = now.date_naive();
            if let Ok(used) = self.store.get_usage(&entry.id, today) {
                let remaining = quota.saturating_sub(used);
                max = Some(match max {
                    Some(m) => m.min(remaining),
                    None => remaining,
                });
            }
        }

        max
    }

    /// Request to launch an entry
    pub fn request_launch(
        &self,
        entry_id: &EntryId,
        now: DateTime<Local>,
    ) -> LaunchDecision {
        // Find entry
        let entry = match self.policy.get_entry(entry_id) {
            Some(e) => e,
            None => {
                return LaunchDecision::Denied {
                    reasons: vec![ReasonCode::Disabled {
                        reason: Some("Entry not found".into()),
                    }],
                };
            }
        };

        // Evaluate availability
        let view = self.evaluate_entry(entry, now);

        if !view.enabled {
            // Log denial
            let _ = self.store.append_audit(AuditEvent::new(AuditEventType::LaunchDenied {
                entry_id: entry_id.clone(),
                reasons: view.reasons.iter().map(|r| format!("{:?}", r)).collect(),
            }));

            return LaunchDecision::Denied {
                reasons: view.reasons,
            };
        }

        // Compute session plan
        let max_duration = view.max_run_if_started_now;
        let plan = SessionPlan {
            session_id: SessionId::new(),
            entry_id: entry_id.clone(),
            label: entry.label.clone(),
            max_duration,
            warnings: entry.warnings.clone(),
        };

        if let Some(max_dur) = max_duration {
            debug!(
                entry_id = %entry_id,
                max_duration_secs = max_dur.as_secs(),
                "Launch approved"
            );
        } else {
            debug!(
                entry_id = %entry_id,
                "Launch approved (unlimited)"
            );
        }

        LaunchDecision::Approved(plan)
    }

    /// Start a session from an approved plan
    pub fn start_session(
        &mut self,
        plan: SessionPlan,
        now: DateTime<Local>,
        now_mono: MonotonicInstant,
    ) -> CoreEvent {
        let session = ActiveSession::new(plan.clone(), now, now_mono);

        let event = CoreEvent::SessionStarted {
            session_id: session.plan.session_id.clone(),
            entry_id: session.plan.entry_id.clone(),
            label: session.plan.label.clone(),
            deadline: session.deadline,
        };

        // Log to audit
        let _ = self.store.append_audit(AuditEvent::new(AuditEventType::SessionStarted {
            session_id: session.plan.session_id.clone(),
            entry_id: session.plan.entry_id.clone(),
            label: session.plan.label.clone(),
            deadline: session.deadline,
        }));

        if let Some(deadline) = session.deadline {
            info!(
                session_id = %session.plan.session_id,
                entry_id = %session.plan.entry_id,
                deadline = %deadline,
                "Session started"
            );
        } else {
            info!(
                session_id = %session.plan.session_id,
                entry_id = %session.plan.entry_id,
                "Session started (unlimited)"
            );
        }

        self.current_session = Some(session);

        event
    }

    /// Attach host handle to current session
    pub fn attach_host_handle(&mut self, handle: HostSessionHandle) {
        if let Some(session) = &mut self.current_session {
            session.attach_handle(handle);
        }
    }

    /// Tick the engine - check for warnings and expiry
    pub fn tick(&mut self, now_mono: MonotonicInstant) -> Vec<CoreEvent> {
        let mut events = Vec::new();

        let session = match &mut self.current_session {
            Some(s) => s,
            None => return events,
        };

        // Check for pending warnings
        for (threshold, remaining) in session.pending_warnings(now_mono) {
            let severity = session
                .plan
                .warnings
                .iter()
                .find(|w| w.seconds_before == threshold)
                .map(|w| w.severity)
                .unwrap_or(WarningSeverity::Warn);

            let message = session
                .plan
                .warnings
                .iter()
                .find(|w| w.seconds_before == threshold)
                .and_then(|w| w.message_template.clone());

            session.mark_warning_issued(threshold);

            // Log to audit
            let _ = self.store.append_audit(AuditEvent::new(AuditEventType::WarningIssued {
                session_id: session.plan.session_id.clone(),
                threshold_seconds: threshold,
            }));

            info!(
                session_id = %session.plan.session_id,
                threshold_seconds = threshold,
                remaining_secs = remaining.as_secs(),
                "Warning issued"
            );

            events.push(CoreEvent::Warning {
                session_id: session.plan.session_id.clone(),
                threshold_seconds: threshold,
                time_remaining: remaining,
                severity,
                message,
            });
        }

        // Check for expiry
        if session.is_expired(now_mono)
            && session.state != shepherd_api::SessionState::Expiring
            && session.state != shepherd_api::SessionState::Ended
        {
            session.mark_expiring();

            info!(
                session_id = %session.plan.session_id,
                "Session expiring"
            );

            events.push(CoreEvent::ExpireDue {
                session_id: session.plan.session_id.clone(),
            });
        }

        events
    }

    /// Notify that a session has exited
    pub fn notify_session_exited(
        &mut self,
        exit_code: Option<i32>,
        now_mono: MonotonicInstant,
        now: DateTime<Local>,
    ) -> Option<CoreEvent> {
        let session = self.current_session.take()?;

        let duration = session.duration_so_far(now_mono);
        let reason = if session.state == shepherd_api::SessionState::Expiring {
            SessionEndReason::Expired
        } else {
            SessionEndReason::ProcessExited { exit_code }
        };

        // Update usage accounting
        let today = now.date_naive();
        let _ = self.store.add_usage(&session.plan.entry_id, today, duration);

        // Set cooldown if configured
        if let Some(entry) = self.policy.get_entry(&session.plan.entry_id) {
            if let Some(cooldown) = entry.limits.cooldown {
                let until = now + chrono::Duration::from_std(cooldown).unwrap();
                let _ = self.store.set_cooldown_until(&session.plan.entry_id, until);
            }
        }

        // Log to audit
        let _ = self.store.append_audit(AuditEvent::new(AuditEventType::SessionEnded {
            session_id: session.plan.session_id.clone(),
            entry_id: session.plan.entry_id.clone(),
            reason: reason.clone(),
            duration,
        }));

        info!(
            session_id = %session.plan.session_id,
            entry_id = %session.plan.entry_id,
            duration_secs = duration.as_secs(),
            reason = ?reason,
            "Session ended"
        );

        Some(CoreEvent::SessionEnded {
            session_id: session.plan.session_id,
            entry_id: session.plan.entry_id,
            reason,
            duration,
        })
    }

    /// Stop the current session
    pub fn stop_current(
        &mut self,
        reason: SessionEndReason,
        now_mono: MonotonicInstant,
        now: DateTime<Local>,
    ) -> StopDecision {
        let session = match self.current_session.take() {
            Some(s) => s,
            None => return StopDecision::NoActiveSession,
        };

        let duration = session.duration_so_far(now_mono);

        // Update usage accounting
        let today = now.date_naive();
        let _ = self.store.add_usage(&session.plan.entry_id, today, duration);

        // Set cooldown if configured
        if let Some(entry) = self.policy.get_entry(&session.plan.entry_id) {
            if let Some(cooldown) = entry.limits.cooldown {
                let until = now + chrono::Duration::from_std(cooldown).unwrap();
                let _ = self.store.set_cooldown_until(&session.plan.entry_id, until);
            }
        }

        // Log to audit
        let _ = self.store.append_audit(AuditEvent::new(AuditEventType::SessionEnded {
            session_id: session.plan.session_id.clone(),
            entry_id: session.plan.entry_id.clone(),
            reason: reason.clone(),
            duration,
        }));

        info!(
            session_id = %session.plan.session_id,
            reason = ?reason,
            "Session stopped"
        );

        StopDecision::Stopped(StopResult {
            session_id: session.plan.session_id,
            entry_id: session.plan.entry_id,
            reason,
            duration,
        })
    }

    /// Get current service state snapshot
    pub fn get_state(&self) -> ServiceStateSnapshot {
        let current_session = self.current_session.as_ref().map(|s| {
            s.to_session_info(MonotonicInstant::now())
        });

        // Build entry views for the snapshot
        let entries = self.list_entries(shepherd_util::now());

        ServiceStateSnapshot {
            api_version: API_VERSION,
            policy_loaded: true,
            current_session,
            entry_count: self.policy.entries.len(),
            entries,
        }
    }

    /// Get current session reference
    pub fn current_session(&self) -> Option<&ActiveSession> {
        self.current_session.as_ref()
    }

    /// Get mutable current session reference
    pub fn current_session_mut(&mut self) -> Option<&mut ActiveSession> {
        self.current_session.as_mut()
    }

    /// Check if a session is active
    pub fn has_active_session(&self) -> bool {
        self.current_session.is_some()
    }

    /// Extend current session (admin action)
    /// Only works for sessions with a deadline (not unlimited sessions).
    pub fn extend_current(
        &mut self,
        by: Duration,
        now_mono: MonotonicInstant,
        now: DateTime<Local>,
    ) -> Option<DateTime<Local>> {
        let session = self.current_session.as_mut()?;

        // Can't extend unlimited sessions - they don't have a deadline
        let deadline_mono = session.deadline_mono?;
        let deadline = session.deadline?;

        let new_deadline_mono = deadline_mono + by;
        let new_deadline = deadline + chrono::Duration::from_std(by).unwrap();

        session.deadline_mono = Some(new_deadline_mono);
        session.deadline = Some(new_deadline);

        // Log to audit
        let _ = self.store.append_audit(AuditEvent::new(AuditEventType::SessionExtended {
            session_id: session.plan.session_id.clone(),
            extended_by: by,
            new_deadline,
        }));

        info!(
            session_id = %session.plan.session_id,
            extended_by_secs = by.as_secs(),
            new_deadline = %new_deadline,
            "Session extended"
        );

        Some(new_deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shepherd_config::{AvailabilityPolicy, Entry, LimitsPolicy};
    use shepherd_api::EntryKind;
    use shepherd_store::SqliteStore;
    use std::collections::HashMap;

    fn make_test_policy() -> Policy {
        Policy {
            service: Default::default(),
            entries: vec![Entry {
                id: EntryId::new("test-game"),
                label: "Test Game".into(),
                icon_ref: None,
                kind: EntryKind::Process {
                    command: "game".into(),
                    args: vec![],
                    env: HashMap::new(),
                    cwd: None,
                },
                availability: AvailabilityPolicy {
                    windows: vec![],
                    always: true,
                },
                limits: LimitsPolicy {
                    max_run: Some(Duration::from_secs(300)),
                    daily_quota: None,
                    cooldown: None,
                },
                warnings: vec![],
                volume: None,
                disabled: false,
                disabled_reason: None,
            }],
            default_warnings: vec![],
            default_max_run: Some(Duration::from_secs(3600)),
            volume: Default::default(),
        }
    }

    #[test]
    fn test_list_entries() {
        let policy = make_test_policy();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let caps = HostCapabilities::minimal();
        let engine = CoreEngine::new(policy, store, caps);

        let entries = engine.list_entries(shepherd_util::now());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].enabled);
    }

    #[test]
    fn test_launch_approval() {
        let policy = make_test_policy();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let caps = HostCapabilities::minimal();
        let engine = CoreEngine::new(policy, store, caps);

        let entry_id = EntryId::new("test-game");
        let decision = engine.request_launch(&entry_id, shepherd_util::now());

        assert!(matches!(decision, LaunchDecision::Approved(_)));
    }

    #[test]
    fn test_session_blocks_new_launch() {
        let policy = make_test_policy();
        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let caps = HostCapabilities::minimal();
        let mut engine = CoreEngine::new(policy, store, caps);

        let entry_id = EntryId::new("test-game");
        let now = shepherd_util::now();
        let now_mono = MonotonicInstant::now();

        // Launch first session
        if let LaunchDecision::Approved(plan) = engine.request_launch(&entry_id, now) {
            engine.start_session(plan, now, now_mono);
        }

        // Try to launch again - should be denied
        let decision = engine.request_launch(&entry_id, now);
        assert!(matches!(decision, LaunchDecision::Denied { .. }));
    }

    #[test]
    fn test_tick_warnings() {
        let policy = Policy {
            entries: vec![Entry {
                id: EntryId::new("test"),
                label: "Test".into(),
                icon_ref: None,
                kind: EntryKind::Process {
                    command: "test".into(),
                    args: vec![],
                    env: HashMap::new(),
                    cwd: None,
                },
                availability: AvailabilityPolicy {
                    windows: vec![],
                    always: true,
                },
                limits: LimitsPolicy {
                    max_run: Some(Duration::from_secs(120)), // 2 minutes
                    daily_quota: None,
                    cooldown: None,
                },
                warnings: vec![shepherd_api::WarningThreshold {
                    seconds_before: 60,
                    severity: WarningSeverity::Warn,
                    message_template: Some("1 minute left".into()),
                }],
                volume: None,
                disabled: false,
                disabled_reason: None,
            }],
            service: Default::default(),
            default_warnings: vec![],
            default_max_run: Some(Duration::from_secs(3600)),
            volume: Default::default(),
        };

        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let caps = HostCapabilities::minimal();
        let mut engine = CoreEngine::new(policy, store, caps);

        let entry_id = EntryId::new("test");
        let now = shepherd_util::now();
        let now_mono = MonotonicInstant::now();

        // Start session
        if let LaunchDecision::Approved(plan) = engine.request_launch(&entry_id, now) {
            engine.start_session(plan, now, now_mono);
        }

        // No warnings initially
        let events = engine.tick(now_mono);
        assert!(events.is_empty());

        // At 70 seconds (10 seconds past warning threshold), warning should fire
        let later = now_mono + Duration::from_secs(70);
        let events = engine.tick(later);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], CoreEvent::Warning { threshold_seconds: 60, .. }));

        // Warning shouldn't fire twice
        let events = engine.tick(later);
        assert!(events.is_empty());
    }

    #[test]
    fn test_session_expiry() {
        let policy = Policy {
            entries: vec![Entry {
                id: EntryId::new("test"),
                label: "Test".into(),
                icon_ref: None,
                kind: EntryKind::Process {
                    command: "test".into(),
                    args: vec![],
                    env: HashMap::new(),
                    cwd: None,
                },
                availability: AvailabilityPolicy {
                    windows: vec![],
                    always: true,
                },
                limits: LimitsPolicy {
                    max_run: Some(Duration::from_secs(60)),
                    daily_quota: None,
                    cooldown: None,
                },
                warnings: vec![],
                volume: None,
                disabled: false,
                disabled_reason: None,
            }],
            service: Default::default(),
            default_warnings: vec![],
            default_max_run: Some(Duration::from_secs(3600)),
            volume: Default::default(),
        };

        let store = Arc::new(SqliteStore::in_memory().unwrap());
        let caps = HostCapabilities::minimal();
        let mut engine = CoreEngine::new(policy, store, caps);

        let entry_id = EntryId::new("test");
        let now = shepherd_util::now();
        let now_mono = MonotonicInstant::now();

        // Start session
        if let LaunchDecision::Approved(plan) = engine.request_launch(&entry_id, now) {
            engine.start_session(plan, now, now_mono);
        }

        // At 61 seconds, should be expired
        let later = now_mono + Duration::from_secs(61);
        let events = engine.tick(later);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], CoreEvent::ExpireDue { .. }));
    }
}
