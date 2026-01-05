# Shepherd Scripts System

This directory contains the unified script system for shepherd-launcher.

## Quick Reference

```sh
# Main entry point
./shepherd --help

# Dependencies
./shepherd deps print build|run|dev
./shepherd deps install build|run|dev

# Building
./shepherd build [--release]

# Configuration
./shepherd config validate [path]

# Development
./shepherd dev run

# Installation
./shepherd install all --user USER [--prefix PREFIX]
./shepherd install bins [--prefix PREFIX]
./shepherd install config --user USER

# Hardening
./shepherd harden apply --user USER
./shepherd harden revert --user USER
```

## Structure

```
scripts/
├── shepherd           # Main CLI dispatcher
├── dev                # Wrapper → shepherd dev run
├── admin              # Wrapper → shepherd install/harden
├── lib/               # Shared libraries
│   ├── common.sh      # Logging, error handling, sudo helpers
│   ├── deps.sh        # Dependency management
│   ├── build.sh       # Cargo build logic
│   ├── config.sh      # Configuration validation
│   ├── sway.sh        # Nested sway execution
│   ├── install.sh     # Installation logic
│   └── harden.sh      # User hardening/unhardening
└── deps/              # Package lists
    ├── build.pkgs     # Build-time dependencies
    ├── run.pkgs       # Runtime dependencies
    └── dev.pkgs       # Development extras
```

## Design Principles

1. **Single source of truth**: All dependency lists are defined once in `deps/*.pkgs`
2. **Composable**: Each command can be called independently
3. **Reversible**: All destructive actions (hardening, installation) can be undone
4. **Shared logic**: Business logic lives in libraries, not duplicated across scripts
5. **Clear separation**: Build-only, runtime-only, and development dependencies are separate

## Usage Examples

### For Developers

```sh
# First time setup (installs system packages + Rust via rustup)
./shepherd deps install dev
./shepherd dev run

# Or use the convenience wrapper
./run-dev
```

### For CI

```sh
# Install only build dependencies (includes Rust via rustup)
./shepherd deps install build

# Build release binaries
./shepherd build --release
```

### For Production Deployment

```sh
# On a runtime-only system
sudo ./shepherd deps install run
./shepherd build --release
sudo ./shepherd install all --user kiosk --prefix /usr

# Optional: lock down the kiosk user
sudo ./shepherd harden apply --user kiosk
```

### For Package Maintainers

```sh
# Print package lists for your distro
./shepherd deps print build > build-deps.txt
./shepherd deps print run > runtime-deps.txt

# Install with custom prefix and DESTDIR
make -j$(nproc)  # or equivalent
sudo DESTDIR=/tmp/staging ./shepherd install bins --prefix /usr
```

## Dependency Sets

- **build**: Packages needed to compile the Rust code (GTK, Wayland dev libs, etc.) + Rust toolchain via rustup
- **run**: Packages needed to run the compiled binaries (Sway, GTK runtime libs)
- **dev**: Union of build + run + dev-specific tools (git, gdb, strace) + Rust toolchain

The dev set is computed as the union of all three package lists, automatically deduplicated.

## Hardening

The hardening system makes reversible changes to restrict a user to kiosk mode:

```sh
# Apply hardening
sudo ./shepherd harden apply --user kiosk

# Check status
sudo ./shepherd harden status --user kiosk

# Revert all changes
sudo ./shepherd harden revert --user kiosk
```

All changes are tracked in `/var/lib/shepherdd/hardening/<user>/` for rollback.

Applied restrictions:
- SSH access denied
- Console (TTY) login restricted
- Sudo access denied
- Shell restricted to Sway sessions only
- Home directory permissions secured

## Adding New Dependencies

Edit the appropriate package list in `deps/`:

- `deps/build.pkgs` - Build-time dependencies
- `deps/run.pkgs` - Runtime dependencies
- `deps/dev.pkgs` - Developer tools

Format: One package per line, `#` for comments.

The CI workflow will automatically use these lists.
