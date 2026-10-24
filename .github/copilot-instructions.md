# Copilot Instructions

## Project Overview

This project builds a Debian 12 (Bookworm, ARM64) development VM image for Apple Silicon Macs using **Packer** + **Tart** + **Ansible**. Packer clones a base image, boots it, and runs an Ansible playbook to provision tools. The result is a local Tart VM image named `debian-ssh` (from `BUILD_VM_NAME` in `build/build.env`). That image is a **template**: users clone it per project (`just clone <name>`) and manage the clones with `tart-ctl.sh`; the template itself is only rebuilt/reprovisioned.

Developer tooling is managed by **mise** (https://mise.jdx.dev) for a single `devbox` user. Only base OS packages, Docker, and the mise binary come from apt.

## Build Commands

```bash
# Full image build (from project root, takes ~10-15 min)
just build                 # or ./build/provision.sh

# Validate without building
just validate              # packer validate + ansible-playbook --syntax-check

# Or step-by-step (from build/ directory; vm_name defaults to debian-ssh in the .pkr.hcl)
cd build/
packer init debian-ssh.pkr.hcl
packer validate -var-file="vars.pkrvars.hcl" -var vm_name=debian-ssh debian-ssh.pkr.hcl
packer build -var-file="vars.pkrvars.hcl" -var vm_name=debian-ssh debian-ssh.pkr.hcl

# Test build under a different image name (does not overwrite debian-ssh)
cd build && packer build -var-file=vars.pkrvars.hcl -var vm_name=debian-ssh-test debian-ssh.pkr.hcl

# Clone the template for a project
just clone my-project      # tart clone debian-ssh my-project
cd ~/Projects/my-project && tart-ctl.sh init   # interactive: writes .tart-ctl-env (TCTL_VM_NAME, TCTL_HOST_SHARE_DIR)
```

## Reprovision the Template VM (Single Role)

`just reprovision` / `just tools` target the template VM named in `build/build.env` via `build/with-vm.sh`: it starts the VM headless if stopped, waits for IP + SSH, substitutes `__VM_IP__` in the command args (and exports `VM_IP`), runs the command, and stops the VM again only if it started it. A manual `ansible-playbook` run needs the VM running already.

```bash
just reprovision <tags>    # e.g. just reprovision mise,runtimes

# equivalent without just
cd build/ansible
ansible-playbook playbook.yml \
  -i "$(tart ip debian-ssh)," \
  --private-key ~/.ssh/id_ed25519_tart \
  -u devbox --become \
  --tags <tag> \
  --extra-vars "ansible_user=devbox ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
```

Available tags: `user`, `common-tools`, `docker`, `mise`, `runtimes`, `cli-tools`, `shell`, `verify`

## Architecture

```
justfile                     # Host task runner for the template image: build, validate, lint, install (tart-ctl.sh + config), clone, reprovision, tools, clean. Reads build/build.env (never the tctl config)
build/
  build.env                  # Build-side config: BUILD_VM_NAME (→ packer -var vm_name), BUILD_SSH_USER, BUILD_IDENTITY_FILE
  debian-ssh.pkr.hcl         # Packer template — source image, provisioners
  vars.pkrvars.hcl           # Build SSH credentials, public key path (no vm_name — comes from build.env)
  provision.sh               # packer init → validate → build; prints "clone it" hint afterwards
  with-vm.sh                 # [-C dir] cmd… — ensure template VM running (start if needed), wait for SSH, run cmd with __VM_IP__ substituted, stop if it started it
  ansible/
    playbook.yml             # Pure orchestration: runs roles in order, no inline tasks
    group_vars/all/
      main.yml               # devbox_user, devbox_home, target_arch
      tool_versions.yml      # mise_runtimes + mise_cli_tools — THE place for tool versions
    roles/
      user/                  # devbox account, NOPASSWD sudo, authorized_keys, sshd_config
      common_tools/          # base apt packages (incl. acl, build-essential; no pip — use uv), git defaults
      docker/                # Docker CE apt repo, packages, devbox in docker group
      mise/                  # mise apt repo, /etc/profile.d/mise.sh, ~/.bashrc, config.toml.j2
        tasks/install.yml    #   reusable: mise install of a dict of tools (var mise_install_tools)
        tasks/verify.yml     #   reusable: run a list of commands as devbox (var mise_verify_commands)
      runtimes/              # include mise/install + verify for mise_runtimes
      cli_tools/             # include mise/install + verify for mise_cli_tools
      shell/                 # zsh login shell, /etc/zsh/zprofile, ~/.zshenv + ~/.zshrc templates, starship.toml, completions
      verify/                # final gate: groups, mise doctor, all *_verify_commands, docker, sudo

bin_template/
  tart-ctl.sh                # init/start/stop/status/provision/env/ssh/ip/update-ssh-config (bash 3.2 compatible)
  .tart-ctl-env              # Project marker template — what `tart-ctl.sh init` generates; found by walking up to /.
                             #   MUST set TCTL_VM_NAME (script dies otherwise); may override TCTL_HOST_SHARE_DIR, TCTL_ENV_PROVISION_FILES, TCTL_LOCAL_FORWARD/TCTL_REMOTE_FORWARD
config_template/             # Copied to ~/.config/tctl/ by `just install` (existing files kept)
  config                     # Global defaults, sourced first — intentionally NO TCTL_VM_NAME: TCTL_SSH_USER=devbox,
                             #   TCTL_SSH_CONFIG, TCTL_IDENTITY_FILE, TCTL_REMOTE_FORWARD / TCTL_LOCAL_FORWARD (comma-separated ssh forward specs),
                             #   TCTL_HOST_SHARE_DIR=auto (auto = dir containing .tart-ctl-env; path; or unset = no share),
                             #   TCTL_ENV_PROVISION_FILES=.env (comma-separated; relative to project root, or absolute/~)
QUICKSTART.md                # First-time setup walkthrough (keep in sync with justfile/tart-ctl.sh commands)
```

Role execution order: `user → common_tools → docker → mise → runtimes → cli_tools → shell → verify`.

## Key Conventions

### Users

- `devbox` is the only developer account (SSH key auth, `NOPASSWD` sudo, `docker` group). All mise tools are installed in its home.
- `admin` is the base image's account; Packer/Ansible connect as `admin` with password auth during the build. Do not remove it — Ansible cannot delete the account it is logged in as.
- Tasks that must run as the developer use `become_user: "{{ devbox_user }}"` and wrap commands in `bash -lc '...'` so mise shims are on `PATH`. The `acl` package (installed by `common_tools`) is required for `become_user` to an unprivileged user.

### Adding or Changing Tools

- **mise-managed tool**: edit `build/ansible/group_vars/all/tool_versions.yml`. Tool names are mise registry short names (`mise registry`) or explicit backends (`github:owner/repo`, `npm:pkg`, `aqua:owner/repo`). Values are version specs (`"latest"`, `"lts"`, `"temurin-25"`, `"3.9.9"`) or a mapping with `version` + backend options.
- Add a `<tool> --version` entry to `runtimes_verify_commands`, `cli_tools_verify_commands` or `shell_verify_commands` (`roles/<role>/defaults/main.yml`) so `verify` covers it.
- **Never** write new download/unarchive/`/etc/profile.d` PATH logic for a tool that mise can install.
- **apt-only tools** go into `roles/common_tools`; tools needing their own apt repo get a role following the `roles/docker` pattern (keyring stat → download → dearmor → sources list → apt).

### Ansible Roles

- **Self-contained** and **idempotent**: `mise/tasks/install.yml` uses `mise ls --missing --json` to decide what to install; apt tasks are naturally idempotent.
- `playbook.yml` holds no vars and no inline tasks.
- Use `ansible_facts['env']['PATH']`, never `ansible_env.PATH` (deprecated since Ansible 2.24+).
- The `verify` role must stay last and must fail on any error — it is the build's quality gate.

### mise on the VM

- Binary from the mise apt repo (`/usr/bin/mise`); data in `/home/devbox/.local/share/mise/`; global config generated from `roles/mise/templates/config.toml.j2` at `/home/devbox/.config/mise/config.toml`.
- `/etc/profile.d/mise.sh`: zsh → `mise activate zsh --shims`; interactive bash → `mise activate bash`; non-interactive bash → `--shims`. `~/.bashrc` → `mise activate bash`.
- zsh (devbox's login shell, `roles/shell`): `~/.zshenv` → shims (every zsh, so `ssh host 'cmd'` works); `~/.zshrc` → `mise activate zsh` + `starship init zsh` + `compinit`; `/etc/zsh/zprofile` sources `/etc/profile` so `/etc/profile.d/*.sh` applies to zsh login shells.
- `trusted_config_paths` includes `/mnt/shared` and `/home/devbox` so project `mise.toml` files work without `mise trust`.
- Tool availability:

```bash
ssh debian-ssh 'java --version'               # OK — zsh reads ~/.zshenv (shims)
ssh debian-ssh 'bash -lc "java --version"'    # OK — bash login shell (Ansible tasks use this form)
ssh debian-ssh 'bash -c "java --version"'     # Wrong — bash without profile
```

- `MISE_GITHUB_TOKEN` set on the host is forwarded to `mise install` to avoid GitHub API rate limits.

### virtiofs Sharing

The Tart virtiofs mount tag is always `com.apple.virtio-fs.automount` regardless of the `--dir=NAME:PATH` share name. The image mounts it on first access via `/etc/fstab` (`roles/common_tools`: `noauto,x-systemd.automount,nofail`, so booting without a share is clean). Each share name becomes a subdirectory under the mount point:

```bash
findmnt /mnt/shared
ls /mnt/shared/workspace/   # workspace = share name from --dir=workspace:...
```

### SSH Authentication

- Build time: password auth as `admin` via paramiko (`--connection=paramiko`, `use_proxy = false` — Packer's SSH proxy is incompatible with paramiko password auth)
- After build: key-only auth as `devbox` (`~/.ssh/id_ed25519_tart`)
- `StrictHostKeyChecking=no` is always required — VM IP changes on each start

### Environment Variables on the VM

`tart-ctl.sh provision` renders the files in `TCTL_ENV_PROVISION_FILES` (comma-separated, default `.env`; relative to the `.tart-ctl-env` directory, absolute/`~` as-is, layered with later keys winning) on the host into `printf %q`-quoted `export` lines and pipes them over SSH to `sudo tee /etc/profile.d/tart-provision.sh`. Sources may be FIFOs/process substitutions (1Password); each is read exactly once. The file persists in the VM and is sourced by every login shell (bash and zsh).
