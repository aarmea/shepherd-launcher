#!/usr/bin/env bash
# Dependency management for shepherd-launcher
# Provides functions to read, union, and install package sets

# Get the directory containing this script
DEPS_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$DEPS_LIB_DIR/common.sh"

# Directory containing package lists
DEPS_DIR="$(get_repo_root)/scripts/deps"

# Rust installation URL
RUSTUP_URL="https://sh.rustup.rs"

# Check if Rust is installed
is_rust_installed() {
    command_exists rustc && command_exists cargo
}

# Install Rust via rustup
install_rust() {
    if is_rust_installed; then
        info "Rust is already installed ($(rustc --version))"
        return 0
    fi
    
    info "Installing Rust via rustup..."
    
    # Download and run rustup installer
    curl --proto '=https' --tlsv1.2 -sSf "$RUSTUP_URL" | sh -s -- -y --profile minimal
    
    # Source cargo env for current session
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env" 2>/dev/null || true
    
    if is_rust_installed; then
        success "Rust installed successfully ($(rustc --version))"
    else
        die "Rust installation failed. Please run: curl --proto '=https' --tlsv1.2 -sSf $RUSTUP_URL | sh"
    fi
}

# Read a package file, stripping comments and empty lines
read_package_file() {
    local file="$1"
    
    if [[ ! -f "$file" ]]; then
        die "Package file not found: $file"
    fi
    
    # Strip comments (# to end of line) and empty lines, trim whitespace
    grep -v '^\s*#' "$file" | grep -v '^\s*$' | sed 's/#.*//' | tr -s '[:space:]' '\n' | grep -v '^$'
}

# Get packages for a specific set
get_packages() {
    local set_name="$1"
    
    case "$set_name" in
        build)
            read_package_file "$DEPS_DIR/build.pkgs"
            ;;
        run)
            read_package_file "$DEPS_DIR/run.pkgs"
            ;;
        dev)
            # Union of all three sets, deduplicated
            {
                read_package_file "$DEPS_DIR/build.pkgs"
                read_package_file "$DEPS_DIR/run.pkgs"
                read_package_file "$DEPS_DIR/dev.pkgs"
            } | sort -u
            ;;
        *)
            die "Unknown package set: $set_name (valid: build, run, dev)"
            ;;
    esac
}

# Print packages for a set (one per line)
deps_print() {
    local set_name="${1:-}"
    
    if [[ -z "$set_name" ]]; then
        die "Usage: shepherd deps print <build|run|dev>"
    fi
    
    get_packages "$set_name"
}

# Install packages for a set
deps_install() {
    local set_name="${1:-}"
    
    if [[ -z "$set_name" ]]; then
        die "Usage: shepherd deps install <build|run|dev>"
    fi
    
    check_ubuntu_version
    
    info "Installing $set_name dependencies..."
    
    # Get the package list
    local packages
    packages=$(get_packages "$set_name" | tr '\n' ' ')
    
    if [[ -z "$packages" ]]; then
        warn "No packages to install for set: $set_name"
        return 0
    fi
    
    info "Packages: $packages"
    
    # Install using apt
    maybe_sudo apt-get update
    # shellcheck disable=SC2086
    maybe_sudo apt-get install -y $packages
    
    # For build and dev sets, also install Rust
    if [[ "$set_name" == "build" ]] || [[ "$set_name" == "dev" ]]; then
        install_rust
    fi
    
    success "Installed $set_name dependencies"
}

# Check if all packages for a set are installed
deps_check() {
    local set_name="${1:-}"
    
    if [[ -z "$set_name" ]]; then
        die "Usage: shepherd deps check <build|run|dev>"
    fi
    
    local packages
    packages=$(get_packages "$set_name")
    
    local missing=()
    while IFS= read -r pkg; do
        if ! dpkg -l "$pkg" &>/dev/null; then
            missing+=("$pkg")
        fi
    done <<< "$packages"
    
    # For build and dev sets, also check Rust
    if [[ "$set_name" == "build" ]] || [[ "$set_name" == "dev" ]]; then
        if ! is_rust_installed; then
            warn "Rust is not installed"
            return 1
        fi
    fi
    
    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "Missing packages: ${missing[*]}"
        return 1
    fi
    
    success "All $set_name dependencies are installed"
    return 0
}

# Main deps command dispatcher
deps_main() {
    local subcmd="${1:-}"
    shift || true
    
    case "$subcmd" in
        print)
            deps_print "$@"
            ;;
        install)
            deps_install "$@"
            ;;
        check)
            deps_check "$@"
            ;;
        ""|help|-h|--help)
            cat <<EOF
Usage: shepherd deps <command> <set>

Commands:
    print   <set>    Print packages in the set (one per line)
    install <set>    Install packages from the set
    check   <set>    Check if all packages from the set are installed

Package sets:
    build    Build-time dependencies (+ Rust via rustup)
    run      Runtime dependencies only
    dev      All dependencies (build + run + dev extras + Rust)

Note: The 'build' and 'dev' sets automatically install Rust via rustup.

Examples:
    shepherd deps print build
    shepherd deps install dev
    shepherd deps check run
EOF
            ;;
        *)
            die "Unknown deps command: $subcmd (try: shepherd deps help)"
            ;;
    esac
}
