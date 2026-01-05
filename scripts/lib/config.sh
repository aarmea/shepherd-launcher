#!/usr/bin/env bash
# Configuration validation logic for shepherd-launcher
# Validates shepherdd configuration files

# Get the directory containing this script
CONFIG_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$CONFIG_LIB_DIR/common.sh"

# Default configuration paths
# Uses XDG_CONFIG_HOME or ~/.config/shepherd/config.toml
get_default_config_path() {
    if [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
        echo "$XDG_CONFIG_HOME/shepherd/config.toml"
    else
        echo "$HOME/.config/shepherd/config.toml"
    fi
}

EXAMPLE_CONFIG_NAME="config.example.toml"

# Get path to the validate-config binary
get_validate_binary() {
    local release="${1:-false}"
    local repo_root
    repo_root="$(get_repo_root)"
    
    if [[ "$release" == "true" ]]; then
        echo "$repo_root/target/release/validate-config"
    else
        echo "$repo_root/target/debug/validate-config"
    fi
}

# Build the validate-config binary if needed
build_validate_binary() {
    local release="${1:-false}"
    local repo_root
    repo_root="$(get_repo_root)"
    
    verify_repo
    require_command cargo rust
    
    cd "$repo_root" || die "Failed to change directory to $repo_root"
    
    if [[ "$release" == "true" ]]; then
        info "Building validate-config (release mode)..."
        cargo build --release --bin validate-config
    else
        info "Building validate-config..."
        cargo build --bin validate-config
    fi
}

# Ensure the validate binary exists, building if necessary
ensure_validate_binary() {
    local release="${1:-false}"
    local binary
    binary="$(get_validate_binary "$release")"
    
    if [[ ! -x "$binary" ]]; then
        build_validate_binary "$release"
    fi
    
    echo "$binary"
}

# Validate a configuration file
validate_config_file() {
    local config_path="$1"
    local release="${2:-false}"
    
    # Build/find the validator
    local binary
    binary="$(ensure_validate_binary "$release")"
    
    # Run validation
    "$binary" "$config_path"
}

# Show config help
config_usage() {
    cat <<EOF
Usage: shepherd config <command> [options]

Commands:
    validate [path]    Validate a configuration file
    help               Show this help message

Options:
    --release          Use release build of validator

The validate command checks a configuration file for:
  - Valid TOML syntax
  - Correct config_version
  - Valid entry definitions
  - Valid time windows and availability specs
  - Warning thresholds that make sense with limits

Examples:
    # Validate the installed config
    shepherd config validate

    # Validate a specific file
    shepherd config validate /path/to/config.toml

    # Validate the example config in the repo
    shepherd config validate config.example.toml

Default paths:
    Installed:  $(get_default_config_path)
    Example:    \$REPO_ROOT/$EXAMPLE_CONFIG_NAME
EOF
}

# Main entry point for config commands
config_main() {
    local subcmd="${1:-}"
    shift || true

    case "$subcmd" in
        validate)
            local config_path=""
            local release="false"
            
            # Parse arguments
            while [[ $# -gt 0 ]]; do
                case "$1" in
                    --release)
                        release="true"
                        shift
                        ;;
                    -h|--help)
                        config_usage
                        return 0
                        ;;
                    -*)
                        die "Unknown option: $1"
                        ;;
                    *)
                        if [[ -z "$config_path" ]]; then
                            config_path="$1"
                        else
                            die "Too many arguments"
                        fi
                        shift
                        ;;
                esac
            done
            
            # Determine config path
            if [[ -z "$config_path" ]]; then
                # Try default path first
                local default_config
                default_config="$(get_default_config_path)"
                if [[ -f "$default_config" ]]; then
                    config_path="$default_config"
                    info "Using installed config: $config_path"
                else
                    # Fall back to example in repo
                    local repo_root
                    repo_root="$(get_repo_root)"
                    local example_path="$repo_root/$EXAMPLE_CONFIG_NAME"
                    if [[ -f "$example_path" ]]; then
                        config_path="$example_path"
                        info "Using example config: $config_path"
                    else
                        die "No config file found. Specify a path or install shepherdd."
                    fi
                fi
            fi
            
            # Resolve relative paths
            if [[ ! "$config_path" = /* ]]; then
                config_path="$(pwd)/$config_path"
            fi
            
            # Check file exists
            if [[ ! -f "$config_path" ]]; then
                die "Configuration file not found: $config_path"
            fi
            
            # Validate
            info "Validating: $config_path"
            echo ""
            validate_config_file "$config_path" "$release"
            ;;
        
        ""|help|-h|--help)
            config_usage
            ;;
        
        *)
            error "Unknown config command: $subcmd"
            echo ""
            config_usage
            exit 1
            ;;
    esac
}
