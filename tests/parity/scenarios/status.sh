# status for stopped, running and unknown VMs; from a subdirectory too.
std_global
cfg_project TCTL_VM_NAME="demo"
vm demo stopped
run_tool status
vm demo running 192.168.64.42
run_tool st
mkdir -p "${PROJECT}/src/deep"
RUN_DIR="${PROJECT}/src/deep" run_tool status
cfg_project TCTL_VM_NAME="ghost"
run_tool status
