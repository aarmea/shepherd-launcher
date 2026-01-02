#!/usr/bin/env bash
# Installation logic for shepherd-launcher
# Handles binary installation, config deployment, and desktop entry setup

# Get the directory containing this script
INSTALL_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$INSTALL_LIB_DIR/common.sh"

# Source build utilities for binary paths
# shellcheck source=build.sh
source "$INSTALL_LIB_DIR/build.sh"

# Default installation paths
DEFAULT_PREFIX="/usr/local"
DEFAULT_BINDIR="bin"

# Standard sway config location
SWAY_CONFIG_DIR="/etc/sway"
SHEPHERD_SWAY_CONFIG="shepherd.conf"

# Desktop entry location
DESKTOP_ENTRY_DIR="share/wayland-sessions"
DESKTOP_ENTRY_NAME="shepherd.desktop"

# Install release binaries
install_bins() {
    local prefix="${1:-$DEFAULT_PREFIX}"
    local destdir="${DESTDIR:-}"
    
    require_root
    
    local bindir="$destdir$prefix/$DEFAULT_BINDIR"
    local target_dir
    target_dir="$(get_target_dir true)"
    
    # Ensure release build exists
    if ! binaries_exist true; then
        die "Release binaries not found. Run 'shepherd build --release' first."
    fi
    
    info "Installing binaries to $bindir..."
    
    ensure_dir "$bindir" 0755
    
    for binary in "${SHEPHERD_BINARIES[@]}"; do
        local src="$target_dir/$binary"
        local dst="$bindir/$binary"
        
        info "  Installing $binary..."
        install -m 0755 "$src" "$dst"
    done
    
    success "Installed binaries to $bindir"
}

# Install the sway configuration
install_sway_config() {
    local destdir="${DESTDIR:-}"
    local repo_root
    repo_root="$(get_repo_root)"
    
    require_root
    
    local src_config="$repo_root/sway.conf"
    local dst_dir="$destdir$SWAY_CONFIG_DIR"
    local dst_config="$dst_dir/$SHEPHERD_SWAY_CONFIG"
    
    if [[ ! -f "$src_config" ]]; then
        die "Source sway.conf not found at $src_config"
    fi
    
    info "Installing sway configuration to $dst_config..."
    
    ensure_dir "$dst_dir" 0755
    
    # Create a production version of the sway config
    # Replace debug paths with installed paths
    local prefix="${1:-$DEFAULT_PREFIX}"
    local bindir="$prefix/$DEFAULT_BINDIR"
    
    # Copy and modify the config for production use
    sed \
        -e "s|./target/debug/shepherd-launcher|$bindir/shepherd-launcher|g" \
        -e "s|./target/debug/shepherd-hud|$bindir/shepherd-hud|g" \
        -e "s|./target/debug/shepherdd|$bindir/shepherdd|g" \
        -e "s|./config.example.toml|~/.config/shepherd/config.toml|g" \
        -e "s|-c ./sway.conf|-c $dst_config|g" \
        "$src_config" > "$dst_config"
    
    chmod 0644 "$dst_config"
    
    success "Installed sway configuration"
}

# Install desktop entry for display manager
install_desktop_entry() {
    local prefix="${1:-$DEFAULT_PREFIX}"
    local destdir="${DESTDIR:-}"
    
    require_root
    
    local dst_dir="$destdir$prefix/$DESKTOP_ENTRY_DIR"
    local dst_entry="$dst_dir/$DESKTOP_ENTRY_NAME"
    
    info "Installing desktop entry to $dst_entry..."
    
    ensure_dir "$dst_dir" 0755
    
    cat > "$dst_entry" <<EOF
[Desktop Entry]
Name=Shepherd Kiosk
Comment=Shepherd game launcher kiosk mode
Exec=sway -c $SWAY_CONFIG_DIR/$SHEPHERD_SWAY_CONFIG
Type=Application
DesktopNames=shepherd
EOF
    
    chmod 0644 "$dst_entry"
    
    success "Installed desktop entry"
}

# Deploy user configuration
install_config() {
    local user="${1:-}"
    local source_config="${2:-}"
    
    if [[ -z "$user" ]]; then
        die "Usage: shepherd install config --user USER [--source CONFIG]"
    fi
    
    validate_user "$user"
    
    local repo_root
    repo_root="$(get_repo_root)"
    
    # Default source is the example config
    if [[ -z "$source_config" ]]; then
        source_config="$repo_root/config.example.toml"
    fi
    
    if [[ ! -f "$source_config" ]]; then
        die "Source config not found: $source_config"
    fi
    
    # Get user's config directory
    local user_home
    user_home="$(get_user_home "$user")"
    local user_config_dir="$user_home/.config/shepherd"
    local dst_config="$user_config_dir/config.toml"
    
    info "Installing user config to $dst_config..."
    
    # Create config directory owned by user
    maybe_sudo mkdir -p "$user_config_dir"
    maybe_sudo chown "$user:$user" "$user_config_dir"
    maybe_sudo chmod 0755 "$user_config_dir"
    
    # Check if config already exists
    if maybe_sudo test -f "$dst_config"; then
        warn "Config file already exists at $dst_config, skipping"
    else
        # Copy config file
        maybe_sudo cp "$source_config" "$dst_config"
        maybe_sudo chown "$user:$user" "$dst_config"
        maybe_sudo chmod 0644 "$dst_config"
        success "Installed user configuration for $user"
    fi
}

# Install everything
install_all() {
    local user="${1:-}"
    local prefix="${2:-$DEFAULT_PREFIX}"
    
    if [[ -z "$user" ]]; then
        die "Usage: shepherd install all --user USER [--prefix PREFIX]"
    fi
    
    require_root
    validate_user "$user"
    
    info "Installing shepherd-launcher (prefix: $prefix)..."
    
    install_bins "$prefix"
    install_sway_config "$prefix"
    install_desktop_entry "$prefix"
    install_config "$user"
    
    success "Installation complete!"
    info ""
    info "Next steps:"
    info "  1. Edit user config at ~$user/.config/shepherd/config.toml"
    info "  2. Select 'Shepherd Kiosk' session at login"
    info "  3. Optionally run 'shepherd harden apply --user $user' for kiosk mode"
}

# Main install command dispatcher
install_main() {
    local subcmd="${1:-}"
    shift || true
    
    local user=""
    local prefix="$DEFAULT_PREFIX"
    local source_config=""
    
    # Parse remaining arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --user)
                user="$2"
                shift 2
                ;;
            --prefix)
                prefix="$2"
                shift 2
                ;;
            --source)
                source_config="$2"
                shift 2
                ;;
            *)
                die "Unknown option: $1"
                ;;
        esac
    done
    
    case "$subcmd" in
        bins)
            install_bins "$prefix"
            ;;
        config)
            install_config "$user" "$source_config"
            ;;
        sway-config)
            install_sway_config "$prefix"
            ;;
        desktop-entry)
            install_desktop_entry "$prefix"
            ;;
        all)
            install_all "$user" "$prefix"
            ;;
        ""|help|-h|--help)
            cat <<EOF
Usage: shepherd install <command> [OPTIONS]

Commands:
    bins              Install release binaries
    config            Deploy user configuration
    sway-config       Install sway configuration
    desktop-entry     Install display manager desktop entry
    all               Install everything

Options:
    --user USER       Target user for config deployment (required for config/all)
    --prefix PREFIX   Installation prefix (default: $DEFAULT_PREFIX)
    --source CONFIG   Source config file (default: config.example.toml)

Environment:
    DESTDIR           Installation root for packaging (default: empty)

Examples:
    shepherd install bins --prefix /usr/local
    shepherd install config --user kiosk
    shepherd install all --user kiosk --prefix /usr
EOF
            ;;
        *)
            die "Unknown install command: $subcmd (try: shepherd install help)"
            ;;
    esac
}
