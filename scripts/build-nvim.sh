#!/bin/sh
# Builds Den's Neovim module and puts it where `require("den_native")` finds
# it: lua/den_native.so in this repository. Also builds the `den` command
# into bin/den (SSH passphrases inside Neovim, git's locked-note helpers)
# and bin/den-agent (holds the unlocked vault key).
#
#   scripts/build-nvim.sh          release build
#   scripts/build-nvim.sh debug    debug build
set -eu
cd "$(dirname "$0")/.."
if [ "${1:-release}" = debug ]; then
  cargo build -p den-nvim -p den-cli -p den-agent
  dir=target/debug
else
  cargo build --release -p den-nvim -p den-cli -p den-agent
  dir=target/release
fi
case "$(uname -s)" in
  Darwin) lib="$dir/libden_native.dylib" ;;
  *) lib="$dir/libden_native.so" ;;
esac
# A new file rather than an overwrite: macOS caches code signatures per file,
# and a library changed in place can be refused at load time.
rm -f lua/den_native.so bin/den bin/den-agent
cp "$lib" lua/den_native.so
mkdir -p bin
cp "$dir/den" "$dir/den-agent" bin/
if [ "$(uname -s)" = Darwin ]; then
  # The hardened runtime, so nothing can be injected into den-agent.
  codesign --force --options runtime --sign - bin/den bin/den-agent 2>/dev/null
fi
echo "built lua/den_native.so, bin/den and bin/den-agent"
