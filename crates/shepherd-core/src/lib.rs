//! Core policy engine and session state machine for shepherdd
//!
//! This crate is the heart of shepherdd, containing:
//! - Policy evaluation (what's available, when, for how long)
//! - Session state machine (Idle -> Launching -> Running -> Warned -> Expiring -> Ended)
//! - Warning and expiry scheduling
//! - Time enforcement using monotonic time

mod engine;
mod events;
mod session;

pub use engine::*;
pub use events::*;
pub use session::*;
