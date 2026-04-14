#!/usr/bin/env bash
set -euo pipefail

VM_NAME="${1:-debian-ssh}"
SHARE_DIR="${2:-$PWD}"

tart run --no-graphics --dir=hostshare:"$SHARE_DIR" "$VM_NAME" &
