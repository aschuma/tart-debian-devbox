# Quickstart

From zero to a running, provisioned Debian dev VM on your Apple Silicon Mac. Each step is a copy-paste block; the [README](README.md) explains the details.

**Mental model:** `just build` creates one **template image** (`debian-ssh`). You never work in it. For every project you **clone** the template into its own VM and drive that clone with `tart-ctl.sh` from inside the project directory.

```
just build ──► debian-ssh (template) ──► just clone my-project ──► my-project (VM)
                                                                     ▲
                                              cd ~/Projects/my-project && tart-ctl.sh init / up / ssh
```

---

## 1. Install the host tools

```bash
brew install cirruslabs/cli/tart          # VM manager for Apple Silicon
brew install hashicorp/tap/packer ansible # build the image
brew install just                         # task runner (optional but assumed below)
```

Optional, for the prompt inside the VM: a Nerd Font in your terminal, e.g. `brew install --cask font-jetbrains-mono-nerd-font`.

## 2. Create the SSH key the image will trust

The build injects `~/.ssh/id_ed25519_tart.pub` for the `devbox` user; `tart-ctl.sh` later connects with the private half.

```bash
ssh-keygen -t ed25519 -f ~/.ssh/id_ed25519_tart -C "tart-devbox" -N ""
```

Already have a different key? Point `ssh_key_path` in `build/vars.pkrvars.hcl` and `BUILD_IDENTITY_FILE` in `build/build.env` at it **before** building, and `TCTL_IDENTITY_FILE` in `~/.config/tctl/config` afterwards.

## 3. Clone this repo and build the template image

```bash
git clone https://github.com/aschuma/tart-debian-devbox.git
cd tart-debian-devbox

just validate     # optional: packer validate + ansible syntax check
just build        # ~10–15 min: packer init → validate → build (downloads the Debian base image on first run)
```

Result: a local Tart image named `debian-ssh` (`BUILD_VM_NAME` in `build/build.env`). `tart list` shows it.

> Hitting GitHub API rate limits during `mise install`? Export `MISE_GITHUB_TOKEN` before `just build` (see README → Troubleshooting).

## 4. Install `tart-ctl.sh` and its global config

```bash
just install
# → ~/.local/bin/tart-ctl.sh
# → ~/.config/tctl/config   (copied from config_template/, never overwritten)
```

Make sure `~/.local/bin` is on your `PATH`, then check:

```bash
tart-ctl.sh --help
```

The global config holds everything that is the same for all projects (SSH user/key, share mode `auto`, env file list `.env`). It intentionally has **no VM name** — that belongs to each project.

## 5. Clone the template for a project

```bash
just clone my-project       # → tart clone debian-ssh my-project
```

Do this once per project (or environment). Clones are cheap and independent; the template stays pristine.

## 6. Mark the project directory (`init`)

Go to the project you want to work on inside the VM and let `init` write the `.tart-ctl-env` marker:

```bash
cd ~/Projects/my-project
tart-ctl.sh init
```

It asks two things:

1. **VM name** → `my-project` (warns if no such Tart VM exists yet — go back to step 5).
2. **Share directory** → `1` auto (this directory appears in the VM at `/mnt/shared/workspace`), `2` a specific path, or `3` no share.

The result is a small shell file you can edit any time:

```bash
TCTL_VM_NAME="my-project"
TCTL_HOST_SHARE_DIR="auto"
# TCTL_ENV_PROVISION_FILES=".env,.env.local"
```

`tart-ctl.sh` finds this file by walking up from wherever you run it, so it works from any subdirectory of the project. Commit it if the whole team uses the same VM name; otherwise add it to `.gitignore`.

## 7. Start the VM and set up SSH

```bash
tart-ctl.sh up                  # boots the clone in the background (with the share, if configured)
tart-ctl.sh status              # running? IP?
tart-ctl.sh update-ssh-config   # writes/updates "Host my-project" in ~/.ssh/config with the current IP
tart-ctl.sh ssh                 # interactive zsh as devbox — mise tools, starship prompt
```

After `update-ssh-config` plain `ssh my-project` and IDE SSH gateways (IntelliJ, VS Code Remote) work too. Run it again after every `up` — the VM gets a new IP each boot, which is also why host-key checking is disabled for these hosts.

Inside the VM:

```bash
ls /mnt/shared/workspace/       # your project (if you chose a share)
mise ls                         # installed toolchain
java --version && node --version
```

## 8. Push project environment variables (`provision`)

Variables your project needs inside the VM (API endpoints, tokens, …) live in `.env` next to `.tart-ctl-env` as plain `KEY=VALUE` lines:

```bash
echo 'MY_API_URL=https://api.example.test' >> .env
tart-ctl.sh env                 # preview what would be pushed (secrets masked)
tart-ctl.sh provision           # → /etc/profile.d/tart-provision.sh in the VM, sourced by every login shell
tart-ctl.sh ssh -- 'echo $MY_API_URL'
```

Layer files with `TCTL_ENV_PROVISION_FILES=".env,.env.local"` in `.tart-ctl-env`, or inject secrets ad hoc: `TCTL_ENV_FILE=<(op inject -i .env.tpl) tart-ctl.sh provision`. `tart-ctl.sh env --diff` tells you when the VM is out of date.

## 9. Daily loop

```bash
cd ~/Projects/my-project
tart-ctl.sh up && tart-ctl.sh update-ssh-config
tart-ctl.sh ssh
# … work …
tart-ctl.sh down
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
| Install `tart-ctl.sh` + config    | `just install`                                                 |
| Create a project VM               | `just clone <name>`                                            |
| Mark a project directory          | `tart-ctl.sh init`                                             |
| Start / stop / status             | `tart-ctl.sh up` · `down` · `status`                           |
| Fix `~/.ssh/config` after a boot  | `tart-ctl.sh update-ssh-config`                                |
| Shell into the VM                 | `tart-ctl.sh ssh` (`ssh -- <cmd>` for one command)             |
| Push `.env` into the VM           | `tart-ctl.sh provision` (preview: `env`, check: `env --diff`)  |
| List VMs                          | `tart list`                                                    |
| Delete a project VM               | `tart delete <name>`                                           |
| Delete the template               | `just clean`                                                   |

**Something off?** `tart-ctl.sh` tells you exactly which config file it loaded (`Global:` / `Project:` lines) and refuses to run without a `.tart-ctl-env` that sets `TCTL_VM_NAME` — run `tart-ctl.sh init` in the project root. More in README → Troubleshooting.
