#!/usr/bin/env bash
set -euo pipefail

# Manual tweak required:
# 
# Edit the sshd_config (System-wide Fix)
#
# Open /etc/ssh/sshd_config.
# Ensure PermitUserEnvironment is set to yes.
# Restart SSH: sudo systemctl restart ssh.
#

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${SCRIPT_DIR}/../shared-env.sh"

# .env file lives alongside provision.d/ (i.e. in bin/)
ENV_FILE="${SCRIPT_DIR}/../.env"

# Base64-encode .env to safely transfer special characters to the VM (empty if file missing)
if [[ -f "$ENV_FILE" || -p "$ENV_FILE" ]]; then
  ENV_B64=$(base64 < "$ENV_FILE")
else
  echo "No .env file found at ${ENV_FILE}, assuming empty content."
  ENV_B64=$(base64 < /dev/null)
fi

ssh -T \
  -F "$SSH_CONFIG" \
  -i "$IDENTITY_FILE" \
  "${SSH_USER}@${HOST}" bash << ENDSSH
set -euo pipefail
ETC_ENV='/etc/environment'
TMPFILE=\$(mktemp)

# If marker already exists, keep only lines before it; otherwise keep all existing lines
if grep -qF '# TART PROVISION' "\$ETC_ENV" 2>/dev/null; then
  awk '/^# TART PROVISION/{exit} {print}' "\$ETC_ENV" > "\$TMPFILE"
else
  cat "\$ETC_ENV" > "\$TMPFILE" 2>/dev/null || touch "\$TMPFILE"
fi

# Append marker followed by the .env content
printf '\n# TART PROVISION\n' >> "\$TMPFILE"
echo '${ENV_B64}' | base64 -d >> "\$TMPFILE"

sudo cp "\$TMPFILE" "\$ETC_ENV"
rm -f "\$TMPFILE"
echo "Updated /etc/environment"
ENDSSH
