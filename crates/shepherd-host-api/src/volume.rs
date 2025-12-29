//! Volume control trait interfaces
//!
//! Defines the capability-based interface for volume control between
//! the shepherdd service and platform-specific implementations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors from volume control operations
#[derive(Debug, Error)]
pub enum VolumeError {
    #[error("Volume control not available: {0}")]
    NotAvailable(String),

    #[error("Backend error: {0}")]
    Backend(String),

    #[error("Volume out of range: {0}")]
    OutOfRange(u8),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type VolumeResult<T> = Result<T, VolumeError>;

/// Volume status
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeStatus {
    /// Volume percentage (0-100, can exceed if system allows)
    pub percent: u8,
    /// Whether audio is muted
    pub muted: bool,
}

impl VolumeStatus {
    /// Get an icon name for the current volume status
    pub fn icon_name(&self) -> &'static str {
        if self.muted || self.percent == 0 {
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

/// Volume capabilities
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VolumeCapabilities {
    /// Whether volume control is available
    pub available: bool,
    /// The detected sound backend (e.g., "pipewire", "pulseaudio", "alsa")
    pub backend: Option<String>,
    /// Whether mute control is available
    pub can_mute: bool,
    /// Maximum volume percentage allowed (for systems that allow >100%)
    pub max_volume: u8,
}

/// Volume restrictions that can be enforced by policy
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VolumeRestrictions {
    /// Maximum volume percentage allowed (enforced by the service)
    pub max_volume: Option<u8>,
    /// Minimum volume percentage allowed (enforced by the service)
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

/// Volume controller trait - implemented by platform-specific adapters
#[async_trait]
pub trait VolumeController: Send + Sync {
    /// Get the capabilities of this volume controller
    fn capabilities(&self) -> &VolumeCapabilities;

    /// Get current volume status
    async fn get_status(&self) -> VolumeResult<VolumeStatus>;

    /// Set volume to a specific percentage
    async fn set_volume(&self, percent: u8) -> VolumeResult<()>;

    /// Increase volume by a step
    async fn volume_up(&self, step: u8) -> VolumeResult<()>;

    /// Decrease volume by a step
    async fn volume_down(&self, step: u8) -> VolumeResult<()>;

    /// Toggle mute state
    async fn toggle_mute(&self) -> VolumeResult<()>;

    /// Set mute state explicitly
    async fn set_mute(&self, muted: bool) -> VolumeResult<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_volume_icon_names() {
        let status = VolumeStatus {
            percent: 0,
            muted: false,
        };
        assert_eq!(status.icon_name(), "audio-volume-muted-symbolic");

        let status = VolumeStatus {
            percent: 50,
            muted: false,
        };
        assert_eq!(status.icon_name(), "audio-volume-medium-symbolic");

        let status = VolumeStatus {
            percent: 100,
            muted: true,
        };
        assert_eq!(status.icon_name(), "audio-volume-muted-symbolic");
    }

    #[test]
    fn test_restrictions_clamp() {
        let restrictions = VolumeRestrictions {
            max_volume: Some(80),
            min_volume: Some(20),
            allow_mute: true,
            allow_change: true,
        };

        assert_eq!(restrictions.clamp_volume(50), 50);
        assert_eq!(restrictions.clamp_volume(10), 20);
        assert_eq!(restrictions.clamp_volume(90), 80);
    }

    #[test]
    fn test_unrestricted() {
        let restrictions = VolumeRestrictions::unrestricted();
        assert_eq!(restrictions.clamp_volume(0), 0);
        assert_eq!(restrictions.clamp_volume(100), 100);
        assert!(restrictions.allow_mute);
        assert!(restrictions.allow_change);
    }
}
