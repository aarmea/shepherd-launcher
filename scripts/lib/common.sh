#!/usr/bin/env bash
# Common utilities for shepherd scripts
# Logging, error handling, and sudo helpers

set -euo pipefail

# Colors for terminal output (disabled if not a tty)
if [[ -t 1 ]]; then
    RED='\033[0;31m'
    YELLOW='\033[0;33m'
    GREEN='\033[0;32m'
    BLUE='\033[0;34m'
    NC='\033[0m' # No Color
else
    RED=''
    YELLOW=''
    GREEN=''
    BLUE=''
    NC=''
fi

# Logging functions
info() {
    echo -e "${BLUE}[INFO]${NC} $*" >&2
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $*" >&2
}

error() {
    echo -e "${RED}[ERROR]${NC} $*" >&2
}

success() {
    echo -e "${GREEN}[OK]${NC} $*" >&2
}

die() {
    error "$@"
    exit 1
}

# Check if running as root
is_root() {
    [[ $EUID -eq 0 ]]
}

# Require root or exit
require_root() {
    if ! is_root; then
        die "This command must be run as root (use sudo)"
    fi
}

# Run a command with sudo if not already root
maybe_sudo() {
    if is_root; then
        "$@"
    else
        sudo "$@"
    fi
}

# Check if a command exists
command_exists() {
    command -v "$1" &>/dev/null
}

# Require a command or exit with helpful message
require_command() {
    local cmd="$1"
    local pkg="${2:-$1}"
    if ! command_exists "$cmd"; then
        die "Required command '$cmd' not found. Install it with: apt install $pkg"
    fi
}

# Get the repository root directory
get_repo_root() {
    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    (cd "$script_dir/../.." && pwd)
}

# Verify we're in the shepherd repository
verify_repo() {
    local repo_root
    repo_root="$(get_repo_root)"
    if [[ ! -f "$repo_root/Cargo.toml" ]]; then
        die "Not in shepherd repository (Cargo.toml not found at $repo_root)"
    fi
    # Check it's the right project
    if ! grep -q 'shepherd-launcher-ui' "$repo_root/Cargo.toml" 2>/dev/null; then
        die "This doesn't appear to be the shepherd-launcher repository"
    fi
}

# Check Ubuntu version and warn if not supported
check_ubuntu_version() {
    local min_version="${1:-25.10}"
    
    if [[ ! -f /etc/os-release ]]; then
        warn "Cannot determine OS version (not Linux or missing /etc/os-release)"
        return 0
    fi
    
    # shellcheck source=/dev/null
    source /etc/os-release
    
    if [[ "${ID:-}" != "ubuntu" ]]; then
        warn "This system is not Ubuntu (detected: ${ID:-unknown}). Some features may not work."
        return 0
    fi
    
    local version="${VERSION_ID:-0}"
    if [[ "$(printf '%s\n' "$min_version" "$version" | sort -V | head -n1)" != "$min_version" ]]; then
        warn "Ubuntu version $version detected. Recommended: $min_version or higher."
    fi
}

# Safe file backup - creates timestamped backup
backup_file() {
    local file="$1"
    local backup_dir="${2:-}"
    
    if [[ ! -e "$file" ]]; then
        return 0
    fi
    
    local timestamp
    timestamp="$(date +%Y%m%d_%H%M%S)"
    local backup_name
    
    if [[ -n "$backup_dir" ]]; then
        mkdir -p "$backup_dir"
        backup_name="$backup_dir/$(basename "$file").$timestamp"
    else
        backup_name="$file.$timestamp.bak"
    fi
    
    cp -a "$file" "$backup_name"
    echo "$backup_name"
}

# Create directory with proper permissions
ensure_dir() {
    local dir="$1"
    local mode="${2:-0755}"
    local owner="${3:-}"
    
    if [[ ! -d "$dir" ]]; then
        mkdir -p "$dir"
        chmod "$mode" "$dir"
        if [[ -n "$owner" ]]; then
            chown "$owner" "$dir"
        fi
    fi
}

# Validate username exists
validate_user() {
    local user="$1"
    if ! id "$user" &>/dev/null; then
        die "User '$user' does not exist"
    fi
}

# Get user's home directory
get_user_home() {
    local user="$1"
    getent passwd "$user" | cut -d: -f6
}

# Kill processes matching a pattern (silent if none found)
kill_matching() {
    local pattern="$1"
    pkill -f "$pattern" 2>/dev/null || true
}

# Wait for a file/socket to appear
wait_for_file() {
    local file="$1"
    local timeout="${2:-30}"
    local elapsed=0
    
    while [[ ! -e "$file" ]] && [[ $elapsed -lt $timeout ]]; do
        sleep 0.5
        elapsed=$((elapsed + 1))
    done
    
    [[ -e "$file" ]]
}
