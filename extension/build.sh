#!/usr/bin/env bash
# Build the Qorebit wallet browser extension into extension/dist (load it unpacked in Chrome / Brave / Edge).
# It is the companion app (companion/) built for a side panel, plus a manifest, icons and a tiny background script.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
cd "$HERE/../companion"
npx tsc --noEmit
npx vite build --base ./ --outDir ../extension/dist --emptyOutDir
cp "$HERE/manifest.json" "$HERE/background.js" "$HERE/inject.js" "$HERE/content.js" "$HERE/dist/"
cp -r "$HERE/icons" "$HERE/dist/icons"
node "$HERE/../tools/check-extension.mjs"
echo "extension built: extension/dist (chrome://extensions -> Developer mode -> Load unpacked)"
