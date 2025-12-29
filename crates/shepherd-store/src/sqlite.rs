//! SQLite-based store implementation

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use rusqlite::{params, Connection, OptionalExtension};
use shepherd_util::EntryId;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tracing::{debug, warn};

use crate::{AuditEvent, SessionSnapshot, StateSnapshot, Store, StoreError, StoreResult};

/// SQLite-based store
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl SqliteStore {
    /// Open or create a store at the given path
    pub fn open(path: impl AsRef<Path>) -> StoreResult<Self> {
        let conn = Connection::open(path)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Create an in-memory store (for testing)
    pub fn in_memory() -> StoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute_batch(
            r#"
            -- Audit log (append-only)
            CREATE TABLE IF NOT EXISTS audit_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                event_json TEXT NOT NULL
            );

            -- Usage accounting
            CREATE TABLE IF NOT EXISTS usage (
                entry_id TEXT NOT NULL,
                day TEXT NOT NULL,
                duration_secs INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (entry_id, day)
            );

            -- Cooldowns
            CREATE TABLE IF NOT EXISTS cooldowns (
                entry_id TEXT PRIMARY KEY,
                until TEXT NOT NULL
            );

            -- State snapshot (single row)
            CREATE TABLE IF NOT EXISTS snapshot (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                snapshot_json TEXT NOT NULL
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_usage_day ON usage(day);
            "#,
        )?;

        debug!("Store schema initialized");
        Ok(())
    }
}

impl Store for SqliteStore {
    fn append_audit(&self, mut event: AuditEvent) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();
        let event_json = serde_json::to_string(&event.event)?;

        conn.execute(
            "INSERT INTO audit_log (timestamp, event_json) VALUES (?, ?)",
            params![event.timestamp.to_rfc3339(), event_json],
        )?;

        event.id = conn.last_insert_rowid();
        debug!(event_id = event.id, "Audit event appended");

        Ok(())
    }

    fn get_recent_audits(&self, limit: usize) -> StoreResult<Vec<AuditEvent>> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_json FROM audit_log ORDER BY id DESC LIMIT ?",
        )?;

        let rows = stmt.query_map([limit], |row| {
            let id: i64 = row.get(0)?;
            let timestamp_str: String = row.get(1)?;
            let event_json: String = row.get(2)?;
            Ok((id, timestamp_str, event_json))
        })?;

        let mut events = Vec::new();
        for row in rows {
            let (id, timestamp_str, event_json) = row?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map(|dt| dt.with_timezone(&Local))
                .unwrap_or_else(|_| shepherd_util::now());
            let event: crate::AuditEventType = serde_json::from_str(&event_json)?;

            events.push(AuditEvent {
                id,
                timestamp,
                event,
            });
        }

        Ok(events)
    }

    fn get_usage(&self, entry_id: &EntryId, day: NaiveDate) -> StoreResult<Duration> {
        let conn = self.conn.lock().unwrap();
        let day_str = day.format("%Y-%m-%d").to_string();

        let secs: Option<i64> = conn
            .query_row(
                "SELECT duration_secs FROM usage WHERE entry_id = ? AND day = ?",
                params![entry_id.as_str(), day_str],
                |row| row.get(0),
            )
            .optional()?;

        Ok(Duration::from_secs(secs.unwrap_or(0) as u64))
    }

    fn add_usage(&self, entry_id: &EntryId, day: NaiveDate, duration: Duration) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();
        let day_str = day.format("%Y-%m-%d").to_string();
        let secs = duration.as_secs() as i64;

        conn.execute(
            r#"
            INSERT INTO usage (entry_id, day, duration_secs)
            VALUES (?, ?, ?)
            ON CONFLICT(entry_id, day)
            DO UPDATE SET duration_secs = duration_secs + excluded.duration_secs
            "#,
            params![entry_id.as_str(), day_str, secs],
        )?;

        debug!(entry_id = %entry_id, day = %day_str, added_secs = secs, "Usage added");
        Ok(())
    }

    fn get_cooldown_until(&self, entry_id: &EntryId) -> StoreResult<Option<DateTime<Local>>> {
        let conn = self.conn.lock().unwrap();

        let until_str: Option<String> = conn
            .query_row(
                "SELECT until FROM cooldowns WHERE entry_id = ?",
                [entry_id.as_str()],
                |row| row.get(0),
            )
            .optional()?;

        let result = until_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Local))
                .ok()
        });

        Ok(result)
    }

    fn set_cooldown_until(
        &self,
        entry_id: &EntryId,
        until: DateTime<Local>,
    ) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();

        conn.execute(
            r#"
            INSERT INTO cooldowns (entry_id, until)
            VALUES (?, ?)
            ON CONFLICT(entry_id)
            DO UPDATE SET until = excluded.until
            "#,
            params![entry_id.as_str(), until.to_rfc3339()],
        )?;

        debug!(entry_id = %entry_id, until = %until, "Cooldown set");
        Ok(())
    }

    fn clear_cooldown(&self, entry_id: &EntryId) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM cooldowns WHERE entry_id = ?", [entry_id.as_str()])?;
        Ok(())
    }

    fn load_snapshot(&self) -> StoreResult<Option<StateSnapshot>> {
        let conn = self.conn.lock().unwrap();

        let json: Option<String> = conn
            .query_row("SELECT snapshot_json FROM snapshot WHERE id = 1", [], |row| {
                row.get(0)
            })
            .optional()?;

        match json {
            Some(s) => {
                let snapshot: StateSnapshot = serde_json::from_str(&s)?;
                Ok(Some(snapshot))
            }
            None => Ok(None),
        }
    }

    fn save_snapshot(&self, snapshot: &StateSnapshot) -> StoreResult<()> {
        let conn = self.conn.lock().unwrap();
        let json = serde_json::to_string(snapshot)?;

        conn.execute(
            r#"
            INSERT INTO snapshot (id, snapshot_json)
            VALUES (1, ?)
            ON CONFLICT(id)
            DO UPDATE SET snapshot_json = excluded.snapshot_json
            "#,
            [json],
        )?;

        debug!("Snapshot saved");
        Ok(())
    }

    fn is_healthy(&self) -> bool {
        match self.conn.lock() {
            Ok(conn) => {
                conn.query_row("SELECT 1", [], |_| Ok(())).is_ok()
            }
            Err(_) => {
                warn!("Store lock poisoned");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuditEventType;

    #[test]
    fn test_in_memory_store() {
        let store = SqliteStore::in_memory().unwrap();
        assert!(store.is_healthy());
    }

    #[test]
    fn test_audit_log() {
        let store = SqliteStore::in_memory().unwrap();

        let event = AuditEvent::new(AuditEventType::ServiceStarted);
        store.append_audit(event).unwrap();

        let events = store.get_recent_audits(10).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].event, AuditEventType::ServiceStarted));
    }

    #[test]
    fn test_usage_accounting() {
        let store = SqliteStore::in_memory().unwrap();
        let entry_id = EntryId::new("game-1");
        let today = shepherd_util::now().date_naive();

        // Initially zero
        let usage = store.get_usage(&entry_id, today).unwrap();
        assert_eq!(usage, Duration::ZERO);

        // Add some usage
        store
            .add_usage(&entry_id, today, Duration::from_secs(300))
            .unwrap();
        let usage = store.get_usage(&entry_id, today).unwrap();
        assert_eq!(usage, Duration::from_secs(300));

        // Add more usage
        store
            .add_usage(&entry_id, today, Duration::from_secs(200))
            .unwrap();
        let usage = store.get_usage(&entry_id, today).unwrap();
        assert_eq!(usage, Duration::from_secs(500));
    }

    #[test]
    fn test_cooldowns() {
        let store = SqliteStore::in_memory().unwrap();
        let entry_id = EntryId::new("game-1");

        // No cooldown initially
        assert!(store.get_cooldown_until(&entry_id).unwrap().is_none());

        // Set cooldown
        let until = shepherd_util::now() + chrono::Duration::hours(1);
        store.set_cooldown_until(&entry_id, until).unwrap();

        let stored = store.get_cooldown_until(&entry_id).unwrap().unwrap();
        assert!((stored - until).num_seconds().abs() < 1);

        // Clear cooldown
        store.clear_cooldown(&entry_id).unwrap();
        assert!(store.get_cooldown_until(&entry_id).unwrap().is_none());
    }

    #[test]
    fn test_snapshot() {
        let store = SqliteStore::in_memory().unwrap();

        // No snapshot initially
        assert!(store.load_snapshot().unwrap().is_none());

        // Save snapshot
        let snapshot = StateSnapshot {
            timestamp: shepherd_util::now(),
            active_session: None,
        };
        store.save_snapshot(&snapshot).unwrap();

        // Load it back
        let loaded = store.load_snapshot().unwrap().unwrap();
        assert!(loaded.active_session.is_none());
    }
}
