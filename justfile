# Host-side task runner for the Tart Debian devbox — builds and maintains the TEMPLATE image.
# Requires: brew install just   (plus packer, ansible, tart for the respective recipes)
#
# The image this builds is a read-only baseline. Clone it per project (`just clone <name>`)
# and manage the clones with tctl (`just install`, then `tctl --help`).
# Settings for the template live in build/build.env — independent of ~/.config/tctl/config.toml.
#
#   just build            build the template image with Packer
#   just clone my-proj    clone the template into a per-project VM
#   just reprovision mise re-run a single Ansible tag against the running template VM

set shell := ["bash", "-euo", "pipefail", "-c"]

build_env    := justfile_directory() / "build/build.env"
vm_name      := shell('source "$1" && printf "%s" "${BUILD_VM_NAME:?BUILD_VM_NAME not set in build/build.env}"', build_env)
ssh_user     := shell('source "$1" && printf "%s" "${BUILD_SSH_USER:?BUILD_SSH_USER not set in build/build.env}"', build_env)
identity     := shell('source "$1" && printf "%s" "${BUILD_IDENTITY_FILE:?BUILD_IDENTITY_FILE not set in build/build.env}"', build_env)
cargo_root   := env_var_or_default("CARGO_INSTALL_ROOT", env_var_or_default("CARGO_HOME", env_var("HOME") + "/.cargo"))
install_dir  := env_var_or_default("TART_CTL_INSTALL_DIR", cargo_root / "bin")
config_dir   := env_var_or_default("XDG_CONFIG_HOME", env_var("HOME") + "/.config") / "tctl"
ssh_opts     := "-o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"

# List recipes
default:
    @just --list

# Build the template image (packer init → validate → build); name from build/build.env
build:
    ./build/provision.sh

# Validate the Packer template and Ansible playbook syntax
validate:
    cd build && packer init debian-ssh.pkr.hcl
    cd build && packer validate -var-file=vars.pkrvars.hcl -var vm_name={{vm_name}} debian-ssh.pkr.hcl
    cd build/ansible && ansible-playbook --syntax-check -i localhost, playbook.yml

# Lint Ansible and YAML (brew install ansible-lint yamllint)
lint:
    yamllint build/ansible .github
    cd build/ansible && ansible-lint

# Run the VM-controller parity suite against the bash reference; `just parity --update` re-records goldens, `just parity --impl tctl/target/debug/tctl --format toml` checks the Rust binary
parity *args:
    tests/parity/run.sh {{args}}

# Rust unit/CLI tests and both sides of the parity suite (no real VM required)
test:
    cargo fmt --manifest-path tctl/Cargo.toml --check
    cargo clippy --manifest-path tctl/Cargo.toml --all-targets --locked -- -D warnings
    cargo test --manifest-path tctl/Cargo.toml --locked
    cargo build --manifest-path tctl/Cargo.toml --locked
    python3 -m unittest discover -s tests
    tests/parity/run.sh
    tests/parity/run.sh --impl tctl/target/debug/tctl --format toml

# Install the Rust CLI, a transitional tart-ctl.sh symlink and TOML defaults (never overwrite config)
install:
    @if [[ -f "{{config_dir}}/config" && ! -f "{{config_dir}}/config.toml" ]]; then echo 'Legacy config found: first run bash scripts/migrate-tctl-env.sh [PROJECT_DIR ...]' >&2; exit 1; fi
    cargo install --path "{{justfile_directory()}}/tctl" --locked --root "{{cargo_root}}"
    mkdir -p "{{install_dir}}" "{{config_dir}}"
    ln -sfn "{{cargo_root}}/bin/tctl" "{{install_dir}}/tart-ctl.sh"
    @if [[ -e "{{config_dir}}/config.toml" ]]; then \
      echo "Kept existing {{config_dir}}/config.toml"; \
    else \
      install -m 0644 "{{justfile_directory()}}/config_template/config.toml" "{{config_dir}}/config.toml"; \
    fi
    @echo "Installed tctl → {{cargo_root}}/bin/tctl (add this directory to PATH)"

# Explicit real-VM smoke test; updates SSH config, but never provisions env files
smoke project:
    cargo build --manifest-path tctl/Cargo.toml --locked
    bash scripts/smoke-tctl.sh "{{justfile_directory()}}/tctl/target/debug/tctl" "{{project}}"

# Clone the template image into a per-project VM, e.g.: just clone my-project
clone name:
    tart clone {{vm_name}} {{name}}
    @echo "Cloned '{{vm_name}}' → '{{name}}'. Point a project at it:"
    @echo "  cd ~/Projects/{{name}} && tctl init && tctl up"

# Re-run the Ansible playbook against the template VM (auto-starts/stops it). Optional tags, e.g.: just reprovision mise,runtimes
reprovision tags="all":
    ./build/with-vm.sh -C build/ansible ansible-playbook playbook.yml \
      -i "__VM_IP__," \
      --private-key "{{identity}}" \
      -u {{ssh_user}} --become \
      --tags {{tags}} \
      --extra-vars "ansible_user={{ssh_user}} ansible_ssh_common_args='{{ssh_opts}}'"

# Show mise-managed tools inside the template VM (auto-starts/stops it)
tools:
    ./build/with-vm.sh ssh -i "{{identity}}" {{ssh_opts}} {{ssh_user}}@__VM_IP__ 'bash -lc "mise ls"'

# Delete the template image (clones are not affected)
clean:
    tart delete {{vm_name}}
