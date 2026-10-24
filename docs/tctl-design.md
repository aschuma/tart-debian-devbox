# Design: `tctl` — per-project Tart VM controller

This document is the detailed design behind ADR-0004 … ADR-0008 (see
`docs/adr/README.md`). It describes how `tctl` is built; the README is the
user-facing reference.

## 1. Scope and goals

**In scope:** per-project VM lifecycle for clones of the template image.
**Out of scope:** `build/with-vm.sh`, `build/provision.sh` and the `justfile` —
they remain thin wrappers around Packer/Ansible/Tart for the template itself.

Goals, in order:

1. Maintainability and correctness: config layering, env merging and
   `~/.ssh/config` rewriting must be unit-testable.
2. Distribution: a single binary on PATH.
3. Robust arg parsing, `--json`, structured errors.
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

Binary name **`tctl`** (matches `~/.config/tctl/`).

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
| `tctl server [--port 7878] [--allow-origin ORIGIN]…` | | local HTTP/WebSocket API (section 9) |

Global flags: `--json`, `--no-color`, `-v/-q` (tracing level).

Bare `tctl` prints help. Nothing ever starts the server implicitly.

## 4. Configuration (TOML)

Two files, project overrides global **per key**; the project file is found by
walking up from the cwd:

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
- Unknown keys are rejected so typos surface immediately.

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

- **VM side**: `provision` writes `/etc/profile.d/tart-provision.sh`; the SSH
  config is maintained as one managed `Host` block in `~/.ssh/config`. Both
  formats are a compatibility surface — change them deliberately.
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
- **SSH config refresh**: commands that need SSH (`ssh`, `provision`,
  `env --diff`) first make sure the managed block has the current IP and user.
- **Secret masking**: heuristic on key names (pattern list: README → "What
  `provision` Does") applied to human **and** JSON output; `--show-secrets` is
  the only reveal path and the server inherits the default.

## 7. Testing

- Rust unit tests: TOML parsing/layering, forward rendering, env layering,
  masking, ssh-config block rewriting, registry.
- **Sandboxed CLI tests**: fake `tart` and `ssh` shims (`tests/fakes/`) on `PATH`
  log their arguments and return canned output; the tests run `tctl` in an
  isolated HOME/project and check the issued commands, exit codes and produced
  files (`~/.ssh/config`, provision payload).
- **Server tests** (`tests/server.rs`): drive the real binary over real sockets
  with the same fake world (section 9).
- `just smoke`: manual run against a real Tart VM before a release (not CI).

## 8. Delivery

- Cargo project lives in this repo (`tctl/`).
- `just install` → `cargo install --path tctl` → `~/.cargo/bin/tctl`;
  GitHub Actions builds release binaries for users without a Rust toolchain.
- Crates: `clap` (derive), `anyhow` (bin) + `thiserror` (lib), `serde`, `toml`,
  `serde_json`, `rusqlite` (bundled), `sha2`/`hex`, `similar`, `libc`, `tracing`;
  server: `tokio`, `axum` (ws), `portable-pty`. Stable Rust, edition 2021. XDG
  paths and simple human formatting are implemented directly.

## 9. `tctl server`

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
  (xterm.js) compatible; the terminal UI itself is out of scope. Output
  buffering is bounded, heartbeats detect dead peers, there is no idle logout;
  disconnect kills the session's ssh, never the VM; terminal traffic is never
  logged. Session/frame limits and the default port are user-facing and
  documented once, in README → "HTTP control server"; `--port 0` prints the
  ephemeral port for scripting.
- **Tests:** `tests/server.rs` covers reads, job lifecycle incl. provision
  payload, 409 conflicts, origin/Host guards, bidirectional WebSocket shell,
  and handshake rejection for stopped VMs.

## 10. Open items

- A real Tart VM smoke run of CLI and server (`just smoke PROJECT`) before each
  release; the hosted GitHub Actions release job has not been exercised yet.
