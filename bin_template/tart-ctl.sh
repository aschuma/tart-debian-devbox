#!/usr/bin/env bash
# tart-ctl.sh — Global Tart VM management script
# Usage: tart-ctl.sh [command]
#
# Loads the global config (~/.config/tctl/config), then recursively searches
# upward from PWD for a .tart-ctl-env project marker, sources it on top, and
# dispatches the given command.

set -euo pipefail

# ─── Constants ────────────────────────────────────────────────────────────────

readonly SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
readonly ENV_FILE_NAME=".tart-ctl-env"
readonly GLOBAL_CONFIG_FILE="${XDG_CONFIG_HOME:-${HOME}/.config}/tctl/config"

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
error()   { printf '%b\n' "${RED}[error]${RESET} $*" >&2; }
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

# Ensures the SSH-related settings are present (needed by ssh / provision / env --diff / update-ssh-config).
require_ssh_settings() {
  local var missing=()
  for var in TCTL_SSH_USER TCTL_SSH_CONFIG TCTL_IDENTITY_FILE; do
    [[ -n "${!var:-}" ]] || missing+=("${var}")
  done
  if [[ ${#missing[@]} -gt 0 ]]; then
    die "Missing settings: ${missing[*]}.\n       Set them in '${GLOBAL_CONFIG_FILE}' (or override in '${CONFIG_FILE}')."
  fi
}

# ─── Help ─────────────────────────────────────────────────────────────────────

usage() {
  cat <<EOF

$(header "tart-ctl.sh — Tart VM Controller")

${BOLD}USAGE${RESET}
    ${SCRIPT_NAME} <command>

${BOLD}COMMANDS${RESET}
    $(printf '%-28s' 'up, start')Start the configured VM in the background.
    $(printf '%-28s' 'down, stop')Stop the configured VM.
    $(printf '%-28s' 'st, status')Show VM state and current IP when running.
    $(printf '%-28s' 'pr, provision')Push merged env files into ${ENV_DEST} inside the VM.
    $(printf '%-28s' 'env [--diff]')Show the effective merged environment and optionally diff against the VM.
    $(printf '%-28s' 'sh, ssh')SSH into the VM as the configured user.
    $(printf '%-28s' 'ip')Print the VM IP if running.
    $(printf '%-28s' 'usc, update-ssh-config')Write or refresh the VM Host block in ${TCTL_SSH_CONFIG:-~/.ssh/config}.
    $(printf '%-28s' 'init')Interactively create ${ENV_FILE_NAME} in the current directory.
    $(printf '%-28s' 'help')Show this command reference.
    $(printf '%-28s' 'help-config')Show config discovery rules and TCTL_* variables.

EOF
}

usage_config() {
  cat <<EOF

$(header "tart-ctl.sh — config discovery and variables")

${BOLD}DISCOVERY${RESET}
    Config is layered (later overrides earlier):
      1) ${GLOBAL_CONFIG_FILE}  — global defaults (SSH user/key, share mode, env file list)
      2) ${ENV_FILE_NAME}       — project marker found by walking upward from PWD to '/'
    The directory containing ${ENV_FILE_NAME} becomes the project root for relative paths.

${BOLD}REQUIRED${RESET}
    TCTL_VM_NAME        VM name to manage (must be set in ${ENV_FILE_NAME}).
    TCTL_SSH_USER       SSH username inside the VM.
    TCTL_SSH_CONFIG     SSH config file path used by this script.
    TCTL_IDENTITY_FILE  SSH private key path used for VM access.

${BOLD}OPTIONAL${RESET}
    TCTL_HOST_SHARE_DIR       Host path to mount into VM, "auto" for project root, or empty for none.
    TCTL_REMOTE_FORWARD       Comma-separated SSH RemoteForward entries ("PORT HOST:PORT").
    TCTL_LOCAL_FORWARD        Comma-separated SSH LocalForward entries ("PORT HOST:PORT").
    TCTL_ENV_PROVISION_FILES  Comma-separated env file list (default: "${ENV_PROVISION_FILES_DEFAULT}").

${BOLD}ENV LAYERS${RESET}
    Env files are read in order; later keys override earlier keys.
    Relative entries in TCTL_ENV_PROVISION_FILES resolve from the project root.

EOF
}

# ─── Config Discovery ─────────────────────────────────────────────────────────

# Walks upward from PWD to the filesystem root looking for .tart-ctl-env.
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

    # Stop at filesystem root
    if [[ "${dir}" == "/" ]]; then
      break
    fi

    dir="$(dirname "${dir}")"
  done

  die "No '${ENV_FILE_NAME}' found (searched from '$(pwd)' upward to '/').\n       Create one in the root of the project you want to manage:\n         cd <project-root> && ${SCRIPT_NAME} init"
}

# Loads the configuration layers: global config (optional) → project marker (required).
load_config() {
  load_global_config

  find_config

  # shellcheck source=/dev/null
  source "${CONFIG_FILE}"

  if [[ -z "${TCTL_VM_NAME:-}" ]]; then
    die "'TCTL_VM_NAME' is not set in '${CONFIG_FILE}'.\n       Each project names its own VM there, e.g.  TCTL_VM_NAME=\"my-project\"\n       Or regenerate the file interactively:   cd '${CONFIG_DIR}' && ${SCRIPT_NAME} init"
  fi

  resolve_share_dir
}

# Sources the global config if present (warns otherwise).
load_global_config() {
  if [[ -r "${GLOBAL_CONFIG_FILE}" ]]; then
    # shellcheck source=/dev/null
    source "${GLOBAL_CONFIG_FILE}"
  else
    warn "No global config at '${GLOBAL_CONFIG_FILE}' — run 'just install' or copy config_template/config there."
  fi
}

# Resolves the effective host share directory into SHARE_DIR (empty = no share).
# The project .tart-ctl-env overrides the global config because it is sourced
# last; the special value "auto" means "the directory containing .tart-ctl-env".
resolve_share_dir() {
  case "${TCTL_HOST_SHARE_DIR:-}" in
    "")   SHARE_DIR="" ;;
    auto) SHARE_DIR="${CONFIG_DIR}" ;;
    *)    SHARE_DIR="${TCTL_HOST_SHARE_DIR}" ;;
  esac
  if [[ -n "${SHARE_DIR}" && ! -d "${SHARE_DIR}" ]]; then
    die "TCTL_HOST_SHARE_DIR '${SHARE_DIR}' is not a directory."
  fi
}

# ─── Commands ─────────────────────────────────────────────────────────────────

# Interactively creates .tart-ctl-env in the current directory.
cmd_init() {
  local target="${PWD}/${ENV_FILE_NAME}"
  local vm_name="" share_mode="" share_dir="" answer=""

  header "Initialise ${ENV_FILE_NAME} in ${PWD}"
  echo

  if [[ -e "${target}" ]]; then
    read -r -p "$(printf '%s' "${YELLOW}[warn]${RESET} '${target}' already exists. Overwrite? [y/N] ")" answer
    [[ "${answer}" =~ ^[Yy]$ ]] || die "Aborted — existing file left untouched."
  fi

  load_global_config

  # ── VM name ──────────────────────────────────────────────────────────────
  local -a vms=()
  if command -v tart >/dev/null 2>&1; then
    local line
    while IFS= read -r line; do vms+=("${line}"); done \
      < <(tart list 2>/dev/null | awk 'NR>1 && $1=="local" {print $2}')
    if (( ${#vms[@]} > 0 )); then
      dim "Local Tart VMs: ${vms[*]}"
    fi
  else
    warn "'tart' not found on PATH — cannot check whether the VM exists."
  fi

  while [[ -z "${vm_name}" ]]; do
    read -r -p "VM name to manage from this project: " vm_name
    vm_name="${vm_name//[[:space:]]/}"
    if [[ -n "${vm_name}" && ! "${vm_name}" =~ ^[A-Za-z0-9._-]+$ ]]; then
      error "Use only letters, digits, '.', '_' and '-'."
      vm_name=""
    fi
  done

  if (( ${#vms[@]} > 0 )) && ! printf '%s\n' "${vms[@]}" | grep -qx -- "${vm_name}"; then
    warn "No local Tart VM named '${vm_name}' exists yet."
    dim "  Create it by cloning the template:  just clone ${vm_name}   (= tart clone debian-ssh ${vm_name})"
  fi

  # ── Share directory ──────────────────────────────────────────────────────
  echo
  echo "Host directory to mount in the VM at /mnt/shared/workspace:"
  echo "  1) auto  — this directory (${PWD})"
  echo "  2) path  — a specific directory"
  echo "  3) none  — start the VM without a share"
  local global_share="${TCTL_HOST_SHARE_DIR-}"
  local default_choice="1"
  case "${global_share}" in
    auto) default_choice="1" ;;
    "")   default_choice="3" ;;
    *)    default_choice="2" ;;
  esac
  dim "  (global config currently: ${global_share:-none}; Enter keeps that → ${default_choice})"

  while [[ -z "${share_mode}" ]]; do
    read -r -p "Choice [${default_choice}]: " answer
    answer="${answer:-${default_choice}}"
    case "${answer}" in
      1|auto) share_mode="auto" ;;
      2|path)
        read -r -e -p "Directory to share: " share_dir
        share_dir="${share_dir/#\~/${HOME}}"
        if [[ -z "${share_dir}" || ! -d "${share_dir}" ]]; then
          error "'${share_dir}' is not a directory."
          continue
        fi
        share_dir="$(cd "${share_dir}" && pwd)"
        share_mode="path"
        ;;
      3|none) share_mode="none" ;;
      *) error "Please answer 1, 2 or 3." ;;
    esac
  done

  # ── Write file ───────────────────────────────────────────────────────────
  {
    printf '# tart-ctl project marker — generated by %s init on %s\n' "${SCRIPT_NAME}" "$(date +%Y-%m-%d)"
    printf '# Global defaults: %s (this file overrides them for this project).\n\n' "${GLOBAL_CONFIG_FILE}"
    printf '# Tart VM managed from this project (clone of the template: just clone %s)\n' "${vm_name}"
    printf 'TCTL_VM_NAME="%s"\n\n' "${vm_name}"
    printf '# Host directory mounted in the VM at /mnt/shared/workspace\n'
    case "${share_mode}" in
      auto) printf '#   "auto" = the directory containing this file\nTCTL_HOST_SHARE_DIR="auto"\n' ;;
      path) printf 'TCTL_HOST_SHARE_DIR=%q\n' "${share_dir}" ;;
      none) printf '#   "" = start the VM without a share\nTCTL_HOST_SHARE_DIR=""\n' ;;
    esac
    printf '\n# Env files pushed by `%s provision` (comma-separated, relative to this directory)\n' "${SCRIPT_NAME}"
    printf '# TCTL_ENV_PROVISION_FILES=".env,.env.local"\n'
  } > "${target}"

  echo
  info "Wrote ${target}"
  dim "Next: ${SCRIPT_NAME} up  →  ${SCRIPT_NAME} update-ssh-config  →  ${SCRIPT_NAME} ssh"
}

cmd_start() {
  if vm_ip_or_false; then
    warn "VM '${TCTL_VM_NAME}' is already running (${VM_IP})."
    return 0
  fi

  info "Starting VM '${TCTL_VM_NAME}' …"
  if [[ -n "${SHARE_DIR}" ]]; then
    dim "  share: ${SHARE_DIR} → /mnt/shared/workspace"
    tart run --no-graphics --dir=workspace:"${SHARE_DIR}" "${TCTL_VM_NAME}" &
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
  echo    "  Global : ${GLOBAL_CONFIG_FILE}"
  echo    "  Project: ${CONFIG_FILE}"
  echo    "  Share  : ${SHARE_DIR:-(none)}"
}

cmd_ip() {
  require_running
  echo "${VM_IP}"
}

# Prints the managed RemoteForward / LocalForward lines for the ~/.ssh/config
# Host block, one per comma-separated entry of TCTL_REMOTE_FORWARD and
# TCTL_LOCAL_FORWARD (each entry is an ssh "PORT HOST:PORT" spec).
render_forward_lines() {
  local keyword list entry
  for keyword in RemoteForward LocalForward; do
    case "${keyword}" in
      RemoteForward) list="${TCTL_REMOTE_FORWARD:-}" ;;
      LocalForward)  list="${TCTL_LOCAL_FORWARD:-}" ;;
    esac
    local IFS=','
    for entry in ${list}; do
      entry="${entry#"${entry%%[![:space:]]*}"}"
      entry="${entry%"${entry##*[![:space:]]}"}"
      if [[ -n "${entry}" ]]; then
        printf '    %s %s\n' "${keyword}" "${entry}"
      fi
    done
  done
}

cmd_update_ssh_config() {
  require_ssh_settings
  require_running
  local ip="${VM_IP}"
  info "Resolved '${TCTL_VM_NAME}' → ${ip}"

  # Ensure SSH config file exists
  mkdir -p "$(dirname "${TCTL_SSH_CONFIG}")"
  touch "${TCTL_SSH_CONFIG}"
  chmod 600 "${TCTL_SSH_CONFIG}"

  local forward_lines
  forward_lines="$(render_forward_lines)"

  if grep -q "^Host ${TCTL_VM_NAME}$" "${TCTL_SSH_CONFIG}"; then
    # Update the managed settings in an existing block (BSD & GNU awk compatible via temp file).
    # All existing RemoteForward/LocalForward lines of the block are replaced by the
    # configured set, emitted where the first one was (or at the end of the block).
    local tmpfile
    tmpfile="$(mktemp)"
    # awk -v does not accept literal newlines (BSD awk), but decodes the escape \n.
    awk -v host="${TCTL_VM_NAME}" -v ip="${ip}" -v user="${TCTL_SSH_USER}" \
      -v forwards="${forward_lines//$'\n'/\\n}" '
      function emit_forwards() {
        if (forwards != "") print forwards
        forward_done = 1
      }
      function finish_block() {
        if (in_block && !forward_done) emit_forwards()
      }
      /^Host / {
        finish_block()
        in_block = ($2 == host)
        forward_done = 0
      }
      in_block && /^[[:space:]]+(RemoteForward|LocalForward)[[:space:]]/ {
        if (!forward_done) emit_forwards()
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
    {
      cat <<EOF

Host ${TCTL_VM_NAME}
    HostName ${ip}
    User ${TCTL_SSH_USER}
    IdentityFile ${TCTL_IDENTITY_FILE}
    IdentitiesOnly yes
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
EOF
      if [[ -n "${forward_lines}" ]]; then
        printf '%s\n' "${forward_lines}"
      fi
    } >> "${TCTL_SSH_CONFIG}"
    info "Added new '${TCTL_VM_NAME}' entry → ${ip}"
  fi
}

# Ensures ~/.ssh/config has a current Host entry for this VM before connecting.
# This avoids stale HostName after VM restarts (IP changes between boots).
ensure_ssh_config_current() {
  local needs_refresh=0 current_hostname="" current_user=""

  if [[ ! -r "${TCTL_SSH_CONFIG}" ]] || ! grep -q "^Host ${TCTL_VM_NAME}$" "${TCTL_SSH_CONFIG}"; then
    needs_refresh=1
  else
    current_hostname="$(awk -v host="${TCTL_VM_NAME}" '
      /^Host[[:space:]]+/ { in_block = ($2 == host) }
      in_block && /^[[:space:]]+HostName[[:space:]]+/ { print $2; exit }
    ' "${TCTL_SSH_CONFIG}")"
    current_user="$(awk -v host="${TCTL_VM_NAME}" '
      /^Host[[:space:]]+/ { in_block = ($2 == host) }
      in_block && /^[[:space:]]+User[[:space:]]+/ { print $2; exit }
    ' "${TCTL_SSH_CONFIG}")"

    [[ "${current_hostname}" == "${VM_IP}" ]] || needs_refresh=1
    [[ "${current_user}" == "${TCTL_SSH_USER}" ]] || needs_refresh=1
  fi

  if (( needs_refresh )); then
    info "Refreshing SSH config entry for '${TCTL_VM_NAME}' …"
    cmd_update_ssh_config
  fi
}

cmd_ssh() {
  # Any arguments after the command are passed through to ssh (e.g. a remote command).
  # Usage: tart-ctl.sh ssh [-- remote_cmd [args…]]
  local -a extra_args=("$@")
  [[ ${#extra_args[@]} -gt 0 && "${extra_args[0]}" == "--" ]] && extra_args=("${extra_args[@]:1}")

  require_ssh_settings
  require_running
  ensure_ssh_config_current
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
    line="${line#"${line%%[![:space:]]*}"}"
    line="${line%"${line##*[![:space:]]}"}"
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
#   1..n  TCTL_ENV_PROVISION_FILES  comma-separated list (default ".env"); relative
#         entries resolve against the directory holding .tart-ctl-env, absolute
#         and ~/ entries are used as-is. Unreadable entries are skipped.
readonly ENV_DEST="/etc/profile.d/tart-provision.sh"
readonly ENV_PROVISION_FILES_DEFAULT=".env"
ENV_LAYERS=()
ENV_RENDERED=""

# Expands one TCTL_ENV_PROVISION_FILES entry to an absolute path (stdout).
resolve_env_path() {
  local entry="$1"
  entry="${entry#"${entry%%[![:space:]]*}"}"   # trim leading whitespace
  entry="${entry%"${entry##*[![:space:]]}"}"   # trim trailing whitespace
  # shellcheck disable=SC2088  # literal "~" case pattern is intended
  case "${entry}" in
    "")   return 1 ;;
    /*)   printf '%s' "${entry}" ;;
    "~"|"~/"*) printf '%s%s' "${HOME}" "${entry#\~}" ;;
    *)    printf '%s/%s' "${CONFIG_DIR}" "${entry}" ;;
  esac
}

load_env_layers() {
  local entry f
  local list="${TCTL_ENV_PROVISION_FILES-${ENV_PROVISION_FILES_DEFAULT}}"
  local -a skipped=()

  IFS=',' read -r -a entries <<< "${list}"
  for entry in "${entries[@]}"; do
    f="$(resolve_env_path "${entry}")" || continue
    if env_readable "${f}"; then ENV_LAYERS+=("${f}"); else skipped+=("${f}"); fi
  done

  for f in "${skipped[@]}"; do dim "  - ${f} (not readable, skipped)"; done
  [[ ${#ENV_LAYERS[@]} -eq 0 ]] && warn "No readable env files (TCTL_ENV_PROVISION_FILES='${list}', resolved against ${CONFIG_DIR})."

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
  require_ssh_settings
  require_running
  ensure_ssh_config_current
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
    require_ssh_settings
    require_running
    ensure_ssh_config_current
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
    -h|--help)
      error "'${1}' is not supported; use '${SCRIPT_NAME} help' instead."
      usage
      exit 1
      ;;
    help)
      usage
      exit 0
      ;;
    help-config|help-vars|help-discovery)
      usage_config
      exit 0
      ;;
    "")
      usage
      exit 0
      ;;
    init)
      cmd_init
      exit 0
      ;;
  esac

  # Discover and load config layers
  load_config

  printf '%s\n' "${DIM}Global : ${GLOBAL_CONFIG_FILE}${RESET}"
  printf '%s\n' "${DIM}Project: ${CONFIG_FILE}${RESET}"
  printf '%s\n' "${DIM}VM     : ${TCTL_VM_NAME}${RESET}"
  printf '%s\n' "${DIM}Share  : ${SHARE_DIR:-(none)}${RESET}"
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
      echo  "Run '${SCRIPT_NAME} help' for commands or '${SCRIPT_NAME} help-config' for config details."
      exit 1
      ;;
  esac
}

main "$@"
