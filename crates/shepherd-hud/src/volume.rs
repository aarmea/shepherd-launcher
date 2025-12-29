//! Volume monitoring and control module
//!
//! Provides volume status and control via shepherdd.
//! The service handles actual volume control and enforces restrictions.

use shepherd_api::{Command, ResponsePayload, VolumeInfo};
use shepherd_ipc::IpcClient;
use std::path::PathBuf;
use tokio::runtime::Runtime;

/// Get the default socket path from environment or fallback
fn get_socket_path() -> PathBuf {
    std::env::var("SHEPHERD_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./dev-runtime/shepherd.sock"))
}

/// Get current volume status from shepherdd
pub fn get_volume_status() -> Option<VolumeInfo> {
    let socket_path = get_socket_path();

    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("Failed to create runtime: {}", e);
            return None;
        }
    };

    rt.block_on(async {
        match IpcClient::connect(&socket_path).await {
            Ok(mut client) => match client.send(Command::GetVolume).await {
                Ok(response) => {
                    if let shepherd_api::ResponseResult::Ok(ResponsePayload::Volume(info)) =
                        response.result
                    {
                        Some(info)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to get volume: {}", e);
                    None
                }
            },
            Err(e) => {
                tracing::debug!("Failed to connect to shepherdd for volume: {}", e);
                None
            }
        }
    })
}

/// Toggle mute state via shepherdd
pub fn toggle_mute() -> anyhow::Result<()> {
    let socket_path = get_socket_path();

    let rt = Runtime::new()?;

    rt.block_on(async {
        let mut client = IpcClient::connect(&socket_path).await?;
        let response = client.send(Command::ToggleMute).await?;

        match response.result {
            shepherd_api::ResponseResult::Ok(ResponsePayload::VolumeSet) => Ok(()),
            shepherd_api::ResponseResult::Ok(ResponsePayload::VolumeDenied { reason }) => {
                Err(anyhow::anyhow!("Volume denied: {}", reason))
            }
            shepherd_api::ResponseResult::Err(e) => {
                Err(anyhow::anyhow!("Error: {}", e.message))
            }
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    })
}

/// Set volume to a specific percentage via shepherdd
pub fn set_volume(percent: u8) -> anyhow::Result<()> {
    let socket_path = get_socket_path();

    let rt = Runtime::new()?;

    rt.block_on(async {
        let mut client = IpcClient::connect(&socket_path).await?;
        let response = client.send(Command::SetVolume { percent }).await?;

        match response.result {
            shepherd_api::ResponseResult::Ok(ResponsePayload::VolumeSet) => Ok(()),
            shepherd_api::ResponseResult::Ok(ResponsePayload::VolumeDenied { reason }) => {
                Err(anyhow::anyhow!("Volume denied: {}", reason))
            }
            shepherd_api::ResponseResult::Err(e) => {
                Err(anyhow::anyhow!("Error: {}", e.message))
            }
            _ => Err(anyhow::anyhow!("Unexpected response")),
        }
    })
}

#[cfg(test)]
mod tests {
    use shepherd_api::VolumeRestrictions;

    #[test]
    fn test_volume_icon_names() {
        // Test that VolumeInfo::icon_name works correctly
        let info = shepherd_api::VolumeInfo {
            percent: 0,
            muted: false,
            available: true,
            backend: Some("test".into()),
            restrictions: VolumeRestrictions::unrestricted(),
        };
        assert_eq!(info.icon_name(), "audio-volume-muted-symbolic");

        let info = shepherd_api::VolumeInfo {
            percent: 50,
            muted: false,
            available: true,
            backend: Some("test".into()),
            restrictions: VolumeRestrictions::unrestricted(),
        };
        assert_eq!(info.icon_name(), "audio-volume-medium-symbolic");

        let info = shepherd_api::VolumeInfo {
            percent: 100,
            muted: true,
            available: true,
            backend: Some("test".into()),
            restrictions: VolumeRestrictions::unrestricted(),
        };
        assert_eq!(info.icon_name(), "audio-volume-muted-symbolic");
    }
}
