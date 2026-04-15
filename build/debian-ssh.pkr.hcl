packer {
  required_plugins {
    tart = {
      version = ">= 1.2.0"
      source  = "github.com/cirruslabs/tart"
    }
    ansible = {
      version = ">= 1.1.0"
      source  = "github.com/hashicorp/ansible"
    }
  }
}

variable "vm_name"      { default = "debian-ssh" }
variable "ssh_user"     { default = "admin" }
variable "ssh_password" {
  default   = "admin"
  sensitive = true
}
variable "ssh_key_path" { default = "~/Ssh/id_ed25519_tart.pub" }

source "tart-cli" "debian" {
  vm_base_name = "ghcr.io/cirruslabs/debian:bookworm"
  vm_name      = var.vm_name
  cpu_count    = 4
  memory_gb    = 10
  headless     = true

  ssh_username = var.ssh_user
  ssh_password = var.ssh_password
  ssh_timeout  = "120s"
}

build {
  sources = ["source.tart-cli.debian"]

  # Ansible needs python on the target — ensure it's there first
  provisioner "shell" {
    inline = [
      "sudo apt-get update -y",
      "sudo apt-get install -y python3"
    ]
  }

  # Run the Ansible playbook
  provisioner "ansible" {
    playbook_file = "./ansible/playbook.yml"
    use_proxy     = false

    extra_arguments = [
      "--extra-vars", "ansible_user=${var.ssh_user}",
      "--extra-vars", "ansible_password=${var.ssh_password}",
      "--extra-vars", "ansible_become_password=${var.ssh_password}",
      "--extra-vars", "ansible_connection=ssh",
      "--extra-vars", "ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'",
      "--extra-vars", "admin_ssh_key_path=${var.ssh_key_path}",
      "--connection=paramiko",
      "-v"   # verbose — remove in prod
    ]
  }
}


