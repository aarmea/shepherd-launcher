//! Shepherd Launcher UI - Main grid interface
//!
//! This is the primary user-facing shell for the kiosk-style environment.
//! It displays available entries from shepherdd and allows launching them.

mod app;
mod client;
mod grid;
mod input;
mod state;
mod tile;

use anyhow::Result;
use clap::Parser;
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
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(&args.log_level)),
        )
        .init();

    tracing::info!("Starting Shepherd Launcher UI");

    // Determine socket path with fallback to default
    let socket_path = args.socket.unwrap_or_else(default_socket_path);

    // Run GTK application
    let application = app::LauncherApp::new(socket_path);
    let exit_code = application.run();

    std::process::exit(exit_code);
}
