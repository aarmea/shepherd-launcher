//! Shared types for the shepherdd API

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use shepherd_util::{EntryId, SessionId};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Entry kind tag for capability matching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKindTag {
    Process,
    Snap,
    Vm,
    Media,
    Custom,
}

/// Entry kind with launch details
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EntryKind {
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
        #[serde(default)]
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
        payload: serde_json::Value,
    },
}

impl EntryKind {
    pub fn tag(&self) -> EntryKindTag {
        match self {
            EntryKind::Process { .. } => EntryKindTag::Process,
            EntryKind::Snap { .. } => EntryKindTag::Snap,
            EntryKind::Vm { .. } => EntryKindTag::Vm,
            EntryKind::Media { .. } => EntryKindTag::Media,
            EntryKind::Custom { .. } => EntryKindTag::Custom,
        }
    }
}

/// View of an entry for UI display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryView {
    pub entry_id: EntryId,
    pub label: String,
    pub icon_ref: Option<String>,
    pub kind_tag: EntryKindTag,
    pub enabled: bool,
    pub reasons: Vec<ReasonCode>,
    /// Maximum run duration if started now. None means:
    /// - If enabled=false: entry is not available
    /// - If enabled=true: entry has no time limit (unlimited)
    pub max_run_if_started_now: Option<Duration>,
}

/// Structured reason codes for why an entry is unavailable
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum ReasonCode {
    /// Outside allowed time window
    OutsideTimeWindow {
        /// When the next window opens (if known)
        next_window_start: Option<DateTime<Local>>,
    },
    /// Daily quota exhausted
    QuotaExhausted {
        used: Duration,
        quota: Duration,
    },
    /// Cooldown period active
    CooldownActive {
        available_at: DateTime<Local>,
    },
    /// Another session is active
    SessionActive {
        entry_id: EntryId,
        /// Time remaining in current session. None means unlimited.
        remaining: Option<Duration>,
    },
    /// Host doesn't support this entry kind
    UnsupportedKind {
        kind: EntryKindTag,
    },
    /// Entry is explicitly disabled
    Disabled {
        reason: Option<String>,
    },
}

/// Warning severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningSeverity {
    Info,
    Warn,
    Critical,
}

/// Warning threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarningThreshold {
    /// Seconds before expiry to issue this warning
    pub seconds_before: u64,
    pub severity: WarningSeverity,
    pub message_template: Option<String>,
}

/// Session end reason
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionEndReason {
    /// Session expired (time limit reached)
    Expired,
    /// User requested stop
    UserStop,
    /// Admin requested stop
    AdminStop,
    /// Process exited on its own
    ProcessExited { exit_code: Option<i32> },
    /// Policy change terminated session
    PolicyStop,
    /// Service shutdown
    ServiceShutdown,
    /// Launch failed
    LaunchFailed { error: String },
}

/// Current session state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Launching,
    Running,
    Warned,
    Expiring,
    Ended,
}

/// Active session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub entry_id: EntryId,
    pub label: String,
    pub state: SessionState,
    pub started_at: DateTime<Local>,
    /// Session deadline. None means unlimited (no time limit).
    pub deadline: Option<DateTime<Local>>,
    /// Time remaining. None means unlimited.
    pub time_remaining: Option<Duration>,
    pub warnings_issued: Vec<u64>,
}

/// Full service state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStateSnapshot {
    pub api_version: u32,
    pub policy_loaded: bool,
    pub current_session: Option<SessionInfo>,
    pub entry_count: usize,
    /// Available entries for UI display
    #[serde(default)]
    pub entries: Vec<EntryView>,
}

/// Role for authorization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientRole {
    /// UI/HUD - can view state, launch entries, stop current
    Shell,
    /// Local admin - can also extend, reload config
    Admin,
    /// Read-only observer
    Observer,
}

impl ClientRole {
    pub fn can_launch(&self) -> bool {
        matches!(self, ClientRole::Shell | ClientRole::Admin)
    }

    pub fn can_stop(&self) -> bool {
        matches!(self, ClientRole::Shell | ClientRole::Admin)
    }

    pub fn can_extend(&self) -> bool {
        matches!(self, ClientRole::Admin)
    }

    pub fn can_reload_config(&self) -> bool {
        matches!(self, ClientRole::Admin)
    }
}

/// Stop mode for session termination
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopMode {
    /// Try graceful termination first
    Graceful,
    /// Force immediate termination
    Force,
}

/// Health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub live: bool,
    pub ready: bool,
    pub policy_loaded: bool,
    pub host_adapter_ok: bool,
    pub store_ok: bool,
}

/// Volume status information
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VolumeInfo {
    /// Volume percentage (0-100)
    pub percent: u8,
    /// Whether audio is muted
    pub muted: bool,
    /// Whether volume control is available
    pub available: bool,
    /// The detected sound backend (e.g., "pipewire", "pulseaudio", "alsa")
    pub backend: Option<String>,
    /// Current restrictions on volume
    pub restrictions: VolumeRestrictions,
}

/// Volume restrictions that are currently in effect
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VolumeRestrictions {
    /// Maximum volume percentage allowed
    pub max_volume: Option<u8>,
    /// Minimum volume percentage allowed
    pub min_volume: Option<u8>,
    /// Whether mute toggle is allowed
    pub allow_mute: bool,
    /// Whether volume changes are allowed at all
    pub allow_change: bool,
}

impl VolumeRestrictions {
    /// Create unrestricted volume settings
    pub fn unrestricted() -> Self {
        Self {
            max_volume: None,
            min_volume: None,
            allow_mute: true,
            allow_change: true,
        }
    }

    /// Clamp a volume value to the allowed range
    pub fn clamp_volume(&self, percent: u8) -> u8 {
        let min = self.min_volume.unwrap_or(0);
        let max = self.max_volume.unwrap_or(100);
        percent.clamp(min, max)
    }
}

impl VolumeInfo {
    /// Get an icon name for the current volume status
    pub fn icon_name(&self) -> &'static str {
        if self.muted {
            "audio-volume-muted-symbolic"
        } else if self.percent == 0 {
            "audio-volume-muted-symbolic"
        } else if self.percent < 33 {
            "audio-volume-low-symbolic"
        } else if self.percent < 66 {
            "audio-volume-medium-symbolic"
        } else {
            "audio-volume-high-symbolic"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_kind_serialization() {
        let kind = EntryKind::Process {
            command: "scummvm".into(),
            args: vec!["-f".into()],
            env: HashMap::new(),
            cwd: None,
        };

        let json = serde_json::to_string(&kind).unwrap();
        let parsed: EntryKind = serde_json::from_str(&json).unwrap();

        assert_eq!(kind, parsed);
    }

    #[test]
    fn reason_code_serialization() {
        let reason = ReasonCode::QuotaExhausted {
            used: Duration::from_secs(3600),
            quota: Duration::from_secs(3600),
        };

        let json = serde_json::to_string(&reason).unwrap();
        assert!(json.contains("quota_exhausted"));
    }
}
