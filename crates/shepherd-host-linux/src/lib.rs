//! Linux host adapter for shepherdd
//!
//! Provides:
//! - Process spawning with process group isolation
//! - Graceful (SIGTERM) and forceful (SIGKILL) termination
//! - Exit observation
//! - stdout/stderr capture
//! - Volume control with auto-detection of sound systems

mod adapter;
mod process;
mod volume;

pub use adapter::*;
pub use process::*;
pub use volume::*;
