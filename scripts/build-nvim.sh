#!/bin/sh
# Builds Den's Neovim module and puts it where `require("den_native")` finds
# it: lua/den_native.so in this repository.
#
#   scripts/build-nvim.sh          release build
#   scripts/build-nvim.sh debug    debug build
set -eu
cd "$(dirname "$0")/.."
if [ "${1:-release}" = debug ]; then
  cargo build -p den-nvim
  dir=target/debug
else
  cargo build --release -p den-nvim
  dir=target/release
fi
case "$(uname -s)" in
  Darwin) lib="$dir/libden_native.dylib" ;;
  *) lib="$dir/libden_native.so" ;;
esac
# A new file rather than an overwrite: macOS caches code signatures per file,
# and a library changed in place can be refused at load time.
rm -f lua/den_native.so
cp "$lib" lua/den_native.so
echo "built lua/den_native.so"
