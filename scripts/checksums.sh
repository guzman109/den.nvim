#!/bin/sh
# Gathers every package's checksum into dist/SHA256SUMS, the one file
# `:Den build` checks a download against.
#
# To sign it (optional; Neovim then checks the signature too, against
# release/allowed_signers in the plugin):
#
#   ssh-keygen -Y sign -n den-release -f ~/.ssh/<key> dist/SHA256SUMS
#
# which writes dist/SHA256SUMS.sig. Sign on your own machine, not in CI.
set -eu
cd "$(dirname "$0")/../dist"
cat ./*.tar.gz.sha256 | sort -k 2 > SHA256SUMS
cat SHA256SUMS
