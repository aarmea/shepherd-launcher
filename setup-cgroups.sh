#!/bin/bash
# Setup cgroups v2 for shepherd-launcher
# This script must be run as root (or with sudo)

set -e

CGROUP_BASE="/sys/fs/cgroup/shepherd"

# Check if cgroups v2 is available
if [ ! -f /sys/fs/cgroup/cgroup.controllers ]; then
    echo "Error: cgroups v2 is not available on this system"
    echo "Make sure your kernel supports cgroups v2 and it's mounted"
    exit 1
fi

# Get the user who will run shepherd (default to SUDO_USER or current user)
SHEPHERD_USER="${1:-${SUDO_USER:-$(whoami)}}"

echo "Setting up cgroups for shepherd-launcher..."
echo "User: $SHEPHERD_USER"

# Create the shepherd cgroup directory
mkdir -p "$CGROUP_BASE"

# Set ownership so the shepherd daemon can create session cgroups
chown "$SHEPHERD_USER:$SHEPHERD_USER" "$CGROUP_BASE"

# Set permissions (owner can read/write/execute, others can read/execute)
chmod 755 "$CGROUP_BASE"

echo "Created $CGROUP_BASE with ownership $SHEPHERD_USER"
echo ""
echo "cgroups v2 setup complete!"
echo "The shepherd daemon can now create session cgroups for reliable process management."
