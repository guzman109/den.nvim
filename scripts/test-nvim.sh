#!/bin/sh
# Runs Den's Neovim tests in a headless Neovim with no user config.
# Build the engine first: scripts/build-nvim.sh
set -eu
cd "$(dirname "$0")/.."
command -v nvim >/dev/null || { echo "nvim is required to run these tests" >&2; exit 1; }
[ -f lua/den_native.so ] || { echo "build the engine first: scripts/build-nvim.sh" >&2; exit 1; }
# The run must end with its summary line; anything else (a crash, an early
# exit) is a failure, never a quiet pass.
out=$(mktemp)
trap 'rm -f "$out"' EXIT
status=0
nvim --headless -u NONE -i NONE -l tests/nvim/run.lua >"$out" 2>&1 || status=$?
cat "$out"
if ! grep -Eq '^[0-9]+ tests, [0-9]+ failed$' "$out"; then
  echo "the Neovim test run did not finish" >&2
  exit 1
fi
exit "$status"
