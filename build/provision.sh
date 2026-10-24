#!/usr/bin/env bash
set -euo pipefail

# Change to build/ directory (where Packer files live)
cd "$(dirname "${BASH_SOURCE[0]}")"

# Build-side settings (template image name, …)
# shellcheck source=build.env
source ./build.env

# 1. Install Packer (if not done yet)
# brew tap hashicorp/tap
# brew install hashicorp/tap/packer

# 2. Install Ansible
# brew install ansible

# 3. Install Ansible collections
ansible-galaxy collection install -r ansible/requirements.yml

# 4. Init Packer plugins
packer init debian-ssh.pkr.hcl

# 5. Validate the template
packer validate -var-file="vars.pkrvars.hcl" -var "vm_name=${BUILD_VM_NAME}" debian-ssh.pkr.hcl

# 6. Build!
packer build -var-file="vars.pkrvars.hcl" -var "vm_name=${BUILD_VM_NAME}" debian-ssh.pkr.hcl

cat <<MSG

==> Template image '${BUILD_VM_NAME}' is ready.
    It is meant as a read-only baseline — clone it for each project instead of working in it:

      just clone my-project            # → tart clone ${BUILD_VM_NAME} my-project
      cd ~/Projects/my-project && tctl init    # writes .tart-ctl.toml (VM name, share dir)
      tctl up

    Full walkthrough: QUICKSTART.md

MSG
