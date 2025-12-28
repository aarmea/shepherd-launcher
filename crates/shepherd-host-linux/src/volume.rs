//! Linux volume control implementation
//!
//! Provides volume control with auto-detection of sound systems:
//! - PipeWire (via `wpctl`)
//! - PulseAudio (via `pactl`)
//! - ALSA (via `amixer`)

use async_trait::async_trait;
use shepherd_host_api::{
    VolumeCapabilities, VolumeController, VolumeError, VolumeResult, VolumeStatus,
};
use std::process::Command;
use tracing::{debug, info, warn};

/// Detected sound backend
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundBackend {
    /// PipeWire with WirePlumber
    PipeWire,
    /// PulseAudio
    PulseAudio,
    /// ALSA (direct)
    Alsa,
}

impl SoundBackend {
    /// Detect the best available sound backend
    pub fn detect() -> Option<Self> {
        // Try PipeWire first (modern systems)
        if Self::is_pipewire_available() {
            info!("Detected PipeWire sound backend");
            return Some(Self::PipeWire);
        }

        // Try PulseAudio
        if Self::is_pulseaudio_available() {
            info!("Detected PulseAudio sound backend");
            return Some(Self::PulseAudio);
        }

        // Try ALSA as fallback
        if Self::is_alsa_available() {
            info!("Detected ALSA sound backend");
            return Some(Self::Alsa);
        }

        warn!("No sound backend detected");
        None
    }

    fn is_pipewire_available() -> bool {
        // Check if wpctl is available and can communicate with PipeWire
        Command::new("wpctl")
            .args(["status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_pulseaudio_available() -> bool {
        // Check if pactl is available and server is running
        Command::new("pactl")
            .args(["info"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_alsa_available() -> bool {
        // Check if amixer is available
        Command::new("amixer")
            .args(["sget", "Master"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::PipeWire => "pipewire",
            Self::PulseAudio => "pulseaudio",
            Self::Alsa => "alsa",
        }
    }
}

/// Linux volume controller with auto-detection
pub struct LinuxVolumeController {
    capabilities: VolumeCapabilities,
    backend: Option<SoundBackend>,
}

impl LinuxVolumeController {
    /// Create a new volume controller with auto-detection
    pub fn new() -> Self {
        let backend = SoundBackend::detect();

        let capabilities = VolumeCapabilities {
            available: backend.is_some(),
            backend: backend.map(|b| b.name().to_string()),
            can_mute: backend.is_some(),
            max_volume: 100,
        };

        Self {
            capabilities,
            backend,
        }
    }

    /// Get volume status via PipeWire
    fn get_status_pipewire() -> VolumeResult<VolumeStatus> {
        // Get volume: wpctl get-volume @DEFAULT_AUDIO_SINK@
        // Output: "Volume: 0.50" or "Volume: 0.50 [MUTED]"
        let output = Command::new("wpctl")
            .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("wpctl get-volume output: {}", stdout.trim());

        let muted = stdout.contains("[MUTED]");

        // Parse "Volume: 0.50" -> 50%
        let percent = stdout
            .split(':')
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f32>().ok())
            .map(|v| (v * 100.0).round() as u8)
            .unwrap_or(0);

        Ok(VolumeStatus { percent, muted })
    }

    /// Get volume status via PulseAudio
    fn get_status_pulseaudio() -> VolumeResult<VolumeStatus> {
        let mut status = VolumeStatus::default();

        // Get default sink info
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("pactl get-sink-volume output: {}", stdout.trim());

            // Output: "Volume: front-left: 65536 / 100% / -0.00 dB, front-right: ..."
            if let Some(percent_str) = stdout.split('/').nth(1) {
                if let Ok(percent) = percent_str.trim().trim_end_matches('%').parse::<u8>() {
                    status.percent = percent;
                }
            }
        }

        // Check mute status
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            debug!("pactl get-sink-mute output: {}", stdout.trim());
            status.muted = stdout.contains("yes");
        }

        Ok(status)
    }

    /// Get volume status via ALSA
    fn get_status_alsa() -> VolumeResult<VolumeStatus> {
        // amixer sget Master
        // Output includes: "Front Left: Playback 65536 [100%] [on]"
        let output = Command::new("amixer")
            .args(["sget", "Master"])
            .output()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        debug!("amixer sget Master output: {}", stdout);

        let mut status = VolumeStatus::default();

        for line in stdout.lines() {
            if line.contains("Playback") && line.contains('%') {
                // Extract percentage: [100%]
                if let Some(start) = line.find('[') {
                    if let Some(end) = line[start..].find('%') {
                        if let Ok(percent) = line[start + 1..start + end].parse::<u8>() {
                            status.percent = percent;
                        }
                    }
                }
                // Check mute status: [on] or [off]
                status.muted = line.contains("[off]");
                break;
            }
        }

        Ok(status)
    }

    /// Set volume via PipeWire
    fn set_volume_pipewire(percent: u8) -> VolumeResult<()> {
        let volume = format!("{}%", percent);
        Command::new("wpctl")
            .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &volume])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Set volume via PulseAudio
    fn set_volume_pulseaudio(percent: u8) -> VolumeResult<()> {
        Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", percent)])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Set volume via ALSA
    fn set_volume_alsa(percent: u8) -> VolumeResult<()> {
        Command::new("amixer")
            .args(["sset", "Master", &format!("{}%", percent)])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Toggle mute via PipeWire
    fn toggle_mute_pipewire() -> VolumeResult<()> {
        Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Toggle mute via PulseAudio
    fn toggle_mute_pulseaudio() -> VolumeResult<()> {
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Toggle mute via ALSA
    fn toggle_mute_alsa() -> VolumeResult<()> {
        Command::new("amixer")
            .args(["sset", "Master", "toggle"])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Set mute state via PipeWire
    fn set_mute_pipewire(muted: bool) -> VolumeResult<()> {
        let state = if muted { "1" } else { "0" };
        Command::new("wpctl")
            .args(["set-mute", "@DEFAULT_AUDIO_SINK@", state])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Set mute state via PulseAudio
    fn set_mute_pulseaudio(muted: bool) -> VolumeResult<()> {
        let state = if muted { "1" } else { "0" };
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", state])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }

    /// Set mute state via ALSA
    fn set_mute_alsa(muted: bool) -> VolumeResult<()> {
        let state = if muted { "mute" } else { "unmute" };
        Command::new("amixer")
            .args(["sset", "Master", state])
            .status()
            .map_err(|e| VolumeError::Backend(e.to_string()))?;
        Ok(())
    }
}

impl Default for LinuxVolumeController {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VolumeController for LinuxVolumeController {
    fn capabilities(&self) -> &VolumeCapabilities {
        &self.capabilities
    }

    async fn get_status(&self) -> VolumeResult<VolumeStatus> {
        match self.backend {
            Some(SoundBackend::PipeWire) => Self::get_status_pipewire(),
            Some(SoundBackend::PulseAudio) => Self::get_status_pulseaudio(),
            Some(SoundBackend::Alsa) => Self::get_status_alsa(),
            None => Err(VolumeError::NotAvailable(
                "No sound backend available".into(),
            )),
        }
    }

    async fn set_volume(&self, percent: u8) -> VolumeResult<()> {
        if percent > self.capabilities.max_volume {
            return Err(VolumeError::OutOfRange(percent));
        }

        match self.backend {
            Some(SoundBackend::PipeWire) => Self::set_volume_pipewire(percent),
            Some(SoundBackend::PulseAudio) => Self::set_volume_pulseaudio(percent),
            Some(SoundBackend::Alsa) => Self::set_volume_alsa(percent),
            None => Err(VolumeError::NotAvailable(
                "No sound backend available".into(),
            )),
        }
    }

    async fn volume_up(&self, step: u8) -> VolumeResult<()> {
        let current = self.get_status().await?;
        let new_volume = current.percent.saturating_add(step).min(self.capabilities.max_volume);
        self.set_volume(new_volume).await
    }

    async fn volume_down(&self, step: u8) -> VolumeResult<()> {
        let current = self.get_status().await?;
        let new_volume = current.percent.saturating_sub(step);
        self.set_volume(new_volume).await
    }

    async fn toggle_mute(&self) -> VolumeResult<()> {
        match self.backend {
            Some(SoundBackend::PipeWire) => Self::toggle_mute_pipewire(),
            Some(SoundBackend::PulseAudio) => Self::toggle_mute_pulseaudio(),
            Some(SoundBackend::Alsa) => Self::toggle_mute_alsa(),
            None => Err(VolumeError::NotAvailable(
                "No sound backend available".into(),
            )),
        }
    }

    async fn set_mute(&self, muted: bool) -> VolumeResult<()> {
        match self.backend {
            Some(SoundBackend::PipeWire) => Self::set_mute_pipewire(muted),
            Some(SoundBackend::PulseAudio) => Self::set_mute_pulseaudio(muted),
            Some(SoundBackend::Alsa) => Self::set_mute_alsa(muted),
            None => Err(VolumeError::NotAvailable(
                "No sound backend available".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_name() {
        assert_eq!(SoundBackend::PipeWire.name(), "pipewire");
        assert_eq!(SoundBackend::PulseAudio.name(), "pulseaudio");
        assert_eq!(SoundBackend::Alsa.name(), "alsa");
    }
}
