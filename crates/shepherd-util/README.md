# shepherd-util

Shared utilities for the Shepherd ecosystem.

## Overview

This crate provides common utilities and types used across all Shepherd crates, including:

- **ID types** - Type-safe identifiers (`EntryId`, `SessionId`, `ClientId`)
- **Time utilities** - Monotonic time handling and duration helpers
- **Error types** - Common error definitions
- **Rate limiting** - Helpers for command rate limiting
- **Default paths** - XDG-compliant default paths for socket, data, and log directories

## Purpose

`shepherd-util` serves as the foundational layer that other Shepherd crates depend on. It ensures consistency across the codebase by providing:

1. **Unified ID management** - All identifiers are strongly typed to prevent mix-ups
2. **Reliable time handling** - Monotonic time for enforcement (immune to wall-clock changes)
3. **Common error patterns** - Consistent error handling across crates

## Key Types

### IDs

```rust
use shepherd_util::{EntryId, SessionId, ClientId};

// Create IDs
let entry_id = EntryId::new("minecraft");
let session_id = SessionId::new();  // UUID-based
let client_id = ClientId::new();    // UUID-based
```

### Time

```rust
use shepherd_util::MonotonicInstant;

// Monotonic time for countdown logic
let start = MonotonicInstant::now();
// ... later ...
let elapsed = start.elapsed();
```

### Rate Limiting

```rust
use shepherd_util::RateLimiter;
use std::time::Duration;

// Per-client command rate limiting
let limiter = RateLimiter::new(10, Duration::from_secs(1));
if limiter.check(&client_id) {
    // Process command
}
```

### Default Paths

```rust
use shepherd_util::{default_socket_path, default_data_dir, default_log_dir};

// Get XDG-compliant paths (no root required)
let socket = default_socket_path();  // $XDG_RUNTIME_DIR/shepherdd/shepherdd.sock
let data = default_data_dir();       // $XDG_DATA_HOME/shepherdd or ~/.local/share/shepherdd
let logs = default_log_dir();        // $XDG_STATE_HOME/shepherdd or ~/.local/state/shepherdd
```

Environment variables `SHEPHERD_SOCKET` and `SHEPHERD_DATA_DIR` can override the defaults.

## Design Philosophy

- **No platform-specific code** - Pure Rust, works everywhere
- **Minimal dependencies** - Only essential crates
- **Type safety** - Prefer typed wrappers over raw strings/numbers

## Dependencies

- `uuid` - For generating unique session/client IDs
- `chrono` - For time handling
- `serde` - For serialization
- `thiserror` - For error types
