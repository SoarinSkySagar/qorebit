#!/bin/bash
# Local single-node QoreChain devnet (chain id qorechain-local) for the Qorebit wallet.
#
# Uses the official qorechaind release (devnet/bin, fetched by `make devnet-bin`), runs on
# localhost only and never joins testnet or mainnet. Hybrid secp256k1 + ML-DSA-87 signatures
# are required from genesis, with the v2 hybrid sign bytes.
#
#   ./devnet.sh up       fresh chain (wipes the old one), started in the background
#   ./devnet.sh start    start the existing chain again (keeps its state)
#   ./devnet.sh down     stop it
#   ./devnet.sh info     endpoints, chain id, funded accounts
#   ./devnet.sh logs     follow the node log
#   ./devnet.sh fund <qor1...> [uqor]   send QOR from the faucet account (default 1,000 QOR)
set -euo pipefail
cd "$(dirname "$0")"
DIR=$(pwd)
BIN="$DIR/bin/qorechaind"
export LD_LIBRARY_PATH="$DIR/bin/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
HOME_DIR="$DIR/.home"
CHAIN_ID=qorechain-local
RPC_PORT=${QORE_RPC_PORT:-26657}
API_PORT=${QORE_API_PORT:-1317}
PID_FILE="$DIR/.node.pid"
LOG="$DIR/node.log"
KR=(--keyring-backend test --home "$HOME_DIR")
# The faucet's words are public on purpose: this chain is local and throwaway.
FAUCET_WORDS="abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art"

need_bin() {
    [ -x "$BIN" ] || { echo "qorechaind missing: run 'make devnet-bin' first" >&2; exit 1; }
}

running() {
    [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null
}

wait_ready() {
    for _ in $(seq 1 60); do
        if curl -sf "http://127.0.0.1:$API_PORT/cosmos/base/tendermint/v1beta1/blocks/latest" >/dev/null 2>&1; then
            h=$(curl -sf "http://127.0.0.1:$API_PORT/cosmos/base/tendermint/v1beta1/blocks/latest" | python3 -c 'import json,sys; print(json.load(sys.stdin)["block"]["header"]["height"])')
            [ "$h" -ge 2 ] && return 0
        fi
        sleep 1
    done
    echo "the node did not produce blocks in 60 s: see $LOG" >&2
    return 1
}

start() {
    need_bin
    if running; then echo "devnet already running (PID $(cat "$PID_FILE"))"; return; fi
    [ -d "$HOME_DIR" ] || { echo "no chain yet: run './devnet.sh up'" >&2; exit 1; }
    # its own process group, so `down` stops exactly this node
    setsid "$BIN" start --home "$HOME_DIR" --minimum-gas-prices 0.1uqor >"$LOG" 2>&1 &
    echo $! >"$PID_FILE"
    echo "starting qorechaind (PID $!), log: $LOG"
    wait_ready
    echo "devnet up: RPC http://127.0.0.1:$RPC_PORT  REST http://127.0.0.1:$API_PORT  chain $CHAIN_ID"
}

down() {
    if running; then
        pid=$(cat "$PID_FILE")
        kill -TERM -- "-$pid" 2>/dev/null || kill -TERM "$pid"
        for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
        echo "devnet stopped"
    else
        echo "devnet not running"
    fi
    rm -f "$PID_FILE"
}

up() {
    need_bin
    down >/dev/null
    rm -rf "$HOME_DIR"
    "$BIN" init local --chain-id "$CHAIN_ID" --home "$HOME_DIR" >/dev/null 2>&1
    "$BIN" keys add val "${KR[@]}" >/dev/null 2>&1
    echo "$FAUCET_WORDS" | "$BIN" keys add faucet --recover "${KR[@]}" >/dev/null 2>&1
    local val faucet
    val=$("$BIN" keys show val -a "${KR[@]}")
    faucet=$("$BIN" keys show faucet -a "${KR[@]}")
    "$BIN" genesis add-genesis-account "$val" 1000000000000uqor --home "$HOME_DIR"
    "$BIN" genesis add-genesis-account "$faucet" 100000000000000uqor --home "$HOME_DIR"
    "$BIN" genesis gentx val 100000000000uqor --chain-id "$CHAIN_ID" "${KR[@]}" >/dev/null 2>&1
    "$BIN" genesis collect-gentxs --home "$HOME_DIR" >/dev/null 2>&1
    # 1 s blocks, REST + CORS on (the companion app talks to it from the browser), localhost only
    python3 - "$HOME_DIR" "$RPC_PORT" "$API_PORT" <<'EOF'
import re, sys
home, rpc, api = sys.argv[1], sys.argv[2], sys.argv[3]
c = open(f"{home}/config/config.toml").read()
c = re.sub(r'timeout_commit = ".*?"', 'timeout_commit = "1s"', c)
c = re.sub(r'laddr = "tcp://127.0.0.1:26657"', f'laddr = "tcp://127.0.0.1:{rpc}"', c)
c = re.sub(r'laddr = "tcp://0.0.0.0:26656"', 'laddr = "tcp://127.0.0.1:26656"', c)
c = re.sub(r'cors_allowed_origins = \[\]', 'cors_allowed_origins = ["*"]', c)
open(f"{home}/config/config.toml", "w").write(c)
a = open(f"{home}/config/app.toml").read()
# the [api] section: enable it on the chosen port with CORS
a = re.sub(r'(\[api\][^\[]*?)enable = false', r'\1enable = true', a, count=1, flags=re.S)
a = a.replace('address = "tcp://localhost:1317"', f'address = "tcp://127.0.0.1:{api}"')
a = a.replace('enabled-unsafe-cors = false', 'enabled-unsafe-cors = true')
a = re.sub(r'minimum-gas-prices = ".*?"', 'minimum-gas-prices = "0.1uqor"', a)
open(f"{home}/config/app.toml", "w").write(a)
EOF
    start
    info
}

info() {
    cat <<EOF
chain id:  $CHAIN_ID
RPC:       http://127.0.0.1:$RPC_PORT
REST:      http://127.0.0.1:$API_PORT
faucet:    $("$BIN" keys show faucet -a "${KR[@]}" 2>/dev/null) (words in devnet.sh, public, local only)
fund:      ./devnet.sh fund <qor1...> [uqor]   (or: make fund ADDR=qor1...)
EOF
}

fund() {
    need_bin
    local to=${1:?usage: devnet.sh fund <qor1...> [uqor]} amount=${2:-1000000000}
    # fund.mjs signs as a software wallet (the faucet's words are public, local only): it registers the
    # faucet's post-quantum key on first use, then sends a hybrid-signed transfer
    node "$DIR/../tools/fund.mjs" "$to" "$amount"
}

case "${1:-}" in
    up) up ;;
    start) start ;;
    down) down ;;
    info) info ;;
    logs) tail -f "$LOG" ;;
    fund) shift; fund "$@" ;;
    *) echo "usage: $0 up|start|down|info|logs|fund <qor1...> [uqor]" >&2; exit 2 ;;
esac
