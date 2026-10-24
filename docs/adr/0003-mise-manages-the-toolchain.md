---
status: accepted
---

# mise manages the developer toolchain; apt only for the OS, Docker and mise itself

Every developer tool that mise can install (Java, Node, Gradle, Maven, Kotlin,
Python, GraalPy, gh, aws-cli, terraform, packer, uv, ruff, starship, AI CLIs, …)
is installed **per user** through mise, with all versions declared in a single
file, `build/ansible/group_vars/all/tool_versions.yml`, rendered into
`/home/devbox/.config/mise/config.toml`. apt is used only for base OS packages,
Docker CE (official repo) and the mise binary. Hand-written download/unarchive/
`/etc/profile.d` PATH logic for tools is forbidden.

## Considered options

- **nvm + SDKMAN + pyenv + manual tarballs**: one mechanism per ecosystem,
  each with its own PATH hook and update story.
- **apt/distro packages for everything**: versions lag badly on Debian stable
  and cannot be pinned per project.
- **asdf**: same model as mise but slower, no built-in backends
  (`github:`, `npm:`, `aqua:`) for tools outside the core registry.

## Consequences

- Shell wiring is part of this decision: `/etc/profile.d/mise.sh` activates
  mise in every login shell (zsh uses `--shims` so `ssh host 'cmd'` works
  without an interactive shell), `~/.zshenv`/`~/.zshrc`/`~/.bashrc` complete
  the picture, and `/etc/zsh/zprofile` sources `/etc/profile` so profile.d
  applies to zsh. `bash -c` without a login shell is unsupported by design.
- `/mnt/shared` and `/home/devbox` are `trusted_config_paths`, so project
  `mise.toml` files on the host share work without `mise trust` (ADR-0006).
- Adding a tool = one line in `tool_versions.yml` plus a `<tool> --version`
  entry in the role's verify list; the `verify` role (`mise doctor`, every
  version command, shell/docker/sudo checks) is the build's quality gate.
- Installs are idempotent via `mise ls --missing --json`; `just reprovision
  mise,runtimes` can refresh the template without a full rebuild.
