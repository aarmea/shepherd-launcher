//! Input handling for keyboard and gamepad navigation

use gilrs::{Axis, Button, Event as GilrsEvent, EventType, Gilrs};
use gtk4::gdk;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{debug, trace, warn};

/// Navigation commands that can be triggered by keyboard or gamepad
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavCommand {
    /// Move selection up
    Up,
    /// Move selection down
    Down,
    /// Move selection left
    Left,
    /// Move selection right
    Right,
    /// Activate/launch the selected item
    Activate,
}

/// Threshold for analog stick to register as a direction
const AXIS_THRESHOLD: f32 = 0.5;

/// Deadzone for analog stick (ignore small movements)
const AXIS_DEADZONE: f32 = 0.2;

/// Delay before repeating analog stick navigation (ms)
const ANALOG_REPEAT_DELAY_MS: u64 = 200;

/// Maps a GTK key to a navigation command
pub fn key_to_nav_command(keyval: gdk::Key) -> Option<NavCommand> {
    match keyval {
        gdk::Key::Up | gdk::Key::KP_Up | gdk::Key::w | gdk::Key::W => Some(NavCommand::Up),
        gdk::Key::Down | gdk::Key::KP_Down | gdk::Key::s | gdk::Key::S => Some(NavCommand::Down),
        gdk::Key::Left | gdk::Key::KP_Left | gdk::Key::a | gdk::Key::A => Some(NavCommand::Left),
        gdk::Key::Right | gdk::Key::KP_Right | gdk::Key::d | gdk::Key::D => Some(NavCommand::Right),
        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::space => Some(NavCommand::Activate),
        _ => None,
    }
}

/// Handles gamepad input in a background thread and sends navigation commands
pub struct GamepadHandler {
    command_rx: mpsc::Receiver<NavCommand>,
    _thread_handle: thread::JoinHandle<()>,
}

impl GamepadHandler {
    /// Create a new gamepad handler that polls for input
    pub fn new() -> Option<Self> {
        let (tx, rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("gamepad-input".into())
            .spawn(move || {
                Self::gamepad_loop(tx);
            })
            .ok()?;

        Some(Self {
            command_rx: rx,
            _thread_handle: handle,
        })
    }

    /// Try to receive any pending navigation commands (non-blocking)
    pub fn try_recv(&self) -> Option<NavCommand> {
        self.command_rx.try_recv().ok()
    }

    /// The main gamepad polling loop
    fn gamepad_loop(tx: mpsc::Sender<NavCommand>) {
        let gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                warn!(error = %e, "Failed to initialize gamepad support");
                return;
            }
        };

        debug!("Gamepad handler initialized");

        // Log connected gamepads
        for (_id, gamepad) in gilrs.gamepads() {
            debug!(
                name = gamepad.name(),
                "Found gamepad"
            );
        }

        let mut gilrs = gilrs;

        // Track analog stick state for repeat navigation
        let mut last_analog_nav: Option<(NavCommand, std::time::Instant)> = None;
        let mut axis_x: f32 = 0.0;
        let mut axis_y: f32 = 0.0;

        loop {
            // Process all pending events
            while let Some(GilrsEvent { event, .. }) = gilrs.next_event() {
                match event {
                    EventType::ButtonPressed(button, _) => {
                        if let Some(cmd) = Self::button_to_nav_command(button) {
                            trace!(button = ?button, command = ?cmd, "Gamepad button pressed");
                            let _ = tx.send(cmd);
                        }
                    }
                    EventType::AxisChanged(axis, value, _) => {
                        match axis {
                            Axis::LeftStickX | Axis::RightStickX => {
                                axis_x = value;
                            }
                            Axis::LeftStickY | Axis::RightStickY => {
                                axis_y = value;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }

            // Handle analog stick navigation with repeat
            let analog_cmd = Self::axis_to_nav_command(axis_x, axis_y);

            match (analog_cmd, last_analog_nav.as_ref()) {
                (Some(cmd), None) => {
                    // New direction - send immediately
                    trace!(command = ?cmd, "Analog stick navigation");
                    let _ = tx.send(cmd);
                    last_analog_nav = Some((cmd, std::time::Instant::now()));
                }
                (Some(cmd), Some((last_cmd, last_time))) => {
                    if cmd != *last_cmd {
                        // Direction changed - send immediately
                        trace!(command = ?cmd, "Analog stick direction changed");
                        let _ = tx.send(cmd);
                        last_analog_nav = Some((cmd, std::time::Instant::now()));
                    } else if last_time.elapsed() >= Duration::from_millis(ANALOG_REPEAT_DELAY_MS) {
                        // Same direction held - repeat
                        trace!(command = ?cmd, "Analog stick repeat");
                        let _ = tx.send(cmd);
                        last_analog_nav = Some((cmd, std::time::Instant::now()));
                    }
                }
                (None, _) => {
                    // Stick returned to center
                    last_analog_nav = None;
                }
            }

            // Sleep briefly to avoid busy-waiting
            thread::sleep(Duration::from_millis(16)); // ~60 Hz
        }
    }

    /// Maps a gamepad button to a navigation command
    fn button_to_nav_command(button: Button) -> Option<NavCommand> {
        match button {
            // D-pad
            Button::DPadUp => Some(NavCommand::Up),
            Button::DPadDown => Some(NavCommand::Down),
            Button::DPadLeft => Some(NavCommand::Left),
            Button::DPadRight => Some(NavCommand::Right),
            // Action buttons - A, B, and Start all activate
            Button::South => Some(NavCommand::Activate), // A on Xbox/Switch, X on PlayStation
            Button::East => Some(NavCommand::Activate),  // B on Xbox, Circle on PlayStation
            Button::Start => Some(NavCommand::Activate),
            _ => None,
        }
    }

    /// Maps analog stick axes to a navigation command
    fn axis_to_nav_command(x: f32, y: f32) -> Option<NavCommand> {
        // Apply deadzone
        let x = if x.abs() < AXIS_DEADZONE { 0.0 } else { x };
        let y = if y.abs() < AXIS_DEADZONE { 0.0 } else { y };

        // Determine primary direction (prioritize the axis with larger magnitude)
        if x.abs() > y.abs() {
            // Horizontal movement
            if x > AXIS_THRESHOLD {
                Some(NavCommand::Right)
            } else if x < -AXIS_THRESHOLD {
                Some(NavCommand::Left)
            } else {
                None
            }
        } else {
            // Vertical movement (note: Y axis is typically inverted on gamepads)
            if y > AXIS_THRESHOLD {
                Some(NavCommand::Down)
            } else if y < -AXIS_THRESHOLD {
                Some(NavCommand::Up)
            } else {
                None
            }
        }
    }
}

impl Default for GamepadHandler {
    fn default() -> Self {
        Self::new().expect("Failed to create gamepad handler")
    }
}
