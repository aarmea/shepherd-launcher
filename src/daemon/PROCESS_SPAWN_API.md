# Process Spawning API

The daemon now supports spawning graphical processes within the current session.

## API Messages

### SpawnProcess
Spawns a new process with the specified command and arguments.

```rust
use crate::daemon::{IpcClient, IpcMessage, IpcResponse};

// Spawn a process with arguments
let message = IpcMessage::SpawnProcess {
    command: "firefox".to_string(),
    args: vec!["--new-window".to_string(), "https://example.com".to_string()],
};

match IpcClient::send_message(&message) {
    Ok(IpcResponse::ProcessSpawned { success, pid, message }) => {
        if success {
            println!("Process spawned with PID: {:?}", pid);
        } else {
            eprintln!("Failed to spawn: {}", message);
        }
    }
    Ok(other) => eprintln!("Unexpected response: {:?}", other),
    Err(e) => eprintln!("IPC error: {}", e),
}
```

### LaunchApp (Legacy)
Spawns a process from a command string (command and args in one string).

```rust
let message = IpcMessage::LaunchApp {
    name: "Terminal".to_string(),
    command: "alacritty".to_string(),
};

match IpcClient::send_message(&message) {
    Ok(IpcResponse::ProcessSpawned { success, pid, message }) => {
        println!("Launch result: {} (PID: {:?})", message, pid);
    }
    _ => {}
}
```

## Process Management

### Automatic Cleanup
The daemon automatically tracks spawned processes and cleans up when they exit:
- Each spawned process is tracked by PID
- The daemon periodically checks for finished processes
- Exited processes are automatically removed from tracking

### Status Query
Get the number of currently running processes:

```rust
match IpcClient::send_message(&IpcMessage::GetStatus) {
    Ok(IpcResponse::Status { uptime_secs, apps_running }) => {
        println!("Daemon uptime: {}s, Processes running: {}", 
                 uptime_secs, apps_running);
    }
    _ => {}
}
```

## Environment Inheritance

Spawned processes inherit the daemon's environment, which includes:
- `WAYLAND_DISPLAY` - for Wayland session access
- `XDG_RUNTIME_DIR` - runtime directory
- `DISPLAY` - for X11 fallback (if available)
- All other environment variables from the daemon

This ensures graphical applications can connect to the display server.

## Examples

### Spawn a terminal emulator
```rust
IpcClient::send_message(&IpcMessage::SpawnProcess {
    command: "alacritty".to_string(),
    args: vec![],
})
```

### Spawn a browser with URL
```rust
IpcClient::send_message(&IpcMessage::SpawnProcess {
    command: "firefox".to_string(),
    args: vec!["https://github.com".to_string()],
})
```

### Spawn with working directory (using sh wrapper)
```rust
IpcClient::send_message(&IpcMessage::SpawnProcess {
    command: "sh".to_string(),
    args: vec![
        "-c".to_string(),
        "cd /path/to/project && code .".to_string()
    ],
})
```

## Response Format

`ProcessSpawned` response contains:
- `success: bool` - Whether the spawn was successful
- `pid: Option<u32>` - Process ID if successful, None on failure
- `message: String` - Human-readable status message

## Error Handling

Common errors:
- Command not found: Returns `success: false` with error message
- Permission denied: Returns `success: false` with permission error
- Invalid arguments: Returns `success: false` with argument error

Always check the `success` field before assuming the process started.
