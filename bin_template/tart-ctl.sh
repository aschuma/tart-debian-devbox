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

# ─── Colours (only when stdout is a terminal) ─────────────────────────────────

if [[ -t 1 ]]; then
  BOLD=$'\033[1m'
  DIM=$'\033[2m'
  GREEN=$'\033[0;32m'
  YELLOW=$'\033[0;33m'
  RED=$'\033[0;31m'
  CYAN=$'\033[0;36m'
  RESET=$'\033[0m'
else
  BOLD='' DIM='' GREEN='' YELLOW='' RED='' CYAN='' RESET=''
fi

# ─── Helpers ──────────────────────────────────────────────────────────────────

info()    { printf '%s\n' "${GREEN}==>${RESET} $*"; }
warn()    { printf '%s\n' "${YELLOW}[warn]${RESET} $*" >&2; }
error()   { printf '%s\n' "${RED}[error]${RESET} $*" >&2; }
header()  { printf '%s\n' "${BOLD}${CYAN}$*${RESET}"; }
dim()     { printf '%s\n' "${DIM}$*${RESET}"; }

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
    $(printf '%-28s' 'pr,  provision')Push .env into the VM's /etc/profile.d/
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
        TCTL_REMOTE_FORWARD Optional "PORT HOST:PORT" written as RemoteForward to ~/.ssh/config
        TCTL_SSH_CONFIG     Path to SSH client config file        (required for provision)
        TCTL_IDENTITY_FILE  Path to SSH private key               (required for provision)
        TCTL_HOST_SHARE_DIR Host directory to mount into the VM   (optional)

    A sibling ${BOLD}.env${RESET} file (plain KEY=VALUE; ${BOLD}.env.local${RESET} takes precedence)
    is pushed by the 'provision' command into the VM's /etc/profile.d/.
    Set ${BOLD}TCTL_ENV_FILE${RESET} to use another source — a path, a FIFO, or a
    process substitution such as <(op inject -i .env.tpl).

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
      printf '%s\n' "  State : ${GREEN}running${RESET}"
      echo    "  IP    : ${VM_IP:-unknown}"
      ;;
    stopped)
      printf '%s\n' "  State : ${YELLOW}stopped${RESET}"
      ;;
    unknown)
      printf '%s\n' "  State : ${RED}unknown${RESET} (VM not found in tart list)"
      ;;
    *)
      printf '%s\n' "  State : ${DIM}${state}${RESET}"
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
    # Update HostName and User in existing block (BSD & GNU awk compatible via temp file)
    local tmpfile
    tmpfile="$(mktemp)"
    awk -v host="${TCTL_VM_NAME}" -v ip="${ip}" -v user="${TCTL_SSH_USER}" '
      /^Host / { in_block = ($2 == host) }
      in_block && /^[[:space:]]+HostName / { sub(/HostName .*/, "HostName " ip) }
      in_block && /^[[:space:]]+User / { sub(/User .*/, "User " user) }
      { print }
    ' "${TCTL_SSH_CONFIG}" > "${tmpfile}"
    mv "${tmpfile}" "${TCTL_SSH_CONFIG}"
    chmod 600 "${TCTL_SSH_CONFIG}"
    info "Updated existing '${TCTL_VM_NAME}' entry → ${ip}"
  else
    # Append a new Host block
    local remote_forward=""
    if [[ -n "${TCTL_REMOTE_FORWARD:-}" ]]; then
      remote_forward="    RemoteForward ${TCTL_REMOTE_FORWARD}"
    fi
    cat >> "${TCTL_SSH_CONFIG}" <<EOF

Host ${TCTL_VM_NAME}
    HostName ${ip}
    User ${TCTL_SSH_USER}
    IdentityFile ${TCTL_IDENTITY_FILE}
    IdentitiesOnly yes
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
${remote_forward}
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

# Converts a KEY=VALUE env file (comments/blank lines allowed, optional quotes)
# into shell-safe `export KEY=VALUE` lines on stdout.
#
# The source may be a regular file, a symlink, a FIFO / process substitution
# (e.g. `op inject`), or a virtual file provided by a secrets manager such as
# 1Password Environments. Therefore: test readability only (not -f), read the
# content exactly once as a sequential stream via cat, never stat/seek it.
render_env_exports() {
  local file="$1" line key val
  echo '#!/usr/bin/env bash'
  echo "# Generated by ${SCRIPT_NAME} provision — do not edit manually"
  [[ -z "${file}" ]] && return 0
  while IFS= read -r line || [[ -n "${line}" ]]; do
    line="${line%$'\r'}"                       # tolerate CRLF
    [[ "${line}" =~ ^[[:space:]]*(#|$) ]] && continue
    line="${line#export }"                     # tolerate `export KEY=VALUE`
    key="${line%%=*}"; key="${key//[[:space:]]/}"
    val="${line#*=}"
    if [[ "${val}" =~ ^\".*\"$ || "${val}" =~ ^\'.*\'$ ]]; then
      val="${val:1:-1}"
    fi
    if [[ ! "${key}" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]]; then
      warn "Skipping invalid key: ${key}"
      continue
    fi
    printf 'export %s=%q\n' "${key}" "${val}"
  done < <(cat -- "${file}")
}

# True if the path can be opened for reading and is not a directory.
# Deliberately not -f: FIFOs, /dev/fd/N and secrets-manager mounts must pass.
env_readable() { [[ -n "$1" && -r "$1" && ! -d "$1" ]]; }

# Pushes .env.local (or .env) into the VM as /etc/profile.d/tart-provision.sh,
# which every login shell sources. The host share is auto-mounted via fstab.
# Override the source with TCTL_ENV_FILE, e.g.
#   TCTL_ENV_FILE=<(op inject -i .env.tpl) tart-ctl.sh provision
cmd_provision() {
  require_running
  local dest="/etc/profile.d/tart-provision.sh" env_file=""

  if   env_readable "${TCTL_ENV_FILE:-}";          then env_file="${TCTL_ENV_FILE}"
  elif env_readable "${CONFIG_DIR}/.env.local";    then env_file="${CONFIG_DIR}/.env.local"
  elif env_readable "${CONFIG_DIR}/.env";          then env_file="${CONFIG_DIR}/.env"
  else warn "No readable .env.local or .env in ${CONFIG_DIR} — pushing empty environment."
  fi

  info "Pushing ${env_file:-"(empty)"} → ${TCTL_SSH_USER}@${TCTL_VM_NAME}:${dest} …"
  # Render fully into a variable first so a one-shot source (pipe) is consumed
  # before SSH is involved, and nothing is pushed if rendering fails.
  local rendered
  rendered="$(render_env_exports "${env_file}")" || die "Failed to read ${env_file}"
  printf '%s\n' "${rendered}" \
    | ssh -T -F "${TCTL_SSH_CONFIG}" -i "${TCTL_IDENTITY_FILE}" \
        "${TCTL_SSH_USER}@${TCTL_VM_NAME}" \
        "sudo tee '${dest}' >/dev/null && sudo chmod 644 '${dest}'"

  info "Done. Open a new login shell in the VM to pick up the variables."
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

  printf '%s\n' "${DIM}Config : ${CONFIG_FILE}${RESET}"
  printf '%s\n' "${DIM}VM     : ${TCTL_VM_NAME}${RESET}"
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
