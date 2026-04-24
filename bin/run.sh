#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/shared-env.sh"

tart run --no-graphics --dir=workspace:"$HOST_SHARE_DIR" "$HOST" &
