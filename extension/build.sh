#!/usr/bin/env bash
# Build the Qorebit wallet browser extension (load it unpacked / temporary):
#   extension/build.sh            Chrome, Brave, Edge  -> extension/dist
#   extension/build.sh firefox    Firefox              -> extension/dist-firefox
#   extension/build.sh zip        both, plus the store upload zips in extension/store/ (see extension/STORE.md)
# It is the companion app (companion/) built for a side panel / sidebar, plus a manifest, icons and a small background script.
set -euo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
TARGET=${1:-chrome}
if [ "$TARGET" = zip ]; then
    "$HERE/build.sh" chrome
    "$HERE/build.sh" firefox
    VERSION=$(node -p "require('$HERE/manifest.json').version")
    mkdir -p "$HERE/store"
    rm -f "$HERE/store/"*
    # the Chrome Web Store assigns its own key: the pinned development key must not be in the uploaded manifest
    TMP=$(mktemp -d)
    cp -r "$HERE/dist/." "$TMP/"
    node -e "const f='$TMP/manifest.json',m=JSON.parse(require('fs').readFileSync(f));delete m.key;require('fs').writeFileSync(f,JSON.stringify(m,null,2))"
    (cd "$TMP" && zip -qr "$HERE/store/qorebit-chrome-$VERSION.zip" .)
    rm -rf "$TMP"
    (cd "$HERE/dist-firefox" && zip -qr "$HERE/store/qorebit-firefox-$VERSION.zip" .)
    # Mozilla asks for the source of anything that is bundled: this repository without the Xous submodule and build outputs
    (cd "$HERE/.." && git archive --format=zip -o "$HERE/store/qorebit-source-$VERSION.zip" HEAD -- . ':!xous-core')
    ls -l "$HERE/store"
    exit 0
fi
case "$TARGET" in
    chrome)  OUT=dist;         MANIFEST=manifest.json ;;
    firefox) OUT=dist-firefox; MANIFEST=manifest.firefox.json ;;
    *) echo "usage: $0 [chrome|firefox|zip]" >&2; exit 2 ;;
esac
cd "$HERE/../companion"
npx tsc --noEmit
npx vite build --base ./ --outDir "../extension/$OUT" --emptyOutDir
cp "$HERE/$MANIFEST" "$HERE/$OUT/manifest.json"
cp "$HERE/background.js" "$HERE/inject.js" "$HERE/content.js" "$HERE/$OUT/"
cp -r "$HERE/icons" "$HERE/$OUT/icons"
node "$HERE/../tools/check-extension.mjs"
if [ "$TARGET" = firefox ]; then
    echo "built: extension/dist-firefox (about:debugging#/runtime/this-firefox -> Load Temporary Add-on -> choose manifest.json)"
else
    echo "built: extension/dist (chrome://extensions -> Developer mode -> Load unpacked)"
fi
