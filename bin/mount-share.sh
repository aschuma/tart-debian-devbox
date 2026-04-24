#!/usr/bin/env bash
set -euo pipefail

HOST="debian-ssh"
SSH_CONFIG="${HOME}/.ssh/config"
IDENTITY_FILE="${HOME}/.ssh/id_ed25519_tart"
USER="admin"


REMOTE_CMD='sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared'

ssh -tt \
  -F "$SSH_CONFIG" \
  -i "$IDENTITY_FILE" \
  "${USER}@${HOST}" \
  "$REMOTE_CMD"
