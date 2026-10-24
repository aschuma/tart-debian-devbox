# Env layering, quoting, comments, CRLF, export prefix, invalid keys, ~ and absolute paths, skipped files.
std_global
cfg_project TCTL_VM_NAME="demo" TCTL_ENV_PROVISION_FILES=".env,.env.local,~/common.env,${HOME}/abs.env,missing.env"
vm demo running
printf 'A=1\n# comment\n\nB="two words"\nC='"'"'single'"'"'\nexport D=exported\r\nE=has=equals\n1BAD=nope\nF=$HOME and `cmd`\n' > "${PROJECT}/.env"
printf 'B=overridden\nG=local-only\n' > "${PROJECT}/.env.local"
printf 'HOME_WIDE=yes\n' > "${HOME}/common.env"
printf 'ABS=yes\nA=from-abs\n' > "${HOME}/abs.env"
run_tool provision
run_tool pr
