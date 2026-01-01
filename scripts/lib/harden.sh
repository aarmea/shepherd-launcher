#!/usr/bin/env bash
# User hardening logic for shepherd-launcher
# Applies and reverts kiosk-style user restrictions

# Get the directory containing this script
HARDEN_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Source common utilities
# shellcheck source=common.sh
source "$HARDEN_LIB_DIR/common.sh"

# State directory for hardening rollback
HARDENING_STATE_DIR="/var/lib/shepherdd/hardening"

# Get the state directory for a user
get_user_state_dir() {
    local user="$1"
    echo "$HARDENING_STATE_DIR/$user"
}

# Check if a user is currently hardened
is_hardened() {
    local user="$1"
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    
    [[ -f "$state_dir/hardened" ]]
}

# Save a file for later restoration
save_for_restore() {
    local user="$1"
    local file="$2"
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    
    local relative_path="${file#/}"
    local backup_path="$state_dir/backup/$relative_path"
    
    mkdir -p "$(dirname "$backup_path")"
    
    if [[ -e "$file" ]]; then
        cp -a "$file" "$backup_path"
        echo "exists" > "$backup_path.meta"
    else
        echo "absent" > "$backup_path.meta"
    fi
}

# Restore a previously saved file
restore_file() {
    local user="$1"
    local file="$2"
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    
    local relative_path="${file#/}"
    local backup_path="$state_dir/backup/$relative_path"
    local meta_file="$backup_path.meta"
    
    if [[ ! -f "$meta_file" ]]; then
        warn "No backup metadata for $file, skipping"
        return 0
    fi
    
    local original_state
    original_state="$(cat "$meta_file")"
    
    if [[ "$original_state" == "exists" ]]; then
        if [[ -e "$backup_path" ]]; then
            cp -a "$backup_path" "$file"
            info "  Restored: $file"
        else
            warn "Backup file missing for $file"
        fi
    else
        # File didn't exist originally, remove it
        rm -f "$file"
        info "  Removed: $file (didn't exist before)"
    fi
}

# Record a change action for rollback
record_action() {
    local user="$1"
    local action="$2"
    local target="$3"
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    
    echo "$action|$target" >> "$state_dir/actions.log"
}

# Apply hardening to a user
harden_apply() {
    local user="$1"
    
    require_root
    validate_user "$user"
    
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    local user_home
    user_home="$(get_user_home "$user")"
    
    if is_hardened "$user"; then
        warn "User $user is already hardened. Use 'shepherd harden revert' first."
        return 0
    fi
    
    info "Applying hardening to user: $user"
    
    # Create state directory
    mkdir -p "$state_dir/backup"
    chmod 0700 "$state_dir"
    
    # Initialize actions log
    : > "$state_dir/actions.log"
    
    # =========================================================================
    # 1. Set user shell to restricted shell or nologin for non-sway access
    # =========================================================================
    info "Configuring user shell..."
    
    local original_shell
    original_shell="$(getent passwd "$user" | cut -d: -f7)"
    echo "$original_shell" > "$state_dir/original_shell"
    
    # Keep bash for sway to work, but we'll restrict other access methods
    # The shell restriction is handled by PAM and session limits instead
    record_action "$user" "shell" "$original_shell"
    
    # =========================================================================
    # 2. Configure user's .bashrc to be restricted
    # =========================================================================
    info "Configuring shell restrictions..."
    
    local bashrc="$user_home/.bashrc"
    save_for_restore "$user" "$bashrc"
    
    # Append restriction to bashrc (if not in sway, exit)
    cat >> "$bashrc" <<'EOF'

# Shepherd hardening: restrict to sway session only
if [[ -z "${WAYLAND_DISPLAY:-}" ]] && [[ -z "${SWAYSOCK:-}" ]]; then
    echo "This account is restricted to the Shepherd kiosk environment."
    exit 1
fi
EOF
    chown "$user:$user" "$bashrc"
    record_action "$user" "file" "$bashrc"
    
    # =========================================================================
    # 3. Disable SSH access for this user
    # =========================================================================
    info "Restricting SSH access..."
    
    local shepherd_sshd_config="/etc/ssh/sshd_config.d/shepherd-$user.conf"
    
    save_for_restore "$user" "$shepherd_sshd_config"
    
    # Create a drop-in config to deny this user
    mkdir -p /etc/ssh/sshd_config.d
    cat > "$shepherd_sshd_config" <<EOF
# Shepherd hardening: deny SSH access for kiosk user
DenyUsers $user
EOF
    chmod 0644 "$shepherd_sshd_config"
    record_action "$user" "file" "$shepherd_sshd_config"
    
    # Reload sshd if running
    if systemctl is-active --quiet sshd 2>/dev/null || systemctl is-active --quiet ssh 2>/dev/null; then
        systemctl reload sshd 2>/dev/null || systemctl reload ssh 2>/dev/null || true
    fi
    
    # =========================================================================
    # 4. Disable virtual console (TTY) access via PAM
    # =========================================================================
    info "Restricting console access..."
    
    local pam_access="/etc/security/access.conf"
    local shepherd_access_marker="# Shepherd hardening for user: $user"
    
    save_for_restore "$user" "$pam_access"
    
    # Add rule to deny console access (but allow via display managers)
    if ! grep -q "$shepherd_access_marker" "$pam_access" 2>/dev/null; then
        cat >> "$pam_access" <<EOF

$shepherd_access_marker
# Deny console login for kiosk user (allow display manager access)
-:$user:tty1 tty2 tty3 tty4 tty5 tty6 tty7
EOF
    fi
    record_action "$user" "file" "$pam_access"
    
    # =========================================================================
    # 5. Set up autologin to shepherd session (systemd override)
    # =========================================================================
    info "Configuring auto-login (if applicable)..."
    
    # Create getty override for auto-login (optional - only if desired)
    # This doesn't force auto-login, but prepares the override if needed
    local getty_override_dir="/etc/systemd/system/getty@tty1.service.d"
    local getty_override="$getty_override_dir/shepherd-autologin.conf"
    
    save_for_restore "$user" "$getty_override"
    
    mkdir -p "$getty_override_dir"
    cat > "$getty_override" <<EOF
# Shepherd hardening: auto-login for kiosk user
# Uncomment the following lines to enable auto-login to tty1
# [Service]
# ExecStart=
# ExecStart=-/sbin/agetty --autologin $user --noclear %I \$TERM
EOF
    chmod 0644 "$getty_override"
    record_action "$user" "file" "$getty_override"
    
    # =========================================================================
    # 6. Lock down sudo access
    # =========================================================================
    info "Restricting sudo access..."
    
    local sudoers_file="/etc/sudoers.d/shepherd-$user"
    save_for_restore "$user" "$sudoers_file"
    
    # Explicitly deny sudo for this user
    cat > "$sudoers_file" <<EOF
# Shepherd hardening: deny sudo access for kiosk user
$user ALL=(ALL) !ALL
EOF
    chmod 0440 "$sudoers_file"
    record_action "$user" "file" "$sudoers_file"
    
    # =========================================================================
    # 7. Set restrictive file permissions on user home
    # =========================================================================
    info "Securing home directory permissions..."
    
    # Save original permissions
    stat -c "%a" "$user_home" > "$state_dir/home_perms"
    
    # Set restrictive permissions
    chmod 0700 "$user_home"
    record_action "$user" "perms" "$user_home"
    
    # =========================================================================
    # Mark as hardened
    # =========================================================================
    date -Iseconds > "$state_dir/hardened"
    echo "$user" > "$state_dir/user"
    
    success "Hardening applied to user: $user"
    info ""
    info "The following restrictions are now active:"
    info "  - SSH access denied"
    info "  - Console (TTY) login restricted"
    info "  - Sudo access denied"
    info "  - Shell restricted to Sway sessions"
    info "  - Home directory secured (mode 0700)"
    info ""
    info "To revert: shepherd harden revert --user $user"
}

# Revert hardening from a user
harden_revert() {
    local user="$1"
    
    require_root
    validate_user "$user"
    
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    local user_home
    user_home="$(get_user_home "$user")"
    
    if ! is_hardened "$user"; then
        warn "User $user is not currently hardened."
        return 0
    fi
    
    info "Reverting hardening for user: $user"
    
    # =========================================================================
    # Restore all saved files
    # =========================================================================
    if [[ -f "$state_dir/actions.log" ]]; then
        while IFS='|' read -r action target; do
            case "$action" in
                file)
                    restore_file "$user" "$target"
                    ;;
                perms)
                    if [[ -f "$state_dir/home_perms" ]]; then
                        local original_perms
                        original_perms="$(cat "$state_dir/home_perms")"
                        chmod "$original_perms" "$target"
                        info "  Restored permissions on: $target"
                    fi
                    ;;
                shell)
                    # Shell wasn't changed, nothing to revert
                    ;;
            esac
        done < "$state_dir/actions.log"
    fi
    
    # =========================================================================
    # Reload services that may have been affected
    # =========================================================================
    if systemctl is-active --quiet sshd 2>/dev/null || systemctl is-active --quiet ssh 2>/dev/null; then
        systemctl reload sshd 2>/dev/null || systemctl reload ssh 2>/dev/null || true
    fi
    
    # =========================================================================
    # Clean up state directory
    # =========================================================================
    rm -rf "$state_dir"
    
    success "Hardening reverted for user: $user"
    info ""
    info "All restrictions have been removed. The user can now:"
    info "  - Access via SSH"
    info "  - Login at console"
    info "  - Use sudo (if previously allowed)"
}

# Show hardening status
harden_status() {
    local user="$1"
    
    validate_user "$user"
    
    local state_dir
    state_dir="$(get_user_state_dir "$user")"
    
    if is_hardened "$user"; then
        local hardened_date
        hardened_date="$(cat "$state_dir/hardened")"
        echo "User '$user' is HARDENED (since $hardened_date)"
        
        if [[ -f "$state_dir/actions.log" ]]; then
            echo ""
            echo "Applied restrictions:"
            while IFS='|' read -r action target; do
                echo "  - $action: $target"
            done < "$state_dir/actions.log"
        fi
    else
        echo "User '$user' is NOT hardened"
    fi
}

# Main harden command dispatcher
harden_main() {
    local subcmd="${1:-}"
    shift || true
    
    local user=""
    
    # Parse remaining arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --user)
                user="$2"
                shift 2
                ;;
            *)
                die "Unknown option: $1"
                ;;
        esac
    done
    
    case "$subcmd" in
        apply)
            if [[ -z "$user" ]]; then
                die "Usage: shepherd harden apply --user USER"
            fi
            harden_apply "$user"
            ;;
        revert)
            if [[ -z "$user" ]]; then
                die "Usage: shepherd harden revert --user USER"
            fi
            harden_revert "$user"
            ;;
        status)
            if [[ -z "$user" ]]; then
                die "Usage: shepherd harden status --user USER"
            fi
            harden_status "$user"
            ;;
        ""|help|-h|--help)
            cat <<EOF
Usage: shepherd harden <command> --user USER

Commands:
    apply     Apply kiosk hardening to a user
    revert    Revert hardening and restore original state
    status    Show hardening status for a user

Options:
    --user USER    Target user for hardening operations (required)

Hardening includes:
    - Denying SSH access
    - Restricting console (TTY) login
    - Denying sudo access
    - Restricting shell to Sway sessions only
    - Securing home directory permissions

State is preserved in: $HARDENING_STATE_DIR/<user>/

Examples:
    shepherd harden apply --user kiosk
    shepherd harden status --user kiosk
    shepherd harden revert --user kiosk
EOF
            ;;
        *)
            die "Unknown harden command: $subcmd (try: shepherd harden help)"
            ;;
    esac
}
