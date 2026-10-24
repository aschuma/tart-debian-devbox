---
status: accepted
---

# One developer account (`devbox`) with key-only SSH; `admin` kept for the build

The image has exactly one developer account, **`devbox`**: zsh login shell,
passwordless sudo, member of `docker`, authenticated only by the SSH public key
injected at build time (`~/.ssh/id_ed25519_tart.pub`). The base image's
**`admin`** account (password `admin`) is used by Packer/Ansible during the
build and is kept afterwards but unreachable over SSH, because the hardened
`sshd_config` disables password login.

## Considered options

- **Per-person or per-role accounts** (`deploy`, `ci`, …): the VM is a single
  developer's disposable clone; extra accounts only split tool installs and
  dotfiles.
- **Removing `admin`**: impossible from Ansible — it is the account Ansible is
  logged in as. Disabling its password SSH login achieves the same exposure.
- **Password sudo**: adds friction to automation (`just reprovision`, `tctl
  provision` writing `/etc/profile.d`) for no security gain inside a local VM.

## Consequences

- All mise tools, dotfiles and completions belong to `devbox`; Ansible tasks
  that must run as the developer use `become_user` + `bash -lc`, which requires
  the `acl` package.
- The same key path is used by the build (`BUILD_IDENTITY_FILE`,
  `vars.pkrvars.hcl`) and by `tctl` (`[ssh].identity_file`); users with a
  different key must change it in both places before building.
- Docker group membership is baked in, so a fresh clone needs no re-login.
