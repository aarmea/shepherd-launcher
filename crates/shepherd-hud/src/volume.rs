//! Volume monitoring and control module
//!
//! Monitors and controls system volume via PulseAudio/PipeWire.
//! Uses the `pactl` command-line tool for simplicity.

use std::process::Command;

/// Volume status
#[derive(Debug, Clone, Default)]
pub struct VolumeStatus {
    /// Volume percentage (0-100+)
    pub percent: u8,
    /// Whether audio is muted
    pub muted: bool,
}

impl VolumeStatus {
    /// Read volume status using pactl
    pub fn read() -> Self {
        let mut status = VolumeStatus::default();

        // Get default sink info
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                // Output looks like: "Volume: front-left: 65536 / 100% / -0.00 dB, front-right: ..."
                if let Some(percent_str) = stdout.split('/').nth(1) {
                    if let Ok(percent) = percent_str.trim().trim_end_matches('%').parse::<u8>() {
                        status.percent = percent;
                    }
                }
            }
        }

        // Check mute status
        if let Ok(output) = Command::new("pactl")
            .args(["get-sink-mute", "@DEFAULT_SINK@"])
            .output()
        {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                // Output looks like: "Mute: yes" or "Mute: no"
                status.muted = stdout.contains("yes");
            }
        }

        status
    }

    /// Toggle mute state
    pub fn toggle_mute() -> anyhow::Result<()> {
        Command::new("pactl")
            .args(["set-sink-mute", "@DEFAULT_SINK@", "toggle"])
            .status()?;
        Ok(())
    }

    /// Increase volume by a step
    pub fn volume_up(step: u8) -> anyhow::Result<()> {
        Command::new("pactl")
            .args([
                "set-sink-volume",
                "@DEFAULT_SINK@",
                &format!("+{}%", step),
            ])
            .status()?;
        Ok(())
    }

    /// Decrease volume by a step
    pub fn volume_down(step: u8) -> anyhow::Result<()> {
        Command::new("pactl")
            .args([
                "set-sink-volume",
                "@DEFAULT_SINK@",
                &format!("-{}%", step),
            ])
            .status()?;
        Ok(())
    }

    /// Set volume to a specific percentage
    pub fn set_volume(percent: u8) -> anyhow::Result<()> {
        Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", percent)])
            .status()?;
        Ok(())
    }

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
}
