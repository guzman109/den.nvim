#!/bin/sh
# Builds Den for this machine and packs it for a release:
#
#   dist/den-<version>-<target>.tar.gz   lua/den_native.so and bin/den
#   dist/den-<version>-<target>.tar.gz.sha256
#
# Any CI (GitLab, GitHub, a laptop) runs this once per platform, then
# scripts/checksums.sh over everything in dist/.
set -eu
cd "$(dirname "$0")/.."

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target=aarch64-apple-darwin; lib=libden_native.dylib ;;
  Darwin-x86_64) target=x86_64-apple-darwin; lib=libden_native.dylib ;;
  Linux-x86_64) target=x86_64-unknown-linux-gnu; lib=libden_native.so ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu; lib=libden_native.so ;;
  *) echo "package.sh: no release target for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

# Binaries keep source paths for panic messages; name them from neutral
# roots, not the builder's home folder.
RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=/home --remap-path-prefix=$(pwd)=/den"
export RUSTFLAGS
cargo build --release --locked -p den-nvim -p den-cli

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/lua" "$stage/bin"
cp "target/release/$lib" "$stage/lua/den_native.so"
cp target/release/den "$stage/bin/den"
if [ "$(uname -s)" = Darwin ]; then
  # An ad-hoc signature, so the files load on Apple silicon after a copy.
  codesign --force --sign - "$stage/lua/den_native.so" "$stage/bin/den"
fi

mkdir -p dist
name="den-$version-$target.tar.gz"
# No owner names in the archive (they would be the builder's account).
if tar --version 2>/dev/null | grep -q GNU; then
  tar --owner=0 --group=0 --numeric-owner -czf "dist/$name" -C "$stage" lua bin
else
  tar --uid 0 --gid 0 --numeric-owner -czf "dist/$name" -C "$stage" lua bin
fi
(cd dist && { sha256sum "$name" 2>/dev/null || shasum -a 256 "$name"; } > "$name.sha256")
echo "dist/$name"
