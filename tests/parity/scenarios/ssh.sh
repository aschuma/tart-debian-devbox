# ssh refreshes a stale/missing ssh config entry first; interactive vs one-shot; not running.
std_global
cfg_project TCTL_VM_NAME="demo"
vm demo running 192.168.64.10
run_tool ssh
snap_ssh_config
run_tool ssh -- echo hello world
run_tool sh -- 'echo $MY_VAR'
vm demo stopped
run_tool ssh
