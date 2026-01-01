#!/usr/bin/env bash
# Build logic for shepherd-launcher
# Wraps cargo build with project-specific settings

# Get the directory containing this script
BUILD_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$BUILD_LIB_DIR/common.sh"

# Binary names produced by the build
SHEPHERD_BINARIES=(
    "shepherdd"
    "shepherd-launcher"
    "shepherd-hud"
)

# Get the target directory for binaries
get_target_dir() {
    local release="${1:-false}"
    local repo_root
    repo_root="$(get_repo_root)"
    
    if [[ "$release" == "true" ]]; then
        echo "$repo_root/target/release"
    else
        echo "$repo_root/target/debug"
    fi
}

# Get the path to a specific binary
get_binary_path() {
    local binary="$1"
    local release="${2:-false}"
    
    echo "$(get_target_dir "$release")/$binary"
}

# Check if all binaries exist
binaries_exist() {
    local release="${1:-false}"
    local target_dir
    target_dir="$(get_target_dir "$release")"
    
    for binary in "${SHEPHERD_BINARIES[@]}"; do
        if [[ ! -x "$target_dir/$binary" ]]; then
            return 1
        fi
    done
    return 0
}

# Build the project
build_cargo() {
    local release="${1:-false}"
    local repo_root
    repo_root="$(get_repo_root)"
    
    verify_repo
    require_command cargo rust
    
    cd "$repo_root" || die "Failed to change directory to $repo_root"
    
    local build_type
    if [[ "$release" == "true" ]]; then
        build_type="release"
        info "Building shepherd (release mode)..."
        cargo build --release
    else
        build_type="debug"
        info "Building shepherd (debug mode)..."
        cargo build
    fi
    
    # Verify binaries were created
    if ! binaries_exist "$release"; then
        die "Build completed but some binaries are missing"
    fi
    
    local target_dir
    target_dir="$(get_target_dir "$release")"
    
    success "Built binaries ($build_type):"
    for binary in "${SHEPHERD_BINARIES[@]}"; do
        info "  $target_dir/$binary"
    done
}

# Clean build artifacts
build_clean() {
    local repo_root
    repo_root="$(get_repo_root)"
    
    verify_repo
    require_command cargo rust
    
    cd "$repo_root" || die "Failed to change directory to $repo_root"
    
    info "Cleaning build artifacts..."
    cargo clean
    success "Build artifacts cleaned"
}

# Main build command dispatcher
build_main() {
    local release=false
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --release|-r)
                release=true
                shift
                ;;
            clean)
                build_clean
                return
                ;;
            help|-h|--help)
                cat <<EOF
Usage: shepherd build [OPTIONS]

Options:
    --release, -r    Build in release mode (optimized)
    clean            Clean build artifacts
    help             Show this help

Examples:
    shepherd build              # Debug build
    shepherd build --release    # Release build
    shepherd build clean        # Clean artifacts
EOF
                return
                ;;
            *)
                die "Unknown build option: $1 (try: shepherd build help)"
                ;;
        esac
    done
    
    build_cargo "$release"
}
