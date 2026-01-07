//! Linux host adapter for shepherdd
//!
//! Provides:
//! - Process spawning with process group isolation
//! - Graceful (SIGTERM) and forceful (SIGKILL) termination
//! - Exit observation
//! - stdout/stderr capture
//! - Volume control with auto-detection of sound systems
//! - Network connectivity monitoring via netlink

mod adapter;
mod connectivity;
mod process;
mod volume;

pub use adapter::*;
pub use connectivity::*;
pub use process::*;
pub use volume::*;
