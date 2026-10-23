# Host-side task runner for the Tart Debian devbox — builds and maintains the TEMPLATE image.
# Requires: brew install just   (plus packer, ansible, tart for the respective recipes)
#
# The image this builds is a read-only baseline. Clone it per project (`just clone <name>`)
# and manage the clones with tart-ctl.sh (`just install`, then `tart-ctl.sh --help`).
# Settings for the template live in build/build.env — independent of ~/.config/tctl/config.
#
#   just build            build the template image with Packer
#   just clone my-proj    clone the template into a per-project VM
#   just reprovision mise re-run a single Ansible tag against the running template VM

set shell := ["bash", "-euo", "pipefail", "-c"]

build_env    := justfile_directory() / "build/build.env"
vm_name      := shell('source "$1" && printf "%s" "${BUILD_VM_NAME:?BUILD_VM_NAME not set in build/build.env}"', build_env)
ssh_user     := shell('source "$1" && printf "%s" "${BUILD_SSH_USER:?BUILD_SSH_USER not set in build/build.env}"', build_env)
identity     := shell('source "$1" && printf "%s" "${BUILD_IDENTITY_FILE:?BUILD_IDENTITY_FILE not set in build/build.env}"', build_env)
install_dir  := env_var_or_default("TART_CTL_INSTALL_DIR", env_var("HOME") + "/.local/bin")
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
    cd build && packer validate -var-file=vars.pkrvars.hcl -var vm_name={{vm_name}} debian-ssh.pkr.hcl
    cd build/ansible && ansible-playbook --syntax-check -i localhost, playbook.yml

# Lint Ansible and YAML (brew install ansible-lint yamllint)
lint:
    yamllint build/ansible .github
    cd build/ansible && ansible-lint

# Install tart-ctl.sh to your PATH (default: ~/.local/bin; override with TART_CTL_INSTALL_DIR) and copy config_template/ → ~/.config/tctl/
install:
    mkdir -p {{install_dir}} {{config_dir}}
    install -m 0755 {{justfile_directory()}}/bin_template/tart-ctl.sh {{install_dir}}/tart-ctl.sh
    @echo "Installed tart-ctl.sh → {{install_dir}}/tart-ctl.sh"
    @for src in {{justfile_directory()}}/config_template/*; do \
      dest="{{config_dir}}/$(basename "$src")"; \
      if [[ -e "$dest" ]]; then \
        echo "Kept existing $dest (template: config_template/$(basename "$src"))"; \
      else \
        install -m 0644 "$src" "$dest" && echo "Created $dest"; \
      fi; \
    done
    @command -v tart-ctl.sh >/dev/null || echo "Note: {{install_dir}} is not on your PATH — add it to use 'tart-ctl.sh' globally."

# Clone the template image into a per-project VM, e.g.: just clone my-project
clone name:
    tart clone {{vm_name}} {{name}}
    @echo "Cloned '{{vm_name}}' → '{{name}}'. Point a project at it:"
    @echo "  cd ~/Projects/{{name}} && tart-ctl.sh init && tart-ctl.sh up"

# Re-run the Ansible playbook against the template VM (auto-starts/stops it). Optional tags, e.g.: just reprovision mise,runtimes
reprovision tags="all":
    ./build/with-vm.sh -C build/ansible ansible-playbook playbook.yml \
      -i "__VM_IP__," \
      --private-key {{identity}} \
      -u {{ssh_user}} --become \
      --tags {{tags}} \
      --extra-vars "ansible_user={{ssh_user}} ansible_ssh_common_args='{{ssh_opts}}'"

# Show mise-managed tools inside the template VM (auto-starts/stops it)
tools:
    ./build/with-vm.sh ssh -i {{identity}} {{ssh_opts}} {{ssh_user}}@__VM_IP__ 'bash -lc "mise ls"'

# Delete the template image (clones are not affected)
clean:
    tart delete {{vm_name}}
