# Host-side task runner for the Tart Debian devbox.
# Requires: brew install just   (plus packer, ansible, tart for the respective recipes)
#
#   just build            build the VM image with Packer
#   just start / stop     control the VM (via tart-ctl.sh, run from your project dir)
#   just reprovision mise re-run a single Ansible tag against the running VM

set shell := ["bash", "-euo", "pipefail", "-c"]

vm_name      := `bash -c '
dir="$PWD"
while true; do
  if [[ -f "$dir/.tart-ctl-env" ]]; then
    source "$dir/.tart-ctl-env"
    printf "%s" "${TCTL_VM_NAME:-debian-ssh}"
    exit
  fi
  [[ -d "$dir/.git" || "$dir" == "/" ]] && break
  dir="$(dirname "$dir")"
done
printf "%s" "${TCTL_VM_NAME:-debian-ssh}"
'`
ssh_user     := env_var_or_default("TCTL_SSH_USER", "devbox")
identity     := env_var_or_default("TCTL_IDENTITY_FILE", env_var("HOME") + "/.ssh/id_ed25519_tart")
tart_ctl     := env_var_or_default("TART_CTL", "tart-ctl.sh")
ssh_opts     := "-o IdentitiesOnly=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null"

# List recipes
default:
    @just --list

# Build the VM image (packer init → validate → build)
build:
    ./build/provision.sh

# Validate the Packer template and Ansible playbook syntax
validate:
    cd build && packer validate -var-file=vars.pkrvars.hcl debian-ssh.pkr.hcl
    cd build/ansible && ansible-playbook --syntax-check -i localhost, playbook.yml

# Lint Ansible and YAML (brew install ansible-lint yamllint)
lint:
    yamllint build/ansible .github
    cd build/ansible && ansible-lint

# Start the VM (uses .tart-ctl-env from the current project)
start:
    {{tart_ctl}} start

# Stop the VM
stop:
    {{tart_ctl}} stop

# Show VM status
status:
    {{tart_ctl}} status

# Push .env into the VM (the host share is auto-mounted at boot)
provision:
    {{tart_ctl}} provision

# Show the effective env that provision would push (pass --diff to compare with the VM)
env *ARGS:
    {{tart_ctl}} env {{ARGS}}

# Open an interactive SSH session in the VM
ssh:
    {{tart_ctl}} ssh

# Update ~/.ssh/config with the VM's current IP
ssh-config:
    {{tart_ctl}} update-ssh-config

# Re-run the Ansible playbook against the running VM. Optional tags, e.g.: just reprovision mise,runtimes
reprovision tags="all":
    cd build/ansible && ansible-playbook playbook.yml \
      -i "$(tart ip {{vm_name}})," \
      --private-key {{identity}} \
      -u {{ssh_user}} --become \
      --tags {{tags}} \
      --extra-vars "ansible_user={{ssh_user}} ansible_ssh_common_args='{{ssh_opts}}'"

# Show mise-managed tools inside the VM
tools:
    ssh -i {{identity}} {{ssh_opts}} {{ssh_user}}@"$(tart ip {{vm_name}})" 'bash -lc "mise ls"'

# Delete the built image
clean:
    tart delete {{vm_name}}
