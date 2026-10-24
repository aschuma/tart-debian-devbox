#!/usr/bin/env bash
# Parity suite runner — executes scenarios against an implementation of the VM
# controller with fake `tart` / `ssh` on PATH and an isolated HOME, then compares
# the captured output/side effects with the golden files.
#
#   tests/parity/run.sh --update                 # (re)record goldens from the bash reference
#   tests/parity/run.sh                          # check bash reference against goldens
#   tests/parity/run.sh --impl target/debug/tctl --format toml   # check the Rust binary
#   tests/parity/run.sh [opts] start_stop status # run selected scenarios only
#
# A scenario is tests/parity/scenarios/<name>.sh, sourced with these helpers:
#   cfg_global  TCTL_VAR=value …      write the global config (format-aware)
#   cfg_project TCTL_VAR=value …      write the project marker in $PROJECT (format-aware)
#   vm NAME running|stopped [IP]      define a VM in the fake tart world
#   run_tool [args…]                  run the implementation from $PROJECT (or $RUN_DIR)
#   run_tool_stdin 'text' [args…]     same, with stdin
# Everything the scenario leaves in $OUT is compared with the golden directory.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

IMPL="${here}/reference/tart-ctl.sh"
FORMAT=env
UPDATE=0
declare -a SELECT=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --impl)   IMPL="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; shift 2 ;;
    --format) FORMAT="$2"; shift 2 ;;
    --update) UPDATE=1; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) SELECT+=("$1"); shift ;;
  esac
done

[[ -x "${IMPL}" ]] || { echo "implementation not executable: ${IMPL}" >&2; exit 2; }

if [[ ${#SELECT[@]} -eq 0 ]]; then
  for f in "${here}"/scenarios/*.sh; do SELECT+=("$(basename "${f}" .sh)"); done
fi

# ─── helpers available to scenarios ──────────────────────────────────────────

# Maps a TCTL_* variable to its TOML section/key.
_toml_key() {
  case "$1" in
    TCTL_VM_NAME)             echo "vm name" ;;
    TCTL_HOST_SHARE_DIR)      echo "share dir" ;;
    TCTL_SSH_USER)            echo "ssh user" ;;
    TCTL_SSH_CONFIG)          echo "ssh config_path" ;;
    TCTL_IDENTITY_FILE)       echo "ssh identity_file" ;;
    TCTL_REMOTE_FORWARD)      echo "ssh remote_forward" ;;
    TCTL_LOCAL_FORWARD)       echo "ssh local_forward" ;;
    TCTL_ENV_PROVISION_FILES) echo "provision env_files" ;;
    *) echo "unknown variable $1" >&2; return 1 ;;
  esac
}

_is_list_var() {
  case "$1" in TCTL_REMOTE_FORWARD|TCTL_LOCAL_FORWARD|TCTL_ENV_PROVISION_FILES) return 0 ;; *) return 1 ;; esac
}

# Writes KEY=VALUE pairs to $1 in the selected format.
_write_cfg() {
  local target="$1"; shift
  mkdir -p "$(dirname "${target}")"
  if [[ "${FORMAT}" == env ]]; then
    : > "${target}"
    local kv; for kv in "$@"; do printf '%s="%s"\n' "${kv%%=*}" "${kv#*=}" >> "${target}"; done
  else
    # Group by section, preserving order of first appearance.
    local -a sections=(); local -A body=()
    local kv var val sec key
    for kv in "$@"; do
      var="${kv%%=*}"; val="${kv#*=}"
      read -r sec key < <(_toml_key "${var}")
      [[ -n "${body[${sec}]+x}" ]] || sections+=("${sec}")
      if _is_list_var "${var}"; then
        local items="" item; local IFS=','
        for item in ${val}; do
          item="${item#"${item%%[![:space:]]*}"}"; item="${item%"${item##*[![:space:]]}"}"
          [[ -n "${item}" ]] && items+="${items:+, }\"${item}\""
        done
        unset IFS
        body[${sec}]+="${key} = [${items}]"$'\n'
      else
        body[${sec}]+="${key} = \"${val}\""$'\n'
      fi
    done
    : > "${target}"
    for sec in "${sections[@]}"; do printf '[%s]\n%s\n' "${sec}" "${body[${sec}]}" >> "${target}"; done
  fi
}

cfg_global() {
  local f="${HOME}/.config/tctl/config"; [[ "${FORMAT}" == toml ]] && f+=".toml"
  _write_cfg "${f}" "$@"
}

cfg_project() {
  local f="${PROJECT}/.tart-ctl-env"; [[ "${FORMAT}" == toml ]] && f="${PROJECT}/.tart-ctl.toml"
  _write_cfg "${f}" "$@"
}

# The standard global config most scenarios start from (mirrors config_template/config).
std_global() {
  cfg_global \
    TCTL_REMOTE_FORWARD="1080 127.0.0.1:1080" \
    TCTL_LOCAL_FORWARD="" \
    TCTL_HOST_SHARE_DIR="auto" \
    TCTL_ENV_PROVISION_FILES=".env" \
    TCTL_SSH_USER="devbox" \
    TCTL_SSH_CONFIG="${HOME}/.ssh/config" \
    TCTL_IDENTITY_FILE="${HOME}/.ssh/id_ed25519_tart" \
    "$@"
}

vm() {
  mkdir -p "${WORLD}/vms/$1"
  echo "$2" > "${WORLD}/vms/$1/state"
  echo "${3:-192.168.64.10}" > "${WORLD}/vms/$1/ip"
}

_step=0
_run() {
  local stdin_text="$1"; shift
  _step=$((_step + 1))
  local label; label="$(printf '%02d-%s' "${_step}" "${1:-noargs}")"
  local rc=0
  local started_before; started_before="$(find "${WORLD}/run-ready" -type f 2>/dev/null | wc -l | tr -d ' ' || true)"
  (
    cd "${RUN_DIR:-${PROJECT}}"
    if [[ -n "${stdin_text}" ]]; then
      printf '%s' "${stdin_text}" | "${IMPL}" "$@" > "${OUT}/${label}.stdout" 2> "${OUT}/${label}.stderr"
    else
      "${IMPL}" "$@" > "${OUT}/${label}.stdout" 2> "${OUT}/${label}.stderr" < /dev/null
    fi
  ) || rc=$?
  echo "${rc}" > "${OUT}/${label}.exit"
  # Bash detaches before the fake even logs its argv. Wait for the fake's
  # readiness marker, not `wait` (the child belongs to an exited subshell).
  if grep -q "^==> Starting VM '" "${OUT}/${label}.stdout"; then
    local ready=0 i count
    for ((i=0; i<500; i++)); do
      count="$(find "${WORLD}/run-ready" -type f 2>/dev/null | wc -l | tr -d ' ' || true)"
      if (( count > started_before )); then ready=1; break; fi
      sleep 0.01
    done
    (( ready )) || { echo "fake tart run did not become ready" >&2; return 1; }
  fi
}
run_tool()       { _run "" "$@"; }
# Snapshot ~/.ssh/config after the current step (numbered like the step).
snap_ssh_config() { cp "${HOME}/.ssh/config" "${OUT}/$(printf '%02d' "${_step}")-ssh_config" 2>/dev/null || true; }
run_tool_stdin() { local s="$1"; shift; _run "${s}" "$@"; }

# ─── normalisation ───────────────────────────────────────────────────────────

_normalise() {
  python3 "${here}/normalise.py" "$1" "$(basename "${IMPL}")"
}

# ─── main loop ───────────────────────────────────────────────────────────────

fail=0
for name in "${SELECT[@]}"; do
  scenario="${here}/scenarios/${name}.sh"
  [[ -f "${scenario}" ]] || { echo "no such scenario: ${name}" >&2; fail=1; continue; }

  SANDBOX="$(mktemp -d "${TMPDIR:-/tmp}/tctl-parity.XXXXXX")"
  SANDBOX="$(cd "${SANDBOX}" && pwd -P)"
  export HOME="${SANDBOX}/home"
  export PROJECT="${SANDBOX}/project"
  export WORLD="${SANDBOX}/world"
  export OUT="${SANDBOX}/out"
  export TCTL_FAKE_WORLD="${WORLD}"
  export PATH="${here}/fakes:${PATH}"
  export NO_COLOR=1
  unset XDG_CONFIG_HOME XDG_DATA_HOME RUN_DIR
  mkdir -p "${HOME}" "${PROJECT}" "${WORLD}" "${OUT}"
  : > "${WORLD}/calls.log"
  _step=0

  # shellcheck source=/dev/null
  ( source "${scenario}" ) || { echo "scenario ${name} crashed" >&2; fail=1; }

  # Collect side effects.
  cp "${WORLD}/calls.log" "${OUT}/calls.log"
  [[ -f "${HOME}/.ssh/config" ]] && cp "${HOME}/.ssh/config" "${OUT}/ssh_config"
  [[ -d "${WORLD}/vm-files" ]] && cp -R "${WORLD}/vm-files" "${OUT}/vm-files"
  [[ -d "${WORLD}/vms" ]] && cp -R "${WORLD}/vms" "${OUT}/vms"
  _normalise "${OUT}"

  golden="${here}/golden/${name}"
  if (( UPDATE )); then
    rm -rf "${golden}"; mkdir -p "${golden}"; cp -R "${OUT}/." "${golden}/"
    echo "recorded  ${name}"
  else
    # Files listed in loose.txt (scenario/file) carry implementation-specific wording
    # (config key names, argument-parser errors) and are excluded from the comparison.
    expected="${SANDBOX}/expected"; cp -R "${golden}" "${expected}"
    _normalise "${expected}"
    while read -r loose; do
      [[ -z "${loose}" || "${loose}" == \#* || "${loose%%/*}" != "${name}" ]] && continue
      rm -f "${expected}/${loose#*/}" "${OUT}/${loose#*/}"
    done < "${here}/loose.txt"
    if diff -ru "${expected}" "${OUT}" > "${SANDBOX}/diff" 2>&1; then
      echo "ok        ${name}"
    else
      echo "FAIL      ${name}"; cat "${SANDBOX}/diff"; fail=1
    fi
  fi
  rm -rf "${SANDBOX}"
done

exit "${fail}"
