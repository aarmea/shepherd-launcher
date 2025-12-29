# shepherd-api

Protocol types for Shepherd IPC communication.

## Overview

This crate defines the stable API between the Shepherd service (`shepherdd`) and its clients (launcher UI, HUD overlay, admin tools). It contains:

- **Commands** - Requests from clients to the service
- **Responses** - Service replies to commands
- **Events** - Asynchronous notifications from service to clients
- **Shared types** - Entry views, session info, reason codes, etc.

## Purpose

`shepherd-api` establishes the contract between components, ensuring:

1. **Stability** - Versioned protocol with backward compatibility
2. **Type safety** - Strongly typed messages prevent protocol errors
3. **Decoupling** - Clients and service can evolve independently

## API Version

```rust
use shepherd_api::API_VERSION;

// Current API version
assert_eq!(API_VERSION, 1);
```

## Key Types

### Commands

Commands are requests sent by clients to the service:

```rust
use shepherd_api::Command;

// Request available entries
let cmd = Command::ListEntries;

// Request to launch an entry
let cmd = Command::Launch { 
    entry_id: "minecraft".into() 
};

// Request to stop current session
let cmd = Command::StopCurrent { 
    mode: StopMode::Graceful 
};

// Subscribe to real-time events
let cmd = Command::SubscribeEvents;
```

Available commands:
- `GetState` - Get full service state snapshot
- `ListEntries` - List all entries with availability
- `Launch { entry_id }` - Launch an entry
- `StopCurrent { mode }` - Stop the current session
- `ReloadConfig` - Reload configuration (admin only)
- `SubscribeEvents` - Subscribe to event stream
- `GetHealth` - Get service health status
- `SetVolume { level }` - Set system volume
- `GetVolume` - Get current volume

### Events

Events are pushed from the service to subscribed clients:

```rust
use shepherd_api::{Event, EventPayload};

// Events received by clients
match event.payload {
    EventPayload::StateChanged(snapshot) => { /* Update UI */ }
    EventPayload::SessionStarted(info) => { /* Show HUD */ }
    EventPayload::WarningIssued { threshold, remaining, severity, message } => { /* Alert user */ }
    EventPayload::SessionExpired { session_id } => { /* Time's up */ }
    EventPayload::SessionEnded { session_id, reason } => { /* Return to launcher */ }
    EventPayload::PolicyReloaded { entry_count } => { /* Refresh entry list */ }
    EventPayload::VolumeChanged(info) => { /* Update volume display */ }
}
```

### Entry Views

Entries as presented to UIs:

```rust
use shepherd_api::EntryView;

let view: EntryView = /* from service */;

if view.enabled {
    // Entry can be launched
    println!("Max run time: {:?}", view.max_run_if_started_now);
} else {
    // Entry unavailable, show reasons
    for reason in &view.reasons {
        match reason {
            ReasonCode::OutsideTimeWindow { next_window_start } => { /* ... */ }
            ReasonCode::QuotaExhausted { used, quota } => { /* ... */ }
            ReasonCode::CooldownActive { available_at } => { /* ... */ }
            ReasonCode::SessionActive { entry_id, remaining } => { /* ... */ }
            // ...
        }
    }
}
```

### Session Info

Information about active sessions:

```rust
use shepherd_api::{SessionInfo, SessionState};

let session: SessionInfo = /* from snapshot */;

match session.state {
    SessionState::Launching => { /* Show spinner */ }
    SessionState::Running => { /* Show countdown */ }
    SessionState::Warned => { /* Highlight urgency */ }
    SessionState::Expiring => { /* Terminating... */ }
    SessionState::Ended => { /* Session over */ }
}
```

### Reason Codes

Structured explanations for unavailability:

- `OutsideTimeWindow` - Not within allowed time window
- `QuotaExhausted` - Daily time limit reached
- `CooldownActive` - Must wait after previous session
- `SessionActive` - Another session is running
- `UnsupportedKind` - Host doesn't support this entry type
- `Disabled` - Entry explicitly disabled in config

## Design Philosophy

- **Service is authoritative** - Clients display state, service enforces policy
- **Structured reasons** - UIs can explain "why is this unavailable?"
- **Event-driven** - Clients subscribe and react to changes
- **Serializable** - All types derive `Serialize`/`Deserialize` for JSON transport

## Dependencies

- `serde` - Serialization/deserialization
- `chrono` - Timestamp types
- `shepherd-util` - ID types
