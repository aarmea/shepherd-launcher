//! Integration tests for shepherdd
//!
//! These tests verify the end-to-end behavior of the daemon.

use chrono::Local;
use shepherd_api::{EntryKind, WarningSeverity, WarningThreshold};
use shepherd_config::{AvailabilityPolicy, Entry, LimitsPolicy, Policy};
use shepherd_core::{CoreEngine, CoreEvent, LaunchDecision};
use shepherd_host_api::{HostCapabilities, MockHost};
use shepherd_store::{SqliteStore, Store};
use shepherd_util::{EntryId, MonotonicInstant};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

fn make_test_policy() -> Policy {
    Policy {
        daemon: Default::default(),
        entries: vec![
            Entry {
                id: EntryId::new("test-game"),
                label: "Test Game".into(),
                icon_ref: None,
                kind: EntryKind::Process {
                    command: "sleep".into(),
                    args: vec!["999".into()],
                    env: HashMap::new(),
                    cwd: None,
                },
                availability: AvailabilityPolicy {
                    windows: vec![],
                    always: true,
                },
                limits: LimitsPolicy {
                    max_run: Some(Duration::from_secs(10)), // Short for testing
                    daily_quota: None,
                    cooldown: None,
                },
                warnings: vec![
                    WarningThreshold {
                        seconds_before: 5,
                        severity: WarningSeverity::Warn,
                        message_template: Some("5 seconds left".into()),
                    },
                    WarningThreshold {
                        seconds_before: 2,
                        severity: WarningSeverity::Critical,
                        message_template: Some("2 seconds left!".into()),
                    },
                ],
                disabled: false,
                disabled_reason: None,
            },
        ],
        default_warnings: vec![],
        default_max_run: Some(Duration::from_secs(3600)),
    }
}

#[test]
fn test_policy_loading() {
    let policy = make_test_policy();
    assert_eq!(policy.entries.len(), 1);
    assert_eq!(policy.entries[0].id.as_str(), "test-game");
}

#[test]
fn test_entry_listing() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let engine = CoreEngine::new(policy, store, caps);

    let entries = engine.list_entries(Local::now());

    assert_eq!(entries.len(), 1);
    assert!(entries[0].enabled);
    assert_eq!(entries[0].entry_id.as_str(), "test-game");
    assert!(entries[0].max_run_if_started_now.is_some());
}

#[test]
fn test_launch_approval() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let decision = engine.request_launch(&entry_id, Local::now());

    assert!(matches!(decision, LaunchDecision::Approved(plan) if plan.max_duration == Some(Duration::from_secs(10))));
}

#[test]
fn test_session_lifecycle() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let mut engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let now = Local::now();
    let now_mono = MonotonicInstant::now();

    // Launch
    let plan = match engine.request_launch(&entry_id, now) {
        LaunchDecision::Approved(p) => p,
        LaunchDecision::Denied { .. } => panic!("Launch should be approved"),
    };

    // Start session
    let event = engine.start_session(plan, now, now_mono);
    assert!(matches!(event, CoreEvent::SessionStarted { .. }));

    // Verify session is active
    assert!(engine.has_active_session());

    // Second launch should be denied
    let decision = engine.request_launch(&entry_id, now);
    assert!(matches!(decision, LaunchDecision::Denied { .. }));
}

#[test]
fn test_warning_emission() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let mut engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let now = Local::now();
    let now_mono = MonotonicInstant::now();

    // Start session
    let plan = match engine.request_launch(&entry_id, now) {
        LaunchDecision::Approved(p) => p,
        _ => panic!(),
    };
    engine.start_session(plan, now, now_mono);

    // No warnings at start
    let events = engine.tick(now_mono);
    assert!(events.is_empty());

    // At 6 seconds (4 seconds remaining), 5-second warning should fire
    let at_6s = now_mono + Duration::from_secs(6);
    let events = engine.tick(at_6s);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], CoreEvent::Warning { threshold_seconds: 5, .. }));

    // At 9 seconds (1 second remaining), 2-second warning should fire
    let at_9s = now_mono + Duration::from_secs(9);
    let events = engine.tick(at_9s);
    assert_eq!(events.len(), 1);
    assert!(matches!(&events[0], CoreEvent::Warning { threshold_seconds: 2, .. }));

    // Warnings shouldn't repeat
    let events = engine.tick(at_9s);
    assert!(events.is_empty());
}

#[test]
fn test_session_expiry() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let mut engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let now = Local::now();
    let now_mono = MonotonicInstant::now();

    // Start session
    let plan = match engine.request_launch(&entry_id, now) {
        LaunchDecision::Approved(p) => p,
        _ => panic!(),
    };
    engine.start_session(plan, now, now_mono);

    // At 11 seconds, session should be expired
    let at_11s = now_mono + Duration::from_secs(11);
    let events = engine.tick(at_11s);

    // Should have both remaining warnings + expiry
    let has_expiry = events.iter().any(|e| matches!(e, CoreEvent::ExpireDue { .. }));
    assert!(has_expiry, "Expected ExpireDue event");
}

#[test]
fn test_usage_accounting() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let store_check = store.clone();
    let caps = HostCapabilities::minimal();
    let mut engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let now = Local::now();
    let now_mono = MonotonicInstant::now();

    // Start session
    let plan = match engine.request_launch(&entry_id, now) {
        LaunchDecision::Approved(p) => p,
        _ => panic!(),
    };
    engine.start_session(plan, now, now_mono);

    // Simulate 5 seconds passing
    let later_mono = now_mono + Duration::from_secs(5);
    let later = now + chrono::Duration::seconds(5);

    // Session exits
    engine.notify_session_exited(Some(0), later_mono, later);

    // Check usage was recorded
    let usage = store_check.get_usage(&entry_id, now.date_naive()).unwrap();
    assert!(usage >= Duration::from_secs(4) && usage <= Duration::from_secs(6));
}

#[tokio::test]
async fn test_mock_host_integration() {
    use shepherd_host_api::{HostAdapter, SpawnOptions};
    use shepherd_util::SessionId;

    let host = MockHost::new();
    let _rx = host.subscribe();

    let session_id = SessionId::new();
    let entry = EntryKind::Process {
        command: "test".into(),
        args: vec![],
        env: HashMap::new(),
        cwd: None,
    };

    // Spawn
    let handle = host
        .spawn(session_id.clone(), &entry, SpawnOptions::default())
        .await
        .unwrap();

    // Verify running
    assert_eq!(host.running_sessions().len(), 1);

    // Stop
    host.stop(
        &handle,
        shepherd_host_api::StopMode::Graceful {
            timeout: Duration::from_secs(1),
        },
    )
    .await
    .unwrap();
}

#[test]
fn test_config_parsing() {
    use shepherd_config::parse_config;

    let config = r#"
        config_version = 1

        [[entries]]
        id = "scummvm"
        label = "ScummVM"
        kind = { type = "process", command = "scummvm", args = ["-f"] }

        [entries.availability]
        [[entries.availability.windows]]
        days = "weekdays"
        start = "14:00"
        end = "18:00"

        [entries.limits]
        max_run_seconds = 3600
        daily_quota_seconds = 7200
        cooldown_seconds = 300

        [[entries.warnings]]
        seconds_before = 300
        severity = "info"
        message = "5 minutes remaining"
    "#;

    let policy = parse_config(config).unwrap();
    assert_eq!(policy.entries.len(), 1);
    assert_eq!(policy.entries[0].id.as_str(), "scummvm");
    assert_eq!(policy.entries[0].limits.max_run, Some(Duration::from_secs(3600)));
    assert_eq!(policy.entries[0].limits.daily_quota, Some(Duration::from_secs(7200)));
    assert_eq!(policy.entries[0].limits.cooldown, Some(Duration::from_secs(300)));
    assert_eq!(policy.entries[0].warnings.len(), 1);
}

#[test]
fn test_session_extension() {
    let policy = make_test_policy();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let caps = HostCapabilities::minimal();
    let mut engine = CoreEngine::new(policy, store, caps);

    let entry_id = EntryId::new("test-game");
    let now = Local::now();
    let now_mono = MonotonicInstant::now();

    // Start session
    let plan = match engine.request_launch(&entry_id, now) {
        LaunchDecision::Approved(p) => p,
        _ => panic!(),
    };
    engine.start_session(plan, now, now_mono);

    // Get original deadline (should be Some for this test)
    let original_deadline = engine.current_session().unwrap().deadline.expect("Expected deadline");

    // Extend by 5 minutes
    let new_deadline = engine.extend_current(Duration::from_secs(300), now_mono, now);
    assert!(new_deadline.is_some());

    let new_deadline = new_deadline.unwrap();
    let extension = new_deadline.signed_duration_since(original_deadline);
    assert!(extension.num_seconds() >= 299 && extension.num_seconds() <= 301);
}
