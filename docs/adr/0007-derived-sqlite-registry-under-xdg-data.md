---
status: accepted
---

# A per-user SQLite registry, derived from project markers and storing hashes rather than values

`tctl` keeps an inventory of known VMs in `~/.local/share/tctl/tctl.db`
(`rusqlite`, bundled SQLite; `XDG_DATA_HOME` honoured). The registry is
**derived state**: refreshed on `init` and every `start`, rebuildable at any
time with `tctl registry scan <dir>`, never committed, and never the source of
truth — `.tart-ctl.toml` is (ADR-0005). It stores the project directory,
effective config, last IP/PID/log path and the SHA-256 of the last provisioned
environment with its timestamp. It stores **no environment values**.

## Considered options

- **No registry; always derive from the current directory**: then nothing can
  list "all my VMs" or address a VM by name from outside its project tree,
  which the HTTP server (ADR-0008) needs.
- **A JSON/TOML state file**: fine for a handful of rows, but concurrent
  updates from `start` and the server would need hand-rolled locking; SQLite
  gives that for free and `bundled` avoids a system dependency.
- **Storing the rendered env for `env --diff`**: the hash is enough to detect
  drift, and keeping secrets out of a long-lived file was the priority.

## Consequences

- Only registered VMs are addressable by the server; `tctl init` or
  `tctl registry scan` is a prerequisite.
- `template` (clone lineage) is reserved but null — Tart exposes no lineage.
- Deleting the database loses nothing that a `scan` cannot recreate, except the
  last-provision hash/timestamp.
