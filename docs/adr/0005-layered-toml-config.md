---
status: accepted
---

# Layered TOML configuration: global defaults plus an upward-discovered project marker

`tctl` reads two TOML files and merges them **per key**, project over global:
`~/.config/tctl/config.toml` (XDG-aware; SSH user/key/config path, port
forwards, share mode, env file list — deliberately **no VM name**) and
`<project>/.tart-ctl.toml`, which is found by walking upward from the current
directory and whose directory becomes the project root. `[vm].name` is required
in the project file and never inherited. Only `~`/`$HOME` are expanded; nothing
else is interpolated.

## Considered options

- **Shell-format `KEY=VALUE` files sourced by the tool**: executes arbitrary
  shell from a project checkout and has no native type for lists (port
  forwards, env files would be comma-separated strings).
- **A single file**: either every project repeats the SSH settings, or the
  global file knows about every project. The split keeps the project file
  small, committable and team-shareable while machine-specific paths stay
  global.
- **Environment variables**: convenient for overrides but invisible in the
  repo and hard to share with a team; the project marker is the shareable unit.

## Consequences

- `tctl init` writes the project marker (interactive or via `--vm`/`--share`)
  and registers the VM (ADR-0007).
- Missing marker, missing `vm.name` and invalid TOML are distinct, actionable
  errors. Unknown keys are rejected (`deny_unknown_fields`) so typos surface.
- Lists are replaced, not appended; `[]` in the project file explicitly
  disables an inherited list.
- `config_template/config.toml` is installed by `just install` only when no
  global config exists; it is never overwritten.
