//! Battery monitoring module
//!
//! Monitors battery status via sysfs or UPower D-Bus interface.

use std::fs;
use std::path::Path;

/// Battery status
#[derive(Debug, Clone, Default)]
pub struct BatteryStatus {
    /// Battery percentage (0-100)
    pub percent: Option<u8>,
    /// Whether the battery is charging
    pub charging: bool,
    /// Whether AC power is connected
    pub ac_connected: bool,
}

impl BatteryStatus {
    /// Read battery status from sysfs
    pub fn read() -> Self {
        let mut status = BatteryStatus::default();

        // Try to find a battery in /sys/class/power_supply
        let power_supply = Path::new("/sys/class/power_supply");
        if !power_supply.exists() {
            return status;
        }

        if let Ok(entries) = fs::read_dir(power_supply) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Check for battery
                if name_str.starts_with("BAT") {
                    if let Some((percent, charging)) = read_battery_info(&path) {
                        status.percent = Some(percent);
                        status.charging = charging;
                    }
                }

                // Check for AC adapter
                if name_str.starts_with("AC") || name_str.contains("ADP") {
                    if let Some(online) = read_ac_status(&path) {
                        status.ac_connected = online;
                    }
                }
            }
        }

        status
    }

    /// Get an icon name for the current battery status
    pub fn icon_name(&self) -> &'static str {
        match (self.percent, self.charging) {
            (None, _) => "battery-missing-symbolic",
            (Some(p), true) if p >= 90 => "battery-full-charging-symbolic",
            (Some(p), true) if p >= 60 => "battery-good-charging-symbolic",
            (Some(p), true) if p >= 30 => "battery-low-charging-symbolic",
            (Some(_), true) => "battery-caution-charging-symbolic",
            (Some(p), false) if p >= 90 => "battery-full-symbolic",
            (Some(p), false) if p >= 60 => "battery-good-symbolic",
            (Some(p), false) if p >= 30 => "battery-low-symbolic",
            (Some(p), false) if p >= 10 => "battery-caution-symbolic",
            (Some(_), false) => "battery-empty-symbolic",
        }
    }

    /// Check if battery is critically low
    pub fn is_critical(&self) -> bool {
        matches!(self.percent, Some(p) if p < 10 && !self.charging)
    }
}

fn read_battery_info(path: &Path) -> Option<(u8, bool)> {
    // Read capacity
    let capacity_path = path.join("capacity");
    let capacity: u8 = fs::read_to_string(&capacity_path)
        .ok()?
        .trim()
        .parse()
        .ok()?;

    // Read status
    let status_path = path.join("status");
    let status = fs::read_to_string(&status_path).ok()?;
    let charging = status.trim().eq_ignore_ascii_case("charging")
        || status.trim().eq_ignore_ascii_case("full");

    Some((capacity.min(100), charging))
}

fn read_ac_status(path: &Path) -> Option<bool> {
    let online_path = path.join("online");
    let online = fs::read_to_string(&online_path).ok()?;
    Some(online.trim() == "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_icon_names() {
        let status = BatteryStatus {
            percent: Some(95),
            charging: false,
            ac_connected: false,
        };
        assert_eq!(status.icon_name(), "battery-full-symbolic");

        let status = BatteryStatus {
            percent: Some(50),
            charging: true,
            ac_connected: true,
        };
        assert_eq!(status.icon_name(), "battery-low-charging-symbolic");

        let status = BatteryStatus {
            percent: Some(5),
            charging: false,
            ac_connected: false,
        };
        assert_eq!(status.icon_name(), "battery-empty-symbolic");
        assert!(status.is_critical());
    }
}
