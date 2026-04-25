#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../shared-env.sh"

REMOTE_CMD='sudo mkdir -p /mnt/shared && sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared'

ssh -tt \
  -F "$SSH_CONFIG" \
  -i "$IDENTITY_FILE" \
  "${SSH_USER}@${HOST}" \
  "$REMOTE_CMD"
