---
status: accepted
---

# Per-project VM control is a Rust CLI (`tctl`) with a synchronous core and a swappable backend

Day-to-day control of the cloned VMs (init, start/stop, status, ip, ssh,
provision, env, SSH-config maintenance) is implemented as a single Rust crate,
`tctl/`. The **core is synchronous** (`tart`/`ssh` are blocking subprocesses
anyway) and every external call goes through a `TartBackend` trait so tests can
drive a fake. Each command returns a `serde::Serialize` result struct from which
both the human output and `--json` (`{"ok":true,"result":…,"messages":…}` /
`{"ok":false,"error":…}`) are rendered.

## Considered options

- **A shell script**: config layering, env merging and rewriting
  `~/.ssh/config` have no realistic test story in Bash, and installation needs
  a directory-on-PATH dance instead of a single binary.
- **Python/Go**: Python would add an interpreter/venv dependency on the host;
  Go was viable, but Rust gives a single static binary, `clap`'s derive parsing
  and `thiserror` structured errors.
- **Async core**: unnecessary complexity for sequential subprocess work; async
  is confined to the HTTP server (ADR-0008), which wraps core calls in
  `spawn_blocking`.

## Consequences

- Scriptability rule: operations take complete inputs; only the CLI prompts,
  and only when a flag is missing *and* stdin is a TTY. `tctl ssh` `exec()`s
  the real `ssh` so terminal, signals and exit code pass through untouched —
  it is the one command whose output is never wrapped in JSON.
- `start` runs `tart run` detached in a new session with logs under
  `~/.local/share/tctl/logs/`, records the PID before waiting for an IP, and
  treats "running" and "has an IP" as independent states.
- Testing never needs a real VM: unit tests plus sandboxed CLI/server tests
  that run the real binary with fake `tart`/`ssh` shims (`tests/fakes/`) in an
  isolated HOME. A real-VM check is an explicit manual `just smoke PROJECT`.
- Distribution: `just install` → `cargo install --path tctl`; tags `tctl-v*`
  build an Apple Silicon release archive in GitHub Actions.
