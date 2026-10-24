# Share resolution: auto → project root; explicit path; none; invalid path.
std_global
cfg_project TCTL_VM_NAME="demo"
vm demo stopped
run_tool status
mkdir -p "${HOME}/shared"
cfg_project TCTL_VM_NAME="demo" TCTL_HOST_SHARE_DIR="${HOME}/shared"
run_tool status
cfg_project TCTL_VM_NAME="demo" TCTL_HOST_SHARE_DIR=""
run_tool status
cfg_project TCTL_VM_NAME="demo" TCTL_HOST_SHARE_DIR="${HOME}/does-not-exist"
run_tool status
