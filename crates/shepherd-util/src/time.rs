//! Time utilities for shepherdd
//!
//! Provides both monotonic time (for countdown enforcement) and
//! wall-clock time (for availability windows).

use chrono::{DateTime, Datelike, Local, NaiveTime, Timelike, Weekday};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

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
}
