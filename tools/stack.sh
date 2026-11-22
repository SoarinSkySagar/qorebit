#!/usr/bin/env bash
# Run the pieces you need to use the wallet, in one terminal; Ctrl+C (or closing the device
# window, without --web) stops all of them.
#
#   tools/stack.sh               bridge + device window for the board (USB)       = make bridge
#   tools/stack.sh --emu         emulator + bridge + device window                = make sim
#   tools/stack.sh --web         bridge + device window + wallet website          = make web
#   tools/stack.sh --emu --web   emulator + bridge + window + wallet website      = make web-emu
#   add --panel (make ... SCREEN=panel): the screen is the display on the board, so the bridge
#   runs without a window
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PY="$ROOT/tools/.venv/bin/python"
FW="$ROOT/firmware/dabao-app-qorebit"
EMU=0 WEB=0 PANEL=0
for a in "$@"; do
    case "$a" in
        --emu) EMU=1 ;;
        --web) WEB=1 ;;
        --panel) PANEL=1 ;;
        *) echo "usage: $0 [--emu] [--web] [--panel]" >&2; exit 2 ;;
    esac
done

port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
# stop a process and everything it started (cargo xtask runs the emulator as child processes)
killtree() {
    local c
    for c in $(pgrep -P "$1" 2>/dev/null); do killtree "$c"; done
    kill "$1" 2>/dev/null
}
pids=()
cleanup() { for p in "${pids[@]}"; do killtree "$p"; done; }
trap cleanup EXIT
trap 'exit 130' INT TERM

for p in 7620 7630 $([ $EMU = 1 ] && echo 7610) $([ $WEB = 1 ] && echo 5173); do
    port_busy "$p" && { echo "port $p is already in use: is a bridge, emulator or website already running?" >&2; exit 1; }
done

if [ $EMU = 1 ]; then
    (cd "$ROOT/xous-core" && exec cargo xtask dabao-emu "$FW/target/release/qorebit") >"$FW/target/emu.log" 2>&1 &
    pids+=($!)
    echo "emulator starting (log: firmware/dabao-app-qorebit/target/emu.log)..."
    for _ in $(seq 1 120); do port_busy 7610 && break; sleep 1; done
    port_busy 7610 || { echo "the emulator did not start; see the log" >&2; exit 1; }
fi

bridge=("$PY" "$ROOT/tools/qorebit_display.py")
[ $EMU = 1 ] && bridge+=(--emu)
if [ $PANEL = 1 ]; then
    bridge+=(--headless)
    echo "SCREEN=panel: approve on the device's own display and buttons (no window here)."
    [ $EMU = 1 ] && echo "note: the emulator has no panel, so it has no approval screen and refuses to sign." >&2
    echo "note: the ST7789 driver (src/panel.rs) hasn't been tried on hardware yet." >&2
fi

if [ $WEB = 1 ]; then
    "${bridge[@]}" &
    pids+=($!)
    cd "$ROOT/companion" && npm run dev
else
    "${bridge[@]}"
fi
