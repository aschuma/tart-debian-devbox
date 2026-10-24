> **New here?** Follow [QUICKSTART.md](QUICKSTART.md) — prerequisites, SSH key, building the template image, cloning it, `tctl init`, provisioning, SSH, start/stop. This README is the reference.

# Packer + Tart + Ansible: Debian 12 Dev VM

Automated Debian 12 (Bookworm, ARM64) development VM for Apple Silicon Macs. A single `devbox` user, Docker, and a [mise](https://mise.jdx.dev)-managed toolchain (Java 25, Node.js LTS, Gradle, Maven, Kotlin, Python 3.13, GraalPy, gh, aws-cli, terraform, packer, uv, ruff, acli, GitHub Copilot CLI, Codex CLI, OpenCode) in a zsh shell with a [starship](https://starship.rs) prompt.

- [Tart](https://tart.run/) — virtual machine manager for Apple Silicon
- [Cirrus Labs Debian Base Image](https://github.com/cirruslabs/linux-image-templates) — upstream Tart VM images
- [Packer](https://www.packer.io/) — automated machine image builds
- [Packer Tart Plugin](https://github.com/cirruslabs/packer-plugin-tart) — Tart builder for Packer
- [Ansible](https://docs.ansible.com/) — infrastructure automation
- [mise](https://mise.jdx.dev/) — polyglot tool version manager (replaces nvm, SDKMAN, manual tarballs)
- [Eclipse Temurin](https://adoptium.net/) — OpenJDK distribution (installed by mise)
- [GraalPy](https://github.com/oracle/graalpython) — GraalVM-based Python implementation (installed by mise)
- [starship](https://starship.rs/) — cross-shell prompt (installed by mise)

## Disclaimer

This project includes content generated with the assistance of artificial intelligence tools. Significant portions of the code, documentation, or other materials may have been created or refined using AI. While efforts have been made to review and validate all outputs, the accuracy and correctness of AI-generated content cannot be guaranteed. Users are encouraged to review and verify the code before use.

---

## Architecture

```
┌──────────────────────── host (macOS) ──────────────────────----──┐
│  packer build ──► tart clone base ──► ansible-playbook ──► image │
│  justfile / tctl                   (paramiko, user admin)        │
└────────────────────────────────────────────────────────────----──┘
                                   │
┌──────────────────────── VM (Debian 12 arm64) ───────────────--─┐
│  users          devbox  (zsh, ssh key, NOPASSWD sudo, docker)  │
│  apt            base CLI tools, Docker CE, mise binary         │
│  mise (devbox)  ~/.config/mise/config.toml  ◄── tool_versions  │
│                 ~/.local/share/mise/installs/<tool>/<version>  │
└──────────────────────────────────────────────────────────────--┘
```

**Packer** clones the base image, boots it, and runs the Ansible playbook over SSH (password auth as `admin` via paramiko). The result is a local Tart image (default name `debian-ssh`) that serves as a **template**: clone it per project (`just clone <name>`) and work in the clones.

**Ansible** is pure orchestration — `playbook.yml` runs these roles in order:

| Role           | Tag            | What it does                                                                                                                                      |
| -------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `user`         | `user`         | Creates `devbox` (sudo, docker, SSH key), hardens `sshd`                                                                                           |
| `common_tools` | `common-tools` | Base apt packages (git, curl, jq, build-essential, …), git defaults for `devbox`                                                                  |
| `docker`       | `docker`       | Docker CE + Buildx + Compose plugin from the official Docker apt repo                                                                             |
| `mise`         | `mise`         | Installs mise from its apt repo, shell activation, generates the global mise config                                                               |
| `runtimes`     | `runtimes`     | `mise install` of Java, Node.js, Gradle, Maven, Kotlin, Python, GraalPy                                                                           |
| `cli_tools`    | `cli-tools`    | `mise install` of gh, aws-cli, terraform, packer, uv, ruff, acli, starship, copilot, codex, opencode                                              |
| `shell`        | `shell`        | zsh as login shell of `devbox`, `~/.zshenv`/`~/.zshrc`, starship prompt, bash + zsh completions for mise/gh/docker/uv/ruff/starship               |
| `verify`       | `verify`       | Final gate: `mise doctor`, every `<tool> --version`, zsh/starship wiring, docker, sudo, group membership — **the build fails if any check fails** |

**Package sources**, in order of preference:

1. **mise** — everything that is a developer tool with a mise registry entry
2. **apt** — base OS tools, Docker, and the mise binary itself
3. no tool is downloaded by hand-written tarball logic

---

## Users

There is exactly one developer account:

| User     | Purpose                                                             | Auth                                                                 | sudo         |
| -------- | ------------------------------------------------------------------- | -------------------------------------------------------------------- | ------------ |
| `devbox` | Day-to-day work, SSH target, owner of all mise tools                | SSH key (`~/.ssh/id_ed25519_tart`)                                   | passwordless |
| `admin`  | Base-image account used by Packer/Ansible **during the build only** | password (`admin`) — password SSH login is disabled by `sshd_config` | yes          |

```bash
id devbox        # uid=1001(devbox) gid=1001(devbox) groups=devbox,sudo,docker
sudo -l          # (ALL) NOPASSWD: ALL
docker ps        # works without sudo
```

> Docker group membership is baked into the image, so there is no "log out and back in" dance on a fresh clone.

---

## Installed Tools

| Category      | Tools                                                                                       | Source                |
| ------------- | ------------------------------------------------------------------------------------------- | --------------------- |
| Runtimes      | Java (Temurin 25), Node.js (LTS, with npm/npx), Kotlin                                      | mise                  |
| Build tools   | Gradle, Maven                                                                               | mise                  |
| Python        | CPython 3.13 (`python`, `pip`), GraalPy (community), uv, ruff                               | mise                  |
| Cloud / infra | aws-cli, terraform, packer                                                                  | mise                  |
| Dev CLIs      | gh (GitHub CLI), acli (Atlassian CLI)                                                       | mise                  |
| Shell         | zsh (login shell), starship prompt                                                          | apt / mise            |
| AI assistants | GitHub Copilot CLI (`copilot`), Codex CLI (`codex`), OpenCode (`opencode`)                  | mise                  |
| Containers    | Docker CE, Buildx, Compose plugin                                                           | apt (docker.com repo) |
| Base          | git, curl, wget, jq, vim, htop, tree, unzip, zip, rsync, build-essential, postgresql-client | apt                   |

All mise tools are installed **per user** under `/home/devbox/.local/share/mise/` and exposed through mise's PATH management (see [Shell, Prompt & Non-Interactive Commands](#shell-prompt--non-interactive-commands)).

---

## Version Management

All versions live in **one file**:

```yaml
# build/ansible/group_vars/all/tool_versions.yml
mise_runtimes:
  java: 'temurin-25'
  node: 'lts'
  gradle: 'latest'
  maven: 'latest'
  kotlin: 'latest'
  python: '3.13'
  'github:oracle/graalpython':
    version: 'latest'
    version_prefix: 'graal-'
    asset_pattern: 'graalpy*-community-{version}-linux-aarch64.tar.gz'

mise_cli_tools:
  gh: 'latest'
  aws-cli: 'latest'
  terraform: 'latest'
  packer: 'latest'
  uv: 'latest'
  ruff: 'latest'
  acli: 'latest'
  starship: 'latest'
  copilot: 'latest'
  codex: 'latest'
  opencode: 'latest'
```

The `mise` role renders both dicts into the devbox user's global config:

```toml
# /home/devbox/.config/mise/config.toml   (generated — do not edit by hand on the VM)
[settings]
trusted_config_paths = ["/mnt/shared", "/home/devbox"]

[tools]
"java" = "temurin-25"
"node" = "lts"
...
```

Values are mise version specs: an exact version (`"3.9.9"`), a prefix (`"21"`, `"temurin-25"`), an alias (`"lts"`), or `"latest"`. A mapping with `version` plus options is used for tools that need backend options (like GraalPy's GitHub release asset pattern).

### Customizing Tools

1. **Change a version** — edit `tool_versions.yml`, then rebuild (`just build`) or reprovision the running VM:
   ```bash
   just reprovision mise,runtimes      # or mise,cli-tools
   ```
2. **Add a tool** — find its short name with `mise registry | grep <name>` and add it to `mise_cli_tools` (or `mise_runtimes`). Add a `<tool> --version` line to the matching `*_verify_commands` list in `roles/<role>/defaults/main.yml` so the verify gate covers it.
3. **Remove a tool** — delete it from the dict and its verify line. On an existing VM, run `mise uninstall <tool>` as `devbox` (reprovisioning only installs; it never removes).
4. **Non-mise tools** — add an apt package to `roles/common_tools/tasks/main.yml`, or create a new role if it needs its own repo/keyring (see `roles/docker` for the pattern).

### Working with mise inside the VM

```bash
mise ls                      # installed tools and which config requested them
mise ls --missing            # tools requested by config but not installed
mise install                 # install everything the active configs request
mise install java@21         # install an additional version
mise use -g node@22          # change the global default (writes ~/.config/mise/config.toml)
mise upgrade                 # bump tools pinned to "latest"/prefixes to the newest matching version
mise outdated                # show what `mise upgrade` would do
mise doctor                  # diagnose activation / PATH problems
mise registry | grep rust    # discover installable tools
```

> Changes made with `mise use -g` on the VM are **not** reflected back into `tool_versions.yml`. Treat the VM as disposable: put permanent changes into the Ansible variables.

### Project-level `.mise.toml`

mise reads `mise.toml` / `.mise.toml` from the current directory upwards, so each project can pin its own toolchain, overriding the global defaults:

```toml
# ~/projects/lh-service/mise.toml
[tools]
java = "temurin-21"
node = "22"
gradle = "8.14"
```

```bash
cd ~/projects/lh-service
mise install          # fetch the pinned versions once
java --version        # → 21.x while inside this directory, 25.x elsewhere
```

Config files under `/mnt/shared` (the virtiofs host share) and `/home/devbox` are **pre-trusted**; anywhere else mise asks you to run `mise trust` first. Commit `mise.toml` to the project so the VM and your teammates' machines agree on versions.

---

## Build the Template Image

> The build produces a **template**: a clean, fully provisioned Tart image that you should not work in directly. Clone it once per project (`just clone <name>`, see [After the Build](#after-the-build-clone-your-image)) and drive the clones with `tctl`. Rebuilding the template never touches your clones.

```bash
# 1. Prerequisites (macOS)
brew install hashicorp/tap/packer ansible just   # just is optional

# 2. Build (run from project root) — ~10-15 minutes
just build          # or: ./build/provision.sh
```

Or run the steps manually from the `build/` directory:

```bash
cd build/
packer init debian-ssh.pkr.hcl
packer validate -var-file="vars.pkrvars.hcl" -var vm_name=debian-ssh debian-ssh.pkr.hcl
packer build -var-file="vars.pkrvars.hcl" -var vm_name=debian-ssh debian-ssh.pkr.hcl
```

Result: a local Tart image named after `BUILD_VM_NAME` in `build/build.env` (`debian-ssh` by default). To build under a different name without touching the existing template:

```bash
cd build && packer build -var-file=vars.pkrvars.hcl -var vm_name=debian-ssh-test debian-ssh.pkr.hcl
```

### Build-time configuration

The build has its own configuration, independent of the `tctl` config in `~/.config/tctl/`.

`build/build.env` — used by `provision.sh` and every `just` recipe:

```bash
BUILD_VM_NAME="debian-ssh"                        # Name of the template image (passed to Packer as vm_name)
BUILD_SSH_USER="devbox"                           # Account `just reprovision` / `just tools` connect as
BUILD_IDENTITY_FILE="${HOME}/.ssh/id_ed25519_tart" # Private key matching ssh_key_path below
```

`build/vars.pkrvars.hcl` — Packer inputs:

```hcl
ssh_user     = "admin"                       # Base-image account used by Packer (do not change)
ssh_password = "admin"                       # Its password
ssh_key_path = "~/.ssh/id_ed25519_tart.pub"  # Public key injected for the devbox user
```

VM resources (CPU/RAM) are set in the `source "tart-cli"` block of `build/debian-ssh.pkr.hcl`.

---

## Reprovisioning

The playbook is idempotent and can be applied to any running VM built from this image. Already-installed tools are skipped; changed versions are installed.

`just reprovision` and `just tools` target the **template** VM named in `build/build.env`. They go through `build/with-vm.sh`, which starts the template headless if it is stopped, waits for an IP and SSH, runs the command, and shuts the VM down again — a VM that was already running is left running. To apply the playbook to a clone, use the manual command below with the clone's name in `tart ip`.

```bash
# everything
just reprovision

# single roles
just reprovision mise,runtimes
just reprovision verify
```

Without `just`:

```bash
cd build/ansible
ansible-playbook playbook.yml \
  -i "$(tart ip debian-ssh)," \
  --private-key ~/.ssh/id_ed25519_tart \
  -u devbox --become \
  --tags mise,runtimes \
  --extra-vars "ansible_user=devbox ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'"
```

Available tags: `user`, `common-tools`, `docker`, `mise`, `runtimes`, `cli-tools`, `verify`

> `StrictHostKeyChecking=no` is required because the VM gets a new IP (and thus an unknown host key) each time it starts. When connecting manually, add `-o IdentitiesOnly=yes` if your ssh-agent holds several keys — `sshd` allows only 3 authentication attempts.

---

## Lifecycle Management

| Task                           | Command                                                                    |
| ------------------------------ | -------------------------------------------------------------------------- |
| Build template image           | `just build`                                                               |
| Install `tctl`                 | `just install` (binary → `~/.cargo/bin`, config → `~/.config/tctl/config.toml`) |
| Clone template for a project   | `just clone my-project` (→ `tart clone debian-ssh my-project`)             |
| Mark a project directory       | `tctl init` (writes `.tart-ctl.toml`) |
| Start / stop / status          | `tctl up`, `tctl down`, `tctl status` |
| Push env files into the VM     | `tctl provision` |
| Shell                          | `tctl ssh` |
| Update `~/.ssh/config`         | `tctl update-ssh-config` |
| List tools in the template VM  | `just tools` (→ `mise ls`; starts/stops the VM as needed)                  |
| Upgrade tools in the template  | `just reprovision mise,runtimes,cli-tools` (then re-clone) or `mise upgrade` inside a VM |
| Validate templates             | `just validate`                                                            |
| Lint Ansible/YAML              | `just lint` (also runs in GitHub Actions)                                  |
| Delete template image          | `just clean` (→ `tart delete`; clones are unaffected)                      |

The `justfile` covers the **template image** — build, clone, maintenance (`build/provision.sh`, `ansible-playbook`, `tart clone`, `tart delete`); it reads its settings from `build/build.env` and every recipe can be run by hand. Day-to-day **VM control** of your clones (start/stop/provision/ssh) is the job of `tctl`, installed once with `just install`, configured in `~/.config/tctl/config.toml`, and usable from any project directory (see [VM Instance Control](#vm-instance-control-tart-ctl)).

---

## After the Build: Clone Your Image

Once the Packer build completes you have a local Tart image named `debian-ssh` (or whatever `BUILD_VM_NAME` is set to in `build/build.env`). Treat it as a **template**: do not start it for daily work. Instead clone it per project — near-instant compared to the ~10-15 minute build — and keep the template untouched as a clean baseline:

```bash
just clone my-project            # → tart clone debian-ssh my-project
```

Then point the project at the clone and start it with `tctl`:

```bash
cd ~/Projects/my-project
tctl init                 # asks for VM name and share directory → writes .tart-ctl.toml
tctl up
```

When you later rebuild or reprovision the template, existing clones are not affected — delete a clone (`tart delete my-project`) and re-clone to pick up the changes.

---

## VM Instance Control (`tart-ctl`)

The command is now `tctl`; the section anchor is kept for existing links.

The Rust crate in `tctl/` provides a synchronous VM-control library and the `tctl` CLI. No daemon is needed.

### Setup

**1. Install the CLI and global config** — requires a stable Rust toolchain, Tart ≥ 2.32, and SSH:

```bash
brew install rust just
just install                         # cargo install --path tctl --locked + TOML defaults
export PATH="$HOME/.cargo/bin:$PATH"
tctl --help
```

The binary is installed under the Cargo install root (`CARGO_INSTALL_ROOT`, else `CARGO_HOME`, else `~/.cargo`). Existing TOML config is never overwritten.

Tags matching `tctl-v*` trigger an Apple Silicon release archive (`tctl-aarch64-apple-darwin.tar.gz`) and SHA-256 checksum. The workflow is configured but has not yet been run; binaries are not signed/notarised.

**2. Mark your project** — `init` asks for missing flags only when stdin is a terminal. It warns if the VM is not a local clone yet:

```bash
cd ~/Projects/my-project
tctl init                          # interactive
# Or script it: tctl init --vm my-project --share auto
echo 'MY_VAR=value' > .env          # optional: env vars pushed by provision
```

This writes `.tart-ctl.toml` and registers the VM. Existing files require `--force` or interactive confirmation. `[vm].name` is **required in the project file**; global defaults never select a VM.

### Configuration

Settings are layered **per key**, not per section: project values override global defaults. Lists are replaced, not appended; `[]` explicitly disables an inherited list.

| File | Purpose |
| --- | --- |
| `~/.config/tctl/config.toml` | Global defaults (template: `config_template/config.toml`; honours `XDG_CONFIG_HOME`) |
| `<project>/.tart-ctl.toml` | Project marker, found by walking upward; must set `[vm].name` |

| TOML key | Description |
| --- | --- |
| `[vm].name` | Local Tart VM name (letters, digits, `.`, `_`, `-`; no leading `-`, `.` or `..` names) |
| `[share].dir` | `"auto"` shares the project root at `/mnt/shared/workspace`; `"none"` disables sharing; otherwise a host path |
| `[provision].env_files` | Ordered array of env files, default `[".env"]`; later keys win, unreadable files are skipped |
| `[ssh].user` | SSH user, normally `"devbox"` |
| `[ssh].config_path` | SSH client config maintained by `update-ssh-config`, normally `"~/.ssh/config"` |
| `[ssh].identity_file` | Private key matching the public key injected during the Packer build |
| `[ssh].remote_forward` | Array of `"PORT HOST:PORT"` RemoteForward entries (VM → host); template default `["1080 127.0.0.1:1080"]` |
| `[ssh].local_forward` | Array of LocalForward entries (host → VM); template default `[]` |

Relative paths resolve from the project root, not the invoking subdirectory. `~`, `$HOME` and `${HOME}` expand in path values; arbitrary shell is never evaluated.

```toml
# ~/Projects/my-project/.tart-ctl.toml
[vm]
name = "my-project"
[share]
dir = "auto"
[provision]
env_files = [".env", ".env.local"]
[ssh]
local_forward = ["5432 127.0.0.1:5432", "15672 127.0.0.1:15672"]
```

**Port forwards** are plain ssh `RemoteForward`/`LocalForward` entries that `update-ssh-config` keeps in the VM's `Host` block (it replaces the previously managed lines, so removing an entry from the config removes it from `~/.ssh/config` on the next run). They are active for the lifetime of an `ssh my-project` session (or `tctl ssh`); use `ssh -N my-project` for a tunnel-only connection. Note that services inside the VM bound to `0.0.0.0` — Docker's `-p` publishes this way by default — are reachable directly at `http://$(tctl --quiet ip):PORT` without any forward; `[ssh].local_forward` is for services that only listen on the VM's loopback.

> **`[ssh].identity_file`** must match the key injected during the Packer build (`ssh_key_path` in `build/vars.pkrvars.hcl`, private half in `BUILD_IDENTITY_FILE` of `build/build.env`). Changing it here without rebuilding the image will break SSH access.

The files listed in **`[provision].env_files`** (plain `KEY=VALUE` pairs; by default just `.env` next to `.tart-ctl.toml`) are pushed by the `provision` command into the VM's `/etc/profile.d/` so those variables are available in every login shell inside the VM. They are **layered** like in dotenv-flow, Vite or Symfony: later files override only the keys they define and do not need to repeat the rest — e.g. `[".env", ".env.local"]` for committed defaults plus gitignored personal overrides, or `["~/.config/tctl/common.env", ".env"]` for host-wide variables shared by every project.

Env files only need to be *readable* — they may be symlinks, FIFOs, or virtual files provided by a secrets manager (e.g. 1Password Environments); each is streamed exactly once and never stat'ed or re-read.

### Usage

`tctl` loads `~/.config/tctl/config.toml`, then walks upward for `.tart-ctl.toml`. It works from any project subdirectory and stops with a clear error for a missing marker, missing VM name, or invalid TOML. Use `tctl init` to create a new marker.

| Command                         | Aliases | Description                                     |
| ------------------------------- | ------- | ----------------------------------------------- |
| `tctl init`                    |         | Create `.tart-ctl.toml`; interactive or `--vm NAME --share auto\|none\|PATH` |
| `tctl start`                   | `up`    | Detach with logs; wait up to 60 s for IP (`--wait SECONDS`, 0 skips waiting) |
| `tctl stop`                    | `down`  | Stop the running VM, even when it has no IP |
| `tctl status`                  | `st`    | Show VM state and current IP |
| `tctl provision`               | `pr`    | Push env layers into `/etc/profile.d/tart-provision.sh` |
| `tctl env [--diff]`             |         | Preview env; diff exits 1 on drift, secrets masked by default |
| `tctl ssh [-- CMD…]`            | `sh`    | Exec SSH, preserving terminal, signals and exit code |
| `tctl ip`                      |         | Print the current IP (`--quiet ip` suppresses config preamble) |
| `tctl update-ssh-config`        | `usc`   | Update the managed SSH Host block |
| `tctl registry list\|show NAME\|scan DIR` | | Inspect/rebuild the per-user SQLite inventory |
| `tctl server [--port 7878] [--allow-origin ORIGIN]` | | Local HTTP/WebSocket control server (see below) |

Global flags: `--json`, `--no-color`, `-v`/`-vv`, `-q`/`--quiet`; `NO_COLOR` is honoured. JSON commands use `{ "ok": true, "result": …, "messages": … }`; errors use `{ "ok": false, "error": … }`. SSH exec is the exception: its output and exit code belong to SSH, not a JSON wrapper.

VM logs and the derived registry live under `~/.local/share/tctl/` (`XDG_DATA_HOME` is honoured). Startup records the PID/log before waiting, including on timeout. The registry stores only a SHA-256 environment hash, never environment values. Rebuild it with `tctl registry scan ~/Projects`.

### HTTP control server (`tctl server`)

`tctl server` runs a foreground HTTP/WebSocket API over the same core as the CLI.

**Security model (deliberate, decided at design time):** the listener binds to `127.0.0.1` only and has **no authentication** — any process on your machine can start, stop, provision and open shells in your VMs while it runs. Reach it from other machines through an SSH tunnel. Browser pages can only call it when their exact `Origin` was allowlisted at startup (those origins get the matching CORS response headers); every request must carry a local `Host` header (DNS-rebinding guard). There is no option to bind to other interfaces.

```bash
tctl server                                        # http://127.0.0.1:7878
tctl server --port 0                               # ephemeral port, printed at startup
tctl server --allow-origin http://localhost:5173   # exact origins only; repeatable
```

Only VMs in the registry are addressable — run `tctl init` or `tctl registry scan` first. Responses use the CLI's JSON envelope (`{"ok":true,"result":…,"messages":…}` / `{"ok":false,"error":…}`).

| Endpoint | Behaviour |
| --- | --- |
| `GET /vms` | Registered VMs with live state and IP |
| `GET /vms/{name}` | One VM's state, IP, project and provision metadata |
| `GET /vms/{name}/env` | Masked environment preview — the server has **no** secret-reveal path |
| `POST /vms/{name}/start\|stop\|provision` | `202 Accepted` + job; one mutating job per VM, otherwise `409` |
| `GET /jobs/{id}` | Job progress: `running` → `done` (with the CLI's report) or `failed` |
| `GET /vms/{name}/shell` | WebSocket terminal into a **running** VM (`409` if stopped) |

Jobs live in server memory; restarting the server forgets job history but never stops VMs.

**Shell protocol** (xterm.js-friendly): binary frames carry raw terminal bytes both ways; text frames carry JSON control messages — the client may send `{"type":"resize","cols":120,"rows":40}`, the server sends `{"type":"exit",…}` before closing. Each session runs `ssh` under a real PTY, so prompts, Ctrl-C and full-screen programs work. Limits: 4 shells per VM, 16 total (`429` beyond that), 64 KiB per client frame. Server pings detect dead peers; idle sessions are never logged out. Closing the socket kills that session's `ssh` only — the VM keeps running (use `tmux` inside the VM to survive disconnects). Terminal traffic is raw and unmasked, and is never written to server logs.

**Common workflow:**

```bash
cd ~/Projects/my-project   # or any subdirectory

tctl up                   # start the VM
tctl provision            # push the env files into the VM
tctl ssh                  # open a shell inside the VM

tctl down                 # shut down when done
```

### What `provision` Does

The `provision` command reads the files listed in `[provision].env_files` in order (whichever are readable) on the host, converts each `KEY=VALUE` line into a shell-quoted `export` statement, and writes them in that order over SSH to `/etc/profile.d/tart-provision.sh` in the VM — later layers override earlier keys. The file persists across reboots and is sourced by every login shell (bash and zsh), so run `provision` once per VM — or again whenever an env file changes — and open a new shell to pick up the variables. Existing sessions are not updated, and the file is regenerated each time (removed keys disappear).

To inspect the result **before** pushing, use `tctl env`. It prints the merged key/value table with the layer each value came from, masking secret-looking keys (name contains `TOKEN`, `SECRET`, `PASS`, `KEY`, `CREDENTIAL` or `AUTH`, case-insensitive; add `--show-secrets` to reveal). `tctl env --diff` additionally fetches the VM's current file and shows a unified diff, exiting non-zero when the VM is out of date — useful as a quick check after editing `.env`:

```bash
tctl env                  # what would be pushed, and from which file
tctl env --diff           # is the VM up to date?
```

The host share needs no provisioning step: the image has an `/etc/fstab` entry (`com.apple.virtio-fs.automount /mnt/shared virtiofs noauto,x-systemd.automount,nofail`), so a share passed at `tctl up` is mounted by systemd on first access of `/mnt/shared`. Without a share the VM boots cleanly; accessing `/mnt/shared` then just reports `No such device`.

---

## Testing the controller

```bash
just test                         # Rust format/lint, unit and sandboxed CLI tests
just smoke ~/Projects/my-project  # explicit real-VM smoke test; requires an existing clone
```

The CLI tests run in isolated HOME/project directories with fake `tart`/`ssh` shims from `tests/fakes/` on `PATH`; they check stdout, exit codes, issued commands, generated files and final fake VM state. Bash is required for the fakes.

The manual smoke test updates the project's managed SSH block, checks SSH and previews masked env. It does not provision, clone or delete anything, and stops the VM only if it started it. It is not part of `just test` or CI.

## Using the VM

Once the VM is running and provisioned (`tctl up && tctl provision`), connect with `tctl ssh`. A few things to be aware of on first use:

### Shell, Prompt & Non-Interactive Commands

`devbox` logs in to **zsh** with a [starship](https://starship.rs) prompt. The files are managed by `roles/shell`:

| File                                     | Purpose                                                                                                                                                                  |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `~/.zshenv`                              | Read by _every_ zsh — puts the mise **shims** on `PATH`, so `ssh debian-ssh 'java --version'` just works                                                                 |
| `/etc/zsh/zprofile`                      | Login shells — sources `/etc/profile`, so `/etc/profile.d/*.sh` (incl. `tart-provision.sh` from `tctl provision`) applies to zsh as well                          |
| `~/.zshrc`                               | Interactive shells — `compinit`, full `mise activate zsh` (per-directory tool switching, `JAVA_HOME`), `starship init zsh`, aliases; sources `~/.zshrc.local` if present |
| `~/.config/starship.toml`                | Prompt layout (OS · path · git · venv · java · node · docker │ memory · duration · time) — copied from `roles/shell/files/starship.toml`                                 |
| `/usr/local/share/zsh/site-functions/_*` | Generated zsh completions for mise, gh, docker, uv, ruff, starship                                                                                                       |
| `/etc/bash_completion.d/*`               | The same completions for bash                                                                                                                                            |

The prompt uses [Nerd Font](https://www.nerdfonts.com) glyphs — install one in your **host** terminal (e.g. `brew install --cask font-jetbrains-mono-nerd-font`) or the icons render as boxes.

```bash
ssh debian-ssh                              # interactive zsh — full mise activation + starship
ssh debian-ssh 'java --version'             # non-interactive — works via shims from ~/.zshenv
ssh debian-ssh 'bash -lc "java --version"'  # bash login shell also works (/etc/profile.d/mise.sh)
mise exec -- java --version                 # explicit alternative that always works
```

bash remains fully configured (`/etc/profile.d/mise.sh`, `~/.bashrc` with mise + starship) — `chsh -s /bin/bash` if you prefer it. Put personal zsh tweaks in `~/.zshrc.local`; `~/.zshrc` is overwritten on reprovision.

IDEs (IntelliJ Gateway, VS Code Remote) connect via a login shell and therefore see all tools. If an IDE needs a fixed path, use `mise where java` (e.g. `/home/devbox/.local/share/mise/installs/java/temurin-25.0.x`).

---

## IntelliJ Remote Development

### Option 1: SSH Gateway (Recommended)

1. Run `tctl update-ssh-config` to write the current VM IP into `~/.ssh/config`
2. **IntelliJ IDEA** → **File** → **Remote Development** → **SSH**
3. **New Connection**:
   - Host: VM name from `[vm].name` (resolved via `~/.ssh/config`)
   - Port: `22`
   - User: `devbox`
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

With `[share].dir = "auto"` the project root (the directory holding `.tart-ctl.toml`) is shared automatically; set an explicit path there to share something else. Then:

```bash
tctl up                # starts VM with the host share attached (auto-mounted)
tctl ssh               # open a shell — project files are at /mnt/shared/workspace/
```

Then configure IntelliJ to use remote SDK:

1. Run `tctl update-ssh-config` so the VM name resolves in `~/.ssh/config`
2. **File** → **Project Structure** → **SDKs** → **+** → **Add SSH SDK**
3. Configure SSH using the VM name from `[vm].name`, user `devbox`
4. Point to the JDK path reported by `mise where java` on the VM

**Benefits**:

- Files stay on host (easier backup, local git)
- Builds/tests run on VM resources

### Useful for Remote Dev

**Port forwarding** for web apps running on VM:

```bash
ssh -L 8080:localhost:8080 debian-ssh
# Access VM's port 8080 at http://localhost:8080 on your Mac
```

**rsync** for fast bi-directional sync:

```bash
# Sync local → VM
rsync -avz --exclude='.git' /local/project/ debian-ssh:/home/devbox/project/

# Continuous watch sync (requires fswatch on Mac)
brew install fswatch
fswatch -o /local/project | xargs -n1 -I{} rsync -avz /local/project/ debian-ssh:/home/devbox/project/
```

**VS Code Remote SSH** also works:

1. Install **Remote - SSH** extension
2. Connect to `debian-ssh` (via `~/.ssh/config`)
3. Open folder on VM
4. Extensions install automatically on remote

---

## Troubleshooting

### Tool Not Found (`java`, `node`, `gh`, …)

All developer tools are managed by mise and only on `PATH` when mise is activated. For `devbox` (zsh) this happens in `~/.zshenv` / `~/.zshrc`; for bash in `/etc/profile.d/mise.sh` / `~/.bashrc`:

```bash
ssh debian-ssh 'java --version'              # ✓ zsh reads ~/.zshenv → shims
ssh debian-ssh 'bash -c "java --version"'    # ✗ bash non-login shell — use bash -lc
mise doctor                                  # shows activation state and problems
mise ls --missing                            # tools in config that are not installed
mise install                                 # install them
```

### Build Fails in the `verify` Role

The last role runs every tool's `--version`. Read the failing task name — it names the command. Typical causes: a tool renamed its binary, an upstream release broke the registry entry, or a GitHub API rate limit during `mise install`. Re-run with `--tags mise,runtimes,cli-tools,verify` to resume; mise skips what is already installed.

### mise GitHub Rate Limits

Tools from the `github:`/`aqua:` backends hit the GitHub API. If a build fails with HTTP 403, export a token before building — Packer passes the host environment through to Ansible, and the mise role forwards `MISE_GITHUB_TOKEN` to the VM for the duration of the install:

```bash
export MISE_GITHUB_TOKEN=ghp_...
just build
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

The share is mounted on first access via `/etc/fstab` (systemd automount). If files aren't visible inside the VM:

```bash
ssh debian-ssh
# 1. Is it mounted?
findmnt /mnt/shared

# 2. If not — the fstab entry is there, so a manual mount shows the real error
grep virtiofs /etc/fstab
sudo mount /mnt/shared

# 3. Your files are in a subdirectory named after the share (workspace)
ls -la /mnt/shared/workspace/
```

**Common issues**:

- **VM not started with a share**: `tctl start` passes `--dir` only when `[share].dir` resolves to a directory (`auto` → project root, or an explicit path). `none` disables it. `tctl status` shows the resolved `Share` path; verify it exists on the host.
- **Wrong mount tag**: Tart uses a single fixed tag `com.apple.virtio-fs.automount` for all shares — do not use the share name as the mount device.
- **`dmesg` shows `tag <name> not found`**: Confirms the wrong tag is being used — switch to `com.apple.virtio-fs.automount`.

---

## Project Structure

```
.
├── justfile                        # Host-side task runner for the template image (build, clone, reprovision, clean)
├── build/                          # Template image creation (Packer + Ansible)
│   ├── build.env                   # Build-side config: BUILD_VM_NAME, BUILD_SSH_USER, BUILD_IDENTITY_FILE
│   ├── debian-ssh.pkr.hcl          # Packer template (plugins, source, provisioners)
│   ├── vars.pkrvars.hcl            # Packer inputs (build SSH credentials, pub key)
│   ├── provision.sh                # packer init → validate → build (vm_name from build.env)
│   ├── with-vm.sh                  # Runs a command against the template VM; auto-starts and auto-stops it (used by just reprovision/tools)
│   └── ansible/
│       ├── playbook.yml            # Orchestration only — role order, no inline tasks
│       ├── group_vars/all/
│       │   ├── main.yml            # devbox_user, target_arch
│       │   └── tool_versions.yml   # ★ all tool versions (mise_runtimes, mise_cli_tools)
│       └── roles/
│           ├── user/               # devbox account, sudoers, authorized_keys, sshd_config
│           ├── common_tools/       # base apt packages, git defaults
│           ├── docker/             # Docker CE apt repo + packages, docker group
│           ├── mise/               # mise apt repo, /etc/profile.d/mise.sh, config.toml.j2
│           │   └── tasks/          #   main.yml, install.yml (reusable), verify.yml (reusable)
│           ├── runtimes/           # mise install of runtimes + verify commands
│           ├── cli_tools/          # mise install of CLI tools + verify commands
│           ├── shell/              # zsh login shell, zshenv/zshrc templates, starship.toml, completions
│           └── verify/             # final verification gate
├── tctl/                           # Rust VM-control library + CLI, Cargo.lock committed
│   ├── src/                        # config, backend, envfile, SSH config, registry, operations, CLI
│   └── tests/                      # lifecycle and sandboxed CLI integration tests
├── tests/fakes/                    # Fake tart/ssh shims used by the sandboxed CLI tests
├── scripts/smoke-tctl.sh            # Explicit manual real-VM check (never provisions env)
├── docs/adr/                        # Architecture decision records (start with README.md)
├── docs/tctl-design.md              # Detailed tctl design (architecture, config, registry, server)
├── config_template/                # Copied to ~/.config/tctl/ by just install (never overwritten)
│   └── config.toml                 # Global defaults; VM names belong to project markers
├── QUICKSTART.md                   # Step-by-step first-time setup
└── README.md
```
