//! Validated policy structures

use crate::schema::{RawConfig, RawDays, RawEntry, RawEntryKind, RawWarningThreshold};
use crate::validation::{parse_days, parse_time};
use shepherd_api::{EntryKind, WarningSeverity, WarningThreshold};
use shepherd_util::{DaysOfWeek, EntryId, TimeWindow, WallClock};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Validated policy ready for use by the core engine
#[derive(Debug, Clone)]
pub struct Policy {
    /// Daemon configuration
    pub daemon: DaemonConfig,

    /// Validated entries
    pub entries: Vec<Entry>,

    /// Default warning thresholds
    pub default_warnings: Vec<WarningThreshold>,

    /// Default max run duration. None means unlimited.
    pub default_max_run: Option<Duration>,
}

impl Policy {
    /// Convert from raw config (after validation)
    pub fn from_raw(raw: RawConfig) -> Self {
        let default_warnings = raw
            .daemon
            .default_warnings
            .clone()
            .map(|w| w.into_iter().map(convert_warning).collect())
            .unwrap_or_else(default_warning_thresholds);

        // 0 means unlimited, None means use 1 hour default
        let default_max_run = raw
            .daemon
            .default_max_run_seconds
            .map(seconds_to_duration_or_unlimited)
            .unwrap_or(Some(Duration::from_secs(3600))); // 1 hour default

        let entries = raw
            .entries
            .into_iter()
            .map(|e| Entry::from_raw(e, &default_warnings, default_max_run))
            .collect();

        Self {
            daemon: DaemonConfig::from_raw(raw.daemon),
            entries,
            default_warnings,
            default_max_run,
        }
    }

    /// Get entry by ID
    pub fn get_entry(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.iter().find(|e| &e.id == id)
    }
}

/// Daemon configuration
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub log_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl DaemonConfig {
    fn from_raw(raw: crate::schema::RawDaemonConfig) -> Self {
        Self {
            socket_path: raw
                .socket_path
                .unwrap_or_else(|| PathBuf::from("/run/shepherdd/shepherdd.sock")),
            log_dir: raw
                .log_dir
                .unwrap_or_else(|| PathBuf::from("/var/log/shepherdd")),
            data_dir: raw
                .data_dir
                .unwrap_or_else(|| PathBuf::from("/var/lib/shepherdd")),
        }
    }
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            socket_path: PathBuf::from("/run/shepherdd/shepherdd.sock"),
            log_dir: PathBuf::from("/var/log/shepherdd"),
            data_dir: PathBuf::from("/var/lib/shepherdd"),
        }
    }
}

/// Validated entry definition
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: EntryId,
    pub label: String,
    pub icon_ref: Option<String>,
    pub kind: EntryKind,
    pub availability: AvailabilityPolicy,
    pub limits: LimitsPolicy,
    pub warnings: Vec<WarningThreshold>,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
}

impl Entry {
    fn from_raw(
        raw: RawEntry,
        default_warnings: &[WarningThreshold],
        default_max_run: Option<Duration>,
    ) -> Self {
        let kind = convert_entry_kind(raw.kind);
        let availability = raw
            .availability
            .map(convert_availability)
            .unwrap_or_default();
        let limits = raw
            .limits
            .map(|l| convert_limits(l, default_max_run))
            .unwrap_or_else(|| LimitsPolicy {
                max_run: default_max_run,
                daily_quota: None, // None means unlimited
                cooldown: None,
            });
        let warnings = raw
            .warnings
            .map(|w| w.into_iter().map(convert_warning).collect())
            .unwrap_or_else(|| default_warnings.to_vec());

        Self {
            id: EntryId::new(raw.id),
            label: raw.label,
            icon_ref: raw.icon,
            kind,
            availability,
            limits,
            warnings,
            disabled: raw.disabled,
            disabled_reason: raw.disabled_reason,
        }
    }
}

/// When an entry is available
#[derive(Debug, Clone, Default)]
pub struct AvailabilityPolicy {
    /// Time windows when entry is available
    pub windows: Vec<TimeWindow>,
    /// If true, always available (ignores windows)
    pub always: bool,
}

impl AvailabilityPolicy {
    /// Check if available at given local time
    pub fn is_available(&self, dt: &chrono::DateTime<chrono::Local>) -> bool {
        if self.always {
            return true;
        }
        if self.windows.is_empty() {
            return true; // No windows = always available
        }
        self.windows.iter().any(|w| w.contains(dt))
    }

    /// Get remaining time in current window
    pub fn remaining_in_window(
        &self,
        dt: &chrono::DateTime<chrono::Local>,
    ) -> Option<Duration> {
        if self.always {
            return None; // No limit from windows
        }
        self.windows.iter().find_map(|w| w.remaining_duration(dt))
    }
}

/// Time limits for an entry
#[derive(Debug, Clone)]
pub struct LimitsPolicy {
    /// Maximum run duration. None means unlimited.
    pub max_run: Option<Duration>,
    /// Daily quota. None means unlimited.
    pub daily_quota: Option<Duration>,
    pub cooldown: Option<Duration>,
}

// Conversion helpers

fn convert_entry_kind(raw: RawEntryKind) -> EntryKind {
    match raw {
        RawEntryKind::Process { command, args, env, cwd } => EntryKind::Process { command, args, env, cwd },
        RawEntryKind::Snap { snap_name, command, args, env } => EntryKind::Snap { snap_name, command, args, env },
        RawEntryKind::Vm { driver, args } => EntryKind::Vm { driver, args },
        RawEntryKind::Media { library_id, args } => EntryKind::Media { library_id, args },
        RawEntryKind::Custom { type_name, payload } => EntryKind::Custom {
            type_name,
            payload: payload.unwrap_or(serde_json::Value::Null),
        },
    }
}

fn convert_availability(raw: crate::schema::RawAvailability) -> AvailabilityPolicy {
    let windows = raw.windows.into_iter().map(convert_time_window).collect();
    AvailabilityPolicy {
        windows,
        always: raw.always,
    }
}

fn convert_time_window(raw: crate::schema::RawTimeWindow) -> TimeWindow {
    let days_mask = parse_days(&raw.days).unwrap_or(0x7F);
    let (start_h, start_m) = parse_time(&raw.start).unwrap_or((0, 0));
    let (end_h, end_m) = parse_time(&raw.end).unwrap_or((23, 59));

    TimeWindow {
        days: DaysOfWeek::new(days_mask),
        start: WallClock::new(start_h, start_m).unwrap(),
        end: WallClock::new(end_h, end_m).unwrap(),
    }
}

/// Convert seconds to Duration, treating 0 as "unlimited" (None)
fn seconds_to_duration_or_unlimited(secs: u64) -> Option<Duration> {
    if secs == 0 {
        None // 0 means unlimited
    } else {
        Some(Duration::from_secs(secs))
    }
}

fn convert_limits(raw: crate::schema::RawLimits, default_max_run: Option<Duration>) -> LimitsPolicy {
    LimitsPolicy {
        max_run: raw
            .max_run_seconds
            .map(seconds_to_duration_or_unlimited)
            .unwrap_or(default_max_run),
        daily_quota: raw
            .daily_quota_seconds
            .and_then(seconds_to_duration_or_unlimited),
        cooldown: raw.cooldown_seconds.map(Duration::from_secs),
    }
}

fn convert_warning(raw: RawWarningThreshold) -> WarningThreshold {
    let severity = match raw.severity.to_lowercase().as_str() {
        "info" => WarningSeverity::Info,
        "critical" => WarningSeverity::Critical,
        _ => WarningSeverity::Warn,
    };

    WarningThreshold {
        seconds_before: raw.seconds_before,
        severity,
        message_template: raw.message,
    }
}

fn default_warning_thresholds() -> Vec<WarningThreshold> {
    vec![
        WarningThreshold {
            seconds_before: 300, // 5 minutes
            severity: WarningSeverity::Info,
            message_template: Some("5 minutes remaining".into()),
        },
        WarningThreshold {
            seconds_before: 60, // 1 minute
            severity: WarningSeverity::Warn,
            message_template: Some("1 minute remaining".into()),
        },
        WarningThreshold {
            seconds_before: 10,
            severity: WarningSeverity::Critical,
            message_template: Some("10 seconds remaining!".into()),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    #[test]
    fn test_availability_always() {
        let policy = AvailabilityPolicy {
            windows: vec![],
            always: true,
        };

        let dt = Local::now();
        assert!(policy.is_available(&dt));
    }

    #[test]
    fn test_availability_window() {
        let policy = AvailabilityPolicy {
            windows: vec![TimeWindow {
                days: DaysOfWeek::ALL_DAYS,
                start: WallClock::new(14, 0).unwrap(),
                end: WallClock::new(18, 0).unwrap(),
            }],
            always: false,
        };

        // 3 PM should be available
        let dt = Local.with_ymd_and_hms(2025, 12, 26, 15, 0, 0).unwrap();
        assert!(policy.is_available(&dt));

        // 10 AM should not be available
        let dt = Local.with_ymd_and_hms(2025, 12, 26, 10, 0, 0).unwrap();
        assert!(!policy.is_available(&dt));
    }
}
