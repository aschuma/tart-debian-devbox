# env table with masking, --show-secrets, --diff before/after provision, unknown option, no files.
std_global
cfg_project TCTL_VM_NAME="demo" TCTL_ENV_PROVISION_FILES=".env,.env.local"
vm demo running
printf 'API_URL=https://x\nAPI_TOKEN=abc123\nDB_PASSWORD=pw\nMY_KEY=k\nAUTH_HEADER=h\nCREDENTIAL_X=c\nSECRET_SAUCE=s\nPLAIN=1\n' > "${PROJECT}/.env"
printf 'PLAIN=2\nAPI_TOKEN=def456\n' > "${PROJECT}/.env.local"
run_tool env
run_tool env --show-secrets
run_tool env --diff
run_tool provision
run_tool env --diff
printf 'PLAIN=3\n' >> "${PROJECT}/.env.local"
run_tool env --diff
run_tool env --bogus
rm "${PROJECT}/.env" "${PROJECT}/.env.local"
run_tool env
