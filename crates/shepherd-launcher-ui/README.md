# shepherd-launcher-ui

Main launcher grid interface for Shepherd.

## Overview

`shepherd-launcher-ui` is the primary user-facing shell for the Shepherd kiosk environment. It presents a grid of available entries (applications, games, media) and allows users to launch them. 

This is what users see when no session is active—the "home screen" of the environment.

## Features

- **Entry grid** - Large, touch-friendly tiles for each available entry
- **Availability display** - Visual indication of enabled/disabled entries
- **Launch requests** - Send launch commands to the service
- **State synchronization** - Always reflects service's authoritative state

## Architecture

```
┌───────────────────────────────────────────────────────┐
│                 Sway / Wayland Compositor             │
│                                                       │
│  ┌──────────────────────────────────────────────────┐ │
│  │            Launcher UI (fullscreen)              │ │
│  │                                                  │ │
│  │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐     │ │
│  │  │        │ │        │ │        │ │        │     │ │
│  │  │Minecraf│ │GCompris│ │ Movies │ │ Books  │     │ │
│  │  │        │ │        │ │        │ │        │     │ │
│  │  └────────┘ └────────┘ └────────┘ └────────┘     │ │
│  │                                                  │ │
│  │  ┌────────┐ ┌────────┐                           │ │
│  │  │        │ │        │                           │ │
│  │  │ ScummVM│ │Bedtime │                           │ │
│  │  │        │ │        │                           │ │
│  │  └────────┘ └────────┘                           │ │
│  │                                                  │ │
│  └──────────────────────────────────────────────────┘ │
└───────────────────────────────────────────────────────┘
```

## Usage

### Running

```bash
# With default socket path
shepherd-launcher

# With custom socket path
shepherd-launcher --socket /custom/path/shepherdd.sock
```

### Command-Line Options

| Option | Default | Description |
|--------|---------|-------------|
| `-s, --socket` | `$XDG_RUNTIME_DIR/shepherdd/shepherdd.sock` | Service socket path |
| `-l, --log-level` | `info` | Log verbosity |

## Grid Behavior

### Entry Tiles

Each tile displays:

- **Icon** - Large, recognizable icon
- **Label** - Entry name
- **Status** - Enabled (bright) or disabled (dimmed)
- **Time indicator** - Max duration if started now (e.g., "30 min")

### Enabled Entries

When an entry is enabled:
1. Tile is fully visible and interactive
2. Tapping sends `Launch` command to service
3. Grid shows "Launching..." state
4. On success: launcher hides, application starts

### Disabled Entries

When an entry is disabled it is not displayed.

### Launch Flow

```
User taps tile
      │
      ▼
Launcher sends Launch command
      │
      ▼
Grid input disabled
"Starting..." overlay shown
      │
      ▼
┌─────┴─────┐
│           │
▼           ▼
Success     Failure
│           │
▼           ▼
Launcher    Error message
hides       Grid restored
```

## State Management

The launcher maintains a reactive state model:

```rust
struct LauncherState {
    entries: Vec<EntryView>,   // From service
    current_session: Option<SessionInfo>,
    connected: bool,
    launching: Option<EntryId>,
}
```

### Event Handling

| Event | Launcher Response |
|-------|-------------------|
| `StateChanged` | Update entry grid |
| `SessionStarted` | Hide launcher |
| `SessionEnded` | Show launcher |
| `PolicyReloaded` | Refresh entry list |

### Visibility Rules

The launcher is visible when:
- No session is running, OR
- User explicitly returns to home (via HUD)

The launcher hides when:
- A session is actively running
- (Fullscreen app is in front)

## Error Handling

### Service Unavailable

If the service is not running at startup:

```
┌────────────────────────────────────────┐
│                                        │
│          System Not Ready              │
│                                        │
│    Waiting for shepherd service...     │
│                                        │
│           [Retry]                      │
│                                        │
└────────────────────────────────────────┘
```

### Launch Failure

If launching fails:

1. Error notification appears
2. Grid is restored to interactive state
3. User can try again or choose another entry

### Connection Loss

If connection to service is lost:

1. Entries become disabled
2. Reconnection attempted automatically
3. State refreshed on reconnection

## Accessibility

- **Touch-first** - Large touch targets (minimum 44px)
- **High contrast** - Clear visual hierarchy
- **Minimal text** - Icon-first design
- **Keyboard navigation** - Arrow keys and Enter
- **No hover-only interactions** - All actions accessible via tap

## Styling

The launcher uses a child-friendly design:

- Large, colorful icons
- Rounded corners
- Clear enabled/disabled distinction
- Smooth transitions
- Dark background (for contrast)

## Dependencies

- `gtk4` - GTK4 bindings
- `tokio` - Async runtime
- `shepherd-api` - Protocol types
- `shepherd-ipc` - Client implementation
- `clap` - Argument parsing
- `tracing` - Logging

## Building

```bash
cargo build --release -p shepherd-launcher-ui
```

The resulting binary is named `shepherd-launcher`.

## Relationship to Service

**Critical**: The launcher is purely a presentation layer. It:
- Displays what the service allows
- Sends launch requests
- Shows service state

It does *not*:
- Tracks time independently
- Decide availability
- Enforce policy

If the launcher crashes, the service continues enforcement. If the launcher is replaced, the system still works.
