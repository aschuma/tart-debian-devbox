# Packer + Tart + Ansible: Debian 12 Dev VM

Automated Debian 12 (Bookworm) development VM with Docker, Java 21, Node.js LTS, build tools, Atlassian CLI, GitHub Copilot CLI, and SSH key authentication.

- [Tart](https://tart.run/) — virtual machine manager for Apple Silicon
- [Cirrus Labs Debian Base Image](https://github.com/cirruslabs/linux-image-templates) — upstream Tart VM images
- [Packer](https://www.packer.io/) — automated machine image builds
- [Packer Tart Plugin](https://github.com/cirruslabs/packer-plugin-tart) — Tart builder for Packer
- [Ansible](https://docs.ansible.com/) — infrastructure automation
- [Eclipse Temurin](https://adoptium.net/) — OpenJDK distribution
- [GraalPy](https://github.com/oracle/graalpython) — GraalVM-based Python implementation

## Disclaimer

This project includes content generated with the assistance of artificial intelligence tools. Significant portions of the code, documentation, or other materials may have been created or refined using AI. While efforts have been made to review and validate all outputs, the accuracy and correctness of AI-generated content cannot be guaranteed. Users are encouraged to review and verify the code before use.


## What Gets Built

- **Base**: Debian 12 Bookworm (ARM64) from `ghcr.io/cirruslabs/debian:bookworm`
- **Users**: `admin` + `deploy` (both with SSH key auth, sudo access)
- **Docker**: Docker CE + Compose plugin (latest from official Debian repo)
- **Java**: OpenJDK (Adoptium/Temurin) at `/opt/jdk` — version configured via `java_build` variable
- **Build Tools**: Gradle + Maven — versions configured via `gradle_version` and `maven_version` variables
- **Python**: GraalPy — version configured via `graalpy_version` variable
- **Node.js**: nvm + Node.js LTS + npx — installed system-wide at `/opt/nvm`; version configured via `nvm_version` and `node_version` variables
- **Atlassian CLI**: `acli` — installed from the official Atlassian apt repository
- **GitHub Copilot CLI**: `gh copilot` — installed via npm (`@github/copilot`)
- **opencode**: `opencode` — installed via npm (`opencode-ai`)
- **CLI Tools**: git, curl, vim, jq, htop, tree, unzip, build-essential

> **Customize versions**: Edit `vars/main.yml` inside each role under `build/ansible/roles/` (see [Software Versions](#software-versions) section)

## Key Concepts

**Packer** orchestrates the build: clones base image → boots VM → runs shell + Ansible → saves image

**Ansible** provisions via **paramiko** (password auth during build) then switches to **key-only** SSH

**Variables** control versions, paths, and SSH credentials — all customizable in `build/vars.pkrvars.hcl`

**Install order**: ssh-setup → common-tools → docker → java → gradle → maven → graalpy → nvm → acli → copilot-cli → opencode

**Role structure**: Each tool lives in its own role under `build/ansible/roles/`, with `tasks/main.yml` (what to do) and `vars/main.yml` (version/path variables). Roles are self-contained — they download, install, configure, verify, and clean up.

**Directory sharing**: Linux VMs use virtiofs — all shares are exposed under a single mount tag (`com.apple.virtio-fs.automount`) and are not auto-mounted like macOS guests. `tart-ctl.sh` handles starting the VM with the share attached and mounting it inside the VM via the `provision` command.

---

## Build the VM

```bash
# 1. Prerequisites (macOS)
brew install hashicorp/tap/packer ansible

# 2. Build (run from project root)
./build/provision.sh
```

Or run the steps manually from the `build/` directory:

```bash
cd build/
packer init debian-ssh.pkr.hcl
packer validate -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
```

Build takes ~10-15 minutes. Result: local Tart image named `debian-ssh`

---

## Reprovision a Running VM

The Ansible playbook runs independently of Packer and can be applied directly to any running VM. All roles are idempotent — already-installed tools are skipped.

```bash
# Run all roles against the running VM
cd build/ansible
ansible-playbook playbook.yml \
  -i "$(tart ip debian-ssh)," \
  --private-key ~/.ssh/id_ed25519_tart \
  -u admin \
  --become \
  --extra-vars "ansible_user=admin ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
```

To apply only specific roles, use `--tags`:

```bash
# Install only nvm and acli on an existing VM
ansible-playbook playbook.yml \
  -i "$(tart ip debian-ssh)," \
  --private-key ~/.ssh/id_ed25519_tart \
  -u admin \
  --become \
  --tags nvm,acli \
  --extra-vars "ansible_user=admin ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
```

Available tags: `ssh`, `tools`, `docker`, `java`, `gradle`, `maven`, `graalpy`, `nvm`, `acli`, `copilot-cli`, `opencode`

> **Note**: `StrictHostKeyChecking=no` is required because the VM gets a new IP (and thus an unknown host key) each time it starts.

---

## Customize via Variables

### VM Resources & Identity

Edit `build/vars.pkrvars.hcl`:

```hcl
vm_name      = "my-dev-vm"              # Change final image name
ssh_user     = "admin"                  # Build-time SSH user
ssh_password = "admin"                  # Build-time password
ssh_key_path = "~/.ssh/id_ed25519.pub" # Your public key to inject
```

Edit `build/debian-ssh.pkr.hcl` source block for resources:

```hcl
source "tart-cli" "debian" {
  cpu_count    = 4      # Increase CPU cores
  memory_gb    = 8      # Increase RAM
  # ...
}
```

### Software Versions

Each tool's version variables live in its own role. Edit the relevant `vars/main.yml`:

| Tool | File |
|------|------|
| Java | `build/ansible/roles/java/vars/main.yml` |
| Gradle | `build/ansible/roles/gradle/vars/main.yml` |
| Maven | `build/ansible/roles/maven/vars/main.yml` |
| GraalPy | `build/ansible/roles/graalpy/vars/main.yml` |
| Docker | `build/ansible/roles/docker/vars/main.yml` |
| nvm / Node.js | `build/ansible/roles/nvm/vars/main.yml` |
| Atlassian CLI | `build/ansible/roles/acli/vars/main.yml` |

Example — bump Java version in `roles/java/vars/main.yml`:

```yaml
java_version: "21"
java_build: "21.0.5+11"
java_install_dir: /opt/jdk
java_home: /opt/jdk/jdk-21.0.5+11
java_download_url: "https://github.com/adoptium/temurin21-binaries/..."
```

Change versions, re-run `packer build` — idempotent (skips if already installed)

---

## After the Build: Clone Your Image

Once the Packer build completes you have a local Tart image named `debian-ssh` (or whatever `vm_name` is set to in `build/vars.pkrvars.hcl`). Rather than rebuilding from scratch for each project or environment, clone the image — this is near-instant compared to the ~10-15 minute build:

```bash
tart clone debian-ssh debian-foo
```

You can then use `debian-foo` as the base for your day-to-day work, keeping the original `debian-ssh` image untouched as a clean baseline to clone from again at any time.

---

## VM Instance Control (`tart-ctl`)

The `bin_template/` folder contains a control script and configuration templates that simplify day-to-day VM management. This is the recommended way to start, stop, provision, and SSH into your VM.

### Setup

**1. Install the script** — move `tart-ctl.sh` to any directory on your `$PATH`:

```bash
mv bin_template/tart-ctl.sh ~/.local/bin/tart-ctl.sh
chmod +x ~/.local/bin/tart-ctl.sh
```

**2. Add config to your project** — copy the environment file and provision scripts to the root of the Git repository you want to work on inside the VM:

```bash
cp bin_template/.tart-ctl-env   ~/Projects/my-project/
cp bin_template/.env            ~/Projects/my-project/
cp -r bin_template/.tart-ctl-provision.d/ ~/Projects/my-project/
```

> Both `.tart-ctl-env` and `.tart-ctl-provision.d/` should be committed to your project's Git repository so all contributors share the same VM configuration.

### Configuration

Edit `.tart-ctl-env` in your project root. The key variables to configure per project are:

| Variable | Description |
|---|---|
| `TCTL_VM_NAME` | Name of the Tart VM to manage (matches the image name built by Packer) |
| `TCTL_HOST_SHARE_DIR` | Host directory mounted into the running VM via virtiofs |
| `TCTL_IDENTITY_FILE` | Path to your SSH private key — **keep unchanged**; also used during image creation |

```bash
# .tart-ctl-env (example)
TCTL_VM_NAME="my-dev-vm"
TCTL_HOST_SHARE_DIR="/Users/you/Projects/my-project/"
TCTL_IDENTITY_FILE="${HOME}/.ssh/id_ed25519_tart"   # do not change
```

> **`TCTL_IDENTITY_FILE`** must match the key injected during the Packer build (`ssh_key_path` in `build/vars.pkrvars.hcl`). Changing it here without rebuilding the image will break SSH access.

The optional **`.env`** file (plain `KEY=VALUE` pairs) in the same directory is picked up by the `provision` command and pushed into the VM's `/etc/profile.d/` so those variables are available in every login shell inside the VM.

### Usage

`tart-ctl.sh` automatically finds its configuration by walking up from the current directory until it finds `.tart-ctl-env` or reaches the Git root. You can run it from any subdirectory of your project.

| Command | Aliases | Description |
|---|---|---|
| `tart-ctl.sh start` | `up` | Start the VM (with host share if configured) |
| `tart-ctl.sh stop` | `down` | Stop the running VM |
| `tart-ctl.sh status` | `st` | Show whether the VM is running or stopped |
| `tart-ctl.sh provision` | `pr` | Run all scripts in `.tart-ctl-provision.d/` |
| `tart-ctl.sh ssh` | `sh` | Open an interactive SSH shell in the VM |
| `tart-ctl.sh ip` | | Print the VM's current IP address |
| `tart-ctl.sh update-ssh-config` | `usc` | Update `~/.ssh/config` with the VM's current IP |

**Common workflow:**

```bash
cd ~/Projects/my-project   # or any subdirectory

tart-ctl.sh up             # start the VM
tart-ctl.sh provision      # mount share + push .env into the VM
tart-ctl.sh ssh            # open a shell inside the VM

tart-ctl.sh down           # shut down when done
```

### Provisioning Steps (`.tart-ctl-provision.d/`)

The `provision` command executes all scripts in `.tart-ctl-provision.d/` in alphabetical order. The bundled steps are:

| Script | What it does |
|---|---|
| `10_mount-share.sh` | Mounts the host virtiofs share at `/mnt/shared` inside the VM |
| `20_init-env.sh` | Pushes `.env` into the VM as `/etc/profile.d/tart-provision.sh` |

Add your own numbered scripts to extend provisioning. All `TCTL_*` variables from `.tart-ctl-env` are available as environment variables inside each script.

---

## Using the VM

Once the VM is running and provisioned (`tart-ctl.sh up && tart-ctl.sh provision`), connect with `tart-ctl.sh ssh`. A few things to be aware of on first use:

### Docker Requires a Re-login

Docker group membership only takes effect after a fresh login. On first connect you may see a permission error:

```bash
# First login — docker group not yet active
docker ps  # permission denied

# Log out and back in (or reboot)
logout
tart-ctl.sh ssh

# Now docker works
docker ps
docker compose version
```

---

## IntelliJ Remote Development

### Option 1: SSH Gateway (Recommended)

1. Run `tart-ctl.sh update-ssh-config` to write the current VM IP into `~/.ssh/config`
2. **IntelliJ IDEA** → **File** → **Remote Development** → **SSH**
3. **New Connection**:
   - Host: VM name from `TCTL_VM_NAME` (resolved via `~/.ssh/config`)
   - Port: `22`
   - User: `admin` or `deploy`
   - Authentication: **Key Pair**
   - Private key: `~/.ssh/id_ed25519_tart`
4. **IDE Version**: Select latest available
5. **Project Directory**: Choose project on VM or create new
6. Click **Connect** — IntelliJ downloads IDE backend to VM, opens remote session

**Benefits**:
- Full IDE runs locally, only project files on VM
- Low latency, UI feels native
- VM resources (Java, Maven, Gradle, Docker) available to IDE

### Option 2: Mount Project via Tart Share

Set `TCTL_HOST_SHARE_DIR` in `.tart-ctl-env` to your local project path, then:

```bash
tart-ctl.sh up          # starts VM with the host share attached
tart-ctl.sh provision   # mounts the share at /mnt/shared inside the VM
tart-ctl.sh ssh         # open a shell — project files are at /mnt/shared/
```

Then configure IntelliJ to use remote SDK:

1. Run `tart-ctl.sh update-ssh-config` so the VM name resolves in `~/.ssh/config`
2. **File** → **Project Structure** → **SDKs** → **+** → **Add SSH SDK**
3. Configure SSH using the VM name from `TCTL_VM_NAME`, user `admin`
4. Point to `/opt/jdk/jdk-21.0.5+11` on the VM

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
./build/provision.sh
```

### Shared Directory Not Visible

If `tart-ctl.sh provision` runs `10_mount-share.sh` but files aren't visible inside the VM:

```bash
# 1. Verify virtiofs support
ssh debian-dev
cat /proc/filesystems | grep virtiofs
# Should show: nodev	virtiofs

# 2. Check if already mounted
mount | grep virtiofs
findmnt -t virtiofs

# 3. If not mounted — use the fixed Tart tag (NOT the share name)
sudo mkdir -p /mnt/shared
sudo mount -t virtiofs com.apple.virtio-fs.automount /mnt/shared

# 4. Your files are in a subdirectory matching the share directory name
ls -la /mnt/shared/
```

**Common issues**:
- **VM not started with a share**: `tart-ctl.sh start` only passes `--dir` when `TCTL_HOST_SHARE_DIR` is set in `.tart-ctl-env`. Verify the variable is set and the path exists on the host.
- **Wrong mount tag**: Tart uses a single fixed tag `com.apple.virtio-fs.automount` for all shares — do not use the share name as the mount device.
- **`dmesg` shows `tag <name> not found`**: Confirms the wrong tag is being used — switch to `com.apple.virtio-fs.automount`.

---

## Configuration Reference

### build/ansible/playbook.yml Key Sections

The playbook is pure orchestration — no vars, no inline tasks. It runs roles in order:

```yaml
roles:
  - { role: ssh-setup,     tags: [ssh]         }
  - { role: common-tools,  tags: [tools]       }
  - { role: docker,        tags: [docker]      }
  - { role: java,          tags: [java]        }
  - { role: gradle,        tags: [gradle]      }
  - { role: maven,         tags: [maven]       }
  - { role: graalpy,       tags: [graalpy]     }
  - { role: nvm,           tags: [nvm]         }
  - { role: acli,          tags: [acli]        }
  - { role: copilot-cli,   tags: [copilot-cli] }
  - { role: opencode,      tags: [opencode]    }
```

Run a single role with `--tags`, e.g. `packer build` → `ansible-playbook --tags docker`.

**Role task structure** (example — java role):

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

### build/debian-ssh.pkr.hcl Key Configuration

```hcl
provisioner "ansible" {
  playbook_file = "./ansible/playbook.yml"
  use_proxy     = false  # Required for paramiko
  
  extra_arguments = [
    "--connection=paramiko",  # Password auth during build
    "--extra-vars", "ssh_setup_admin_ssh_key_path=${var.ssh_key_path}"
  ]
}
```

---

## Project Structure

```
.
├── build/                          # Image creation (Packer + Ansible)
│   ├── debian-ssh.pkr.hcl          # Packer template
│   │                               #   (plugins, source, provisioners)
│   ├── vars.pkrvars.hcl            # Variable values
│   │                               #   (VM name, SSH credentials)
│   ├── provision.sh                # One-command build script
│   └── ansible/                    # Ansible provisioning
│       ├── playbook.yml            # Orchestration only
│       │                           #   (no vars, no inline tasks)
│       └── roles/
│           ├── ssh-setup/          # SSH install, user setup,
│           │   |                   #   key injection, daemon hardening
│           │   ├── tasks/main.yml
│           │   └── files/sshd_config
│           ├── common-tools/       # Base CLI packages + git config
│           │   └── tasks/main.yml
│           ├── docker/             # Docker CE + Compose plugin
│           │   ├── tasks/main.yml
│           │   └── vars/main.yml   # arch
│           ├── java/               # Adoptium/Temurin JDK
│           │   ├── tasks/main.yml
│           │   └── vars/main.yml   # java_version, java_build,
│           |                       #   java_home, …
│           ├── gradle/             # Gradle build tool
│           │   ├── tasks/main.yml
│           │   └── vars/main.yml   # gradle_version, gradle_install_dir, …
│           ├── maven/              # Apache Maven
│           │   ├── tasks/main.yml
│           │   └── vars/main.yml   # maven_version, maven_install_dir, …
│           ├── graalpy/            # GraalVM Python
│           │   ├── tasks/main.yml
│           │   └── vars/main.yml   # graalpy_version, graalpy_install_dir, …
|           └── ...
├── bin_template/                   # VM instance control (copy to your project)
│   ├── tart-ctl.sh                 # Control script — install to $PATH
│   ├── .tart-ctl-env               # Per-project config (VM name, share dir, …)
│   ├── .env                        # Project env vars pushed into the VM
│   └── .tart-ctl-provision.d/      # Provisioning steps run by `tart-ctl.sh provision`
│       ├── 10_mount-share.sh       # Mount host virtiofs share at /mnt/shared
│       └── 20_init-env.sh          # Push .env into VM's /etc/profile.d/
└── README.md
```
