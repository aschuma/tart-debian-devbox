#!/usr/bin/env bash
# Explicit, manual real-VM smoke test. Does not clone/delete VMs or provision
# env files. Updates the managed SSH block. Stops the VM only if we started it.
set -euo pipefail
binary="$1"
cd "$2"
state="$("$binary" --json status | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["state"])')"
case "$state" in running|stopped) ;; *) echo 'Smoke test requires an existing, running or stopped project clone.' >&2; exit 1 ;; esac
started=0
cleanup() { if (( started )); then "$binary" --quiet stop; fi; }
trap cleanup EXIT
if [[ "$state" == stopped ]]; then started=1; fi
"$binary" start
"$binary" status
"$binary" update-ssh-config
ready=0
for ((i=0; i<30; i++)); do
  if "$binary" --quiet ssh -- true; then ready=1; break; fi
  sleep 1
done
(( ready )) || { echo 'SSH did not become ready.' >&2; exit 1; }
"$binary" env
echo 'Smoke test passed (no environment provisioned).'
