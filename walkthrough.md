# Qorebit walkthrough: run it yourself

Qorebit is a hardware wallet for QoreChain with **one button**. You do not need our hardware: everything runs in an
**emulator** with the device's screen drawn in a window on your computer. If you do have a Baochip Dabao board, sections
7 and 8 flash it. An agent can do all of this for you: run **`/setup`** in Claude Code (see section 10).

Tested on Linux x86-64. The emulator and website also run on macOS (the local devnet node is Linux-only; use the
Testnet tab there).

## 1. What you need

`git`, `curl`, Python 3 with `venv`, Node 22+, Rust (rustup). `make install` installs Rust and Node if missing (no root)
and prints the commands for anything that needs `sudo`. Check with `make doctor`.

## 2. Get it

```bash
git clone --recursive <repo url> qorebit && cd qorebit
make install          # tools + project setup, about 10 minutes the first time (the Xous toolchain is the slow part)
```

Only want the emulator? `make setup-emu` skips the Xous toolchain and is much faster.

## 3. Run it: the emulator with a display (no hardware)

```bash
make demo
```

This starts, in one terminal: a one-node **local QoreChain** (`qorechain-local`), the **emulated device** with its
**screen window**, and the **wallet website** at <http://localhost:5173>. The first run compiles the firmware (a minute
or two). `Ctrl+C` stops the window, emulator and website; `make devnet-down` stops the chain.

No Linux x86-64 (so no local chain)? `make web-emu` starts the same without it; use the **Testnet** tab.

## 4. The one button

The device window shows the device's screen and one button. **Tap** = 0 = reject. **Hold about 0.7 seconds** = 1 = approve
(the button fills; it turns green once releasing will approve). Space bar or the mouse both work.

The same two answers are used everywhere: approving a transaction, typing the 8-press PIN, confirming, and picking words
when restoring (tap = "earlier", hold = "this word or later").

## 5. First use, step by step

Open <http://localhost:5173>. The header shows the network (default: Local devnet) and the device status.

1. **Set the PIN.** Click *Set the PIN on the device*. On the device screen press the button 8 times (tap = 0, hold = 1),
   then the same 8 again. It is asked after every approval; 5 wrong tries lock signing.
2. **Get funds.** Click *Get 1,000 test QOR* (the local chain's faucet). On the testnet use QoreChain's faucet link.
3. **Register the post-quantum key.** QoreChain refuses transfers until an account's ML-DSA key is registered. Click
   *Register the post-quantum key*; on the device **hold** to approve (twice: the proof, then the transaction), then the PIN.
4. **Send.** Paste a `qor1…` address (account 2 is a good one: pick it in the account bar, copy its address, switch back),
   an amount, *Review on the device*. Read the screen: the amount, the address, the network. **Hold** to approve, enter
   the PIN. The website broadcasts it and shows it in *Activity*.
5. **Try saying no.** Send again and **tap** instead: "REJECTED", nothing is signed.

What to look for: the device decodes the transaction itself, so what its screen shows is what it signs, whatever the
website says. A transfer from another account, two messages in one transaction, or an unknown message type is refused or
marked **BLIND**.

## 6. The three networks

Switch in the header. All three go through the bridge (QoreChain's public endpoints allow no browser access).

| Tab | Chain id | Use |
|---|---|---|
| Mainnet | `qorechain-vladi` | real funds; an amber banner and **MAINNET** on the device screen. Read-only unless you fund it |
| Testnet | `qorechain-diana` | test QOR from QoreChain's faucet |
| Local devnet | `qorechain-local` | `make devnet`; one-click faucet |

## 7. With a Dabao board and the display on your laptop (virtual display)

The board does the cryptography and keeps the keys; the screen and button are the window on your laptop (a development
stand-in: malware on the laptop could fake them).

```bash
make flash-virtual    # build + flash. When asked: unplug the Dabao, hold PROG, plug it back in
make board-virtual    # the window + website (re-run this any time; no reflash needed)
```

The board's own **PROG** button is the same one button. If `make flash-virtual` says the board is not in its bootloader,
repeat the plug-in with PROG held. `make security` shows the chip's state; `make info` its firmware.

## 8. With a Dabao and its own display and button (no window)

For a board wired with the ST7789 panel and a button on PB1 (wiring in `firmware/dabao-app-qorebit/README.md`):

```bash
make flash-panel      # builds the panel firmware (screen and button stay on the device) and flashes
make board-panel      # the bridge and website only
```

`make bio-test` checks the display and button engines without the panel. The panel path has not been run on hardware yet.

## 9. Checks you can run

```bash
make test             # the signer against QoreChain's own library (byte for byte), firmware, website tests
make devnet && make test-emu   # the real thing: firmware in the emulator signs, a real QoreChain node verifies
make screens          # every device screen as an image, in screens/
make stack            # how much memory the post-quantum signing needs
```

`make test-emu` prints 29 `ok` lines: key registration, transfers, tap vs hold at the 700 ms threshold, a wrong PIN,
and the transactions the device must refuse.

## 10. Set it up with an agent

In Claude Code (or any agent that reads `.claude/skills`), open this folder and run **`/setup`**. It checks the tools, starts
the local chain, runs the emulator with its window and the website, and stops at the PIN for you. Variants: `/setup emu`
(never touch a board), `/setup nodevnet` (testnet only), `/setup board` (flash a Dabao, virtual display), `/setup panel`.

## 11. Troubleshooting

| Symptom | Fix |
|---|---|
| `port 7610/7620/7630/5173 is already in use` | an earlier run is still going: Ctrl+C it, or find it with `ss -ltnp` |
| the window does not open (no `DISPLAY`) | run on a desktop session; over SSH use `ssh -X`. `make test-emu` needs no window |
| website says *The bridge is not running* | start it with `make demo` / `make web-emu` |
| *No device answers* | the emulator is still compiling (wait), or the board is not running Qorebit (`make boot`) |
| `make devnet` fails | the node release is Linux x86-64 only; use the Testnet tab. `devnet/node.log` has details |
| *post-quantum key not registered* | click *Register the post-quantum key* (section 5, step 3) |
| *this account has no funds* | faucet button (devnet) or QoreChain's faucet link (testnet) |
| `SemVer::from_git: no major version` while flashing | `make submodules` fetches the tags the image signer needs |
| the board stays in its bootloader | `make boot`, or press PROG |
| PIN locked | *Forgot PIN* in the website's Device card: enter your 24 words on the device |

## 12. Stopping and resetting

`Ctrl+C` in the terminal; `make devnet-down` for the chain. `make emu-reset` forgets the emulator's wallet (a new device
next start). The emulator keeps its wallet in `firmware/dabao-app-qorebit/target/qorebit-state.bin`: not secret, test use only.
