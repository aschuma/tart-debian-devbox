# Design: `tctl` — Rust replacement for `tart-ctl.sh`

Status: **phases 1 and 2 implemented; real-VM acceptance pending**. This document
records the agreed design and the implementation checkpoint below.

## 1. Scope and goals

**In scope:** `bin_template/tart-ctl.sh` (per-project VM lifecycle).
**Out of scope:** `build/with-vm.sh`, `build/provision.sh`, the `justfile` — they
remain thin wrappers around Packer/Ansible/Tart.

Goals, in order:

1. Maintainability and correctness (750 lines of bash with config layering, env
   merging and `~/.ssh/config` rewriting is hard to test).
2. Distribution: a single binary, no `~/.local/bin`-on-PATH dance.
3. Features bash makes painful: robust arg parsing, `--json`, structured errors.
4. A foundation for controlling the VMs over HTTP (`tctl server`).

## 2. Architecture

- One crate **`tctl`** with `src/lib.rs` (core), `src/main.rs` (CLI) and
  `src/server.rs` + `src/server/shell.rs` (HTTP/WebSocket front-end).
- The **core is synchronous**: `tart`/`ssh` are blocking subprocesses anyway.
  `tokio`/`axum` are used only by the `server` subcommand (compiled in, no
  feature flag); the server wraps core calls in `spawn_blocking`.
- `trait TartBackend` wraps every `tart` / `ssh` invocation
  (`std::process::Command`). A fake implementation drives the tests.
  Tart ≥ 2.32 provides `tart list --format json`; no column parsing.
- Every command returns a `serde::Serialize` **result struct**. Human output
  (`Display`/table) and `--json` are rendered from the same value; `NO_COLOR`
  and `--no-color` are honoured.
- Operations take complete inputs; the CLI alone owns prompting, and only for
  flags that are missing *and* stdin is a TTY — otherwise a missing flag is an
  error. This keeps every command scriptable; `init` stays CLI-only and is
  deliberately absent from the server.

## 3. CLI surface

Binary name **`tctl`** (matches `~/.config/tctl/`). `just install` also creates a
transitional `tart-ctl.sh → tctl` symlink. The production Bash script is removed;
the compatibility symlink remains during the transition.

| Command | Aliases | Notes |
| --- | --- | --- |
| `tctl init [--vm NAME] [--share auto\|PATH\|none]` | | writes `.tart-ctl.toml`, registers VM |
| `tctl start` | `up` | detached `tart run`, waits for IP |
| `tctl stop` | `down` | |
| `tctl status` | `st` | |
| `tctl provision` | `pr` | |
| `tctl env [--diff] [--show-secrets]` | | |
| `tctl ssh [-- CMD…]` | `sh` | `exec()`s ssh |
| `tctl ip` | | |
| `tctl update-ssh-config` | `usc` | |
| `tctl registry list\|show NAME\|scan DIR` | | per-user inventory |
| `tctl server [--port 7878] [--allow-origin ORIGIN]…` | | local HTTP/WebSocket API (phase 2 section below) |

Global flags: `--json`, `--no-color`, `-v/-q` (tracing level).

Bare `tctl` prints help. Nothing ever starts the server implicitly.

## 4. Configuration (TOML)

Two files, project overrides global **per key**; the project file is found by
walking up from the cwd (same as today):

```toml
# ~/.config/tctl/config.toml — global defaults, no VM name
[ssh]
user          = "devbox"
identity_file = "~/.ssh/id_ed25519_tart"
config_path   = "~/.ssh/config"
remote_forward = ["1080 127.0.0.1:1080"]
local_forward  = []

[share]
dir = "auto"              # "auto" | "/path" | "none"

[provision]
env_files = [".env"]      # relative → project root; "~/" and absolute as-is
```

```toml
# <project>/.tart-ctl.toml — committed, team-shareable
[vm]
name = "my-project"       # required

[share]
dir = "auto"

[provision]
env_files = [".env", ".env.local"]

[ssh]
local_forward = ["5432 127.0.0.1:5432"]
```

- `~` and `$HOME` are expanded in path values; nothing else is interpolated.
- Old shell-format files (`.tart-ctl-env`, `~/.config/tctl/config`) found
  without a TOML counterpart → hard error pointing at the migration script.
- **Migration is a throwaway bash script** (`scripts/migrate-tctl-env.sh`),
  referenced from README/QUICKSTART, run once per machine. It converts both
  file types and never deletes the originals.

## 5. Registry (SQLite)

`~/.local/share/tctl/tctl.db`, `rusqlite` with the `bundled` feature.

Per-user, never committed, **derived** from `.tart-ctl.toml`; refreshed on
`init` and every `start`; rebuildable with `tctl registry scan <dir>`.

Table `vms` (one row per VM):

| column | source |
| --- | --- |
| `name` (PK) | `[vm].name` |
| `template` | reserved; `tart` exposes no clone lineage, so currently always null |
| `project_dir` | directory holding `.tart-ctl.toml` |
| `share_dir`, `remote_forward`, `local_forward`, `env_files` | effective config (JSON) |
| `last_ip`, `pid`, `log_path` | set by `start` |
| `last_provision_hash`, `last_provision_at` | set by `provision` (SHA-256 of rendered file) |
| `updated_at` | |

The registry stores **no env values**, only the hash.

## 6. Behaviour details

- **Drop-in on the VM side**: identical `/etc/profile.d/tart-provision.sh`
  content and identical managed block in `~/.ssh/config`.
- **`start`**: spawn `tart run --no-graphics [--dir=workspace:…] NAME` in a new
  session (`setsid`), stdio → `~/.local/share/tctl/logs/<vm>.log`, record PID
  **before waiting**, then poll `tart ip` until it answers or a timeout (default
  60 s) expires. Running state is independent of IP availability: do not start
  duplicates, and allow `stop` even when DHCP/IP lookup fails.
- **`ssh`**: `CommandExt::exec()` for interactive and `-- CMD` forms (terminal,
  signals and exit code pass straight through). Programmatic remote execution
  (`provision`, `env --diff`, IP wait, server) uses `run_remote(cmd) -> Output`
  which captures stdout/stderr/exit code. `--json ssh` still transfers control
  to SSH: remote/session output and exit code are not a JSON envelope.
- **Secret masking**: unchanged heuristic (`*TOKEN*`, `*SECRET*`, `*PASS*`,
  `*KEY*`, … case-insensitive) applied to human **and** JSON output;
  `--show-secrets` is the only reveal path and the server inherits the default.

## 7. Testing

- Rust unit tests: TOML parsing/layering, forward rendering, env layering,
  masking, ssh-config block rewriting, registry.
- **Parity suite** (gates deletion of the bash script): fake `tart` and `ssh`
  shims on `PATH` log their arguments and return canned output; the harness runs
  `tart-ctl.sh` and `tctl` for each command and diffs the issued commands and the
  produced files (`~/.ssh/config`, provision payload).
- `just smoke`: manual run against a real Tart VM before a release (not CI).

## 8. Delivery

- Cargo project lives in this repo (`tctl/`).
- `just install` → `cargo install --path tctl` → `~/.cargo/bin/tctl` + symlink;
  GitHub Actions builds release binaries for users without a Rust toolchain.
- Crates: `clap` (derive), `anyhow` (bin) + `thiserror` (lib), `serde`, `toml`,
  `serde_json`, `rusqlite` (bundled), `sha2`/`hex`, `similar`, `libc`, `tracing`;
  server: `tokio`, `axum` (ws), `portable-pty`. Stable Rust, edition 2021. XDG
  paths and simple human formatting are implemented directly; unused
  formatting/path crates were removed.

## 9. Phasing

**Phase 1 — CLI parity**
`tctl` with all commands above, TOML config, SQLite registry, detached `start`
with log + IP wait, `--json`, parity suite green, README/QUICKSTART updated,
`bin_template/tart-ctl.sh` deleted, `tctl server` stub.

**Phase 2 — `tctl server`** (implemented; settled in the second design session)

Foreground axum server reusing the synchronous core via `spawn_blocking`.

- **Exposure:** binds `127.0.0.1` only; no option to bind elsewhere. Remote use
  goes through an SSH tunnel.
- **Authentication: none** (owner's explicit decision). Mitigations: loopback
  bind, exact `Origin` allowlist (`--allow-origin`, repeatable, no wildcards),
  and a local-`Host` check against DNS rebinding. Allowlisted origins receive
  `Access-Control-Allow-Origin` and a preflight answer so a browser UI can
  actually read responses; everything else gets no CORS headers.
- **Surface:** `GET /vms`, `GET /vms/{name}`, `GET /vms/{name}/env` (masked,
  no reveal path), `POST /vms/{name}/start|stop|provision`, `GET /jobs/{id}`,
  `GET /vms/{name}/shell` (WebSocket). Only registered VMs are addressable; no
  arbitrary command execution, no clone/delete, no client-supplied paths.
- **Jobs:** mutations return `202` + job id; one mutating job per VM (`409`);
  in-memory history, lost on restart without affecting VMs.
- **Shell:** PTY-backed `ssh` per session; binary frames = terminal bytes,
  text frames = JSON control (`resize` in, `exit` out). Browser-terminal
  (xterm.js) compatible; the terminal UI itself is out of scope. Limits:
  4/VM, 16 total, 64 KiB frames, bounded output buffering; heartbeats detect
  dead peers, no idle logout; disconnect kills the session's ssh, never the
  VM; terminal traffic is never logged.
- **Default port:** 7878; `--port 0` prints the ephemeral port for scripting.
- **Tests:** `tests/server.rs` drives the real binary over real sockets with
  the fake `tart`/`ssh` world: reads, job lifecycle incl. provision payload,
  409 conflicts, origin/Host guards, bidirectional WebSocket shell, and
  handshake rejection for stopped VMs.

## 10. Recovery checkpoint

Recovered from the power-loss session and continued in
`/Users/aschuma/Documents/Tart/github__debian_ansible` (not the Lighthouse repo).

Implemented:
- Rust library/CLI, TOML layering and validation, SQLite inventory, detached start,
  SSH exec, env rendering/masking/diff, and server stub.
- Production install switched to Rust + compatibility symlink. Bash and its old
  templates are frozen test fixtures only; the script matches the old tracked
  implementation byte-for-byte.
- One-off converter preserves originals and existing TOML. It sources trusted
  shell configs, so run it only on your own reviewed files.
- README, QUICKSTART and agent guidance updated; `just test` and manual
  `just smoke PROJECT` added. GitHub Actions test and tag-release workflows added.

Verification (local):
- `just test`: Rust formatting, strict Clippy, unit/CLI/lifecycle tests, converter
  checks, and all 12 parity scenarios against both Bash and Rust.
- Isolated installation: release build, compatibility symlink, existing config
  preservation, and legacy-config guard verified under a temporary install root.
- Workflow YAML lint, shell syntax checks and `git diff --check` pass.

Parity normalises config names/keys, read-only Tart queries, the added startup IP
and documented clap diagnostics. Mutating calls, SSH arguments, VM-side files,
exit codes and final fake VM state remain strict. Goldens come only from Bash.
The harness now waits for detached fake startup; the original runner's `wait`
could miss the final `tart run` because it belonged to an exited subshell.

Not yet verified: a real Tart VM smoke run (CLI and server) and the hosted
GitHub Actions release job. No real VM was started/stopped/provisioned; no user
config was migrated and no binary was installed into the real user PATH.
Changes are uncommitted.

Next acceptance step: review the diff, then explicitly run `just smoke PROJECT`
on a disposable existing clone. The smoke test never provisions env, but updates
the managed SSH block and starts/stops the clone if it was initially stopped.
Phase 2 is implemented and tested against the fake world (see above); a real-VM
shell/API session remains part of the same manual acceptance step.
