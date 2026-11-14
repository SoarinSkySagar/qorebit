#!/usr/bin/env bash
# `make devnet-bin`: fetch the official QoreChain node release into devnet/bin (for the local devnet).
#
# QoreChain publishes its node as a release archive (a build from public source refuses to join the
# public networks; the archive's lib/ folder must stay next to the binary). This reads the release
# manifest, downloads the archive the manifest names, checks its sha256 and unpacks it. The node
# only ever runs on this computer as a one-validator chain of its own (devnet.sh).
set -euo pipefail
cd "$(dirname "$0")"
MANIFEST=${QORE_MANIFEST:-https://download.qore.host/testnet/latest.json}

[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] \
    || { echo "the QoreChain node release is built for Linux x86-64; on this system use the testnet in the wallet" >&2; exit 1; }

if [ -x bin/qorechaind ] && [ "${1:-}" != "--force" ]; then
    echo "devnet/bin/qorechaind is already there ($(LD_LIBRARY_PATH=bin/lib bin/qorechaind version 2>&1 | head -1)); --force fetches it again"
    exit 0
fi

read -r url sha version < <(curl -sSf "$MANIFEST" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["binary"]["url"], d["binary"]["sha256"], d["version"])')
echo "QoreChain node $version: $url"
mkdir -p bin
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sSfL -o "$tmp/node.tar.gz" "$url"
echo "$sha  $tmp/node.tar.gz" | sha256sum -c - >/dev/null || { echo "the download does not match the sha256 in the manifest: not unpacking it" >&2; exit 1; }
tar -xzf "$tmp/node.tar.gz" -C bin
chmod +x bin/qorechaind
echo "unpacked into devnet/bin (sha256 checked)"
