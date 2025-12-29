//! Protocol types for shepherdd IPC
//!
//! This crate defines the stable API between shepherdd and clients:
//! - Commands (requests from clients)
//! - Responses
//! - Events (service -> clients)
//! - Versioning

mod commands;
mod events;
mod types;

pub use commands::*;
pub use events::*;
pub use types::*;

/// Current API version
pub const API_VERSION: u32 = 1;
