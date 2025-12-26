//! Host capabilities model

use serde::{Deserialize, Serialize};
use shepherd_api::EntryKindTag;
use std::collections::HashSet;

/// Describes what a host adapter can do
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostCapabilities {
    /// Entry kinds this host can spawn
    pub spawn_kinds_supported: HashSet<EntryKindTag>,

    /// Can forcefully kill processes/sessions
    pub can_kill_forcefully: bool,

    /// Can attempt graceful stop (e.g., SIGTERM)
    pub can_graceful_stop: bool,

    /// Can group process trees (process groups, job objects)
    pub can_group_process_tree: bool,

    /// Can observe process exit
    pub can_observe_exit: bool,

    /// Can detect when window/app is ready (optional)
    pub can_observe_window_ready: bool,

    /// Can force an app to foreground (optional)
    pub can_force_foreground: bool,

    /// Can force fullscreen mode (optional)
    pub can_force_fullscreen: bool,

    /// Can lock to single app (MDM/kiosk mode, optional)
    pub can_lock_to_single_app: bool,
}

impl HostCapabilities {
    /// Create minimal capabilities (process spawn/kill only)
    pub fn minimal() -> Self {
        let mut spawn_kinds = HashSet::new();
        spawn_kinds.insert(EntryKindTag::Process);

        Self {
            spawn_kinds_supported: spawn_kinds,
            can_kill_forcefully: true,
            can_graceful_stop: true,
            can_group_process_tree: false,
            can_observe_exit: true,
            can_observe_window_ready: false,
            can_force_foreground: false,
            can_force_fullscreen: false,
            can_lock_to_single_app: false,
        }
    }

    /// Create capabilities for a full Linux host with Sway
    pub fn linux_full() -> Self {
        let mut spawn_kinds = HashSet::new();
        spawn_kinds.insert(EntryKindTag::Process);
        spawn_kinds.insert(EntryKindTag::Vm);
        spawn_kinds.insert(EntryKindTag::Media);

        Self {
            spawn_kinds_supported: spawn_kinds,
            can_kill_forcefully: true,
            can_graceful_stop: true,
            can_group_process_tree: true,
            can_observe_exit: true,
            can_observe_window_ready: true,
            can_force_foreground: true,
            can_force_fullscreen: true,
            can_lock_to_single_app: false, // Would need additional setup
        }
    }

    /// Check if this host can spawn the given entry kind
    pub fn supports_kind(&self, kind: EntryKindTag) -> bool {
        self.spawn_kinds_supported.contains(&kind)
    }
}

impl Default for HostCapabilities {
    fn default() -> Self {
        Self::minimal()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_capabilities() {
        let caps = HostCapabilities::minimal();
        assert!(caps.supports_kind(EntryKindTag::Process));
        assert!(!caps.supports_kind(EntryKindTag::Vm));
        assert!(caps.can_kill_forcefully);
    }

    #[test]
    fn linux_full_capabilities() {
        let caps = HostCapabilities::linux_full();
        assert!(caps.supports_kind(EntryKindTag::Process));
        assert!(caps.supports_kind(EntryKindTag::Vm));
        assert!(caps.can_group_process_tree);
    }
}
