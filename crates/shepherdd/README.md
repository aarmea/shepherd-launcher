# shepherdd

The Shepherd background service.

## Overview

`shepherdd` is the authoritative policy and enforcement service for the Shepherd ecosystem. It is the central coordinator that:

- Loads and validates configuration
- Evaluates policy to determine availability
- Manages session lifecycles
- Enforces time limits
- Emits warnings and events
- Serves multiple clients via IPC

**Key principle**: `shepherdd` is the single source of truth. User interfaces only request actions and display state—they never enforce policy independently.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                         shepherdd                            │
│                                                              │
│  ┌─────────────┐   ┌─────────────┐   ┌────────────────────┐  │
│  │   Config    │   │    Store    │   │    Core Engine     │  │
│  │   Loader    │──▶│  (SQLite)   │──▶│ (Policy + Session) │  │
│  └─────────────┘   └─────────────┘   └──────────┬─────────┘  │
│                                                 │            │
│  ┌─────────────┐  ┌─────────────┐               │            │
│  │    Host     │  │     IPC     │◀──────────────┘            │
│  │   Adapter   │◀─│   Server    │                            │
│  │  (Linux)    │  │             │                            │
│  └──────┬──────┘  └──────┬──────┘                            │
│         │                │                                   │
│         │      Unix Domain Socket                            │
│         │                │                                   │
└─────────┼────────────────┼───────────────────────────────────┘
          │                │
          ▼                ▼
    Supervised        ┌─────────┐  ┌─────────┐  ┌─────────┐
    Applications      │Launcher │  │   HUD   │  │  Admin  │
                      │   UI    │  │ Overlay │  │  Tools  │
                      └─────────┘  └─────────┘  └─────────┘
```

## Usage

### Running

```bash
# With default config location
shepherdd

# With custom config
shepherdd --config /path/to/config.toml

# Override socket and data paths
shepherdd --socket /tmp/shepherdd.sock --data-dir /tmp/shepherdd-data

# Debug logging
shepherdd --log-level debug
```

### Command-Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `-c, --config` | `~/.config/shepherd/config.toml` | Configuration file path |
| `-s, --socket` | From config | IPC socket path |
| `-d, --data-dir` | From config | Data directory |
| `-l, --log-level` | `info` | Log verbosity |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `SHEPHERD_SOCKET` | Override socket path (default: `$XDG_RUNTIME_DIR/shepherdd/shepherdd.sock`) |
| `SHEPHERD_DATA_DIR` | Override data directory (default: `$XDG_DATA_HOME/shepherdd`) |
| `RUST_LOG` | Tracing filter (e.g., `shepherdd=debug`) |

## Main Loop

The service runs an async event loop that processes:

1. **IPC messages** - Commands from clients
2. **Host events** - Process exits, window events
3. **Timer ticks** - Check for warnings and expiry
4. **Signals** - SIGHUP for config reload, SIGTERM for shutdown

```
┌────────────────────────────────────────────────────┐
│                    Main Loop                       │
│                                                    │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │
│  │   IPC   │ │  Host   │ │  Timer  │ │ Signal  │   │
│  │ Channel │ │ Events  │ │  Tick   │ │ Handler │   │
│  └────┬────┘ └────┬────┘ └────┬────┘ └────┬────┘   │
│       │           │           │           │        │
│       └───────────┴─────┬─────┴───────────┘        │
│                         │                          │
│                         ▼                          │
│              ┌──────────────────┐                  │
│              │  Process Event   │                  │
│              └──────────────────┘                  │
│                         │                          │
│                         ▼                          │
│              ┌──────────────────┐                  │
│              │ Broadcast Events │                  │
│              └──────────────────┘                  │
└────────────────────────────────────────────────────┘
```

## Command Handling

### Client Commands

| Command | Description | Role Required |
|---------|-------------|---------------|
| `GetState` | Get full state snapshot | Any |
| `ListEntries` | Get available entries | Any |
| `Launch` | Start a session | Shell/Admin |
| `StopCurrent` | End current session | Shell/Admin |
| `ReloadConfig` | Hot-reload configuration | Admin |
| `SubscribeEvents` | Subscribe to event stream | Any |
| `GetHealth` | Health check | Any |
| `SetVolume` | Set system volume | Shell/Admin |
| `GetVolume` | Get volume info | Any |

### Response Flow

```
Client Request
      │
      ▼
Role Check ──────▶ Denied Response
      │
      ▼
Command Handler
      │
      ▼
Core Engine
      │
      ▼
Response + Events ──────▶ Broadcast to Subscribers
```

## Session Lifecycle

### Launch

1. Client sends `Launch { entry_id }`
2. Core engine evaluates policy
3. If denied: respond with reasons
4. If approved: create session plan
5. Host adapter spawns process
6. Session transitions to Running
7. `SessionStarted` event broadcast

### Enforcement

1. Timer ticks every 100ms
2. Core engine checks warnings and expiry
3. At warning thresholds: `WarningIssued` event
4. At deadline: initiate graceful stop
5. After grace period: force kill
6. `SessionEnded` event broadcast

### Termination

1. Stop triggered (expiry, user, admin, process exit)
2. Host adapter signals process (SIGTERM)
3. Wait for grace period
4. Force kill if needed (SIGKILL)
5. Record usage in store
6. Set cooldown if configured
7. Clear session state

## Configuration Reload

On SIGHUP or `ReloadConfig` command:

1. Parse new configuration file
2. Validate completely
3. If invalid: keep old config, log error
4. If valid: atomic swap to new policy
5. Emit `PolicyReloaded` event
6. Current session continues with original plan

## Health Monitoring

The service exposes health status via `GetHealth`:

```json
{
  "status": "healthy",
  "policy_loaded": true,
  "store_healthy": true,
  "host_healthy": true,
  "uptime_seconds": 3600,
  "current_session": null
}
```

## Logging

Uses structured logging via `tracing`:

```
2025-01-15T14:30:00.000Z INFO  shepherdd: Starting shepherd service
2025-01-15T14:30:00.050Z INFO  shepherd_config: Configuration loaded entries=5
2025-01-15T14:30:00.100Z INFO  shepherd_ipc: IPC server listening path=/run/shepherdd/shepherdd.sock
2025-01-15T14:30:15.000Z INFO  shepherd_core: Session started session_id=abc123 entry_id=minecraft
2025-01-15T14:59:45.000Z WARN  shepherd_core: Warning issued session_id=abc123 threshold=60
2025-01-15T15:00:45.000Z INFO  shepherd_core: Session expired session_id=abc123
```

## Persistence

State is persisted to SQLite:

```
/var/lib/shepherdd/
├── shepherdd.db       # SQLite database
└── logs/
    └── sessions/      # Session stdout/stderr
```

## Signals

| Signal | Action |
|--------|--------|
| `SIGHUP` | Reload configuration |
| `SIGTERM` | Graceful shutdown |
| `SIGINT` | Graceful shutdown |

## Dependencies

This binary wires together all the library crates:

- `shepherd-config` - Configuration loading
- `shepherd-core` - Policy engine
- `shepherd-host-api` - Host adapter trait
- `shepherd-host-linux` - Linux implementation
- `shepherd-ipc` - IPC server
- `shepherd-store` - Persistence
- `shepherd-api` - Protocol types
- `shepherd-util` - Utilities
- `tokio` - Async runtime
- `clap` - CLI parsing
- `tracing` - Logging
- `anyhow` - Error handling

## Building

```bash
cargo build --release -p shepherdd
```

## Installation

The service is typically started by the compositor:

`sway.conf`
```conf
# Start shepherdd FIRST - it needs to create the socket before HUD/launcher connect
# Running inside sway ensures all spawned processes use the nested compositor
exec ./target/debug/shepherdd -c ./config.example.toml
```

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for development setup.
