#!/usr/bin/env bash
set -euo pipefail

# Change to build/ directory (where Packer files live)
cd "$(dirname "${BASH_SOURCE[0]}")"

# 1. Install Packer (if not done yet)
# brew tap hashicorp/tap
# brew install hashicorp/tap/packer

# 2. Install Ansible
# brew install ansible

# 3. Init Packer plugins
packer init debian-ssh.pkr.hcl

# 4. Validate the template
packer validate -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl

# 5. Build!
packer build -var-file="vars.pkrvars.hcl" debian-ssh.pkr.hcl
