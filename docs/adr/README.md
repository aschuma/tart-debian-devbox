# Architecture Decision Records

Decisions that shaped this repository, in the order you would take them to
rebuild the current state from an empty project. Each record states the context,
the decision, the alternatives that were rejected and the non-obvious
consequences. A new record takes the next free number, states the decision in
its title, and needs only a short context/decision/why paragraph; *Considered
options* and *Consequences* are added only when the rejected alternatives or
downstream effects are worth remembering.

| # | Decision | Status |
| --- | --- | --- |
| [0001](0001-tart-template-image-built-with-packer-and-ansible.md) | Tart template image built with Packer + Ansible, cloned per project | accepted |
| [0002](0002-single-devbox-user-with-key-only-ssh.md) | One developer account (`devbox`) with key-only SSH; `admin` kept for the build | accepted |
| [0003](0003-mise-manages-the-toolchain.md) | mise manages the developer toolchain; apt only for the OS, Docker and mise itself | accepted |
| [0004](0004-rust-tctl-cli-with-synchronous-core.md) | Per-project VM control is a Rust CLI (`tctl`) with a synchronous core and a swappable backend | accepted |
| [0005](0005-layered-toml-config.md) | Layered TOML configuration: global defaults plus an upward-discovered project marker | accepted |
| [0006](0006-project-vm-integration-share-env-sshconfig.md) | Project ↔ VM integration: virtiofs share, env files pushed to `/etc/profile.d`, managed `~/.ssh/config` block | accepted |
| [0007](0007-derived-sqlite-registry-under-xdg-data.md) | A per-user SQLite registry, derived from project markers and storing hashes rather than values | accepted |
| [0008](0008-local-http-websocket-control-server-without-auth.md) | A loopback-only HTTP/WebSocket control server with no authentication | accepted |

## How they fit together

```
0001 template image ──► 0002 devbox user ──► 0003 mise toolchain        (what is inside the VM)
        │
        └─► 0004 tctl CLI ──► 0005 TOML config ──► 0006 share / env / ssh-config
                                     │
                                     └─► 0007 registry ──► 0008 HTTP server   (how the host drives clones)
```

`docs/tctl-design.md` is the detailed design document behind 0004–0008; the
ADRs are the stable summary.
