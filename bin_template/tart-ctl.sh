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
    $(printf '%-28s' 'env [--diff]')Show the effective env; --diff compares with the VM
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

    Sibling ${BOLD}.env${RESET} and ${BOLD}.env.local${RESET} files (plain KEY=VALUE) are layered —
    .env.local overrides individual keys of .env — and pushed by the
    'provision' command into the VM's /etc/profile.d/. Set ${BOLD}TCTL_ENV_FILE${RESET}
    to add a third layer on top: a path, a FIFO, or a process substitution
    such as <(op inject -i .env.tpl).

${BOLD}EXAMPLES${RESET}
    ${SCRIPT_NAME} up
    ${SCRIPT_NAME} provision
    ${SCRIPT_NAME} env --diff
    TCTL_ENV_FILE=<(op inject -i .env.tpl) ${SCRIPT_NAME} env --show-secrets
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
    # Update the managed settings in an existing block (BSD & GNU awk compatible via temp file)
    local tmpfile
    tmpfile="$(mktemp)"
    awk -v host="${TCTL_VM_NAME}" -v ip="${ip}" -v user="${TCTL_SSH_USER}" \
      -v remote="${TCTL_REMOTE_FORWARD:-}" '
      function finish_block() {
        if (in_block && !forward_done && remote != "")
          print "    RemoteForward " remote
      }
      /^Host / {
        finish_block()
        in_block = ($2 == host)
        forward_done = 0
      }
      in_block && /^[[:space:]]+RemoteForward[[:space:]]/ {
        if (remote != "" && !forward_done)
          print "    RemoteForward " remote
        forward_done = 1
        next
      }
      in_block && /^[[:space:]]+HostName / { sub(/HostName .*/, "HostName " ip) }
      in_block && /^[[:space:]]+User / { sub(/User .*/, "User " user) }
      { print }
      END { finish_block() }
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

# Converts one KEY=VALUE env file (comments/blank lines allowed, optional quotes)
# into shell-safe `export KEY=VALUE` lines on stdout.
#
# The source may be a regular file, a symlink, a FIFO / process substitution
# (e.g. `op inject`), or a virtual file provided by a secrets manager such as
# 1Password Environments. Therefore: test readability only (not -f), read the
# content exactly once as a sequential stream via cat, never stat/seek it.
render_env_exports() {
  local file="$1" line key val
  echo
  echo "# --- ${file} ---"
  while IFS= read -r line || [[ -n "${line}" ]]; do
    line="${line%$'\r'}"                       # tolerate CRLF
    [[ "${line}" =~ ^[[:space:]]*(#|$) ]] && continue
    line="${line#export }"                     # tolerate `export KEY=VALUE`
    key="${line%%=*}"; key="${key//[[:space:]]/}"
    val="${line#*=}"
    if [[ "${val}" =~ ^\".*\"$ || "${val}" =~ ^\'.*\'$ ]]; then
      val="${val#?}"
      val="${val%?}"
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

# Collects the readable env layers into ENV_LAYERS and renders them into
# ENV_RENDERED (the full content of the profile.d file). Rendering happens
# exactly once so one-shot sources (pipes) are fully consumed here.
#
# Layers (later ones override earlier keys, like dotenv-flow / Symfony / Vite):
#   1. .env           shared defaults (committed)
#   2. .env.local     personal overrides (gitignored)
#   3. TCTL_ENV_FILE  optional extra source, e.g. <(op inject -i .env.tpl)
readonly ENV_DEST="/etc/profile.d/tart-provision.sh"
ENV_LAYERS=()
ENV_RENDERED=""

load_env_layers() {
  local f
  for f in "${CONFIG_DIR}/.env" "${CONFIG_DIR}/.env.local" "${TCTL_ENV_FILE:-}"; do
    env_readable "${f}" && ENV_LAYERS+=("${f}")
  done
  [[ ${#ENV_LAYERS[@]} -eq 0 ]] && warn "No readable .env / .env.local in ${CONFIG_DIR}."

  ENV_RENDERED="$(
    echo '#!/usr/bin/env bash'
    echo "# Generated by ${SCRIPT_NAME} provision — do not edit manually"
    echo "# Later sections override earlier ones."
    for f in "${ENV_LAYERS[@]}"; do render_env_exports "${f}"; done
  )" || die "Failed to render env layers"
}

# Replaces values of secret-looking keys with ******** (stdin → stdout).
mask_secrets() {
  awk '
    /^[-+ ]?export [A-Za-z_][A-Za-z0-9_]*=/ {
      prefix = ""
      if (substr($0, 1, 1) == "+" || substr($0, 1, 1) == "-" || substr($0, 1, 1) == " ")
        prefix = substr($0, 1, 1)
      line = substr($0, length(prefix) + 1)
      key = toupper(substr(line, 8, index(line, "=") - 8))
      if (key ~ /(TOKEN|SECRET|PASS|KEY|CREDENTIAL|AUTH)/) sub(/=.*/, "=********", line)
      print prefix line
      next
    }
    { print }'
}

# Pushes the layered env into the VM as /etc/profile.d/tart-provision.sh,
# which every login shell sources. The host share is auto-mounted via fstab.
cmd_provision() {
  require_running
  load_env_layers

  info "Pushing ${#ENV_LAYERS[@]} env layer(s) → ${TCTL_SSH_USER}@${TCTL_VM_NAME}:${ENV_DEST}"
  local f; for f in "${ENV_LAYERS[@]}"; do dim "  + ${f}"; done

  printf '%s\n' "${ENV_RENDERED}" \
    | ssh -T -F "${TCTL_SSH_CONFIG}" -i "${TCTL_IDENTITY_FILE}" \
        "${TCTL_SSH_USER}@${TCTL_VM_NAME}" \
        "sudo tee '${ENV_DEST}' >/dev/null && sudo chmod 644 '${ENV_DEST}'"

  info "Done. Open a new login shell in the VM to pick up the variables."
}

# Shows the effective (merged) environment without touching the VM.
# Usage: tart-ctl.sh env [--diff] [--show-secrets]
#   --diff          compare against the VM's current profile.d file
#   --show-secrets  do not mask secret-looking values
cmd_env() {
  local do_diff=0 show_secrets=0 arg
  for arg in "$@"; do
    case "${arg}" in
      --diff)         do_diff=1 ;;
      --show-secrets) show_secrets=1 ;;
      *) die "Unknown option for env: '${arg}'" ;;
    esac
  done
  load_env_layers

  local filter="cat"
  (( show_secrets )) || filter="mask_secrets"

  info "Env layers (later override earlier):"
  local f; for f in "${ENV_LAYERS[@]}"; do dim "  + ${f}"; done
  echo

  # Effective values: last definition of each key wins; print in first-seen order.
  header "Effective environment"
  printf '%s\n' "${ENV_RENDERED}" | ${filter} | awk '
    /^# --- / { src = $0; sub(/^# --- /, "", src); sub(/ ---$/, "", src); next }
    /^export / {
      kv = substr($0, 8); eq = index(kv, "=")
      key = substr(kv, 1, eq - 1); val = substr(kv, eq + 1)
      if (!(key in value)) order[++n] = key
      value[key] = val; source[key] = src
    }
    END {
      for (i = 1; i <= n; i++) {
        k = order[i]
        printf "  %-24s = %-32s  # %s\n", k, value[k], source[k]
      }
      if (n == 0) print "  (empty)"
    }'
  echo
  (( show_secrets )) || dim "Secret-looking values are masked; use --show-secrets to reveal."

  if (( do_diff )); then
    echo
    require_running
    header "Diff: VM ${ENV_DEST} → local layers"
    local remote
    remote="$(ssh -T -F "${TCTL_SSH_CONFIG}" -i "${TCTL_IDENTITY_FILE}" \
      "${TCTL_SSH_USER}@${TCTL_VM_NAME}" "cat '${ENV_DEST}' 2>/dev/null || true")"
    local diff_output
    if diff_output="$(diff -u --label "vm:${ENV_DEST}" --label "local (rendered)" \
         <(printf '%s\n' "${remote}") \
         <(printf '%s\n' "${ENV_RENDERED}") 2>&1)"; then
      info "VM is up to date."
    else
      printf '%s\n' "${diff_output}" | ${filter}
      warn "VM differs — run '${SCRIPT_NAME} provision' to update."
      return 1
    fi
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
    env)            cmd_env "$@"   ;;
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
