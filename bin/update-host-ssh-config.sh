#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/shared-env.sh"

# Resolve current VM IP
IP=$(tart ip "$HOST" 2>/dev/null) || { echo "Error: VM '$HOST' not running"; exit 1; }
echo "Resolved $HOST → $IP"

# Ensure ~/.ssh/config exists with correct permissions
mkdir -p "$(dirname "$SSH_CONFIG")"
chmod 700 "$(dirname "$SSH_CONFIG")"
touch "$SSH_CONFIG"
chmod 600 "$SSH_CONFIG"

# Check if Host block already exists
if grep -q "^Host ${HOST}$" "$SSH_CONFIG"; then
    # Update existing HostName
    sed -i '' "/^Host ${HOST}$/,/^Host / {
        s/^    HostName .*/    HostName ${IP}/
    }" "$SSH_CONFIG"
    echo "Updated existing '$HOST' entry with IP $IP"
else
    # Append new block
    cat >> "$SSH_CONFIG" <<EOF

Host ${HOST}
    HostName ${IP}
    User ${SSH_USER}
    IdentityFile ${IDENTITY_FILE}
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
    RemoteForward 1080 127.0.0.1:1080
EOF
    echo "Added new '$HOST' entry with IP $IP"
fi
