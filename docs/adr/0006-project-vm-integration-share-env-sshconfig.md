---
status: accepted
---

# Project ↔ VM integration: virtiofs share at `/mnt/shared`, env files pushed to `/etc/profile.d`, managed `~/.ssh/config` block

A project is wired to its VM through three host-driven mechanisms, none of
which require a daemon or agent inside the VM:

1. **Host share.** `tctl start` passes the project root (or a configured path)
   as a Tart virtiofs share; the image has an `/etc/fstab` entry
   (`com.apple.virtio-fs.automount /mnt/shared virtiofs noauto,x-systemd.automount,nofail`)
   so it is mounted on first access and the VM boots cleanly without one.
2. **Environment provisioning.** `tctl provision` reads the project's
   `[provision].env_files` in order on the host, renders them into shell-quoted
   `export` lines (later layers win) and writes the result over SSH to
   `/etc/profile.d/tart-provision.sh`, which every login shell sources. The
   file is regenerated each time, so removed keys disappear. `tctl env` previews
   the merge and `tctl env --diff` compares against the VM, exiting non-zero on
   drift.
3. **SSH config.** `tctl update-ssh-config` — also run implicitly whenever a
   command needs SSH (`ssh`, `provision`, `env --diff`) and the block is stale —
   rewrites a single managed `Host` block in the user's SSH config with the
   current IP, identity file and configured `RemoteForward`/`LocalForward`
   entries, leaving the rest of the file untouched.

## Considered options

- **Mounting secrets/env via the share**: would put `.env` files on a shared
  filesystem visible to every process in the VM and tie them to the mount;
  rendering into profile.d works for `ssh host 'cmd'` too.
- **cloud-init / guest agent**: Tart has no guest agent for Linux; SSH plus
  sudo is already available (ADR-0002) and keeps the image generic.
- **Printing an IP and letting users edit `~/.ssh/config`**: IPs change via
  DHCP on every start; the managed block makes `ssh <vm>` and IDE remote
  plugins stable.

## Consequences

- Env sources only need to be readable (symlinks, FIFOs, secret-manager
  mounts) and are streamed exactly once. Secret-looking keys are masked in
  human and JSON output (pattern list: README → "What `provision` Does");
  `--show-secrets` is the only reveal path and does not exist in the HTTP
  server (ADR-0008).
- The VM-side payload path/format and the SSH block format are a compatibility
  surface; change them deliberately.
- Default global forwards (e.g. `1080 127.0.0.1:1080` RemoteForward) let VM
  processes reach host services; container ports published with `docker run -p`
  need no forwarding because the VM IP is routable from the host.
