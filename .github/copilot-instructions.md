# Copilot Instructions

## Project Overview

This project builds a Debian 12 (Bookworm, ARM64) development VM image for Apple Silicon Macs using **Packer** + **Tart** + **Ansible**. Packer clones a base image, boots it, and runs an Ansible playbook to provision tools. The result is a local Tart VM image named `debian-ssh`.

Developer tooling is managed by **mise** (https://mise.jdx.dev) for a single `devbox` user. Only base OS packages, Docker, and the mise binary come from apt.

## Build Commands

```bash
# Full image build (from project root, takes ~10-15 min)
just build                 # or ./build/provision.sh

# Validate without building
just validate              # packer validate + ansible-playbook --syntax-check

# Or step-by-step (from build/ directory)
cd build/
packer init debian-ssh.pkr.hcl
packer validate -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl

# Test build under a different image name (does not overwrite debian-ssh)
cd build && packer build -var-file=vars.pkrvars.hcl -var vm_name=debian-ssh-test debian-ssh.pkr.hcl
```

## Reprovision a Running VM (Single Role)

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

Available tags: `user`, `common-tools`, `docker`, `mise`, `runtimes`, `cli-tools`, `verify`

## Architecture

```
justfile                     # Host task runner: build, validate, start/stop/ssh (via tart-ctl), reprovision
build/
  debian-ssh.pkr.hcl         # Packer template — source image, provisioners
  vars.pkrvars.hcl           # VM identity, build SSH credentials, public key path
  provision.sh               # packer init → validate → build
  ansible/
    playbook.yml             # Pure orchestration: runs roles in order, no inline tasks
    group_vars/all/
      main.yml               # devbox_user, devbox_home, target_arch
      tool_versions.yml      # mise_runtimes + mise_cli_tools — THE place for tool versions
    roles/
      user/                  # devbox account, NOPASSWD sudo, authorized_keys, sshd_config
      common_tools/          # base apt packages (incl. acl for become_user), git defaults
      docker/                # Docker CE apt repo, packages, devbox in docker group
      mise/                  # mise apt repo, /etc/profile.d/mise.sh, ~/.bashrc, config.toml.j2
        tasks/install.yml    #   reusable: mise install of a dict of tools (var mise_install_tools)
        tasks/verify.yml     #   reusable: run a list of commands as devbox (var mise_verify_commands)
      runtimes/              # include mise/install + verify for mise_runtimes
      cli_tools/             # include mise/install + verify for mise_cli_tools
      verify/                # final gate: groups, mise doctor, all *_verify_commands, docker, sudo

bin_template/
  tart-ctl.sh                # start/stop/status/provision/ssh/ip/update-ssh-config
  .tart-ctl-env              # TCTL_VM_NAME, TCTL_SSH_USER=devbox, TCTL_IDENTITY_FILE, TCTL_HOST_SHARE_DIR
  .env / .env.local          # Project env vars pushed into the VM
  .tart-ctl-provision.d/
    10_mount-share.sh        # Mounts virtiofs share on running VM
    20_init-env.sh           # Pushes .env → /etc/profile.d/tart-provision.sh on VM
```

Role execution order: `user → common_tools → docker → mise → runtimes → cli_tools → verify`.

## Key Conventions

### Users

- `devbox` is the only developer account (SSH key auth, `NOPASSWD` sudo, `docker` group). All mise tools are installed in its home.
- `admin` is the base image's account; Packer/Ansible connect as `admin` with password auth during the build. Do not remove it — Ansible cannot delete the account it is logged in as.
- Tasks that must run as the developer use `become_user: "{{ devbox_user }}"` and wrap commands in `bash -lc '...'` so mise shims are on `PATH`. The `acl` package (installed by `common_tools`) is required for `become_user` to an unprivileged user.

### Adding or Changing Tools

- **mise-managed tool**: edit `build/ansible/group_vars/all/tool_versions.yml`. Tool names are mise registry short names (`mise registry`) or explicit backends (`github:owner/repo`, `npm:pkg`, `aqua:owner/repo`). Values are version specs (`"latest"`, `"lts"`, `"temurin-25"`, `"3.9.9"`) or a mapping with `version` + backend options.
- Add a `<tool> --version` entry to `runtimes_verify_commands` or `cli_tools_verify_commands` (`roles/<role>/defaults/main.yml`) so `verify` covers it.
- **Never** write new download/unarchive/`/etc/profile.d` PATH logic for a tool that mise can install.
- **apt-only tools** go into `roles/common_tools`; tools needing their own apt repo get a role following the `roles/docker` pattern (keyring stat → download → dearmor → sources list → apt).

### Ansible Roles

- **Self-contained** and **idempotent**: `mise/tasks/install.yml` uses `mise ls --missing --json` to decide what to install; apt tasks are naturally idempotent.
- `playbook.yml` holds no vars and no inline tasks.
- Use `ansible_facts['env']['PATH']`, never `ansible_env.PATH` (deprecated since Ansible 2.24+).
- The `verify` role must stay last and must fail on any error — it is the build's quality gate.

### mise on the VM

- Binary from the mise apt repo (`/usr/bin/mise`); data in `/home/devbox/.local/share/mise/`; global config generated from `roles/mise/templates/config.toml.j2` at `/home/devbox/.config/mise/config.toml`.
- `/etc/profile.d/mise.sh`: interactive login shells → `mise activate bash`; non-interactive login shells → `mise activate bash --shims`. `~/.bashrc` → `mise activate bash`.
- `trusted_config_paths` includes `/mnt/shared` and `/home/devbox` so project `mise.toml` files work without `mise trust`.
- Tools are therefore only available in **login shells**:

```bash
ssh debian-ssh 'java --version'               # Wrong — no profile loaded
ssh debian-ssh 'bash -lc "java --version"'    # Correct
```

- `MISE_GITHUB_TOKEN` set on the host is forwarded to `mise install` to avoid GitHub API rate limits.

### virtiofs Sharing

The Tart virtiofs mount tag is always `com.apple.virtio-fs.automount` regardless of the `--dir=NAME:PATH` share name. Each share name becomes a subdirectory under the mount point:

```bash
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared
ls /mnt/shared/workspace/   # workspace = share name from --dir=workspace:...
```

### SSH Authentication

- Build time: password auth as `admin` via paramiko (`--connection=paramiko`, `use_proxy = false` — Packer's SSH proxy is incompatible with paramiko password auth)
- After build: key-only auth as `devbox` (`~/.ssh/id_ed25519_tart`)
- `StrictHostKeyChecking=no` is always required — VM IP changes on each start

### Environment Variables on the VM

`20_init-env.sh` reads `.env` / `.env.local` and writes exports to `/etc/profile.d/tart-provision.sh` on the VM. Values are base64-encoded for safe transfer.
