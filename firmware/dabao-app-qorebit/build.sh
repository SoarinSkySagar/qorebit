#!/bin/bash
# Build the Qorebit dabao app, for the emulator and/or the device, then the dabao image.
# Uses the xous-core submodule (qorebit/xous-core; `make setup` fetches it) and the Xous
# toolchain (`make toolchain`, i.e. `cargo xtask install-toolkit` in xous-core).
#   ./build.sh emu      host binary, then run:   (cd $XOUS && cargo xtask dabao-emu $APP_HOST)
#   ./build.sh device   riscv binary + loader.uf2 / xous.uf2 / apps.uf2 (xous-core target dir)
#   ./build.sh          both
# Options:
#   --vdisplay[=true|false]   approval screen on the laptop (tools/qorebit_display.py).
#                             Default: true (no real display yet). With false the firmware has no
#                             approval UI and refuses to sign, unless --panel.
#   --panel-udma              like --panel, drawing through the chip's hardware SPI (uDMA SPI2) and
#                             polling the button from the CPU instead of using the BIO engines
#   --panel                   the ST7789 panel + the one button on the board (src/panel.rs; untested
#                             on hardware until the panel is wired). Use with --vdisplay=false.
set -e
cd "$(dirname "$0")"
XOUS=$(cd ../../xous-core 2>/dev/null && pwd) || { echo "xous-core submodule missing: run 'make setup' (or git submodule update --init xous-core)" >&2; exit 1; }
what=all
vdisplay=true
panel=false
udma=false
for arg in "$@"; do
    case "$arg" in
        emu|device|all) what=$arg ;;
        --vdisplay|--vdisplay=true) vdisplay=true ;;
        --vdisplay=false|--no-vdisplay) vdisplay=false ;;
        --panel) panel=true ;;
        --panel-udma) panel=true; udma=true ;;
        *) echo "usage: $0 [emu|device|all] [--vdisplay[=true|false]] [--panel|--panel-udma]" >&2; exit 2 ;;
    esac
done
if [ "$vdisplay" = true ]; then
    feat_default=()
    echo "approval UI: virtual display (run tools/qorebit_display.py on the laptop)"
elif [ "$panel" = true ]; then
    feat_default=(--no-default-features)
    echo "approval UI: the ST7789 panel and the one button on the board"
else
    feat_default=(--no-default-features)
    echo "approval UI: none (signing is refused)"
fi
panel_feat=()
[ "$panel" = true ] && panel_feat=(--features panel)
[ "$udma" = true ] && panel_feat=(--features panel,panel-udma)
# the source revision the device reports (CMD_GET_SECURITY, info): compare it with the release
export QOREBIT_GIT=$(git -C ../.. describe --always --dirty --tags 2>/dev/null || echo unknown)
echo "firmware id: $QOREBIT_GIT"
if [ "$what" = emu ] || [ "$what" = all ]; then
    cargo build --release "${feat_default[@]}"
    echo "emulator binary: $(pwd)/target/release/qorebit"
    echo "run it:          (cd $XOUS && cargo xtask dabao-emu $(pwd)/target/release/qorebit)"
fi
if [ "$what" = device ] || [ "$what" = all ]; then
    cargo build --release --target riscv32imac-unknown-xous-elf --features board-dabao,bao1x "${panel_feat[@]}" "${feat_default[@]}"
    app_elf="$(pwd)/target/riscv32imac-unknown-xous-elf/release/qorebit"
    (cd "$XOUS" && cargo xtask dabao "$app_elf" --feature usb-bao1x/generic-hid)
    echo "device images:   $XOUS/target/riscv32imac-unknown-xous-elf/release/{loader,xous,apps}.uf2"
    # what was built, for comparing a flashed board or another build with this one
    imgs="$XOUS/target/riscv32imac-unknown-xous-elf/release"
    # sha256sum on Linux, shasum on macOS
    sha() { if command -v sha256sum >/dev/null; then sha256sum "$@"; else shasum -a 256 "$@"; fi; }
    { echo "firmware $QOREBIT_GIT"; (cd "$imgs" && sha loader.uf2 xous.uf2 apps.uf2); (cd "$(dirname "$app_elf")" && sha qorebit); } \
        > target/firmware-hashes.txt
    echo "image hashes:    $(pwd)/target/firmware-hashes.txt"
fi
