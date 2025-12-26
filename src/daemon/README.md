# Daemon and IPC Implementation

This directory contains the daemon process and IPC (Inter-Process Communication) implementation for shepherd-launcher.

## Architecture

The application uses a multi-process architecture:
- **Main Process**: Spawns the daemon and runs the UI
- **Daemon Process**: Background service that handles application launching and state management
- **IPC**: Unix domain sockets for communication between processes

## Files

- `mod.rs`: Module exports
- `daemon.rs`: Daemon process implementation
- `ipc.rs`: IPC protocol, message types, client and server implementations

## IPC Protocol

Communication uses JSON-serialized messages over Unix domain sockets.

### Message Types (UI → Daemon)
- `Ping`: Simple health check
- `GetStatus`: Request daemon status (uptime, running apps)
- `LaunchApp { name, command }`: Request to launch an application
- `Shutdown`: Request daemon shutdown

### Response Types (Daemon → UI)
- `Pong`: Response to Ping
- `Status { uptime_secs, apps_running }`: Daemon status information
- `AppLaunched { success, message }`: Result of app launch request
- `ShuttingDown`: Acknowledgment of shutdown request
- `Error { message }`: Error response

## Socket Location

The IPC socket is created at: `$XDG_RUNTIME_DIR/shepherd-launcher.sock` (typically `/run/user/1000/shepherd-launcher.sock`)

## Usage Example

```rust
use crate::daemon::{IpcClient, IpcMessage, IpcResponse};

// Send a ping
match IpcClient::send_message(&IpcMessage::Ping) {
    Ok(IpcResponse::Pong) => println!("Daemon is alive!"),
    Ok(other) => println!("Unexpected response: {:?}", other),
    Err(e) => eprintln!("IPC error: {}", e),
}

// Get daemon status
match IpcClient::send_message(&IpcMessage::GetStatus) {
    Ok(IpcResponse::Status { uptime_secs, apps_running }) => {
        println!("Uptime: {}s, Apps: {}", uptime_secs, apps_running);
    }
    _ => {}
}

// Launch an app
let msg = IpcMessage::LaunchApp {
    name: "Firefox".to_string(),
    command: "firefox".to_string(),
};
match IpcClient::send_message(&msg) {
    Ok(IpcResponse::AppLaunched { success, message }) => {
        println!("Launch {}: {}", if success { "succeeded" } else { "failed" }, message);
    }
    _ => {}
}
```

## Current Functionality

Currently this is a dummy implementation demonstrating the IPC pattern:
- The daemon process runs in the background
- The UI periodically queries the daemon status (every 5 seconds)
- Messages are printed to stdout for debugging
- App launching is simulated (doesn't actually launch apps yet)

## Future Enhancements

- Actual application launching logic
- App state tracking
- Bi-directional notifications (daemon → UI events)
- Multiple concurrent IPC connections
- Authentication/security
