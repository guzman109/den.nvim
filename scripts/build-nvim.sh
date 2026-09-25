#!/bin/sh
# Builds Den's Neovim module and puts it where `require("den_native")` finds
# it: lua/den_native.so in this repository. Also builds the `den` command
# into bin/den, which a sync started from Neovim uses to ask for an SSH
# passphrase inside Neovim.
#
#   scripts/build-nvim.sh          release build
#   scripts/build-nvim.sh debug    debug build
set -eu
cd "$(dirname "$0")/.."
if [ "${1:-release}" = debug ]; then
  cargo build -p den-nvim -p den-cli
  dir=target/debug
else
  cargo build --release -p den-nvim -p den-cli
  dir=target/release
fi
case "$(uname -s)" in
  Darwin) lib="$dir/libden_native.dylib" ;;
  *) lib="$dir/libden_native.so" ;;
esac
# A new file rather than an overwrite: macOS caches code signatures per file,
# and a library changed in place can be refused at load time.
rm -f lua/den_native.so bin/den
cp "$lib" lua/den_native.so
mkdir -p bin
cp "$dir/den" bin/den
echo "built lua/den_native.so and bin/den"
