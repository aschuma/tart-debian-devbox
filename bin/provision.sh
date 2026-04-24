#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROVISION_D="${SCRIPT_DIR}/provision.d"

mapfile -t SCRIPTS < <(find "$PROVISION_D" -maxdepth 1 -name '*.sh' -type f | sort)

if [[ ${#SCRIPTS[@]} -eq 0 ]]; then
  echo "No scripts found in ${PROVISION_D}"
  exit 0
fi

for SCRIPT in "${SCRIPTS[@]}"; do
  echo "==> Running $(basename "$SCRIPT") ..."
  if bash "$SCRIPT"; then
    echo "==> Done: $(basename "$SCRIPT")"
  else
    echo "==> FAILED: $(basename "$SCRIPT") (exit $?), continuing..."
  fi
done
