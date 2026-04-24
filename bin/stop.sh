#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/shared-env.sh"

if tart ip "$HOST" >/dev/null 2>&1; then
  tart stop "$HOST"
  echo "Stopped VM '$HOST'"
else
  echo "VM '$HOST' is not running"
fi
