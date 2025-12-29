# shepherd-store

Persistence layer for Shepherd.

## Overview

This crate provides durable storage for the Shepherd service, including:

- **Audit log** - Append-only record of all significant events
- **Usage accounting** - Track time used per entry per day
- **Cooldown tracking** - Remember when entries become available again
- **State snapshots** - Enable crash recovery

## Purpose

The store ensures:
- **Time accounting correctness** - Usage is recorded durably
- **Crash tolerance** - Service can resume after unexpected shutdown
- **Auditability** - All actions are logged for later inspection

## Backend

The primary implementation uses **SQLite** for reliability:

```rust
use shepherd_store::SqliteStore;

let store = SqliteStore::open("/var/lib/shepherdd/shepherdd.db")?;
```

SQLite provides:
- ACID transactions for usage accounting
- Automatic crash recovery via WAL mode
- Single-file database, easy to backup

## Store Trait

All storage operations go through the `Store` trait:

```rust
pub trait Store: Send + Sync {
    // Audit log
    fn append_audit(&self, event: AuditEvent) -> StoreResult<()>;
    fn get_recent_audits(&self, limit: usize) -> StoreResult<Vec<AuditEvent>>;

    // Usage accounting
    fn get_usage(&self, entry_id: &EntryId, day: NaiveDate) -> StoreResult<Duration>;
    fn add_usage(&self, entry_id: &EntryId, day: NaiveDate, duration: Duration) -> StoreResult<()>;

    // Cooldown tracking
    fn get_cooldown_until(&self, entry_id: &EntryId) -> StoreResult<Option<DateTime<Local>>>;
    fn set_cooldown_until(&self, entry_id: &EntryId, until: DateTime<Local>) -> StoreResult<()>;
    fn clear_cooldown(&self, entry_id: &EntryId) -> StoreResult<()>;

    // State snapshot
    fn load_snapshot(&self) -> StoreResult<Option<StateSnapshot>>;
    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StoreResult<()>;

    // Health
    fn is_healthy(&self) -> bool;
}
```

## Usage

### Recording Session Usage

```rust
use chrono::Local;

// When a session ends
let duration = session.actual_duration();
let today = Local::now().date_naive();

store.add_usage(&entry_id, today, duration)?;
```

### Checking Quota Remaining

```rust
let today = Local::now().date_naive();
let used = store.get_usage(&entry_id, today)?;

if let Some(quota) = entry.limits.daily_quota {
    let remaining = quota.saturating_sub(used);
    if remaining.is_zero() {
        // Quota exhausted
    }
}
```

### Setting Cooldowns

```rust
use chrono::{Duration, Local};

// After session ends, set cooldown
let cooldown_until = Local::now() + Duration::minutes(10);
store.set_cooldown_until(&entry_id, cooldown_until)?;
```

### Checking Cooldown

```rust
if let Some(until) = store.get_cooldown_until(&entry_id)? {
    if until > Local::now() {
        // Still in cooldown
    }
}
```

## Audit Log

The audit log records significant events:

```rust
use shepherd_store::{AuditEvent, AuditEventType};

// Event types logged
store.append_audit(AuditEvent::new(AuditEventType::PolicyLoaded { entry_count: 5 }))?;
store.append_audit(AuditEvent::new(AuditEventType::SessionStarted { 
    session_id, 
    entry_id 
}))?;
store.append_audit(AuditEvent::new(AuditEventType::SessionEnded { 
    session_id, 
    reason: SessionEndReason::Expired 
}))?;
store.append_audit(AuditEvent::new(AuditEventType::WarningIssued { 
    session_id, 
    threshold_secs: 60 
}))?;
```

### Audit Event Types

- `PolicyLoaded` - Configuration loaded/reloaded
- `SessionStarted` - New session began
- `SessionEnded` - Session terminated (with reason)
- `WarningIssued` - Time warning shown to user
- `LaunchDenied` - Launch request rejected (with reasons)
- `ConfigReloaded` - Configuration hot-reloaded
- `ServiceStarted` - Service process started
- `ServiceStopped` - Service process stopped

## State Snapshots

For crash recovery, the service can save state snapshots:

```rust
use shepherd_store::{StateSnapshot, SessionSnapshot};

// Save current state
let snapshot = StateSnapshot {
    timestamp: Local::now(),
    active_session: Some(SessionSnapshot {
        session_id,
        entry_id,
        started_at,
        deadline,
        warnings_issued: vec![300, 60],
    }),
};
store.save_snapshot(&snapshot)?;

// On startup, check for unfinished session
if let Some(snapshot) = store.load_snapshot()? {
    if let Some(session) = snapshot.active_session {
        // Potentially recover or clean up
    }
}
```

## Database Schema

The SQLite store uses this schema:

```sql
-- Audit log (append-only)
CREATE TABLE audit_log (
    id INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    event_type TEXT NOT NULL,
    event_data TEXT NOT NULL  -- JSON
);

-- Usage tracking (one row per entry per day)
CREATE TABLE usage (
    entry_id TEXT NOT NULL,
    day TEXT NOT NULL,  -- YYYY-MM-DD
    duration_secs INTEGER NOT NULL,
    PRIMARY KEY (entry_id, day)
);

-- Cooldown tracking
CREATE TABLE cooldowns (
    entry_id TEXT PRIMARY KEY,
    until TEXT NOT NULL  -- ISO 8601 timestamp
);

-- State snapshot (single row)
CREATE TABLE snapshot (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    data TEXT NOT NULL  -- JSON
);
```

## Design Philosophy

- **Durability over performance** - Writes are synchronous by default
- **Simple queries** - No complex joins or aggregations needed at runtime
- **Append-only audit** - Never modify history
- **Portable format** - JSON for event data enables future migration

## Dependencies

- `rusqlite` - SQLite bindings
- `serde` / `serde_json` - Event serialization
- `chrono` - Timestamp handling
- `thiserror` - Error types
