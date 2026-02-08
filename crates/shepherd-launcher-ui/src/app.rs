//! Main GTK4 application for the launcher

use gtk4::glib;
use gtk4::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::client::{CommandClient, ServiceClient};
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
    background: #16213e;
    background-color: #16213e;
    border-radius: 16px;
    padding: 16px;
    min-width: 140px;
    min-height: 140px;
    border: 2px solid transparent;
    transition: all 200ms ease;
    color: #e0e0e0;
    box-shadow: none;
}

.launcher-tile:hover {
    background: #1f3460;
    background-color: #1f3460;
    border-color: #4a90d9;
}

.launcher-tile:focus,
.launcher-tile:focus-visible {
    background: #1f3460;
    background-color: #1f3460;
    border-color: #ffd166;
}

.launcher-tile:active {
    background: #0f3460;
    background-color: #0f3460;
}

.launcher-tile:disabled {
    opacity: 0.4;
}

.tile-label {
    color: #e0e0e0;
    font-size: 14px;
    font-weight: 500;
}

.launcher-tile image {
    -gtk-icon-style: regular;
    color: #e0e0e0;
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

.session-sublabel {
    color: #888888;
    font-size: 16px;
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
        provider.load_from_data(LAUNCHER_CSS);
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
        Self::setup_keyboard_input(&window, &grid);
        Self::setup_gamepad_input(&window, &grid);

        // Create shared state
        let state = SharedState::new();
        let state_receiver = state.subscribe();

        // Create tokio runtime for async operations
        let runtime = Arc::new(Runtime::new().expect("Failed to create tokio runtime"));

        // Create command channel
        let (_command_tx, command_rx) = mpsc::unbounded_channel();

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
                        // Handle error responses from shepherdd
                        match response.result {
                            shepherd_api::ResponseResult::Ok(payload) => {
                                // Check what kind of success response we got
                                match payload {
                                    shepherd_api::ResponsePayload::LaunchApproved { session_id, deadline } => {
                                        info!(session_id = %session_id, "Launch approved, setting SessionActive");
                                        let now = shepherd_util::now();
                                        // For unlimited sessions (deadline=None), time_remaining is None
                                        let time_remaining = deadline.and_then(|d| {
                                            if d > now {
                                                (d - now).to_std().ok()
                                            } else {
                                                Some(std::time::Duration::ZERO)
                                            }
                                        });
                                        state.set(LauncherState::SessionActive {
                                            session_id,
                                            entry_label: entry_id.to_string(),
                                            time_remaining,
                                        });
                                    }
                                    shepherd_api::ResponsePayload::LaunchDenied { reasons } => {
                                        let message = reasons
                                            .iter()
                                            .map(|r| format!("{:?}", r))
                                            .collect::<Vec<_>>()
                                            .join(", ");
                                        error!(message = %message, "Launch denied");
                                        state.set(LauncherState::Error { message });
                                    }
                                    _ => {
                                        // Other OK responses - events will update state
                                    }
                                }
                            }
                            shepherd_api::ResponseResult::Err(err) => {
                                // Launch failed on server side - refresh state to recover
                                error!(error = %err.message, "Launch failed on server");
                                // Request fresh state from shepherdd to get back to correct state
                                match client.get_state().await {
                                    Ok(state_resp) => {
                                        if let shepherd_api::ResponseResult::Ok(
                                            shepherd_api::ResponsePayload::State(snapshot)
                                        ) = state_resp.result {
                                            if snapshot.current_session.is_some() {
                                                // Session is still active somehow
                                                debug!("Session still active after spawn failure");
                                            } else {
                                                // No session - return to idle with entries
                                                state.set(LauncherState::Idle {
                                                    entries: snapshot.entries,
                                                });
                                            }
                                        } else {
                                            // Unexpected response, show error
                                            state.set(LauncherState::Error {
                                                message: format!("Launch failed: {}", err.message),
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        // Can't get state, show error
                                        error!(error = %e, "Failed to get state after launch failure");
                                        state.set(LauncherState::Error {
                                            message: format!("Launch failed: {}", err.message),
                                        });
                                    }
                                }
                            }
                        }
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

        // Start shepherdd client in background thread (separate from GTK main loop)
        // This ensures the tokio runtime is properly driven for event reception
        let state_for_client = state.clone();
        let socket_for_client = socket_path.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime for event loop");
            rt.block_on(async move {
                let client = ServiceClient::new(socket_for_client, state_for_client, command_rx);
                client.run().await;
            });
        });

        // Set up state change handler
        let stack_weak = stack.downgrade();
        let grid_weak = grid.downgrade();
        let window_weak = window.downgrade();
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
                let window = window_weak.upgrade();

                match state {
                    LauncherState::Disconnected => {
                        if let Some(ref win) = window {
                            win.set_visible(true);
                        }
                        stack.set_visible_child_name("disconnected");
                    }
                    LauncherState::Connecting => {
                        if let Some(ref win) = window {
                            win.set_visible(true);
                        }
                        stack.set_visible_child_name("loading");
                    }
                    LauncherState::Idle { entries } => {
                        if let Some(grid) = grid {
                            grid.set_entries(entries);
                            grid.set_tiles_sensitive(true);
                            grid.grab_focus();
                        }
                        if let Some(ref win) = window {
                            win.set_visible(true);
                        }
                        stack.set_visible_child_name("grid");
                    }
                    LauncherState::Launching { entry_id: _ } => {
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
                        session_label.set_text(&format!("Loading: {}", entry_label));
                        // Show the session view as a loading screen behind the game
                        // The game window will appear on top when it launches
                        if let Some(ref win) = window {
                            win.set_visible(true);
                        }
                        stack.set_visible_child_name("session");
                    }
                    LauncherState::Error { message } => {
                        if let Some(ref win) = window {
                            win.set_visible(true);
                        }
                        error_label.set_text(&message);
                        stack.set_visible_child_name("error");
                    }
                }
            }
        });

        window.present();
    }

    fn setup_keyboard_input(window: &gtk4::ApplicationWindow, grid: &LauncherGrid) {
        let key_controller = gtk4::EventControllerKey::new();
        key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
        let grid_weak = grid.downgrade();
        key_controller.connect_key_pressed(move |_, key, _, _| {
            let Some(grid) = grid_weak.upgrade() else {
                return glib::Propagation::Proceed;
            };

            let handled = match key {
                gtk4::gdk::Key::Up | gtk4::gdk::Key::w | gtk4::gdk::Key::W => {
                    grid.move_selection(0, -1);
                    true
                }
                gtk4::gdk::Key::Down | gtk4::gdk::Key::s | gtk4::gdk::Key::S => {
                    grid.move_selection(0, 1);
                    true
                }
                gtk4::gdk::Key::Left | gtk4::gdk::Key::a | gtk4::gdk::Key::A => {
                    grid.move_selection(-1, 0);
                    true
                }
                gtk4::gdk::Key::Right | gtk4::gdk::Key::d | gtk4::gdk::Key::D => {
                    grid.move_selection(1, 0);
                    true
                }
                gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter | gtk4::gdk::Key::space => {
                    grid.launch_selected();
                    true
                }
                _ => false,
            };

            if handled {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(key_controller);

        let exit_controller = gtk4::EventControllerKey::new();
        let window_weak = window.downgrade();
        exit_controller.connect_key_pressed(move |_, key, _, modifiers| {
            let alt_f4 = key == gtk4::gdk::Key::F4
                && modifiers.intersects(gtk4::gdk::ModifierType::ALT_MASK);
            let ctrl_w = (key == gtk4::gdk::Key::w || key == gtk4::gdk::Key::W)
                && modifiers.intersects(gtk4::gdk::ModifierType::CONTROL_MASK);
            let home = key == gtk4::gdk::Key::Home || key == gtk4::gdk::Key::HomePage;

            if alt_f4 || ctrl_w || home {
                if let Some(window) = window_weak.upgrade() {
                    window.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        window.add_controller(exit_controller);
    }

    fn setup_gamepad_input(window: &gtk4::ApplicationWindow, grid: &LauncherGrid) {
        let mut gilrs = match gilrs::Gilrs::new() {
            Ok(gilrs) => gilrs,
            Err(e) => {
                warn!(error = %e, "Gamepad input unavailable");
                return;
            }
        };

        let grid_weak = grid.downgrade();
        let window_weak = window.downgrade();
        let mut axis_state = GamepadAxisState::default();

        glib::timeout_add_local(Duration::from_millis(16), move || {
            while let Some(event) = gilrs.next_event() {
                let Some(grid) = grid_weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };

                match event.event {
                    gilrs::EventType::ButtonPressed(button, _) => match button {
                        gilrs::Button::DPadUp => grid.move_selection(0, -1),
                        gilrs::Button::DPadDown => grid.move_selection(0, 1),
                        gilrs::Button::DPadLeft => grid.move_selection(-1, 0),
                        gilrs::Button::DPadRight => grid.move_selection(1, 0),
                        gilrs::Button::South | gilrs::Button::East | gilrs::Button::Start => {
                            grid.launch_selected();
                        }
                        gilrs::Button::Mode => {
                            if let Some(window) = window_weak.upgrade() {
                                window.close();
                                return glib::ControlFlow::Break;
                            }
                        }
                        _ => {}
                    },
                    gilrs::EventType::AxisChanged(axis, value, _) => {
                        Self::handle_gamepad_axis(&grid, axis, value, &mut axis_state);
                    }
                    _ => {}
                }
            }

            glib::ControlFlow::Continue
        });
    }

    fn handle_gamepad_axis(
        grid: &LauncherGrid,
        axis: gilrs::Axis,
        value: f32,
        axis_state: &mut GamepadAxisState,
    ) {
        const THRESHOLD: f32 = 0.65;

        match axis {
            gilrs::Axis::LeftStickX | gilrs::Axis::DPadX => {
                if value <= -THRESHOLD {
                    if !axis_state.left {
                        grid.move_selection(-1, 0);
                    }
                    axis_state.left = true;
                    axis_state.right = false;
                } else if value >= THRESHOLD {
                    if !axis_state.right {
                        grid.move_selection(1, 0);
                    }
                    axis_state.right = true;
                    axis_state.left = false;
                } else {
                    axis_state.left = false;
                    axis_state.right = false;
                }
            }
            gilrs::Axis::LeftStickY => {
                if value <= -THRESHOLD {
                    if !axis_state.down {
                        grid.move_selection(0, 1);
                    }
                    axis_state.down = true;
                    axis_state.up = false;
                } else if value >= THRESHOLD {
                    if !axis_state.up {
                        grid.move_selection(0, -1);
                    }
                    axis_state.up = true;
                    axis_state.down = false;
                } else {
                    axis_state.up = false;
                    axis_state.down = false;
                }
            }
            gilrs::Axis::DPadY => {
                if value <= -THRESHOLD {
                    if !axis_state.up {
                        grid.move_selection(0, -1);
                    }
                    axis_state.up = true;
                    axis_state.down = false;
                } else if value >= THRESHOLD {
                    if !axis_state.down {
                        grid.move_selection(0, 1);
                    }
                    axis_state.down = true;
                    axis_state.up = false;
                } else {
                    axis_state.up = false;
                    axis_state.down = false;
                }
            }
            _ => {}
        }
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

        let spinner = gtk4::Spinner::new();
        spinner.set_spinning(true);
        spinner.add_css_class("launching-spinner");
        container.append(&spinner);

        let label = gtk4::Label::new(Some("Loading..."));
        label.add_css_class("session-label");
        container.append(&label);

        let hint = gtk4::Label::new(Some("Please wait while the application starts"));
        hint.add_css_class("session-sublabel");
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

#[derive(Default)]
struct GamepadAxisState {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}
