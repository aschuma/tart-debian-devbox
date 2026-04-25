# Copilot Instructions

## Project Overview

This project builds a Debian 12 (Bookworm, ARM64) development VM image for Apple Silicon Macs using **Packer** + **Tart** + **Ansible**. Packer clones a base image, boots it, and runs an Ansible playbook to provision tools. The result is a local Tart VM image named `debian-ssh`.

## Build Commands

```bash
# Full image build (from project root, takes ~10-15 min)
./build/provision.sh

# Or step-by-step (from build/ directory)
cd build/
packer init debian-ssh.pkr.hcl
packer validate -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
```

## Reprovision a Running VM (Single Role)

```bash
cd build/ansible
ansible-playbook playbook.yml \
  -i "$(tart ip debian-ssh)," \
  --private-key ~/.ssh/id_ed25519_tart \
  -u admin --become \
  --tags <tag> \
  --extra-vars "ansible_user=admin ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
```

Available tags: `ssh`, `tools`, `docker`, `java`, `gradle`, `maven`, `graalpy`, `nvm`, `acli`, `copilot-cli`, `opencode`

## Architecture

```
build/
  debian-ssh.pkr.hcl        # Packer template — source image, provisioners
  vars.pkrvars.hcl           # VM identity, SSH credentials (not committed)
  provision.sh               # packer init → validate → build
  ansible/
    playbook.yml             # Pure orchestration: runs roles in order, no inline tasks
    roles/<name>/
      tasks/main.yml         # What to install/configure
      vars/main.yml          # Version pinning and install paths

bin/
  shared-env.sh              # Common variables (HOST, SSH_USER, IDENTITY_FILE, HOST_SHARE_DIR)
  run.sh                     # tart run with virtiofs directory share
  stop.sh                    # Stop the VM
  update-host-ssh-config.sh  # Updates ~/.ssh/config with current VM IP
  provision.sh               # Runs bin/provision.d/*.sh in sort order
  provision.d/
    10_mount-share.sh        # Mounts virtiofs share on running VM
    20_init-env.sh           # Pushes bin/.env → /etc/profile.d/tart-provision.sh on VM
```

## Key Conventions

### Ansible Roles

- **Self-contained**: each role handles download, install, configure, verify, and cleanup
- **Idempotent**: tasks use `when: not <binary>.stat.exists` — safe to re-run
- **Version variables**: all version/path variables live in `roles/<name>/vars/main.yml`
- **Environment variables in tasks**: always use `ansible_facts['env']['PATH']`, never `ansible_env.PATH` (deprecated since Ansible 2.24+)

### Updating a Tool Version

Edit the relevant `vars/main.yml` (e.g., `build/ansible/roles/java/vars/main.yml`) and update the version, build string, download URL, and install path together. Then re-run the build or reprovision with the matching tag.

### virtiofs Sharing

The Tart virtiofs mount tag is always `com.apple.virtio-fs.automount` regardless of the `--dir=NAME:PATH` share name. Each share name becomes a subdirectory under the mount point:

```bash
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared
ls /mnt/shared/workspace/   # workspace = share name from --dir=workspace:...
```

### SSH Authentication

- Build time: password auth via paramiko (`--connection=paramiko`)
- After build: key-only auth (`~/.ssh/id_ed25519_tart`)
- `StrictHostKeyChecking=no` is always required — VM IP changes on each start

### Tools in /opt/*

Java, Gradle, Maven, GraalPy, and nvm are installed under `/opt/` and are only available in **login shells**:

```bash
# Wrong — non-login shell, PATH not set
ssh debian-ssh 'java -version'

# Correct — login shell loads /etc/profile.d/
ssh debian-ssh 'bash -l -c "java -version"'
```

### Environment Variables on the VM

`bin/20_init-env.sh` reads `bin/.env` and writes exports to `/etc/profile.d/tart-provision.sh` on the VM. Values are base64-encoded for safe transfer. The `.env` file is gitignored.

### Packer Ansible Provisioner

Uses `use_proxy = false` with `--connection=paramiko` because Packer's default SSH proxy mode is incompatible with paramiko password auth.

### SSH Config

`bin/shared-env.sh` defines `HOST=debian-ssh` and `IDENTITY_FILE=~/.ssh/id_ed25519_tart`. Run `bin/update-host-ssh-config.sh` after each `tart run` to update the VM IP in `~/.ssh/config`.
