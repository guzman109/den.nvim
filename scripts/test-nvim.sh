#!/bin/sh
# Runs Den's Neovim tests in a headless Neovim with no user config.
# Build the engine first: scripts/build-nvim.sh
set -eu
cd "$(dirname "$0")/.."
command -v nvim >/dev/null || { echo "nvim is required to run these tests" >&2; exit 1; }
[ -f lua/den_native.so ] || { echo "build the engine first: scripts/build-nvim.sh" >&2; exit 1; }
exec nvim --headless -u NONE -i NONE -l tests/nvim/run.lua
