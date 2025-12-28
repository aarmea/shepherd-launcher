//! Session state machine

use chrono::{DateTime, Local};
use shepherd_api::{SessionEndReason, SessionState, WarningThreshold};
use shepherd_host_api::HostSessionHandle;
use shepherd_util::{EntryId, MonotonicInstant, SessionId};
use std::time::Duration;

/// Session plan computed at launch approval
#[derive(Debug, Clone)]
pub struct SessionPlan {
    pub session_id: SessionId,
    pub entry_id: EntryId,
    pub label: String,
    /// Maximum duration for this session. None means unlimited.
    pub max_duration: Option<Duration>,
    pub warnings: Vec<WarningThreshold>,
}

impl SessionPlan {
    /// Compute warning times (as durations after start)
    /// Returns empty vec for unlimited sessions.
    pub fn warning_times(&self) -> Vec<(u64, Duration)> {
        let max_duration = match self.max_duration {
            Some(d) => d,
            None => return Vec::new(), // No warnings for unlimited sessions
        };
        self.warnings
            .iter()
            .filter(|w| Duration::from_secs(w.seconds_before) < max_duration)
            .map(|w| {
                let trigger_after =
                    max_duration - Duration::from_secs(w.seconds_before);
                (w.seconds_before, trigger_after)
            })
            .collect()
    }
}

/// Active session tracking
#[derive(Debug)]
pub struct ActiveSession {
    /// Session plan
    pub plan: SessionPlan,

    /// Current state
    pub state: SessionState,

    /// Wall-clock start time (for display/logging)
    pub started_at: DateTime<Local>,

    /// Monotonic start time (for enforcement)
    pub started_at_mono: MonotonicInstant,

    /// Wall-clock deadline (for display). None means unlimited.
    pub deadline: Option<DateTime<Local>>,

    /// Monotonic deadline (for enforcement). None means unlimited.
    pub deadline_mono: Option<MonotonicInstant>,

    /// Warning thresholds already issued (seconds before expiry)
    pub warnings_issued: Vec<u64>,

    /// Host session handle (for stopping)
    pub host_handle: Option<HostSessionHandle>,
}

impl ActiveSession {
    /// Create a new session from an approved plan
    pub fn new(
        plan: SessionPlan,
        now: DateTime<Local>,
        now_mono: MonotonicInstant,
    ) -> Self {
        let (deadline, deadline_mono) = match plan.max_duration {
            Some(max_dur) => {
                let deadline = now + chrono::Duration::from_std(max_dur).unwrap();
                let deadline_mono = now_mono + max_dur;
                (Some(deadline), Some(deadline_mono))
            }
            None => (None, None), // Unlimited session
        };

        Self {
            plan,
            state: SessionState::Launching,
            started_at: now,
            started_at_mono: now_mono,
            deadline,
            deadline_mono,
            warnings_issued: Vec::new(),
            host_handle: None,
        }
    }

    /// Attach the host handle once spawn succeeds
    pub fn attach_handle(&mut self, handle: HostSessionHandle) {
        self.host_handle = Some(handle);
        self.state = SessionState::Running;
    }

    /// Get time remaining using monotonic time. None means unlimited.
    pub fn time_remaining(&self, now_mono: MonotonicInstant) -> Option<Duration> {
        self.deadline_mono.map(|deadline| deadline.saturating_duration_until(now_mono))
    }

    /// Check if session is expired (never true for unlimited sessions)
    pub fn is_expired(&self, now_mono: MonotonicInstant) -> bool {
        match self.deadline_mono {
            Some(deadline) => now_mono >= deadline,
            None => false, // Unlimited sessions never expire
        }
    }

    /// Get pending warnings (not yet issued) that should fire now.
    /// Returns empty vec for unlimited sessions (no warnings to issue).
    pub fn pending_warnings(&self, now_mono: MonotonicInstant) -> Vec<(u64, Duration)> {
        // Unlimited sessions don't have warnings
        let remaining = match self.time_remaining(now_mono) {
            Some(r) => r,
            None => return Vec::new(),
        };

        let elapsed = now_mono.duration_since(self.started_at_mono);

        self.plan
            .warning_times()
            .into_iter()
            .filter(|(threshold, trigger_after)| {
                // Should trigger if elapsed >= trigger_after and not already issued
                elapsed >= *trigger_after && !self.warnings_issued.contains(threshold)
            })
            .map(|(threshold, _)| (threshold, remaining))
            .collect()
    }

    /// Mark a warning as issued
    pub fn mark_warning_issued(&mut self, threshold: u64) {
        if !self.warnings_issued.contains(&threshold) {
            self.warnings_issued.push(threshold);
        }
        // Update state to Warned if not already expiring
        if self.state == SessionState::Running {
            self.state = SessionState::Warned;
        }
    }

    /// Mark session as expiring
    pub fn mark_expiring(&mut self) {
        self.state = SessionState::Expiring;
    }

    /// Mark session as ended
    pub fn mark_ended(&mut self) {
        self.state = SessionState::Ended;
    }

    /// Get session duration so far
    pub fn duration_so_far(&self, now_mono: MonotonicInstant) -> Duration {
        now_mono.duration_since(self.started_at_mono)
    }

    /// Get session info for API
    pub fn to_session_info(&self, now_mono: MonotonicInstant) -> shepherd_api::SessionInfo {
        shepherd_api::SessionInfo {
            session_id: self.plan.session_id.clone(),
            entry_id: self.plan.entry_id.clone(),
            label: self.plan.label.clone(),
            state: self.state,
            started_at: self.started_at,
            deadline: self.deadline,
            time_remaining: self.time_remaining(now_mono),
            warnings_issued: self.warnings_issued.clone(),
        }
    }
}

/// Result of stopping a session
#[derive(Debug)]
pub struct StopResult {
    pub session_id: SessionId,
    pub entry_id: EntryId,
    pub reason: SessionEndReason,
    pub duration: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shepherd_api::WarningSeverity;

    fn make_test_plan(duration_secs: u64) -> SessionPlan {
        SessionPlan {
            session_id: SessionId::new(),
            entry_id: EntryId::new("test"),
            label: "Test".into(),
            max_duration: Some(Duration::from_secs(duration_secs)),
            warnings: vec![
                WarningThreshold {
                    seconds_before: 60,
                    severity: WarningSeverity::Warn,
                    message_template: None,
                },
                WarningThreshold {
                    seconds_before: 10,
                    severity: WarningSeverity::Critical,
                    message_template: None,
                },
            ],
        }
    }

    #[test]
    fn test_session_creation() {
        let plan = make_test_plan(300);
        let now = Local::now();
        let now_mono = MonotonicInstant::now();

        let session = ActiveSession::new(plan, now, now_mono);

        assert_eq!(session.state, SessionState::Launching);
        assert!(session.warnings_issued.is_empty());
        assert_eq!(session.time_remaining(now_mono), Some(Duration::from_secs(300)));
    }

    #[test]
    fn test_warning_times() {
        let plan = make_test_plan(300); // 5 minutes

        let times = plan.warning_times();
        assert_eq!(times.len(), 2);

        // 60s warning should trigger at 240s (4 min)
        let w60 = times.iter().find(|(t, _)| *t == 60).unwrap();
        assert_eq!(w60.1, Duration::from_secs(240));

        // 10s warning should trigger at 290s
        let w10 = times.iter().find(|(t, _)| *t == 10).unwrap();
        assert_eq!(w10.1, Duration::from_secs(290));
    }

    #[test]
    fn test_warning_not_issued_for_short_session() {
        // Session shorter than warning threshold
        let plan = SessionPlan {
            session_id: SessionId::new(),
            entry_id: EntryId::new("test"),
            label: "Test".into(),
            max_duration: Some(Duration::from_secs(30)), // 30 seconds
            warnings: vec![WarningThreshold {
                seconds_before: 60, // 60 second warning - longer than session!
                severity: WarningSeverity::Warn,
                message_template: None,
            }],
        };

        let times = plan.warning_times();
        assert!(times.is_empty()); // No warnings should be scheduled
    }

    #[test]
    fn test_pending_warnings() {
        let plan = make_test_plan(300);
        let now = Local::now();
        let now_mono = MonotonicInstant::now();

        let mut session = ActiveSession::new(plan, now, now_mono);

        // At start, no warnings pending
        let pending = session.pending_warnings(now_mono);
        assert!(pending.is_empty());

        // Simulate time passing - at 250s, 60s warning should be pending
        let later = now_mono + Duration::from_secs(250);
        let pending = session.pending_warnings(later);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, 60);

        // Mark it issued
        session.mark_warning_issued(60);
        let pending = session.pending_warnings(later);
        assert!(pending.is_empty());
    }
}
