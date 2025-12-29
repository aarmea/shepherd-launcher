# shepherd-host-linux

Linux host adapter for Shepherd.

## Overview

This crate implements the `HostAdapter` trait for Linux systems, providing:

- **Process spawning** with process group isolation
- **Process termination** via graceful (SIGTERM) and forceful (SIGKILL) signals
- **Exit observation** through async process monitoring
- **Snap application support** via systemd scope-based management
- **stdout/stderr capture** to log files
- **Volume control** with auto-detection of sound systems (PipeWire, PulseAudio, ALSA)

## Capabilities

The Linux adapter reports these capabilities:

```rust
HostCapabilities {
    // Supported entry kinds
    spawn_kind_supported: [Process, Snap],
    
    // Enforcement capabilities
    can_kill_forcefully: true,    // SIGKILL
    can_graceful_stop: true,      // SIGTERM
    can_group_process_tree: true, // Process groups (pgid)
    can_observe_exit: true,       // async wait
    
    // Optional features (not yet implemented)
    can_observe_window_ready: false,
    can_force_foreground: false,
    can_force_fullscreen: false,
    can_lock_to_single_app: false,
}
```

## Usage

### Creating the Adapter

```rust
use shepherd_host_linux::LinuxHost;

let host = LinuxHost::new();

// Check capabilities
let caps = host.capabilities();
assert!(caps.can_kill_forcefully);
```

### Spawning Processes

```rust
use shepherd_host_api::{SpawnOptions, EntryKind};

let entry_kind = EntryKind::Process {
    command: "/usr/bin/game".to_string(),
    args: vec!["--fullscreen".to_string()],
    env: Default::default(),
    cwd: None,
};

let options = SpawnOptions {
    capture_stdout: true,
    capture_stderr: true,
    log_path: Some("/var/log/shepherdd/sessions".into()),
    fullscreen: false,
    foreground: false,
};

let handle = host.spawn(session_id, &entry_kind, options).await?;
```

### Spawning Snap Applications

Snap applications are managed using systemd scopes for proper process tracking:

```rust
let entry_kind = EntryKind::Snap {
    snap_name: "mc-installer".to_string(),
    command: None,  // Defaults to snap_name
    args: vec![],
    env: Default::default(),
};

// Spawns via: snap run mc-installer
// Process group is isolated within a systemd scope
let handle = host.spawn(session_id, &entry_kind, options).await?;
```

### Stopping Sessions

```rust
use shepherd_host_api::StopMode;
use std::time::Duration;

// Graceful: SIGTERM, wait 5s, then SIGKILL
host.stop(&handle, StopMode::Graceful {
    timeout: Duration::from_secs(5),
}).await?;

// Force: immediate SIGKILL
host.stop(&handle, StopMode::Force).await?;
```

### Monitoring Exits

```rust
let mut events = host.subscribe();

tokio::spawn(async move {
    while let Some(event) = events.recv().await {
        match event {
            HostEvent::Exited { handle, status } => {
                println!("Session {} exited: {:?}", handle.session_id(), status);
            }
            _ => {}
        }
    }
});
```

## Volume Control

The crate includes `LinuxVolumeController` which auto-detects the available sound system:

```rust
use shepherd_host_linux::LinuxVolumeController;

let controller = LinuxVolumeController::new().await?;

// Get current volume (0-100)
let volume = controller.get_volume().await?;

// Set volume with enforcement of configured maximum
controller.set_volume(75).await?;

// Mute/unmute
controller.set_muted(true).await?;
```

### Sound System Detection Order

1. **PipeWire** (`wpctl` or `pw-cli`) - Modern default on Ubuntu 22.04+, Fedora
2. **PulseAudio** (`pactl`) - Legacy but widely available
3. **ALSA** (`amixer`) - Fallback for systems without a sound server

## Process Group Handling

All spawned processes are placed in their own process group:

```rust
// Internally uses setsid() or setpgid()
// This allows killing the entire process tree
```

When stopping a session:
1. SIGTERM is sent to the process group (`-pgid`)
2. After timeout, SIGKILL is sent to the process group
3. Orphaned children are cleaned up

## Log Capture

stdout and stderr can be captured to session log files:

```
/var/log/shepherdd/sessions/
├── 2025-01-15-abc123-minecraft.log
├── 2025-01-15-def456-gcompris.log
└── ...
```

## Future Enhancements

Planned features (hooks are designed in):

- **cgroups v2** - CPU/memory/IO limits per session
- **Namespace isolation** - Optional sandboxing
- **Sway/Wayland integration** - Focus and fullscreen control
- **D-Bus monitoring** - Window readiness detection

## Dependencies

- `nix` - Unix system calls
- `tokio` - Async runtime
- `tracing` - Logging
- `serde` - Serialization
- `shepherd-host-api` - Trait definitions
- `shepherd-api` - Entry types
