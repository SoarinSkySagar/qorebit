---
name: setup
description: Get the Qorebit wallet ready end to end. Starts the local QoreChain devnet (or reuses a running one), builds and flashes the firmware to the Dabao board (or uses the emulator), and starts the bridge, the device window and the wallet website, leaving everything waiting for the user to set the signing PIN in the website. Use when the user runs /setup or asks to set up, prepare or start the wallet or the demo.
argument-hint: "[emu] [fresh] [nodevnet]"
---

# /setup: the Qorebit wallet, ready to use

Bring the whole stack up from this repository's root (the folder with the `Makefile`), then stop with
everything running and the website waiting for the user. Everything goes through `make` (see `make help` and
`README.md`).

Arguments (optional, any order). **Default: the emulator** (anyone can run it, no hardware):
- `emu` (default): emulator + device window on this computer.
- `board`: a Dabao with the device screen as a window here (virtual display): flash it, then `make board-virtual`.
- `panel`: a Dabao with its own ST7789 panel + button (no window): `make flash-panel`, then `make board-panel`.
- `fresh`: restart the devnet from a new chain even if one is running (wipes its state).
- `nodevnet`: skip the local devnet (the website's testnet and mainnet need nothing local).

The human-readable version of these steps is `walkthrough.md`: point the user to it.

Track the steps with a task list and tell the user in one line what each step is doing. Long builds take
minutes: run them in the background and wait for them to finish rather than polling.

## Rules

- Never run `make harden`, `make devnet-up` over a chain the user wants to keep (only with `fresh`), `make emu-reset`,
  `make clean`, or anything that changes one-way counters or deletes data, unless the user asks for it by name.
- Stop processes only by PID or process group, never with broad `pkill -f` patterns.
- Don't commit, push or edit project files: this skill only builds and runs.
- Never send anything to mainnet or the testnet: setup only reads from them.
- If a step fails, show the last lines of its output, say what it means and stop; don't guess around it.

## 1. Tools

Run `make doctor`.
- `MISSING` git / cargo / node / python3 / curl: run `make install` (installs what needs no root and prints the
  commands for the rest), then `make doctor` again. Commands that need `sudo` (the udev rule) are for the user to
  run: show them and stop.
- `MISSING` submodule, `tools/.venv` or `companion/node_modules`: `make setup`.

## 2. Local devnet (one QoreChain node)

Skip with `nodevnet`. The node release only exists for Linux x86-64: on another system say so and use the testnet.

```bash
curl -s -m 3 http://127.0.0.1:1317/cosmos/base/tendermint/v1beta1/blocks/latest | head -c 200
```

- A block comes back and `fresh` was not asked: reuse it.
- Otherwise: `make devnet` (fetches the official node release into `devnet/bin` and checks its sha256, then starts
  a fresh single-node chain, `qorechain-local`, in the background). It takes about 15 seconds. If the chain exists
  but is stopped and `fresh` was not asked, `make devnet-start` keeps its state.

Note the endpoints from `make devnet-info` for the summary.

## 3. Nothing else running

`make web` / `make web-emu` refuse to start when their ports are taken: `ss -ltn | grep -E ':(7610|7620|7630|5173) '`.
If something holds them, tell the user what (`ss -ltnp`) and ask before stopping it; stop a stack this session
started earlier by its process group.

## 4a. Board (`board` or `panel` only)

0. If there is no `DISPLAY` (SSH, CI) the device window cannot open: say so; `make test-emu` still proves the signing without one.
1. Find the Dabao: `lsusb | grep -i 1d50:61`.
   - `1d50:6196`: in its bootloader, ready to flash.
   - `1d50:6197`: running an app. To flash it must restart in its bootloader: ask the user to **unplug it and plug
     it back in** (boot wait is on by default, so no PROG; if it starts the app again, boot wait is off: hold PROG
     once while plugging in). They can do that while the build runs.
   - Nothing: ask the user to plug the board in. If they say there is no board, continue with the emulator (4b).
2. `make flash-only` (for `panel`: `make flash-only SCREEN=panel`) in the background: builds the firmware (several minutes the first time), waits up to 3
   minutes for the `BAOCHIP` drive, copies the images, turns boot1's "boot wait" on (so no PROG is ever needed to flash) and starts the app
   (`BOOTWAIT=off` makes a normal plug-in start the app instead). Then check the board shows as `1d50:6197`; if it stays in the bootloader run
   `make boot` or ask the user to press PROG.
3. `make board-virtual` (or `make board-panel` for `panel`) in the background: bridge + website (+ the device window for `board`). It keeps running: never wait for it to exit.

## 4b. Emulator (default)

`make demo` in the background (it also starts the local devnet if step 2 did not; `make web-emu` if `nodevnet`): builds the emulator firmware, starts the emulator, the bridge with the device
window, and the website. It keeps running.

## 5. Check it works

Wait for the ports (up to about 2 minutes on a first build):

```bash
curl -s -m 3 http://127.0.0.1:7630/        # the bridge: {"qorebit": "bridge", ...}
curl -s -m 3 -o /dev/null -w '%{http_code}\n' http://127.0.0.1:5173/   # the website: 200
make security                               # the chip, the seed storage, PIN set or not
```

## 6. Hand over

Tell the user, briefly:
- The wallet website: http://localhost:5173. The device window is open on this computer; its **two buttons**: the
  left one rejects (0), the right one approves (1). Click them, or the Left / Right arrow keys.
- Next in the website: **Set the PIN on the device** (eight presses, left = 0, right = 1, then again), choose the
  network (devnet / testnet / mainnet), get funds (devnet: the faucet button; testnet: QoreChain's faucet link),
  **register the post-quantum key** once, then send.
- The devnet endpoints (REST :1317, RPC :26657, chain id `qorechain-local`).
- How to stop: Ctrl+C in the stack's terminal; `make devnet-down` stops the devnet.
