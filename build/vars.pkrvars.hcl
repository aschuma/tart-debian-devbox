# Packer variables. The image name (vm_name) is NOT set here — it comes from
# build/build.env (BUILD_VM_NAME) via provision.sh / just, or -var vm_name=…
ssh_user     = "admin"
ssh_password = "admin"
ssh_key_path = "~/.ssh/id_ed25519_tart.pub"
