#!/usr/bin/env bash
# One-off migration of YOUR TRUSTED shell configs. This sources the originals,
# just as tart-ctl.sh did: never run it on an untrusted project's config.
# Usage: bash scripts/migrate-tctl-env.sh [PROJECT_DIR ...]  (default: PWD)
# Also converts ~/.config/tctl/config (or XDG_CONFIG_HOME). Originals are kept;
# existing TOML files are never overwritten. Review the results before use.
set -euo pipefail

convert() (
  local source_file="$1" target="$2"
  [[ -f "$source_file" ]] || exit 0
  if [[ -e "$target" ]]; then
    printf 'Kept existing %s\n' "$target"
    exit 0
  fi
  unset TCTL_VM_NAME TCTL_HOST_SHARE_DIR TCTL_SSH_USER TCTL_SSH_CONFIG \
    TCTL_IDENTITY_FILE TCTL_REMOTE_FORWARD TCTL_LOCAL_FORWARD TCTL_ENV_PROVISION_FILES
  set -a
  # shellcheck source=/dev/null
  source "$source_file"
  set +a
  python3 - "$target" <<'PY'
import json
import os
from pathlib import Path
import sys

sections = {
    'vm': {'name': 'TCTL_VM_NAME'},
    'share': {'dir': 'TCTL_HOST_SHARE_DIR'},
    'ssh': {'user': 'TCTL_SSH_USER', 'config_path': 'TCTL_SSH_CONFIG',
            'identity_file': 'TCTL_IDENTITY_FILE', 'remote_forward': 'TCTL_REMOTE_FORWARD',
            'local_forward': 'TCTL_LOCAL_FORWARD'},
    'provision': {'env_files': 'TCTL_ENV_PROVISION_FILES'},
}
arrays = {'remote_forward', 'local_forward', 'env_files'}
lines = ['# Migrated from shell config; review before use.\n']
for section, keys in sections.items():
    present = [(key, os.environ[var]) for key, var in keys.items() if var in os.environ]
    if not present:
        continue
    lines.append(f'[{section}]')
    for key, value in present:
        if key in arrays:
            value = [part.strip() for part in value.split(',') if part.strip()]
        elif section == 'share' and not value:
            value = 'none'
        lines.append(f'{key} = {json.dumps(value, ensure_ascii=False)}')
    lines.append('')
path = Path(sys.argv[1])
with path.open('x') as out:
    out.write('\n'.join(lines))
print(f'Created {path} (original kept)')
PY
)

config_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/tctl"
convert "${config_dir}/config" "${config_dir}/config.toml"
[[ $# -gt 0 ]] || set -- "$PWD"
for project in "$@"; do
  convert "${project}/.tart-ctl-env" "${project}/.tart-ctl.toml"
done
