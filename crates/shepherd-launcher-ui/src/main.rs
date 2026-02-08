//! Shepherd Launcher UI - Main grid interface
//!
//! This is the primary user-facing shell for the kiosk-style environment.
//! It displays available entries from shepherdd and allows launching them.

mod app;
mod client;
mod grid;
mod state;
mod tile;

use crate::client::CommandClient;
use anyhow::Result;
use clap::Parser;
use shepherd_api::{ErrorCode, ResponsePayload, ResponseResult};
use shepherd_util::default_socket_path;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// Shepherd Launcher - Child-friendly kiosk launcher
#[derive(Parser, Debug)]
#[command(name = "shepherd-launcher")]
#[command(about = "GTK4 launcher UI for shepherdd", long_about = None)]
struct Args {
    /// Socket path for shepherdd connection (or set SHEPHERD_SOCKET env var)
    #[arg(short, long, env = "SHEPHERD_SOCKET")]
    socket: Option<PathBuf>,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Send StopCurrent to shepherdd and exit (for compositor keybindings)
    #[arg(long)]
    stop_current: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level)),
        )
        .init();

    tracing::info!("Starting Shepherd Launcher UI");

    // Determine socket path with fallback to default
    let socket_path = args.socket.unwrap_or_else(default_socket_path);

    if args.stop_current {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async move {
            let client = CommandClient::new(&socket_path);
            match client.stop_current().await {
                Ok(response) => match response.result {
                    ResponseResult::Ok(ResponsePayload::Stopped) => {
                        tracing::info!("StopCurrent succeeded");
                        Ok(())
                    }
                    ResponseResult::Err(err) if err.code == ErrorCode::NoActiveSession => {
                        tracing::debug!("No active session to stop");
                        Ok(())
                    }
                    ResponseResult::Err(err) => {
                        anyhow::bail!("StopCurrent failed: {}", err.message)
                    }
                    ResponseResult::Ok(payload) => {
                        anyhow::bail!("Unexpected StopCurrent response: {:?}", payload)
                    }
                },
                Err(e) => anyhow::bail!("Failed to send StopCurrent: {}", e),
            }
        })?;
        return Ok(());
    }

    // Run GTK application
    let application = app::LauncherApp::new(socket_path);
    let exit_code = application.run();

    std::process::exit(exit_code);
}
