---
status: accepted
---

# A loopback-only HTTP/WebSocket control server with no authentication

`tctl server` exposes the same core as the CLI over HTTP (`GET /vms`,
`GET /vms/{name}[/env]`, `POST /vms/{name}/start|stop|provision`,
`GET /jobs/{id}`) and a WebSocket PTY shell (`GET /vms/{name}/shell`), so a
browser or editor UI can drive VMs. It is a foreground axum/tokio process that
binds **`127.0.0.1` only** (not configurable) and has **no authentication** —
an explicit owner decision. Mitigations instead of auth: loopback bind, an exact
`Origin` allowlist (`--allow-origin`, repeatable, no wildcards; only those
origins get CORS headers), and a local-`Host` check against DNS rebinding.
Remote use goes through an SSH tunnel. Nothing ever starts the server
implicitly.

## Considered options

- **Token/password auth**: a local-only single-user tool gains little — any
  process that can read the token file can also run `tctl` directly — while
  every client would need a credential flow.
- **Unix domain socket**: safer, but browsers cannot connect to it, and a
  browser terminal (xterm.js) was the primary consumer.
- **Making the server the primary interface with the CLI as a client**: would
  force a daemon lifecycle on everyone; the CLI stays self-sufficient and the
  server is optional (ADR-0004).

## Consequences

- Mutations are jobs: `202 Accepted` with a job id, one mutating job per VM
  (`409` otherwise), in-memory history lost on restart without affecting VMs.
- Capability is deliberately narrow: only registered VMs (ADR-0007), no clone/
  delete, no arbitrary remote command, no client-supplied paths, no `init`, and
  `GET …/env` is always masked with no reveal path (ADR-0006).
- Shell protocol: binary frames = terminal bytes, text frames = JSON control
  (`resize` in, `exit` out); each session is a real `ssh` under a PTY. Closing
  the socket kills that `ssh`, never the VM. Terminal traffic is raw, unmasked,
  and never logged. Session limits, frame size and the default port are
  documented in README → "HTTP control server".
