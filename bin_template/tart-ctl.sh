#!/usr/bin/env bash
# tart-ctl.sh — Global Tart VM management script
# Usage: tart-ctl.sh [command]
#
# Recursively searches upward from PWD for a .tart-ctl-env file,
# sources it to load VM configuration, then dispatches the given command.

set -euo pipefail

# ─── Constants ────────────────────────────────────────────────────────────────

readonly SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly ENV_FILE_NAME=".tart-ctl-env"
readonly PROVISION_DIR_NAME=".tart-ctl-provision.d"

# ─── Colours (only when stdout is a terminal) ─────────────────────────────────

if [[ -t 1 ]]; then
  BOLD='\033[1m'
  DIM='\033[2m'
  GREEN='\033[0;32m'
  YELLOW='\033[0;33m'
  RED='\033[0;31m'
  CYAN='\033[0;36m'
  RESET='\033[0m'
else
  BOLD='' DIM='' GREEN='' YELLOW='' RED='' CYAN='' RESET=''
fi

# ─── Helpers ──────────────────────────────────────────────────────────────────

info()    { echo -e "${GREEN}==>${RESET} $*"; }
warn()    { echo -e "${YELLOW}[warn]${RESET} $*" >&2; }
error()   { echo -e "${RED}[error]${RESET} $*" >&2; }
header()  { echo -e "${BOLD}${CYAN}$*${RESET}"; }
dim()     { echo -e "${DIM}$*${RESET}"; }

die() {
  error "$*"
  exit 1
}

# Returns the VM state by parsing `tart list` output: "running", "stopped", or "unknown".
vm_get_state() {
  local state
  state="$(tart list 2>/dev/null | awk -v name="${TCTL_VM_NAME}" 'NR>1 && $2==name {print $NF}')"
  if [[ -z "${state}" ]]; then
    echo "unknown"
  else
    echo "${state}"
  fi
}

# Returns 0 and sets VM_IP if the VM is running and has a reachable IP, 1 otherwise.
vm_ip_or_false() {
  local state
  state="$(vm_get_state)"
  if [[ "${state}" != "running" ]]; then
    return 1
  fi
  VM_IP="$(tart ip "${TCTL_VM_NAME}" 2>/dev/null)"
  [[ -n "${VM_IP}" ]]
}

# Ensures the VM is running; exits with a clear error if not.
# Sets VM_IP to the current IP on success.
require_running() {
  if ! vm_ip_or_false; then
    die "VM '${TCTL_VM_NAME}' is not running. Start it with: ${SCRIPT_NAME} up"
  fi
}

# ─── Help ─────────────────────────────────────────────────────────────────────

usage() {
  cat <<EOF

$(header "tart-ctl.sh — Tart VM Controller")

$(dim "Searches upward from PWD for ${ENV_FILE_NAME}, sources it, then")
$(dim "dispatches the given command against the configured VM.")

${BOLD}USAGE${RESET}
    ${SCRIPT_NAME} <command>
    ${SCRIPT_NAME} -h | --help

${BOLD}COMMANDS${RESET}
    $(printf '%-28s' 'up,  start')Start the VM
    $(printf '%-28s' 'down, stop')Stop the VM
    $(printf '%-28s' 'st,  status')Show VM status (running / stopped)
    $(printf '%-28s' 'pr,  provision')Run all scripts in ${PROVISION_DIR_NAME}
    $(printf '%-28s' 'sh,  ssh')Open an interactive SSH shell in the VM
    $(printf '%-28s' 'ip')Print the VM's current IP address
    $(printf '%-28s' 'usc, update-ssh-config')Update ~/.ssh/config with the VM's current IP
    $(printf '%-28s' 'help, -h, --help')Show this help message

${BOLD}DISCOVERY${RESET}
    The script walks upward from the current directory looking for
    ${ENV_FILE_NAME}.  The search stops at a .git directory or the
    filesystem root.  The found file is sourced to populate variables
    such as TCTL_VM_NAME that are used by the commands above.

${BOLD}ENVIRONMENT FILE${RESET}
    Expected variables in ${ENV_FILE_NAME} (all prefixed TCTL_):
        TCTL_VM_NAME        Name of the Tart VM to manage         (required)
        TCTL_SSH_USER       SSH user inside the VM                (required for provision)
        TCTL_SSH_CONFIG     Path to SSH client config file        (required for provision)
        TCTL_IDENTITY_FILE  Path to SSH private key               (required for provision)
        TCTL_HOST_SHARE_DIR Host directory to mount into the VM   (optional)

    A sibling ${BOLD}.env${RESET} file (plain KEY=VALUE) is picked up by the
    'provision' command and pushed into the VM's /etc/profile.d/.

${BOLD}EXAMPLES${RESET}
    ${SCRIPT_NAME} up
    ${SCRIPT_NAME} provision
    ${SCRIPT_NAME} st
    ${SCRIPT_NAME} ssh
    ${SCRIPT_NAME} ssh -- htop

EOF
}

# ─── Config Discovery ─────────────────────────────────────────────────────────

# Walks upward from PWD looking for .tart-ctl-env.
# Sets CONFIG_DIR and CONFIG_FILE on success; calls die() on failure.
find_config() {
  local dir
  dir="$(pwd)"

  while true; do
    if [[ -f "${dir}/${ENV_FILE_NAME}" ]]; then
      CONFIG_DIR="${dir}"
      CONFIG_FILE="${dir}/${ENV_FILE_NAME}"
      return 0
    fi

    # Stop at a git root
    if [[ -d "${dir}/.git" ]]; then
      break
    fi

    # Stop at filesystem root
    if [[ "${dir}" == "/" ]]; then
      break
    fi

    dir="$(dirname "${dir}")"
  done

  die "No '${ENV_FILE_NAME}' found (searched from '$(pwd)' upward to '${dir}').\n       Create one alongside your VM project or place it at a git root."
}

# ─── Commands ─────────────────────────────────────────────────────────────────

cmd_start() {
  if vm_ip_or_false; then
    warn "VM '${TCTL_VM_NAME}' is already running (${VM_IP})."
    return 0
  fi

  info "Starting VM '${TCTL_VM_NAME}' …"
  if [[ -n "${TCTL_HOST_SHARE_DIR:-}" ]]; then
    tart run --no-graphics --dir=workspace:"${TCTL_HOST_SHARE_DIR}" "${TCTL_VM_NAME}" &
  else
    tart run --no-graphics "${TCTL_VM_NAME}" &
  fi
  info "VM '${TCTL_VM_NAME}' started (background PID $!)."
}

cmd_stop() {
  if vm_ip_or_false; then
    info "Stopping VM '${TCTL_VM_NAME}' …"
    tart stop "${TCTL_VM_NAME}"
    info "VM '${TCTL_VM_NAME}' stopped."
  else
    warn "VM '${TCTL_VM_NAME}' is not running."
  fi
}

cmd_status() {
  header "Status: ${TCTL_VM_NAME}"
  local state
  state="$(vm_get_state)"
  case "${state}" in
    running)
      vm_ip_or_false || true
      echo -e "  State : ${GREEN}running${RESET}"
      echo    "  IP    : ${VM_IP:-unknown}"
      ;;
    stopped)
      echo -e "  State : ${YELLOW}stopped${RESET}"
      ;;
    unknown)
      echo -e "  State : ${RED}unknown${RESET} (VM not found in tart list)"
      ;;
    *)
      echo -e "  State : ${DIM}${state}${RESET}"
      ;;
  esac
  echo    "  Config: ${CONFIG_FILE}"
}

cmd_ip() {
  require_running
  echo "${VM_IP}"
}

cmd_update_ssh_config() {
  require_running
  local ip="${VM_IP}"
  info "Resolved '${TCTL_VM_NAME}' → ${ip}"

  # Ensure SSH config file exists
  mkdir -p "$(dirname "${TCTL_SSH_CONFIG}")"
  touch "${TCTL_SSH_CONFIG}"
  chmod 600 "${TCTL_SSH_CONFIG}"

  if grep -q "^Host ${TCTL_VM_NAME}$" "${TCTL_SSH_CONFIG}"; then
    # Update HostName in existing block (BSD & GNU sed compatible via temp file)
    local tmpfile
    tmpfile="$(mktemp)"
    awk -v host="${TCTL_VM_NAME}" -v ip="${ip}" '
      /^Host / { in_block = ($2 == host) }
      in_block && /^[[:space:]]+HostName / { sub(/HostName .*/, "HostName " ip) }
      { print }
    ' "${TCTL_SSH_CONFIG}" > "${tmpfile}"
    mv "${tmpfile}" "${TCTL_SSH_CONFIG}"
    chmod 600 "${TCTL_SSH_CONFIG}"
    info "Updated existing '${TCTL_VM_NAME}' entry → ${ip}"
  else
    # Append a new Host block
    cat >> "${TCTL_SSH_CONFIG}" <<EOF

Host ${TCTL_VM_NAME}
    HostName ${ip}
    User ${TCTL_SSH_USER}
    IdentityFile ${TCTL_IDENTITY_FILE}
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
    RemoteForward 1080 127.0.0.1:1080
EOF
    info "Added new '${TCTL_VM_NAME}' entry → ${ip}"
  fi
}

cmd_ssh() {
  # Any arguments after the command are passed through to ssh (e.g. a remote command).
  # Usage: tart-ctl.sh ssh [-- remote_cmd [args…]]
  local -a extra_args=("$@")

  require_running
  info "Connecting to '${TCTL_VM_NAME}' (${VM_IP}) …"

  # Build the ssh argument list
  local -a ssh_args=(
    -F "${TCTL_SSH_CONFIG}"
    -i "${TCTL_IDENTITY_FILE}"
  )

  if [[ ${#extra_args[@]} -gt 0 ]]; then
    # Non-interactive: pass remote command through
    exec ssh "${ssh_args[@]}" -T \
      "${TCTL_SSH_USER}@${TCTL_VM_NAME}" \
      "${extra_args[@]}"
  else
    # Interactive login shell
    exec ssh "${ssh_args[@]}" -t \
      "${TCTL_SSH_USER}@${TCTL_VM_NAME}"
  fi
}

cmd_provision() {
  require_running
  local provision_dir="${CONFIG_DIR}/${PROVISION_DIR_NAME}"

  if [[ ! -d "${provision_dir}" ]]; then
    warn "Provision directory not found: ${provision_dir}"
    warn "Nothing to do."
    return 0
  fi

  # Collect scripts in alphabetical (sorted) order
  local -a scripts
  mapfile -t scripts < <(find "${provision_dir}" -maxdepth 1 -name '*.sh' -type f | sort)

  if [[ ${#scripts[@]} -eq 0 ]]; then
    warn "No *.sh scripts found in ${provision_dir}"
    return 0
  fi

  info "Running ${#scripts[@]} provision script(s) from ${provision_dir} …"
  echo

  # Export all TCTL_ variables so provision scripts inherit them without
  # needing to source .tart-ctl-env themselves.
  local var
  while IFS= read -r var; do
    export "${var?}"
  done < <(compgen -v TCTL_)

  # Also export CONFIG_DIR so scripts can locate sibling files (e.g. .env)
  export TCTL_CONFIG_DIR="${CONFIG_DIR}"

  local failed=0
  for script in "${scripts[@]}"; do
    local name
    name="$(basename "${script}")"
    echo -e "${BOLD}──► ${name}${RESET}"
    if bash "${script}"; then
      echo -e "    ${GREEN}✓ done${RESET}"
    else
      local rc=$?
      echo -e "    ${RED}✗ FAILED (exit ${rc})${RESET}"
      (( failed++ )) || true
    fi
    echo
  done

  if (( failed > 0 )); then
    warn "${failed} script(s) failed."
  else
    info "All provision scripts completed successfully."
  fi
}

# ─── Main ─────────────────────────────────────────────────────────────────────

main() {
  # Handle help before anything else (no config needed)
  case "${1:-}" in
    -h|--help|help)
      usage
      exit 0
      ;;
    "")
      usage
      exit 0
      ;;
  esac

  # Discover and source config
  find_config

  # shellcheck source=/dev/null
  source "${CONFIG_FILE}"

  # Validate required variables
  if [[ -z "${TCTL_VM_NAME:-}" ]]; then
    die "'TCTL_VM_NAME' is not set in '${CONFIG_FILE}'."
  fi

  echo -e "${DIM}Config : ${CONFIG_FILE}${RESET}"
  echo -e "${DIM}VM     : ${TCTL_VM_NAME}${RESET}"
  echo

  # Dispatch command
  local cmd="${1}"
  shift
  case "${cmd}" in
    up|start)       cmd_start      ;;
    down|stop)      cmd_stop       ;;
    st|status)      cmd_status     ;;
    pr|provision)   cmd_provision  ;;
    ip)             cmd_ip                  ;;
    usc|update-ssh-config) cmd_update_ssh_config ;;
    sh|ssh)         cmd_ssh "$@"            ;;
    *)
      error "Unknown command: '${cmd}'"
      echo  "Run '${SCRIPT_NAME} --help' for usage."
      exit 1
      ;;
  esac
}

main "$@"
