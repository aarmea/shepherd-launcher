#!/usr/bin/env bash
# Sway compositor helpers for shepherd-launcher
# Handles nested sway execution for development and production

# Get the directory containing this script
SWAY_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$SWAY_LIB_DIR/common.sh"

# Source build utilities for binary paths
# shellcheck source=build.sh
source "$SWAY_LIB_DIR/build.sh"

# PID of the running sway process (for cleanup)
SWAY_PID=""

# Default directories
DEFAULT_DEV_RUNTIME="./dev-runtime"
DEFAULT_DATA_DIR="$DEFAULT_DEV_RUNTIME/data"
DEFAULT_SOCKET_PATH="$DEFAULT_DEV_RUNTIME/shepherd.sock"

# Cleanup function for sway processes
sway_cleanup() {
    info "Cleaning up sway session..."
    
    # Kill the nested sway - this will clean up everything inside it
    if [[ -n "${SWAY_PID:-}" ]]; then
        kill "$SWAY_PID" 2>/dev/null || true
    fi
    
    # Explicitly kill any shepherd processes that might have escaped
    kill_matching "shepherdd"
    kill_matching "shepherd-launcher"
    kill_matching "shepherd-hud"
    
    # Remove socket
    if [[ -n "${SHEPHERD_SOCKET:-}" ]]; then
        rm -f "$SHEPHERD_SOCKET"
    fi
}

# Kill any existing dev instances
sway_kill_existing() {
    info "Cleaning up any existing dev instances..."
    kill_matching "sway -c.*sway.conf"
    kill_matching "shepherdd"
    kill_matching "shepherd-launcher"
    kill_matching "shepherd-hud"
    
    # Remove stale socket if it exists
    if [[ -n "${SHEPHERD_SOCKET:-}" ]] && [[ -e "$SHEPHERD_SOCKET" ]]; then
        rm -f "$SHEPHERD_SOCKET"
    fi
    
    # Brief pause to allow cleanup
    sleep 0.5
}

# Set up environment for shepherd binaries
sway_setup_env() {
    local data_dir="${1:-$DEFAULT_DATA_DIR}"
    local socket_path="${2:-$DEFAULT_SOCKET_PATH}"
    
    # Create directories
    mkdir -p "$data_dir"
    
    # Export environment variables
    export SHEPHERD_SOCKET="$socket_path"
    export SHEPHERD_DATA_DIR="$data_dir"
}

# Generate a sway config for development
# Uses debug binaries and development paths
sway_generate_dev_config() {
    local repo_root
    repo_root="$(get_repo_root)"
    
    # Use the existing sway.conf as template
    local sway_config="$repo_root/sway.conf"
    
    if [[ ! -f "$sway_config" ]]; then
        die "sway.conf not found at $sway_config"
    fi
    
    # Return path to the sway config (we use the existing one for dev)
    echo "$sway_config"
}

# Start a nested sway session for development
sway_start_nested() {
    local sway_config="$1"
    
    require_command sway
    
    info "Starting nested sway session..."
    
    # Set up cleanup trap
    trap sway_cleanup EXIT
    
    # Start sway with wayland backend (nested in current session)
    WLR_BACKENDS=wayland WLR_LIBINPUT_NO_DEVICES=1 sway -c "$sway_config" &
    SWAY_PID=$!
    
    info "Sway started with PID $SWAY_PID"
    
    # Wait for sway to exit
    wait "$SWAY_PID"
}

# Run a development session (build + nested sway)
sway_dev_run() {
    local repo_root
    repo_root="$(get_repo_root)"
    
    verify_repo
    cd "$repo_root" || die "Failed to change directory to $repo_root"
    
    # Set up environment
    sway_setup_env "$DEFAULT_DATA_DIR" "$DEFAULT_SOCKET_PATH"
    
    # Kill any existing instances
    sway_kill_existing
    
    # Build debug binaries
    info "Building shepherd binaries..."
    build_cargo false
    
    # Get sway config
    local sway_config
    sway_config="$(sway_generate_dev_config)"
    
    # Start nested sway (blocking)
    sway_start_nested "$sway_config"
}

# Main sway command dispatcher (internal use)
sway_main() {
    local subcmd="${1:-}"
    shift || true
    
    case "$subcmd" in
        run)
            sway_dev_run "$@"
            ;;
        *)
            # Default to run for backwards compatibility
            sway_dev_run "$@"
            ;;
    esac
}
