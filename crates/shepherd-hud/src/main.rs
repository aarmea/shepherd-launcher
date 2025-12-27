//! Shepherd HUD - Always-visible overlay
//!
//! This is the heads-up display that remains visible during active sessions.
//! It shows time remaining, battery, volume, and provides session controls.

mod app;
mod battery;
mod state;
mod time_display;
mod volume;

use anyhow::Result;
use clap::Parser;
use gtk4::prelude::*;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

/// Shepherd HUD - Always-visible overlay for shepherdd sessions
#[derive(Parser, Debug)]
#[command(name = "shepherd-hud")]
#[command(about = "GTK4 layer-shell HUD for shepherdd", long_about = None)]
struct Args {
    /// Socket path for shepherdd connection
    #[arg(short, long, default_value = "/run/shepherdd/shepherdd.sock")]
    socket: PathBuf,

    /// Log level
    #[arg(short, long, default_value = "info")]
    log_level: String,

    /// Anchor position (top, bottom)
    #[arg(short, long, default_value = "top")]
    anchor: String,

    /// Height of the HUD bar in pixels
    #[arg(long, default_value = "48")]
    height: i32,
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

    tracing::info!("Starting Shepherd HUD");

    // Run GTK application
    let application = app::HudApp::new(args.socket, args.anchor, args.height);
    let exit_code = application.run();

    std::process::exit(exit_code);
}
