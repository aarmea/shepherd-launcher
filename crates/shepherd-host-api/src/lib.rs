//! Host adapter trait interfaces for shepherdd
//!
//! This crate defines the capability-based interface between the daemon core
//! and platform-specific implementations. It contains no platform code itself.

mod capabilities;
mod handle;
mod mock;
mod traits;
mod volume;

pub use capabilities::*;
pub use handle::*;
pub use mock::*;
pub use traits::*;
pub use volume::*;
