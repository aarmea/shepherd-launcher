//! Raw configuration schema (as parsed from TOML)

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Raw configuration as parsed from TOML
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawConfig {
    /// Config schema version
    pub config_version: u32,

    /// Global service settings
    #[serde(default, alias = "daemon")]
    pub service: RawServiceConfig,

    /// List of allowed entries
    #[serde(default)]
    pub entries: Vec<RawEntry>,
}

/// Service-level settings
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawServiceConfig {
    /// IPC socket path (default: $XDG_RUNTIME_DIR/shepherdd/shepherdd.sock)
    pub socket_path: Option<PathBuf>,

    /// Log directory (default: $XDG_STATE_HOME/shepherdd)
    pub log_dir: Option<PathBuf>,

    /// Data directory for store (default: $XDG_DATA_HOME/shepherdd)
    pub data_dir: Option<PathBuf>,

    /// Default warning thresholds (can be overridden per entry)
    pub default_warnings: Option<Vec<RawWarningThreshold>>,

    /// Default max run duration
    pub default_max_run_seconds: Option<u64>,

    /// Global volume restrictions
    #[serde(default)]
    pub volume: Option<RawVolumeConfig>,
}

/// Raw entry definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawEntry {
    /// Unique stable ID
    pub id: String,

    /// Display label
    pub label: String,

    /// Icon reference (opaque, interpreted by shell)
    pub icon: Option<String>,

    /// Entry kind and launch details
    pub kind: RawEntryKind,

    /// Availability time windows
    #[serde(default)]
    pub availability: Option<RawAvailability>,

    /// Time limits
    #[serde(default)]
    pub limits: Option<RawLimits>,

    /// Warning configuration
    #[serde(default)]
    pub warnings: Option<Vec<RawWarningThreshold>>,

    /// Volume restrictions for this entry (overrides global)
    #[serde(default)]
    pub volume: Option<RawVolumeConfig>,

    /// Explicitly disabled
    #[serde(default)]
    pub disabled: bool,

    /// Reason for disabling
    pub disabled_reason: Option<String>,
}

/// Raw entry kind
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RawEntryKind {
    Process {
        /// Command to run (required)
        command: String,
        /// Additional command-line arguments
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        cwd: Option<PathBuf>,
    },
    /// Snap application - uses systemd scope-based process management
    Snap {
        /// The snap name (e.g., "mc-installer")
        snap_name: String,
        /// Command to run (defaults to snap_name if not specified)
        command: Option<String>,
        /// Additional command-line arguments
        #[serde(default)]
        args: Vec<String>,
        /// Additional environment variables
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Vm {
        driver: String,
        #[serde(default)]
        args: HashMap<String, serde_json::Value>,
    },
    Media {
        library_id: String,
        #[serde(default)]
        args: HashMap<String, serde_json::Value>,
    },
    Custom {
        type_name: String,
        #[serde(default)]
        payload: Option<serde_json::Value>,
    },
}

/// Availability configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawAvailability {
    /// Time windows when entry is available
    #[serde(default)]
    pub windows: Vec<RawTimeWindow>,

    /// If true, entry is always available (ignores windows)
    #[serde(default)]
    pub always: bool,
}

/// Time window
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawTimeWindow {
    /// Days of week: "weekdays", "weekends", "all", or list like ["mon", "tue", "wed"]
    pub days: RawDays,

    /// Start time (HH:MM format)
    pub start: String,

    /// End time (HH:MM format)
    pub end: String,
}

/// Days specification
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RawDays {
    Preset(String),
    List(Vec<String>),
}

/// Time limits
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawLimits {
    /// Maximum run duration in seconds
    pub max_run_seconds: Option<u64>,

    /// Daily quota in seconds
    pub daily_quota_seconds: Option<u64>,

    /// Cooldown after session ends, in seconds
    pub cooldown_seconds: Option<u64>,
}

/// Warning threshold
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RawWarningThreshold {
    /// Seconds before expiry
    pub seconds_before: u64,

    /// Severity: "info", "warn", "critical"
    #[serde(default = "default_severity")]
    pub severity: String,

    /// Message template
    pub message: Option<String>,
}

fn default_severity() -> String {
    "warn".to_string()
}

/// Volume control configuration
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RawVolumeConfig {
    /// Maximum volume percentage allowed (0-100)
    pub max_volume: Option<u8>,

    /// Minimum volume percentage allowed (0-100)
    pub min_volume: Option<u8>,

    /// Whether mute toggle is allowed (default: true)
    #[serde(default = "default_true")]
    pub allow_mute: bool,

    /// Whether volume changes are allowed at all (default: true)
    #[serde(default = "default_true")]
    pub allow_change: bool,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_process_entry() {
        let toml_str = r#"
            config_version = 1

            [[entries]]
            id = "scummvm"
            label = "ScummVM"
            kind = { type = "process", command = "scummvm", args = ["-f"] }

            [entries.limits]
            max_run_seconds = 3600
        "#;

        let config: RawConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entries.len(), 1);
        assert_eq!(config.entries[0].id, "scummvm");
    }

    #[test]
    fn parse_time_windows() {
        let toml_str = r#"
            config_version = 1

            [[entries]]
            id = "game"
            label = "Game"
            kind = { type = "process", command = "/bin/game" }

            [entries.availability]
            [[entries.availability.windows]]
            days = "weekdays"
            start = "14:00"
            end = "18:00"

            [[entries.availability.windows]]
            days = ["sat", "sun"]
            start = "10:00"
            end = "20:00"
        "#;

        let config: RawConfig = toml::from_str(toml_str).unwrap();
        let avail = config.entries[0].availability.as_ref().unwrap();
        assert_eq!(avail.windows.len(), 2);
    }
}
