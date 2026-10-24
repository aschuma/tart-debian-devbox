# up with share, up when already running, down, down when stopped, up without share.
std_global
cfg_project TCTL_VM_NAME="demo"
vm demo stopped
run_tool up
run_tool start
run_tool down
run_tool stop
cfg_project TCTL_VM_NAME="demo" TCTL_HOST_SHARE_DIR=""
run_tool up
