#!/usr/bin/env bash
# Flash the Dabao with the images built by `make firmware-device`, then start it.
#
#   tools/flash.sh            copy loader/xous/apps.uf2 to the BAOCHIP drive, then boot
#   tools/flash.sh --boot     only send `boot` to a board sitting in the bootloader
#   --keep-bootwait           leave boot1's boot wait as it is (make ... BOOTWAIT=keep)
#
# The board shows up as a USB drive named BAOCHIP in its bootloader (USB 1d50:6196). If it
# starts the app instead, hold PROG while plugging it in. After the copy, `boot` on its
# serial console starts Xous with Qorebit (USB 1d50:6197), like pressing PROG.
#
# It also turns boot1's "boot wait" off (`bootwait disable`, a one-way counter toggled by
# parity; nothing changes when it is already off), so the board starts Qorebit on every normal
# plug-in. Holding PROG while plugging in still enters the bootloader for the next flash.
# --keep-bootwait (make ... BOOTWAIT=keep) leaves the setting as it is.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGES="$ROOT/xous-core/target/riscv32imac-unknown-xous-elf/release"

find_drive() {
    for d in "/media/${USER:-}/BAOCHIP" "/run/media/${USER:-}/BAOCHIP" /Volumes/BAOCHIP; do
        [ -d "$d" ] && { echo "$d"; return 0; }
    done
    return 1
}

find_serial() {
    for p in /dev/ttyACM* /dev/cu.usbmodem*; do
        [ -e "$p" ] && { echo "$p"; return 0; }
    done
    return 1
}

boot() {
    local tty
    tty=$(find_serial) || { echo "no serial port for the bootloader: press PROG on the board to start it" >&2; return 1; }
    if [ "$(uname)" = Darwin ]; then stty -f "$tty" 115200 raw -echo; else stty -F "$tty" 115200 raw -echo; fi
    if [ "$KEEP_BOOTWAIT" != 1 ]; then
        # start Qorebit on every normal plug-in from now on (idempotent)
        printf 'bootwait disable\r' > "$tty"
        sleep 1
        echo "boot wait off: a normal plug-in starts Qorebit (hold PROG for the bootloader)"
    fi
    printf 'boot\r' > "$tty"
    echo "sent boot to $tty"
    if command -v lsusb >/dev/null; then
        for _ in $(seq 1 30); do
            lsusb | grep -q 1d50:6197 && { echo "Qorebit is running (USB 1d50:6197)"; return 0; }
            sleep 1
        done
        echo "the board did not come up as 1d50:6197 yet: press PROG if it stays in the bootloader" >&2
    fi
}

ONLY_BOOT=0 KEEP_BOOTWAIT=0
for a in "$@"; do
    case "$a" in
        --boot) ONLY_BOOT=1 ;;
        --keep-bootwait) KEEP_BOOTWAIT=1 ;;
        *) echo "usage: $0 [--boot] [--keep-bootwait]" >&2; exit 2 ;;
    esac
done
if [ $ONLY_BOOT = 1 ]; then boot; exit; fi

for f in loader xous apps; do
    [ -f "$IMAGES/$f.uf2" ] || { echo "$IMAGES/$f.uf2 missing: run 'make firmware-device'" >&2; exit 1; }
done

if ! drive=$(find_drive); then
    echo "Waiting for the BAOCHIP drive: unplug the Dabao, hold PROG and plug it back in..."
    for _ in $(seq 1 180); do
        drive=$(find_drive) && break
        sleep 1
    done
    [ -n "${drive:-}" ] || { echo "no BAOCHIP drive after 3 minutes" >&2; exit 1; }
fi

echo "flashing $drive"
cp "$IMAGES/loader.uf2" "$IMAGES/xous.uf2" "$IMAGES/apps.uf2" "$drive/"
sync
echo "copied loader.uf2, xous.uf2, apps.uf2"
sleep 1
boot
