# Installation

`shepherd-launcher` can be installed on Linux with a modern Wayland compositor.
It is currently developed and tested on Ubuntu 25.10.

`shepherd-launcher` currently must be built from source. `./scripts/shepherd`
can help set up your build environment and manage your installation.

## Basic setup

```sh
# 0. Install build dependencies
sudo ./scripts/shepherd deps build run

# 1. Install runtime dependencies
sudo ./scripts/shepherd deps install run

# 2. Build release binaries
./scripts/shepherd build --release

# 3. Install everything for a kiosk user
sudo ./scripts/shepherd install all --user kiosk
```

This installs:
- Binaries to `/usr/local/bin/`
- System Sway configuration to `/etc/sway/shepherd.conf`
- Display manager desktop entry ("Shepherd Kiosk" session)
- System config template to `/etc/shepherd/config.toml`
- User config to `~kiosk/.config/shepherd/config.toml`

For custom installation paths:
```sh
# Install to /usr instead of /usr/local
sudo ./scripts/shepherd install all --user kiosk --prefix /usr
```

Individual installation steps can be run separately:
```sh
sudo ./scripts/shepherd install bins --prefix /usr/local
sudo ./scripts/shepherd install config --user kiosk
```

## Kiosk hardening (optional)

Kiosk hardening is optional and intended for devices primarily used by
children, not developer machines.

```sh
sudo ./scripts/shepherd harden apply --user kiosk
```

This restricts the user to only access the Shepherd session by:
- Denying SSH access
- Restricting console (TTY) login
- Denying sudo access
- Restricting shell to Sway sessions only

To revert (all changes are reversible):
```sh
sudo ./scripts/shepherd harden revert --user kiosk
```

## Complete documentation

See the scripts' [README](../scripts/README.md) for more.
