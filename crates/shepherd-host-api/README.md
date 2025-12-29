# shepherd-host-api

Host adapter trait interfaces for Shepherd.

## Overview

This crate defines the capability-based interface between the Shepherd core and platform-specific implementations. It contains **no platform code itself**—only traits, types, and a mock implementation for testing.

### Purpose

Desktop operating systems have fundamentally different process control models:
- **Linux** can kill process groups and use cgroups
- **macOS** requires MDM for true kiosk mode
- **Windows** uses job objects and shell policies
- **Android** has managed launcher and device-owner workflows

`shepherd-host-api` acknowledges these differences honestly through a capability-based design rather than pretending all platforms are equivalent.

## Key Concepts

### Capabilities

The `HostCapabilities` struct declares what a host adapter can actually do:

```rust
use shepherd_host_api::HostCapabilities;

let caps = host.capabilities();

// Check supported entry kinds
if caps.supports_kind(EntryKindTag::Process) { /* ... */ }
if caps.supports_kind(EntryKindTag::Snap) { /* ... */ }

// Check enforcement capabilities
if caps.can_kill_forcefully { /* Can use SIGKILL/TerminateProcess */ }
if caps.can_graceful_stop { /* Can request graceful shutdown */ }
if caps.can_group_process_tree { /* Can kill entire process tree */ }

// Check optional features
if caps.can_observe_window_ready { /* Get notified when GUI appears */ }
if caps.can_force_foreground { /* Can bring window to front */ }
if caps.can_force_fullscreen { /* Can set fullscreen mode */ }
```

The core engine uses these capabilities to:
- Filter available entries (don't show what can't be run)
- Choose termination strategies
- Decide whether to attempt optional behaviors

### Host Adapter Trait

Platform adapters implement this trait:

```rust
use shepherd_host_api::{HostAdapter, SpawnOptions, StopMode, HostEvent};

#[async_trait]
pub trait HostAdapter: Send + Sync {
    /// Get the capabilities of this host adapter
    fn capabilities(&self) -> &HostCapabilities;

    /// Spawn a new session
    async fn spawn(
        &self,
        session_id: SessionId,
        entry_kind: &EntryKind,
        options: SpawnOptions,
    ) -> HostResult<HostSessionHandle>;

    /// Stop a running session
    async fn stop(&self, handle: &HostSessionHandle, mode: StopMode) -> HostResult<()>;

    /// Subscribe to host events (exits, window ready, etc.)
    fn subscribe(&self) -> mpsc::UnboundedReceiver<HostEvent>;

    // Optional methods with default implementations
    async fn set_foreground(&self, handle: &HostSessionHandle) -> HostResult<()>;
    async fn set_fullscreen(&self, handle: &HostSessionHandle) -> HostResult<()>;
    async fn ensure_shell_visible(&self) -> HostResult<()>;
}
```

### Session Handles

`HostSessionHandle` is an opaque container for platform-specific identifiers:

```rust
use shepherd_host_api::HostSessionHandle;

// Created by spawn(), contains platform-specific data
let handle: HostSessionHandle = host.spawn(session_id, &entry_kind, options).await?;

// On Linux, internally contains pid, pgid
// On Windows, would contain job object handle
// On macOS, might contain bundle identifier
```

### Stop Modes

Session termination can be graceful or forced:

```rust
use shepherd_host_api::StopMode;
use std::time::Duration;

// Try graceful shutdown with timeout, then force
host.stop(&handle, StopMode::Graceful { 
    timeout: Duration::from_secs(5) 
}).await?;

// Immediate termination
host.stop(&handle, StopMode::Force).await?;
```

### Host Events

Adapters emit events via an async channel:

```rust
use shepherd_host_api::HostEvent;

let mut events = host.subscribe();
while let Some(event) = events.recv().await {
    match event {
        HostEvent::Exited { handle, status } => {
            // Process ended (normally or killed)
        }
        HostEvent::WindowReady { handle } => {
            // GUI window appeared (if observable)
        }
        HostEvent::SpawnFailed { session_id, error } => {
            // Launch failed after handle was created
        }
    }
}
```

### Volume Control

The crate also defines a volume controller interface:

```rust
use shepherd_host_api::VolumeController;

#[async_trait]
pub trait VolumeController: Send + Sync {
    /// Get current volume (0-100)
    async fn get_volume(&self) -> HostResult<u8>;
    
    /// Set volume (0-100)
    async fn set_volume(&self, level: u8) -> HostResult<()>;
    
    /// Check if muted
    async fn is_muted(&self) -> HostResult<bool>;
    
    /// Set mute state
    async fn set_muted(&self, muted: bool) -> HostResult<()>;
    
    /// Subscribe to volume changes
    fn subscribe(&self) -> mpsc::UnboundedReceiver<VolumeEvent>;
}
```

## Mock Implementation

For testing, the crate provides `MockHost`:

```rust
use shepherd_host_api::MockHost;

let mock = MockHost::new();

// Spawns will "succeed" with fake handles
let handle = mock.spawn(session_id, &entry_kind, options).await?;

// Inject events for testing
mock.inject_exit(handle.clone(), ExitStatus::Code(0));
```

## Design Philosophy

- **Honest capabilities** - Don't pretend all platforms are equal
- **Platform code stays out** - This crate is pure interface
- **Extensible** - New capabilities can be added without breaking existing adapters
- **Testable** - Mock implementation enables unit testing

## Available Implementations

| Adapter | Crate | Status |
|---------|-------|--------|
| Linux | `shepherd-host-linux` | Implemented |
| macOS | `shepherd-host-macos` | Planned |
| Windows | `shepherd-host-windows` | Planned |
| Android | `shepherd-host-android` | Planned |

## Dependencies

- `async-trait` - Async trait support
- `tokio` - Async runtime types
- `serde` - Serialization for handles
- `shepherd-api` - Entry kind types
- `shepherd-util` - ID types
