//! Main GTK4 application for the launcher

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::client::{ClientCommand, CommandClient, DaemonClient};
use crate::grid::LauncherGrid;
use crate::state::{LauncherState, SharedState};

/// CSS styling for the launcher
const LAUNCHER_CSS: &str = r#"
window {
    background-color: #1a1a2e;
}

.launcher-grid {
    padding: 48px;
}

.launcher-tile {
    background-color: #16213e;
    border-radius: 16px;
    padding: 16px;
    min-width: 140px;
    min-height: 140px;
    border: 2px solid transparent;
    transition: all 200ms ease;
}

.launcher-tile:hover {
    background-color: #1f3460;
    border-color: #4a90d9;
}

.launcher-tile:active {
    background-color: #0f3460;
}

.launcher-tile:disabled {
    opacity: 0.4;
}

.tile-label {
    color: #ffffff;
    font-size: 14px;
    font-weight: 500;
}

.status-label {
    color: #888888;
    font-size: 18px;
}

.error-label {
    color: #ff6b6b;
    font-size: 16px;
}

.launching-spinner {
    min-width: 64px;
    min-height: 64px;
}

.session-active-box {
    padding: 48px;
}

.session-label {
    color: #ffffff;
    font-size: 24px;
    font-weight: 600;
}
"#;

pub struct LauncherApp {
    socket_path: PathBuf,
}

impl LauncherApp {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn run(&self) -> i32 {
        let app = gtk4::Application::builder()
            .application_id("org.shepherd.launcher")
            .build();

        let socket_path = self.socket_path.clone();

        app.connect_activate(move |app| {
            Self::build_ui(app, socket_path.clone());
        });

        app.run().into()
    }

    fn build_ui(app: &gtk4::Application, socket_path: PathBuf) {
        // Load CSS
        let provider = gtk4::CssProvider::new();
        provider.load_from_string(LAUNCHER_CSS);
        gtk4::style_context_add_provider_for_display(
            &gtk4::gdk::Display::default().expect("Could not get default display"),
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Create main window
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Shepherd Launcher")
            .default_width(1280)
            .default_height(720)
            .build();

        // Make fullscreen
        window.fullscreen();

        // Create main stack for different views
        let stack = gtk4::Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::Crossfade);
        stack.set_transition_duration(300);

        // Create views
        let grid = LauncherGrid::new();
        let loading_view = Self::create_loading_view();
        let error_view = Self::create_error_view();
        let session_view = Self::create_session_view();
        let disconnected_view = Self::create_disconnected_view();

        stack.add_named(&grid, Some("grid"));
        stack.add_named(&loading_view, Some("loading"));
        stack.add_named(&error_view.0, Some("error"));
        stack.add_named(&session_view.0, Some("session"));
        stack.add_named(&disconnected_view.0, Some("disconnected"));

        window.set_child(Some(&stack));

        // Create shared state
        let state = SharedState::new();
        let state_receiver = state.subscribe();

        // Create tokio runtime for async operations
        let runtime = Arc::new(Runtime::new().expect("Failed to create tokio runtime"));

        // Create command channel
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        // Create command client for sending commands
        let command_client = Arc::new(CommandClient::new(&socket_path));

        // Connect grid launch callback
        let cmd_client = command_client.clone();
        let state_clone = state.clone();
        let rt = runtime.clone();
        grid.connect_launch(move |entry_id| {
            info!(entry_id = %entry_id, "Launch requested");
            state_clone.set(LauncherState::Launching {
                entry_id: entry_id.to_string(),
            });

            let client = cmd_client.clone();
            let state = state_clone.clone();
            let entry_id = entry_id.clone();
            rt.spawn(async move {
                match client.launch(&entry_id).await {
                    Ok(response) => {
                        debug!(response = ?response, "Launch response");
                        // State will be updated via events
                    }
                    Err(e) => {
                        error!(error = %e, "Launch failed");
                        state.set(LauncherState::Error {
                            message: format!("Launch failed: {}", e),
                        });
                    }
                }
            });
        });

        // Connect retry button
        let cmd_client = command_client.clone();
        let state_clone = state.clone();
        let rt = runtime.clone();
        disconnected_view.1.connect_clicked(move |_| {
            info!("Retry connection requested");
            state_clone.set(LauncherState::Connecting);

            let client = cmd_client.clone();
            let state = state_clone.clone();
            rt.spawn(async move {
                match client.get_state().await {
                    Ok(_) => {
                        // Will trigger state update
                    }
                    Err(e) => {
                        error!(error = %e, "Reconnect failed");
                        state.set(LauncherState::Disconnected);
                    }
                }
            });
        });

        // Start daemon client in background
        let state_for_client = state.clone();
        let socket_for_client = socket_path.clone();
        runtime.spawn(async move {
            let client = DaemonClient::new(socket_for_client, state_for_client, command_rx);
            client.run().await;
        });

        // Set up state change handler
        let stack_weak = stack.downgrade();
        let grid_weak = grid.downgrade();
        let error_label = error_view.1.clone();
        let session_label = session_view.1.clone();

        glib::spawn_future_local(async move {
            let mut receiver = state_receiver;

            loop {
                receiver.changed().await.ok();

                let state = receiver.borrow().clone();

                let Some(stack) = stack_weak.upgrade() else {
                    break;
                };

                let grid = grid_weak.upgrade();

                match state {
                    LauncherState::Disconnected => {
                        stack.set_visible_child_name("disconnected");
                    }
                    LauncherState::Connecting => {
                        stack.set_visible_child_name("loading");
                    }
                    LauncherState::Idle { entries } => {
                        if let Some(grid) = grid {
                            grid.set_entries(entries);
                            grid.set_tiles_sensitive(true);
                        }
                        stack.set_visible_child_name("grid");
                    }
                    LauncherState::Launching { entry_id } => {
                        if let Some(grid) = grid {
                            grid.set_tiles_sensitive(false);
                        }
                        stack.set_visible_child_name("loading");
                    }
                    LauncherState::SessionActive {
                        session_id: _,
                        entry_label,
                        time_remaining: _,
                    } => {
                        session_label.set_text(&format!("Running: {}", entry_label));
                        stack.set_visible_child_name("session");
                    }
                    LauncherState::Error { message } => {
                        error_label.set_text(&message);
                        stack.set_visible_child_name("error");
                    }
                }
            }
        });

        window.present();
    }

    fn create_loading_view() -> gtk4::Box {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);

        let spinner = gtk4::Spinner::new();
        spinner.set_spinning(true);
        spinner.add_css_class("launching-spinner");
        container.append(&spinner);

        let label = gtk4::Label::new(Some("Loading..."));
        label.add_css_class("status-label");
        container.append(&label);

        container
    }

    fn create_error_view() -> (gtk4::Box, gtk4::Label) {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 16);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);

        let icon = gtk4::Image::from_icon_name("dialog-error");
        icon.set_pixel_size(64);
        container.append(&icon);

        let label = gtk4::Label::new(Some("An error occurred"));
        label.add_css_class("error-label");
        label.set_wrap(true);
        label.set_max_width_chars(40);
        container.append(&label);

        (container, label)
    }

    fn create_session_view() -> (gtk4::Box, gtk4::Label) {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);
        container.add_css_class("session-active-box");

        let label = gtk4::Label::new(Some("Session Active"));
        label.add_css_class("session-label");
        container.append(&label);

        let hint = gtk4::Label::new(Some("Use the HUD to view time remaining"));
        hint.add_css_class("status-label");
        container.append(&hint);

        (container, label)
    }

    fn create_disconnected_view() -> (gtk4::Box, gtk4::Button) {
        let container = gtk4::Box::new(gtk4::Orientation::Vertical, 24);
        container.set_halign(gtk4::Align::Center);
        container.set_valign(gtk4::Align::Center);

        let icon = gtk4::Image::from_icon_name("network-offline");
        icon.set_pixel_size(64);
        container.append(&icon);

        let label = gtk4::Label::new(Some("System not ready"));
        label.add_css_class("status-label");
        container.append(&label);

        let retry_button = gtk4::Button::with_label("Retry");
        retry_button.add_css_class("launcher-tile");
        container.append(&retry_button);

        (container, retry_button)
    }
}
