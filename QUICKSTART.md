# Quickstart

From zero to a running, provisioned Debian dev VM on your Apple Silicon Mac. Each step is a copy-paste block; the [README](README.md) explains the details.

**Mental model:** `just build` creates one **template image** (`debian-ssh`). You never work in it. For every project you **clone** the template into its own VM and drive that clone with the Rust CLI `tctl` from inside the project directory.

```
just build ──► debian-ssh (template) ──► just clone my-project ──► my-project (VM)
                                                                     ▲
                                              cd ~/Projects/my-project && tctl init / up / ssh
```

---

## 1. Install the host tools

```bash
brew install cirruslabs/cli/tart          # VM manager for Apple Silicon
brew install hashicorp/tap/packer ansible # build the image
brew install just rust                    # task runner and Rust toolchain to build tctl
# Alternatively, use mise install rust@stable and mise exec rust@stable -- just install
```

Optional, for the prompt inside the VM: a Nerd Font in your terminal, e.g. `brew install --cask font-jetbrains-mono-nerd-font`.

## 2. Create the SSH key the image will trust

The build injects `~/.ssh/id_ed25519_tart.pub` for the `devbox` user; `tctl` later connects with the private half.

```bash
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519_tart -C "tart-devbox" -N ""
```

Already have a different key? Point `ssh_key_path` in `build/vars.pkrvars.hcl` and `BUILD_IDENTITY_FILE` in `build/build.env` at it **before** building, and `[ssh].identity_file` in `~/.config/tctl/config.toml` afterwards.

## 3. Clone this repo and build the template image

```bash
git clone https://github.com/aschuma/tart-debian-devbox.git
cd tart-debian-devbox

just validate     # optional: packer validate + ansible syntax check
just build        # ~10–15 min: packer init → validate → build (downloads the Debian base image on first run)
```

Result: a local Tart image named `debian-ssh` (`BUILD_VM_NAME` in `build/build.env`). `tart list` shows it.

> Hitting GitHub API rate limits during `mise install`? Export `MISE_GITHUB_TOKEN` before `just build` (see README → Troubleshooting).

## 4. Install `tctl` and its global config

```bash
just install
# → ~/.cargo/bin/tctl
# → ~/.config/tctl/config.toml (existing config never overwritten)
export PATH="$HOME/.cargo/bin:$PATH"
tctl --help
```

The global config holds everything that is the same for all projects (SSH user/key, share mode `auto`, env file list `.env`). It intentionally has **no VM name** — that belongs to each project.

## 5. Clone the template for a project

```bash
just clone my-project       # → tart clone debian-ssh my-project
```

Do this once per project (or environment). Clones are cheap and independent; the template stays pristine.

## 6. Mark the project directory (`init`)

Go to the project you want to work on inside the VM and let `init` write the `.tart-ctl.toml` marker:

```bash
cd ~/Projects/my-project
tctl init
# Non-interactive alternative: tctl init --vm my-project --share auto
```

It asks two things:

1. **VM name** → `my-project` (warns if no such Tart VM exists yet — go back to step 5).
2. **Share directory** → `1` auto (this directory appears in the VM at `/mnt/shared/workspace`), `2` a specific path, or `3` no share.

The result is a small TOML file you can edit any time:

```toml
[vm]
name = "my-project"
[share]
dir = "auto"
[provision]
env_files = [".env", ".env.local"]
```

`tctl` finds this file by walking up from wherever you run it, so it works from any subdirectory of the project. Commit it if the whole team uses the same VM name; otherwise add it to `.gitignore`.

## 7. Start the VM and set up SSH

```bash
tctl up                     # detached boot with logs; waits up to 60 seconds for an IP
tctl status                 # running? IP?
tctl update-ssh-config       # writes/updates "Host my-project" in ~/.ssh/config
tctl ssh                    # interactive zsh as devbox — mise tools, starship prompt
```

After `update-ssh-config` plain `ssh my-project` and IDE SSH gateways (IntelliJ, VS Code Remote) work too. Run it again after every `up` — the VM gets a new IP each boot, which is also why host-key checking is disabled for these hosts.

Inside the VM:

```bash
ls /mnt/shared/workspace/       # your project (if you chose a share)
mise ls                         # installed toolchain
java --version && node --version
```

## 8. Push project environment variables (`provision`)

Variables your project needs inside the VM (API endpoints, tokens, …) live in `.env` next to `.tart-ctl.toml` as plain `KEY=VALUE` lines:

```bash
echo 'MY_API_URL=https://api.example.test' >> .env
tctl env                    # preview what would be pushed (secrets masked)
tctl provision              # → /etc/profile.d/tart-provision.sh, sourced by login shells
tctl ssh -- 'echo $MY_API_URL'
```

Layer files with `[provision].env_files = [".env", ".env.local"]` in `.tart-ctl.toml`. `tctl env --diff` tells you when the VM is out of date. Secrets are masked in both text and `--json` output unless you pass `--show-secrets`.

## 9. Daily loop

```bash
cd ~/Projects/my-project
tctl up && tctl update-ssh-config
tctl ssh
# … work …
tctl down
```

## 10. Updating the toolchain

Tool versions live in `build/ansible/group_vars/all/tool_versions.yml`. Two ways to roll out a change:

- **Rebuild the template** (`just build`) and re-clone: `tart delete my-project && just clone my-project` — clean, but the clone's local state is gone.
- **Reprovision the template in place** with `just reprovision mise,runtimes` (it boots the template, applies the tags, shuts it down again), then re-clone as above.
- **Upgrade one clone directly** from inside it: `mise upgrade`, or run the Ansible playbook against the clone (README → Reprovisioning, manual command with the clone's name).

---

## Cheat sheet

| I want to…                        | Command                                                        |
| --------------------------------- | -------------------------------------------------------------- |
| Build / rebuild the template      | `just build`                                                   |
| Install `tctl` + config           | `just install`                                                 |
| Create a project VM               | `just clone <name>`                                            |
| Mark a project directory          | `tctl init`                                                   |
| Start / stop / status             | `tctl up` · `down` · `status`                                  |
| Fix `~/.ssh/config` after a boot   | `tctl update-ssh-config`                                       |
| Shell into the VM                 | `tctl ssh` (`ssh -- <cmd>` for one command)                     |
| Push `.env` into the VM           | `tctl provision` (preview: `env`, check: `env --diff`)            |
| Inspect registered VMs            | `tctl registry list` · `registry scan ~/Projects`               |
| List VMs                          | `tart list`                                                    |
| Delete a project VM               | `tart delete <name>`                                           |
| Delete the template               | `just clean`                                                   |

**Something off?** `tctl` prints the loaded config paths (`Global:` / `Project:` lines) and requires a `.tart-ctl.toml` with `[vm].name`. Run `tctl init` in the project root. Startup logs live in `~/.local/share/tctl/logs/<vm>.log`; the derived SQLite registry stores environment hashes, never values. `tctl server` exposes the same operations plus a WebSocket terminal on `127.0.0.1` without authentication — see README → HTTP control server before using it. More in README → Troubleshooting.
