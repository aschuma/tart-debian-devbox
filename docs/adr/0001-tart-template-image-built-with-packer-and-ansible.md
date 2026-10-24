---
status: accepted
---

# Tart template image built with Packer + Ansible, cloned per project

We need a reproducible Debian (ARM64) development VM on Apple Silicon Macs. We
build **one read-only template image** with Packer (Tart plugin, starting from
the Cirrus Labs `debian:bookworm` image) provisioned by an Ansible playbook, and
then **clone** it per project with `tart clone`; nobody works in the template
itself. Rebuilding the template never touches existing clones, and the template
can be re-provisioned in place by running single Ansible tags against it.

## Considered options

- **Docker / devcontainers**: no full OS, systemd, or Docker-in-VM isolation;
  the goal is a disposable "real machine" per project.
- **Lima / UTM / Colima**: workable, but Tart is the only tool with first-class
  Packer support and OCI-style base images on Apple Silicon.
- **One shared VM for all projects**: simpler, but projects would leak tools,
  env and ports into each other. Clones are cheap (APFS copy-on-write).
- **Shell provisioning instead of Ansible**: Ansible gives idempotent roles,
  tags for partial re-runs and a lint story; the Packer step only bootstraps
  `python3` so Ansible can run.

## Consequences

- Two independent tool surfaces, deliberately kept apart: the `justfile` and
  `build/build.env` manage the **template** (build, validate, lint,
  reprovision, clone, clean); the `tctl` CLI (ADR-0004) manages the **clones**.
  `build/build.env` is intentionally separate from `~/.config/tctl/config.toml`.
- Ansible is pure orchestration: `playbook.yml` holds no vars and no inline
  tasks, each role is self-contained and idempotent, and the `verify` role is
  last and fails the build on any check (ADR-0003).
- The image pins resources in `debian-ssh.pkr.hcl` (4 CPU, 10 GB RAM, 20 GB
  disk); clones inherit them.
