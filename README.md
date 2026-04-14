# Packer + Tart + Ansible: Debian 12 Dev VM

Automated Debian 12 (Bookworm) development VM with Docker, Java 21, build tools, and SSH key authentication.

## What Gets Built

- **Base**: Debian 12 Bookworm (ARM64) from `ghcr.io/cirruslabs/debian:bookworm`
- **Users**: `admin` + `deploy` (both with SSH key auth, sudo access)
- **Docker**: Docker CE + Compose plugin (latest from official Debian repo)
- **Java**: OpenJDK (Adoptium/Temurin) at `/opt/jdk` — version configured via `java_build` variable
- **Build Tools**: Gradle + Maven — versions configured via `gradle_version` and `maven_version` variables
- **Python**: GraalPy — version configured via `graalpy_version` variable
- **CLI Tools**: git, curl, vim, jq, htop, tree, unzip, build-essential

> **Customize versions**: Edit variables in `ansible/playbook.yml` (see [Software Versions](#software-versions) section)

## Key Concepts

**Packer** orchestrates the build: clones base image → boots VM → runs shell + Ansible → saves image

**Ansible** provisions via **paramiko** (password auth during build) then switches to **key-only** SSH

**Variables** control versions, paths, and SSH credentials — all customizable in `vars.pkrvars.hcl`

**Install order**: Docker → GraalPy → Java → Gradle → Maven

**Directory sharing**: Linux VMs use virtiofs — all shares are exposed under a single mount tag (`com.apple.virtio-fs.automount`) and must be manually mounted (not auto-mounted like macOS guests). Each `--dir=NAME:PATH` becomes a subdirectory under the mount point.

---

## Build the VM

```bash
# 1. Prerequisites (macOS)
brew install hashicorp/tap/packer ansible

# 2. Initialize Packer plugins
packer init debian-ssh.pkr.hcl

# 3. Validate configuration
packer validate -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl

# 4. Build the image
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
```

Build takes ~10-15 minutes. Result: local Tart image named `debian-ssh`

---

## Customize via Variables

### VM Resources & Identity

Edit `vars.pkrvars.hcl`:

```hcl
vm_name      = "my-dev-vm"              # Change final image name
ssh_user     = "admin"                  # Build-time SSH user
ssh_password = "admin"                  # Build-time password
ssh_key_path = "~/.ssh/id_ed25519.pub" # Your public key to inject
```

Edit `debian-ssh.pkr.hcl` source block for resources:

```hcl
source "tart-cli" "debian" {
  cpu_count    = 4      # Increase CPU cores
  memory_gb    = 8      # Increase RAM
  # ...
}
```

### Software Versions

Edit `ansible/playbook.yml` vars section:

```yaml
vars:
  java_version: "21"              # Java major version
  java_build: "21.0.5+11"         # Specific Adoptium build
  gradle_version: "8.12"          # Gradle version
  maven_version: "3.9.9"          # Maven version
  graalpy_version: "25.0.2"       # GraalPy version
```

Change versions, re-run `packer build` — idempotent (skips if already installed)

---

## Using the VM

### Start with Host Folder Mount

Map a local folder from your Mac to the VM using Tart's virtiofs sharing:

```bash
# Start VM with directory mounted (headless, no GUI)
tart run --no-graphics --dir=hostshare:/path/to/local/folder debian-ssh

# Or use current directory
tart run --no-graphics --dir=hostshare:$PWD debian-ssh
```

**Mount the shared directory inside the VM:**

The shared directory is **NOT auto-mounted** in Debian. Tart exposes all `--dir` shares under a single virtiofs tag (`com.apple.virtio-fs.automount`). Each share name becomes a subdirectory under the mount point.

```bash
# SSH into the VM
ssh -i ~/.ssh/id_ed25519_tart admin@$(tart ip debian-ssh)

# Mount the virtiofs filesystem
sudo mkdir -p /mnt/shared
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared

# Your files are in a subdirectory matching the share name
ls -la /mnt/shared/hostshare/
```

**Make it persistent across reboots** (add to `/etc/fstab`):

```bash
# Inside the VM
echo "com.apple.virtio-fs.automount /mnt/shared virtiofs defaults,nofail 0 0" | sudo tee -a /etc/fstab

# Test the fstab entry
sudo mount -a
ls -la /mnt/shared/hostshare/
```

> **Note**: The `nofail` option allows the VM to boot normally even when started without `--dir`. The `--dir=hostshare:...` flag makes the share *available* to the VM; you must always pass it when starting the VM for the mount to work.

### SSH into the VM

```bash
# Get VM IP
tart ip debian-ssh

# SSH with key (password auth disabled after build)
ssh -i ~/.ssh/id_ed25519_tart admin@192.168.64.X

# Or use deploy user
ssh -i ~/.ssh/id_ed25519_tart deploy@192.168.64.X

# Then simply:
ssh debian-ssh
```

**Add or update `~/.ssh/config` automatically** (resolves current VM IP):

```bash
#!/usr/bin/env bash
set -euo pipefail

HOST="debian-ssh"
SSH_CONFIG="${HOME}/.ssh/config"
IDENTITY_FILE="~/.ssh/id_ed25519_tart"
USER="admin"

# Resolve current VM IP
IP=$(tart ip "$HOST" 2>/dev/null) || { echo "Error: VM '$HOST' not running"; exit 1; }
echo "Resolved $HOST → $IP"

# Ensure ~/.ssh/config exists
mkdir -p "$(dirname "$SSH_CONFIG")"
touch "$SSH_CONFIG"
chmod 600 "$SSH_CONFIG"

# Check if Host block already exists
if grep -q "^Host ${HOST}$" "$SSH_CONFIG"; then
    # Update existing HostName
    sed -i '' "/^Host ${HOST}$/,/^Host / {
        s/^    HostName .*/    HostName ${IP}/
    }" "$SSH_CONFIG"
    echo "Updated existing '$HOST' entry with IP $IP"
else
    # Append new block
    cat >> "$SSH_CONFIG" <<EOF

Host ${HOST}
    HostName ${IP}
    User ${USER}
    IdentityFile ${IDENTITY_FILE}
    StrictHostKeyChecking no
    UserKnownHostsFile /dev/null
EOF
    echo "Added new '$HOST' entry with IP $IP"
fi
```

> **Tip**: The VM IP may change after each `tart run`. Re-run the script above to update `~/.ssh/config` accordingly.

### Test Docker (Requires Reboot)

```bash
ssh debian-dev

# First login — docker group not yet active
docker ps  # permission denied

# Reboot or re-login
logout
ssh debian-dev

# Now docker works
docker ps
docker compose version
```

---

## IntelliJ Remote Development

### Option 1: SSH Gateway (Recommended)

1. **IntelliJ IDEA** → **File** → **Remote Development** → **SSH**
2. **New Connection**:
   - Host: `192.168.64.X` (from `tart ip debian-ssh`)
   - Port: `22`
   - User: `admin` or `deploy`
   - Authentication: **Key Pair**
   - Private key: `~/.ssh/id_ed25519_tart`
3. **IDE Version**: Select latest available
4. **Project Directory**: Choose project on VM or create new
5. Click **Connect** — IntelliJ downloads IDE backend to VM, opens remote session

**Benefits**:
- Full IDE runs locally, only project files on VM
- Low latency, UI feels native
- VM resources (Java, Maven, Gradle, Docker) available to IDE

### Option 2: Mount Project via Tart Share

```bash
# Start VM with project mounted (headless)
tart run --no-graphics --dir=project:/path/to/local/project debian-ssh
```

Then mount it inside the VM:

```bash
# SSH into VM
ssh -i ~/.ssh/id_ed25519_tart admin@$(tart ip debian-ssh)

# Mount all Tart shares
sudo mkdir -p /mnt/shared
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared

# Project files are at /mnt/shared/project/
ls -la /mnt/shared/project/

# Make persistent (add to /etc/fstab)
echo "com.apple.virtio-fs.automount /mnt/shared virtiofs defaults,nofail 0 0" | sudo tee -a /etc/fstab
```

Then configure IntelliJ to use remote SDK:

1. **File** → **Project Structure** → **SDKs** → **+** → **Add SSH SDK**
2. Configure SSH to `admin@192.168.64.X`
3. Point to `/opt/jdk/jdk-21.0.5+11` on the VM

**Benefits**:
- Files stay on host (easier backup, local git)
- Builds/tests run on VM resources

### Useful for Remote Dev

**Port forwarding** for web apps running on VM:

```bash
ssh -L 8080:localhost:8080 debian-dev
# Access VM's port 8080 at http://localhost:8080 on your Mac
```

**rsync** for fast bi-directional sync:

```bash
# Sync local → VM
rsync -avz --exclude='.git' /local/project/ debian-dev:/home/admin/project/

# Continuous watch sync (requires fswatch on Mac)
brew install fswatch
fswatch -o /local/project | xargs -n1 -I{} rsync -avz /local/project/ debian-dev:/home/admin/project/
```

**VS Code Remote SSH** also works:

1. Install **Remote - SSH** extension
2. Connect to `debian-dev` (via `~/.ssh/config`)
3. Open folder on VM
4. Extensions install automatically on remote

---

## Troubleshooting

### Ansible Deprecation Warnings

If you see `ansible_env.PATH` warnings, ensure playbook uses:

```yaml
environment:
  PATH: "{{ ansible_facts['env']['PATH'] }}:{{ java_home }}/bin"
```

Not `ansible_env.PATH` (deprecated in Ansible 2.24+)

### Java Not Found

Tools installed to `/opt/*` only in **login shells**. Use:

```bash
# Wrong (non-login)
ssh debian-dev 'java -version'  # command not found

# Right (login shell)
ssh debian-dev 'bash -l -c "java -version"'

# Or just login interactively
ssh debian-dev
java -version  # works
```

### Docker Permission Denied

Docker group changes require logout/login or reboot:

```bash
ssh debian-dev
sudo reboot

# Wait ~30s, then reconnect
ssh debian-dev
docker ps  # now works
```

### VM Won't Start

```bash
# List VMs
tart list

# Delete and rebuild
tart delete debian-ssh
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
```

### Shared Directory Not Visible

If you started the VM with `--dir=hostshare:...` but can't see files:

```bash
# 1. Verify the virtiofs is supported
ssh debian-dev
cat /proc/filesystems | grep virtiofs
# Should show: nodev	virtiofs

# 2. Check if already mounted
mount | grep virtiofs
findmnt -t virtiofs

# 3. If not mounted — use the fixed Tart tag (NOT the share name)
sudo mkdir -p /mnt/shared
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared

# 4. Your files are in a subdirectory matching the --dir name
ls -la /mnt/shared/hostshare/

# 5. Make permanent with nofail (survives reboot, safe without --dir)
echo "com.apple.virtio-fs.automount /mnt/shared virtiofs defaults,nofail 0 0" | sudo tee -a /etc/fstab
```

**Common issues**:
- **Wrong mount tag**: Do NOT use the share name as the mount device. Tart uses a single fixed tag `com.apple.virtio-fs.automount` for all shares.
- **Missing `--dir` flag**: The `--dir=NAME:PATH` flag must be passed every time you start the VM with `tart run`. Without it, the virtiofs device doesn't exist and mounting fails.
- **`dmesg` shows `tag <name> not found`**: This confirms the wrong tag is being used — switch to `com.apple.virtio-fs.automount`.

---

## Configuration Reference

### ansible/playbook.yml Key Sections

**Software version variables** — customize tool versions without editing tasks:

```yaml
vars:
  java_version: "21"
  java_build: "21.0.5+11"
  gradle_version: "8.12"
  maven_version: "3.9.9"
  graalpy_version: "25.0.2"
```

**Installation tasks** (excerpt):

```yaml
# Java — downloaded from Adoptium, not apt
- name: Download OpenJDK {{ java_version }}
  get_url:
    url: "{{ java_download_url }}"
    dest: "/tmp/openjdk-{{ java_version }}.tar.gz"
  when: not java_bin.stat.exists

# Gradle/Maven verification requires JAVA_HOME
- name: Verify Gradle
  command: "{{ gradle_install_dir }}/gradle-{{ gradle_version }}/bin/gradle --version"
  environment:
    JAVA_HOME: "{{ java_home }}"
    PATH: "{{ ansible_facts['env']['PATH'] }}:{{ java_home }}/bin"

# GraalPy uses dynamic directory detection
- name: Find actual GraalPy directory
  find:
    paths: "{{ graalpy_install_dir }}"
    patterns: "graalpy*"
    file_type: directory
  register: graalpy_dirs
```

**Key implementation details**:

- **Java 21**: Downloaded from Adoptium (Debian repos only have Java 17)
- **Environment variables**: Uses `ansible_facts['env']['PATH']` (not deprecated `ansible_env.PATH`)
- **GraalPy**: Installed before Java, uses `find` module for directory detection
- **Docker**: Official Debian bookworm repository
- **SSH**: Paramiko connection during build, key-only after
- **Idempotency**: All tasks check if already installed, safe to re-run

### debian-ssh.pkr.hcl Key Configuration

```hcl
provisioner "ansible" {
  playbook_file = "./ansible/playbook.yml"
  use_proxy     = false  # Required for paramiko
  
  extra_arguments = [
    "--connection=paramiko",  # Password auth during build
    "--extra-vars", "admin_ssh_key_path=${var.ssh_key_path}"
  ]
}
```

---

## Project Files

- `debian-ssh.pkr.hcl` — Packer template (plugins, source, provisioners)
- `vars.pkrvars.hcl` — Variable values (VM name, SSH credentials)
- `ansible/playbook.yml` — Complete provisioning playbook
- `ansible/roles/ssh-setup/` — SSH hardening role (sshd_config)

See source files for complete implementation details
