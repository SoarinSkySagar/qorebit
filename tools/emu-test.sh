#!/usr/bin/env bash
# `make test-emu`: start a fresh emulator (a known wallet, in a temp dir) and a headless bridge,
# run tools/emu-test.mjs against them and the local devnet, then stop both. Needs the devnet
# (make devnet). Its bridge uses its own ports (7621 tools, 7631 browsers), so a bridge you run
# for the board (7620/7630) can stay up. If the emulator port 7610 or those test ports are taken,
# it stops instead of guessing which device is behind them.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PY="$ROOT/tools/.venv/bin/python"

port_busy() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
for p in 7610 7621 7631; do
    if port_busy "$p"; then
        echo "port $p is in use (an emulator or bridge is already running): stop it and re-run" >&2
        echo "(or run 'node tools/emu-test.mjs' against your own emulator bridge)" >&2
        exit 1
    fi
done
curl -sf http://127.0.0.1:1317/cosmos/base/tendermint/v1beta1/node_info >/dev/null \
    || { echo "the local devnet is not running: make devnet" >&2; exit 1; }

state=$(mktemp -d)
pids=()
cleanup() {
    for pg in "${pids[@]}"; do kill -- "-$pg" 2>/dev/null || true; done
    rm -rf "$state"
}
trap cleanup EXIT

# a new wallet per run (so the devnet, which keeps its state, has never seen its keys), with
# entropy the test knows, so it can derive the same keys with QoreSign
entropy=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
node "$ROOT/tools/make-emu-state.mjs" "$state/qorebit-state.bin" "$entropy"

echo "starting the emulator (a new wallet in $state)..."
# setsid: each child leads its own process group, so cleanup stops it with all its children
QOREBIT_STATE="$state/qorebit-state.bin" setsid bash -c \
    "cd '$ROOT/xous-core' && exec cargo xtask dabao-emu '$ROOT/firmware/dabao-app-qorebit/target/release/qorebit'" \
    >"$state/emu.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 240); do port_busy 7610 && break; sleep 1; done
port_busy 7610 || { echo "the emulator did not start; log:" >&2; tail -20 "$state/emu.log" >&2; exit 1; }

setsid "$PY" -u "$ROOT/tools/qorebit_display.py" --emu --headless --listen 7621 --http 7631 >"$state/bridge.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 30); do port_busy 7631 && break; sleep 1; done

node "$ROOT/tools/emu-test.mjs" --bridge 7621 --http 7631 --entropy "$entropy"
