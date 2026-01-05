# shepherd-config

Configuration parsing and validation for Shepherd.

## Overview

This crate handles loading, parsing, and validating the TOML configuration that defines what entries are available, when they're available, and for how long. It provides:

- **Schema definitions** - Raw configuration structure as parsed from TOML
- **Policy objects** - Validated, ready-to-use policy structures
- **Validation** - Detailed error messages for misconfiguration
- **Hot reload support** - Configuration can be reloaded at runtime

## Configuration Format

Shepherd uses TOML for configuration. Here's a complete example:

```toml
config_version = 1

[service]
socket_path = "/run/shepherdd/shepherdd.sock"
data_dir = "/var/lib/shepherdd"
default_max_run_seconds = 1800  # 30 minutes default

# Global volume restrictions
[service.volume]
max_volume = 80
allow_unmute = true

# Default warning thresholds (seconds before expiry)
[[service.default_warnings]]
seconds_before = 300  # 5 minutes
severity = "info"

[[service.default_warnings]]
seconds_before = 60   # 1 minute
severity = "warn"

[[service.default_warnings]]
seconds_before = 10
severity = "critical"
message_template = "Closing in {remaining} seconds!"

# Entry definitions
[[entries]]
id = "minecraft"
label = "Minecraft"
icon = "minecraft"
kind = { type = "snap", snap_name = "mc-installer" }

[entries.availability]
[[entries.availability.windows]]
days = "weekdays"
start = "15:00"
end = "18:00"

[[entries.availability.windows]]
days = "weekends"
start = "10:00"
end = "20:00"

[entries.limits]
max_run_seconds = 1800       # 30 minutes per session
daily_quota_seconds = 7200   # 2 hours per day
cooldown_seconds = 600       # 10 minutes between sessions

[[entries]]
id = "educational-game"
label = "GCompris"
icon = "gcompris-qt"
kind = { type = "process", command = "gcompris-qt" }

[entries.availability]
always = true  # Always available

[entries.limits]
max_run_seconds = 3600  # 1 hour
```

## Usage

### Loading Configuration

```rust
use shepherd_config::{load_config, parse_config, Policy};
use std::path::Path;

// Load from file (typically ~/.config/shepherd/config.toml)
let policy = load_config("config.toml")?;

// Parse from string
let toml_content = std::fs::read_to_string("config.toml")?;
let policy = parse_config(&toml_content)?;

// Access entries
for entry in &policy.entries {
    println!("{}: {:?}", entry.label, entry.kind);
}
```

### Entry Kinds

Entries can be of several types:

```toml
# Regular process
kind = { type = "process", command = "/usr/bin/game", args = ["--fullscreen"] }

# Snap application
kind = { type = "snap", snap_name = "mc-installer" }

# Virtual machine (future)
kind = { type = "vm", driver = "qemu", args = { disk = "game.qcow2" } }

# Media playback (future)
kind = { type = "media", library_id = "movies" }

# Custom type
kind = { type = "custom", type_name = "my-launcher", payload = { ... } }
```

### Time Windows

Time windows control when entries are available:

```toml
[entries.availability]
[[entries.availability.windows]]
days = "weekdays"        # or "weekends", "all"
start = "15:00"
end = "18:00"

[[entries.availability.windows]]
days = ["sat", "sun"]    # Specific days
start = "09:00"
end = "21:00"
```

### Limits

Control session duration and frequency:

```toml
[entries.limits]
max_run_seconds = 1800        # Max duration per session
daily_quota_seconds = 7200    # Total daily limit
cooldown_seconds = 600        # Wait time between sessions
```

## Validation

The configuration is validated at load time. Validation catches:

- **Duplicate entry IDs** - Each entry must have a unique ID
- **Empty commands** - Process entries must specify a command
- **Invalid time windows** - Start time must be before end time
- **Invalid thresholds** - Warning thresholds must be less than max run time
- **Negative durations** - All durations must be positive
- **Unknown kinds** - Entry types must be recognized (unless Custom)

```rust
use shepherd_config::{parse_config, ConfigError};

let result = parse_config(toml_str);
match result {
    Ok(policy) => { /* Use policy */ }
    Err(ConfigError::ValidationFailed { errors }) => {
        for error in errors {
            eprintln!("Config error: {}", error);
        }
    }
    Err(e) => eprintln!("Failed to load config: {}", e),
}
```

## Hot Reload

Configuration can be reloaded at runtime via the service's `ReloadConfig` command or by sending `SIGHUP` to the service process. Reload is atomic: either the new configuration is fully applied or the old one remains.

Active sessions continue with their original time limits when configuration is reloaded.

## Key Types

- `Policy` - Validated policy ready for the core engine
- `Entry` - A launchable entry definition
- `AvailabilityPolicy` - Time window rules
- `LimitsPolicy` - Duration and quota limits
- `WarningPolicy` - Warning threshold configuration
- `VolumePolicy` - Volume restrictions

## Design Philosophy

- **Human-readable** - TOML is easy to read and write
- **Strict validation** - Catch errors at load time, not runtime
- **Versioned schema** - `config_version` enables future migrations
- **Sensible defaults** - Minimal config is valid

## Dependencies

- `toml` - TOML parsing
- `serde` - Deserialization
- `chrono` - Time types
- `thiserror` - Error types
