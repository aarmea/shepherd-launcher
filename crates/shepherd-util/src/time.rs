//! Time utilities for shepherdd
//!
//! Provides both monotonic time (for countdown enforcement) and
//! wall-clock time (for availability windows).
//!
//! # Mock Time for Development
//!
//! In debug builds, the `SHEPHERD_MOCK_TIME` environment variable can be set
//! to override the system time for all time-sensitive operations. This is useful
//! for testing availability windows and time-based policies.
//!
//! Format: `YYYY-MM-DD HH:MM:SS` (e.g., `2025-12-25 14:30:00`)
//!
//! Example:
//! ```bash
//! SHEPHERD_MOCK_TIME="2025-12-25 14:30:00" ./run-dev
//! ```

use chrono::{DateTime, Datelike, Local, NaiveDateTime, NaiveTime, TimeZone, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Environment variable name for mock time (debug builds only)
pub const MOCK_TIME_ENV_VAR: &str = "SHEPHERD_MOCK_TIME";

/// Cached mock time offset from the real time when the process started.
/// This allows mock time to advance naturally.
static MOCK_TIME_OFFSET: OnceLock<Option<chrono::Duration>> = OnceLock::new();

/// Initialize the mock time offset based on the environment variable.
/// Returns the offset between mock time and real time at process start.
#[allow(clippy::disallowed_methods)] // This is the internal implementation that wraps Local::now()
fn get_mock_time_offset() -> Option<chrono::Duration> {
    *MOCK_TIME_OFFSET.get_or_init(|| {
        #[cfg(debug_assertions)]
        {
            if let Ok(mock_time_str) = std::env::var(MOCK_TIME_ENV_VAR) {
                // Parse the mock time string
                if let Ok(naive_dt) = NaiveDateTime::parse_from_str(&mock_time_str, "%Y-%m-%d %H:%M:%S") {
                    if let Some(mock_dt) = Local.from_local_datetime(&naive_dt).single() {
                        let real_now = chrono::Local::now();
                        let offset = mock_dt.signed_duration_since(real_now);
                        tracing::info!(
                            mock_time = %mock_time_str,
                            offset_secs = offset.num_seconds(),
                            "Mock time enabled"
                        );
                        return Some(offset);
                    } else {
                        tracing::warn!(
                            mock_time = %mock_time_str,
                            "Failed to convert mock time to local timezone"
                        );
                    }
                } else {
                    tracing::warn!(
                        mock_time = %mock_time_str,
                        expected_format = "%Y-%m-%d %H:%M:%S",
                        "Invalid mock time format"
                    );
                }
            }
            None
        }
        #[cfg(not(debug_assertions))]
        {
            None
        }
    })
}

/// Returns whether mock time is currently active.
pub fn is_mock_time_active() -> bool {
    get_mock_time_offset().is_some()
}

/// Get the current local time, respecting mock time settings in debug builds.
///
/// In release builds, this always returns the real system time.
/// In debug builds, if `SHEPHERD_MOCK_TIME` is set, this returns a time
/// that advances from the mock time at the same rate as real time.
#[allow(clippy::disallowed_methods)] // This is the wrapper that provides mock time support
pub fn now() -> DateTime<Local> {
    let real_now = chrono::Local::now();
    
    if let Some(offset) = get_mock_time_offset() {
        real_now + offset
    } else {
        real_now
    }
}

/// Format a DateTime for display in the HUD clock.
pub fn format_clock_time(dt: &DateTime<Local>) -> String {
    dt.format("%H:%M").to_string()
}

/// Format a DateTime for display with full date and time.
pub fn format_datetime_full(dt: &DateTime<Local>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Represents a point in monotonic time for countdown enforcement.
/// This is immune to wall-clock changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct MonotonicInstant(Instant);

impl MonotonicInstant {
    pub fn now() -> Self {
        Self(Instant::now())
    }

    pub fn elapsed(&self) -> Duration {
        self.0.elapsed()
    }

    pub fn duration_since(&self, earlier: MonotonicInstant) -> Duration {
        self.0.duration_since(earlier.0)
    }

    pub fn checked_add(&self, duration: Duration) -> Option<MonotonicInstant> {
        self.0.checked_add(duration).map(MonotonicInstant)
    }

    /// Returns duration until `self`, or zero if `self` is in the past
    pub fn saturating_duration_until(&self, from: MonotonicInstant) -> Duration {
        if self.0 > from.0 {
            self.0.duration_since(from.0)
        } else {
            Duration::ZERO
        }
    }
}

impl std::ops::Add<Duration> for MonotonicInstant {
    type Output = MonotonicInstant;

    fn add(self, rhs: Duration) -> Self::Output {
        MonotonicInstant(self.0 + rhs)
    }
}

/// Wall-clock time for availability windows
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WallClock {
    pub hour: u8,
    pub minute: u8,
}

impl WallClock {
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        if hour < 24 && minute < 60 {
            Some(Self { hour, minute })
        } else {
            None
        }
    }

    pub fn to_naive_time(self) -> NaiveTime {
        NaiveTime::from_hms_opt(self.hour as u32, self.minute as u32, 0).unwrap()
    }

    pub fn from_naive_time(time: NaiveTime) -> Self {
        Self {
            hour: time.hour() as u8,
            minute: time.minute() as u8,
        }
    }

    /// Returns seconds since midnight
    pub fn as_seconds_from_midnight(&self) -> u32 {
        (self.hour as u32) * 3600 + (self.minute as u32) * 60
    }
}

impl PartialOrd for WallClock {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for WallClock {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_seconds_from_midnight()
            .cmp(&other.as_seconds_from_midnight())
    }
}

/// Days of the week mask
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DaysOfWeek(u8);

impl DaysOfWeek {
    pub const MONDAY: u8 = 1 << 0;
    pub const TUESDAY: u8 = 1 << 1;
    pub const WEDNESDAY: u8 = 1 << 2;
    pub const THURSDAY: u8 = 1 << 3;
    pub const FRIDAY: u8 = 1 << 4;
    pub const SATURDAY: u8 = 1 << 5;
    pub const SUNDAY: u8 = 1 << 6;

    pub const WEEKDAYS: DaysOfWeek = DaysOfWeek(
        Self::MONDAY | Self::TUESDAY | Self::WEDNESDAY | Self::THURSDAY | Self::FRIDAY,
    );
    pub const WEEKENDS: DaysOfWeek = DaysOfWeek(Self::SATURDAY | Self::SUNDAY);
    pub const ALL_DAYS: DaysOfWeek = DaysOfWeek(0x7F);
    pub const NONE: DaysOfWeek = DaysOfWeek(0);

    pub fn new(mask: u8) -> Self {
        Self(mask & 0x7F)
    }

    pub fn contains(&self, weekday: Weekday) -> bool {
        let bit = match weekday {
            Weekday::Mon => Self::MONDAY,
            Weekday::Tue => Self::TUESDAY,
            Weekday::Wed => Self::WEDNESDAY,
            Weekday::Thu => Self::THURSDAY,
            Weekday::Fri => Self::FRIDAY,
            Weekday::Sat => Self::SATURDAY,
            Weekday::Sun => Self::SUNDAY,
        };
        (self.0 & bit) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for DaysOfWeek {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

/// A time window during which an entry is available
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub days: DaysOfWeek,
    pub start: WallClock,
    pub end: WallClock,
}

impl TimeWindow {
    pub fn new(days: DaysOfWeek, start: WallClock, end: WallClock) -> Self {
        Self { days, start, end }
    }

    /// Check if the given local datetime falls within this window
    pub fn contains(&self, dt: &DateTime<Local>) -> bool {
        let weekday = dt.weekday();
        if !self.days.contains(weekday) {
            return false;
        }

        let time = WallClock::from_naive_time(dt.time());

        // Handle windows that don't cross midnight
        if self.start <= self.end {
            time >= self.start && time < self.end
        } else {
            // Window crosses midnight (e.g., 22:00 - 02:00)
            time >= self.start || time < self.end
        }
    }

    /// Calculate duration remaining in this window from the given time
    pub fn remaining_duration(&self, dt: &DateTime<Local>) -> Option<Duration> {
        if !self.contains(dt) {
            return None;
        }

        let now_time = WallClock::from_naive_time(dt.time());
        let now_secs = now_time.as_seconds_from_midnight();
        let end_secs = self.end.as_seconds_from_midnight();

        let remaining_secs = if self.start <= self.end {
            // Normal window
            end_secs.saturating_sub(now_secs)
        } else {
            // Cross-midnight window
            if now_secs >= self.start.as_seconds_from_midnight() {
                // We're in the evening portion, count until midnight then add morning
                (86400 - now_secs) + end_secs
            } else {
                // We're in the morning portion
                end_secs.saturating_sub(now_secs)
            }
        };

        Some(Duration::from_secs(remaining_secs as u64))
    }
}

/// Helper to format durations in human-readable form
pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_wall_clock_ordering() {
        let morning = WallClock::new(8, 0).unwrap();
        let noon = WallClock::new(12, 0).unwrap();
        let evening = WallClock::new(18, 30).unwrap();

        assert!(morning < noon);
        assert!(noon < evening);
        assert!(morning < evening);
    }

    #[test]
    fn test_days_of_week() {
        let weekdays = DaysOfWeek::WEEKDAYS;
        assert!(weekdays.contains(Weekday::Mon));
        assert!(weekdays.contains(Weekday::Fri));
        assert!(!weekdays.contains(Weekday::Sat));
        assert!(!weekdays.contains(Weekday::Sun));

        let weekends = DaysOfWeek::WEEKENDS;
        assert!(!weekends.contains(Weekday::Mon));
        assert!(weekends.contains(Weekday::Sat));
        assert!(weekends.contains(Weekday::Sun));
    }

    #[test]
    fn test_time_window_contains() {
        let window = TimeWindow::new(
            DaysOfWeek::WEEKDAYS,
            WallClock::new(14, 0).unwrap(), // 2 PM
            WallClock::new(18, 0).unwrap(), // 6 PM
        );

        // Monday at 3 PM - should be in window
        let dt = Local.with_ymd_and_hms(2025, 12, 29, 15, 0, 0).unwrap(); // Monday
        assert!(window.contains(&dt));

        // Monday at 10 AM - outside window
        let dt = Local.with_ymd_and_hms(2025, 12, 29, 10, 0, 0).unwrap();
        assert!(!window.contains(&dt));

        // Saturday at 3 PM - wrong day
        let dt = Local.with_ymd_and_hms(2025, 12, 27, 15, 0, 0).unwrap();
        assert!(!window.contains(&dt));
    }

    #[test]
    fn test_time_window_remaining() {
        let window = TimeWindow::new(
            DaysOfWeek::ALL_DAYS,
            WallClock::new(14, 0).unwrap(),
            WallClock::new(18, 0).unwrap(),
        );

        let dt = Local.with_ymd_and_hms(2025, 12, 26, 15, 0, 0).unwrap(); // 3 PM
        let remaining = window.remaining_duration(&dt).unwrap();
        assert_eq!(remaining, Duration::from_secs(3 * 3600)); // 3 hours
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h 1m 1s");
    }

    #[test]
    fn test_monotonic_instant() {
        let t1 = MonotonicInstant::now();
        std::thread::sleep(Duration::from_millis(10));
        let t2 = MonotonicInstant::now();

        assert!(t2 > t1);
        assert!(t2.duration_since(t1) >= Duration::from_millis(10));
    }

    #[test]
    fn test_format_clock_time() {
        let dt = Local.with_ymd_and_hms(2025, 12, 25, 14, 30, 45).unwrap();
        assert_eq!(format_clock_time(&dt), "14:30");
    }

    #[test]
    fn test_format_datetime_full() {
        let dt = Local.with_ymd_and_hms(2025, 12, 25, 14, 30, 45).unwrap();
        assert_eq!(format_datetime_full(&dt), "2025-12-25 14:30:45");
    }

    #[test]
    fn test_now_returns_time() {
        // Basic test that now() returns a valid time
        let t = now();
        // Should be a reasonable year (after 2020, before 2100)
        assert!(t.year() >= 2020);
        assert!(t.year() <= 2100);
    }

    #[test]
    fn test_mock_time_env_var_name() {
        // Verify the environment variable name is correct
        assert_eq!(MOCK_TIME_ENV_VAR, "SHEPHERD_MOCK_TIME");
    }

    #[test]
    fn test_parse_mock_time_format() {
        // Test that the expected format parses correctly
        let valid_formats = [
            "2025-12-25 14:30:00",
            "2025-01-01 00:00:00",
            "2025-12-31 23:59:59",
            "2020-06-15 12:00:00",
        ];

        for format_str in &valid_formats {
            let result = NaiveDateTime::parse_from_str(format_str, "%Y-%m-%d %H:%M:%S");
            assert!(
                result.is_ok(),
                "Expected '{}' to parse successfully, got {:?}",
                format_str,
                result
            );
        }
    }

    #[test]
    fn test_parse_mock_time_invalid_formats() {
        // Test that invalid formats are rejected
        let invalid_formats = [
            "2025-12-25",           // Missing time
            "14:30:00",             // Missing date
            "2025/12/25 14:30:00",  // Wrong date separator
            "2025-12-25T14:30:00",  // ISO format (not supported)
            "Dec 25, 2025 14:30",   // Wrong format
            "25-12-2025 14:30:00",  // Wrong date order
            "",                     // Empty string
            "not a date",           // Invalid string
        ];

        for format_str in &invalid_formats {
            let result = NaiveDateTime::parse_from_str(format_str, "%Y-%m-%d %H:%M:%S");
            assert!(
                result.is_err(),
                "Expected '{}' to fail parsing, but it succeeded",
                format_str
            );
        }
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // Testing the offset calculation requires real time
    fn test_mock_time_offset_calculation() {
        // Test that the offset calculation works correctly
        let mock_time_str = "2025-12-25 14:30:00";
        let naive_dt = NaiveDateTime::parse_from_str(mock_time_str, "%Y-%m-%d %H:%M:%S").unwrap();
        let mock_dt = Local.from_local_datetime(&naive_dt).single().unwrap();
        let real_now = chrono::Local::now();
        
        let offset = mock_dt.signed_duration_since(real_now);
        
        // The offset should be applied correctly
        let simulated_now = real_now + offset;
        
        // The simulated time should be very close to the mock time
        // (within a second, accounting for test execution time)
        let diff = (simulated_now - mock_dt).num_seconds().abs();
        assert!(
            diff <= 1,
            "Expected simulated time to be within 1 second of mock time, got {} seconds difference",
            diff
        );
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // Testing time advancement requires real time
    fn test_mock_time_advances_with_real_time() {
        // Test that mock time advances at the same rate as real time
        // This tests the concept, not the actual implementation (since OnceLock is static)
        
        let mock_time_str = "2025-12-25 14:30:00";
        let naive_dt = NaiveDateTime::parse_from_str(mock_time_str, "%Y-%m-%d %H:%M:%S").unwrap();
        let mock_dt = Local.from_local_datetime(&naive_dt).single().unwrap();
        
        let real_t1 = chrono::Local::now();
        let offset = mock_dt.signed_duration_since(real_t1);
        
        // Simulate time passing
        std::thread::sleep(Duration::from_millis(100));
        
        let real_t2 = chrono::Local::now();
        let simulated_t1 = real_t1 + offset;
        let simulated_t2 = real_t2 + offset;
        
        // The simulated times should have advanced by the same amount as real times
        let real_elapsed = real_t2.signed_duration_since(real_t1);
        let simulated_elapsed = simulated_t2.signed_duration_since(simulated_t1);
        
        assert_eq!(
            real_elapsed.num_milliseconds(),
            simulated_elapsed.num_milliseconds(),
            "Mock time should advance at the same rate as real time"
        );
    }

    #[test]
    fn test_availability_with_specific_time() {
        // Test that availability windows work correctly with a specific time
        // This validates that the mock time would affect availability checks
        
        let window = TimeWindow::new(
            DaysOfWeek::ALL_DAYS,
            WallClock::new(14, 0).unwrap(),  // 2 PM
            WallClock::new(18, 0).unwrap(),  // 6 PM
        );
        
        // Time within window
        let in_window = Local.with_ymd_and_hms(2025, 12, 25, 15, 0, 0).unwrap();
        assert!(window.contains(&in_window), "15:00 should be within 14:00-18:00 window");
        
        // Time before window
        let before_window = Local.with_ymd_and_hms(2025, 12, 25, 10, 0, 0).unwrap();
        assert!(!window.contains(&before_window), "10:00 should be before 14:00-18:00 window");
        
        // Time after window
        let after_window = Local.with_ymd_and_hms(2025, 12, 25, 20, 0, 0).unwrap();
        assert!(!window.contains(&after_window), "20:00 should be after 14:00-18:00 window");
    }

    #[test]
    fn test_availability_with_day_restriction() {
        // Test that day-of-week restrictions work correctly
        let window = TimeWindow::new(
            DaysOfWeek::WEEKDAYS,
            WallClock::new(14, 0).unwrap(),
            WallClock::new(18, 0).unwrap(),
        );
        
        // Thursday at 3 PM - should be available (weekday, in time window)
        let thursday = Local.with_ymd_and_hms(2025, 12, 25, 15, 0, 0).unwrap(); // Christmas 2025 is Thursday
        assert!(window.contains(&thursday), "Thursday 15:00 should be in weekday afternoon window");
        
        // Saturday at 3 PM - should NOT be available (weekend)
        let saturday = Local.with_ymd_and_hms(2025, 12, 27, 15, 0, 0).unwrap();
        assert!(!window.contains(&saturday), "Saturday should not be in weekday window");
        
        // Sunday at 3 PM - should NOT be available (weekend)
        let sunday = Local.with_ymd_and_hms(2025, 12, 28, 15, 0, 0).unwrap();
        assert!(!window.contains(&sunday), "Sunday should not be in weekday window");
    }
}

/// Tests that require running in a separate process to test environment variable handling.
/// These are integration-style tests for the mock time feature.
#[cfg(test)]
mod mock_time_integration_tests {
    use super::*;

    /// This test documents the expected behavior of the mock time feature.
    /// Due to the static OnceLock, actual integration testing requires
    /// running with the environment variable set externally.
    /// 
    /// To manually test:
    /// ```bash
    /// SHEPHERD_MOCK_TIME="2025-12-25 14:30:00" cargo test
    /// ```
    #[test]
    fn test_mock_time_documentation() {
        // This test verifies the mock time constants and expected behavior
        assert_eq!(MOCK_TIME_ENV_VAR, "SHEPHERD_MOCK_TIME");
        
        // The expected format is documented
        let expected_format = "%Y-%m-%d %H:%M:%S";
        let example = "2025-12-25 14:30:00";
        assert!(NaiveDateTime::parse_from_str(example, expected_format).is_ok());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_is_mock_time_active_in_debug() {
        // In debug mode, is_mock_time_active() should return based on env var
        // Since we can't control the env var within a single test run due to OnceLock,
        // we just verify the function doesn't panic
        let _ = is_mock_time_active();
    }

    #[test]
    fn test_now_consistency() {
        // now() should return consistent, advancing times
        let t1 = now();
        std::thread::sleep(Duration::from_millis(50));
        let t2 = now();
        
        // t2 should be after t1
        assert!(t2 > t1, "Time should advance forward");
        
        // The difference should be approximately 50ms (with some tolerance)
        let diff = t2.signed_duration_since(t1);
        assert!(
            diff.num_milliseconds() >= 40 && diff.num_milliseconds() <= 200,
            "Expected ~50ms difference, got {}ms",
            diff.num_milliseconds()
        );
    }
}
