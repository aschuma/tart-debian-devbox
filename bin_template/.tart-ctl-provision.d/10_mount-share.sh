#!/usr/bin/env bash
# Mounts the host virtiofs share inside the VM.
# All TCTL_ variables are injected by tart-ctl.sh before this script runs.
set -euo pipefail

MOUNT_TAG="com.apple.virtio-fs.automount"
MOUNT_POINT="/mnt/shared"
REMOTE_CMD="sudo mkdir -p ${MOUNT_POINT} && (mountpoint -q ${MOUNT_POINT} || sudo mount -t virtiofs ${MOUNT_TAG} ${MOUNT_POINT})"

if [[ -z "${TCTL_HOST_SHARE_DIR:-}" ]]; then
  echo "  TCTL_HOST_SHARE_DIR is not set in .env — VM was started without --dir, nothing to mount. Skipping."
  exit 0
fi

echo "  Host share : ${TCTL_HOST_SHARE_DIR}"
echo "  Mount point: ${MOUNT_POINT} (virtiofs tag: ${MOUNT_TAG})"
echo "  Target     : ${TCTL_SSH_USER}@${TCTL_VM_NAME}"
echo "  SSH config : ${TCTL_SSH_CONFIG}"
echo "  Identity   : ${TCTL_IDENTITY_FILE}"
echo

echo "  Mounting share on VM …"
ssh -tt \
  -F "${TCTL_SSH_CONFIG}" \
  -i "${TCTL_IDENTITY_FILE}" \
  "${TCTL_SSH_USER}@${TCTL_VM_NAME}" \
  "${REMOTE_CMD}"

echo "  Share mounted at ${MOUNT_POINT}."
