# mise — tool version manager (https://mise.jdx.dev)
# Login shells: interactive ones get the full activation (PATH + env hooks),
# non-interactive ones (ssh host 'cmd', bash -lc) get the shims directory.
if command -v mise >/dev/null 2>&1; then
  case $- in
    *i*) eval "$(mise activate bash)" ;;
    *)   eval "$(mise activate bash --shims)" ;;
  esac
fi
