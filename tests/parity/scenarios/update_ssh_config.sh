# New block, update of existing block (IP change), forwards replaced, other hosts untouched.
std_global
cfg_project TCTL_VM_NAME="demo"
vm demo running 192.168.64.10
mkdir -p "${HOME}/.ssh"
cat > "${HOME}/.ssh/config" <<'SSH'
Host other
    HostName 10.0.0.1
    User someone
SSH
run_tool update-ssh-config
snap_ssh_config
vm demo running 192.168.64.11
cfg_project TCTL_VM_NAME="demo" TCTL_LOCAL_FORWARD="5432 127.0.0.1:5432, 15672 127.0.0.1:15672"
run_tool usc
snap_ssh_config
cfg_project TCTL_VM_NAME="demo" TCTL_REMOTE_FORWARD="" TCTL_LOCAL_FORWARD=""
run_tool usc
snap_ssh_config
cat >> "${HOME}/.ssh/config" <<'SSH'

Host trailing
    HostName 10.0.0.2
SSH
vm demo running 192.168.64.12
run_tool usc
snap_ssh_config
cfg_project TCTL_VM_NAME="demo" TCTL_LOCAL_FORWARD="8080 127.0.0.1:8080"
run_tool usc
snap_ssh_config
vm demo stopped
run_tool usc
snap_ssh_config
