#!/usr/bin/env bash
# with-vm.sh — run a host command against the TEMPLATE VM, starting it if needed.
#
# Usage: with-vm.sh [-C <dir>] <command> [args…]
#
#   * Reads BUILD_VM_NAME / BUILD_SSH_USER / BUILD_IDENTITY_FILE from build.env.
#   * If the VM is not running, starts it headless and waits until it has an IP
#     and accepts SSH. If it was already running, leaves it alone.
#   * Replaces the literal token __VM_IP__ in every argument with the VM's IP
#     and also exports VM_IP, then runs the command (optionally after cd -C <dir>).
#   * Afterwards stops the VM again — but only if this script started it.
#
# Used by the justfile (`just reprovision`, `just tools`).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=build.env
source "${SCRIPT_DIR}/build.env"
: "${BUILD_VM_NAME:?BUILD_VM_NAME not set in build/build.env}"
: "${BUILD_SSH_USER:?BUILD_SSH_USER not set in build/build.env}"
: "${BUILD_IDENTITY_FILE:?BUILD_IDENTITY_FILE not set in build/build.env}"

readonly SSH_OPTS=(-o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null
                   -o BatchMode=yes -o ConnectTimeout=5 -o LogLevel=ERROR)
readonly BOOT_TIMEOUT="${WITH_VM_BOOT_TIMEOUT:-180}"   # seconds to wait for IP + SSH

info() { printf '==> %s\n' "$*" >&2; }
die()  { printf '[error] %s\n' "$*" >&2; exit 1; }

workdir=""
while getopts ":C:" opt; do
  case "${opt}" in
    C) workdir="${OPTARG}" ;;
    *) die "Usage: $(basename "$0") [-C <dir>] <command> [args…]" ;;
  esac
done
shift $((OPTIND - 1))
(( $# > 0 )) || die "Usage: $(basename "$0") [-C <dir>] <command> [args…]"

vm_state() {
  tart list 2>/dev/null | awk -v name="${BUILD_VM_NAME}" 'NR>1 && $2==name {print $NF}'
}

STARTED_BY_US=0
RUN_PID=""
cleanup() {
  local rc=$?
  if (( STARTED_BY_US )); then
    info "Stopping '${BUILD_VM_NAME}' (started by this command) …"
    tart stop "${BUILD_VM_NAME}" >/dev/null 2>&1 || true
    [[ -n "${RUN_PID}" ]] && wait "${RUN_PID}" 2>/dev/null || true
  fi
  exit "${rc}"
}
trap cleanup EXIT
trap 'exit 130' INT TERM

state="$(vm_state)"
case "${state}" in
  running) info "VM '${BUILD_VM_NAME}' is already running — leaving it running afterwards." ;;
  "")      die "No Tart VM named '${BUILD_VM_NAME}' — build it first: just build" ;;
  *)
    info "Starting '${BUILD_VM_NAME}' (state: ${state}) …"
    tart run --no-graphics "${BUILD_VM_NAME}" >/dev/null 2>&1 &
    RUN_PID=$!
    STARTED_BY_US=1
    ;;
esac

# Wait for an IP, then for sshd to accept our key.
VM_IP=""
ready=0
deadline=$(( SECONDS + BOOT_TIMEOUT ))
while (( SECONDS < deadline )); do
  if [[ -n "${RUN_PID}" ]] && ! kill -0 "${RUN_PID}" 2>/dev/null; then
    die "'tart run ${BUILD_VM_NAME}' exited unexpectedly."
  fi
  if [[ -z "${VM_IP}" ]]; then
    VM_IP="$(tart ip "${BUILD_VM_NAME}" 2>/dev/null || true)"
  fi
  if [[ -n "${VM_IP}" ]] && ssh "${SSH_OPTS[@]}" -i "${BUILD_IDENTITY_FILE}" \
       "${BUILD_SSH_USER}@${VM_IP}" true 2>/dev/null; then
    ready=1
    break
  fi
  sleep 2
done
if (( ! ready )); then
  [[ -n "${VM_IP}" ]] || die "Timed out after ${BOOT_TIMEOUT}s waiting for '${BUILD_VM_NAME}' to get an IP."
  die "Timed out after ${BOOT_TIMEOUT}s waiting for SSH on ${BUILD_SSH_USER}@${VM_IP}."
fi
info "VM '${BUILD_VM_NAME}' reachable at ${VM_IP}"
export VM_IP

# Substitute the IP token and run the command.
cmd=()
for arg in "$@"; do cmd+=("${arg//__VM_IP__/${VM_IP}}"); done
[[ -n "${workdir}" ]] && cd "${workdir}"
"${cmd[@]}"
