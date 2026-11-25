# Qorebit

A hardware wallet for [QoreChain](https://qorechain.io) on the Baochip-1x (Dabao board), with **two buttons**:
left is 0 / reject, right is 1 / approve. The device holds the keys, reads every transaction itself, shows it on
its own screen, and signs only after you press the right button and enter the signing PIN.

QoreChain's native lane requires a **hybrid signature on every transaction**: secp256k1 over the Cosmos `SignDoc`
*and* ML-DSA-87 (FIPS 204, post-quantum) over the hybrid sign bytes. The device makes both. The wallet website has a
network switch for **mainnet** (`qorechain-vladi`), **testnet** (`qorechain-diana`) and a **local devnet**
(`qorechain-local`, one node on your computer).

It is the same setup as [Quazar](https://github.com/SoarinSkySagar/quazar-wallet) (Xous firmware, a virtual display
through a bridge, an emulator, flashing scripts, a companion website) with QoreChain as the chain, the hybrid
signature scheme, and the two-button input. It is a research prototype and has not been audited.

## Quick start

Everything runs through `make` (`make help` lists every command).

```bash
git clone --recursive <this repo> && cd qorebit
make install      # Rust, Node, submodule, npm, Python venv, Xous toolchain (make setup-emu: emulator only, faster)
make demo         # NO HARDWARE NEEDED: local QoreChain + emulated device with its screen window + website

make flash-virtual && make board-virtual   # a Dabao, its screen as a window on your laptop
make flash-panel   && make board-panel     # a Dabao with its own panel + button, no window
```

**Step by step guide: [walkthrough.md](walkthrough.md).** In the website: **Set the PIN on the device**, choose the Pin connections for a board: [HARDWARE.md](HARDWARE.md). Browser extension (side panel): `make extension`, see the walkthrough.
network, get funds (the devnet has a one-click faucet, the testnet a link to QoreChain's), **register the
post-quantum key** once per account and network, then send.

With Claude Code, run **`/setup`** (or `/setup emu`): it does the steps above and waits for you at the PIN.

### Multisig

Shared accounts ("N of M must agree") use QoreChain's group module, so no special signing mode is needed: creating a
multisig, proposing a payment, voting and running it are each an ordinary transaction that one member approves on their
own device. The device decodes these messages itself (it shows every member and the threshold, the proposed payment, the
proposal number and your vote), and refuses what is not its own account's. Limits: threshold multisigs of up to 4
equal-weight members created on the device, one proposed action per proposal; the device cannot read the chain, so for
votes it shows the proposal number. See [walkthrough.md](walkthrough.md) (section 6c).

### The two buttons

| | **left** button (PB1) | **right** button (PB2) |
|---|---|---|
| Approval screen | 0 · **reject** | 1 · **approve** |
| PIN, 8 presses | 0 | 1 |
| Word by word recovery, passphrase | "earlier" | "this word or later" |
| Yes / no questions | no | yes |

One press of a button is the answer: there is no tap or hold. In the dev build the buttons are the two in the
device window (click them, or the Left / Right arrow keys); on the panel build they are two switches wired to PB1 and
PB2 (see [HARDWARE.md](HARDWARE.md)). PROG is only used to enter the bootloader.

## How a transaction is signed

```
companion ──(body, auth info, chain id, account number)──▶ device
   device decodes it itself ──▶ shows it ──▶ right button ──▶ PIN
   device: ML-DSA-87 over  "qorechain-pqc-hybrid-v2" | chain id | body (no PQC ext) | auth info
           secp256k1 over  SignDoc(body + PQC ext, auth info, chain id, account number)   (SHA-256, low-S)
   ◀── TxRaw ── companion broadcasts it
```

* Keys come from the 24 words exactly as QoreChain's wallets derive them (`@qorechain/qoresign`), so the same
  words give the same accounts in other wallets: secp256k1 at `m/44'/118'/0'/0/i`; the ML-DSA-87 key is the
  account's "canonical" key, `SHAKE256("qorechain:pqc:v1|" + address + "|" + mnemonic)`.
* The device refuses what it can't show completely: one message per transaction, the message must be from this
  account, one signer in direct mode, no unknown body fields. Unknown message types are shown as **BLIND** (type and
  a fingerprint). Mainnet transactions say **MAINNET** on the screen; unknown chain ids say so too.
* A new account must register its ML-DSA key first (one transaction signed classical-only, with a proof of
  possession). The device proves the key and signs that one transaction; it signs classical-only for nothing else.
* Every signature is verified with its public key before it leaves the device (a fault-injection check).
* No hardware accelerators: ML-DSA-87 on the CPU is the fast path (about a millisecond on a laptop; the chip's
  Keccak engines, which leanSPHINCS needed, are slower than the CPU for this).

The Rust signer ([`crates/qorebit-signer`](crates/qorebit-signer)) is tested **byte for byte** against QoreChain's own
`@qorechain/qoresign`: keys, addresses, both signatures and the whole `TxRaw`, for v1 and v2 sign bytes, with and
without a passphrase (`make test-rust`, vectors in `tests/vectors.json`, regenerated with `make vectors`).
`make test-emu` goes further: the firmware, in the Xous emulator, signs a key registration and transfers that a
**real QoreChain node** (the local devnet) verifies and executes.

## Layout

| Path | What |
|---|---|
| `crates/qorebit-signer` | `no_std` + `alloc`: BIP-39 → keys, transaction decoding and policy, hybrid signing |
| `firmware/dabao-app-qorebit` | the Xous app: protocol, screens, two-button input, PIN, recovery, storage (host build = emulator) |
| `firmware/bio` | BIO-core programs for the panel's display and button (as in Quazar; the panel build only) |
| `tools/qorebit_display.py` | the **bridge**: device over USB HID or the emulator, the virtual display window, and the HTTP API for the website (`/rpc`, `/proxy/<network>/…`, `/devnet/fund`) |
| `tools/networks.json` | the three networks (chain ids, endpoints) |
| `tools/emu-test.sh`, `emu-test.mjs` | the end-to-end test on the emulator and the devnet |
| `tools/fund.mjs`, `qorelib.mjs`, `gen-vectors.mjs`, `press.mjs` | the devnet faucet, shared helpers, QoreSign vectors, a scripted "button" for demos |
| `companion/` | the wallet website (Vite + React + TypeScript) |
| `devnet/` | the local devnet: `get-bin.sh` (official node release, sha256 checked), `devnet.sh` |
| `xous-core/` | submodule: btchip/xous-core `devcon8-workshop` |

## The networks and the bridge

QoreChain's public endpoints send no CORS headers, so a browser can't call them. The bridge proxies exactly the
three networks in `tools/networks.json` (reads and the broadcast only, an allowlist of paths) and accepts only
localhost origins. The website also checks, before signing, that the node it talks to reports the chain id of the
selected network.

## Honest status

* **Verified:** the signer against QoreSign; the firmware in the emulator against a real node (registration,
  transfers, reject/approve threshold, PIN lockout, refusals); the website against the emulator, the testnet and
  mainnet (reads) and the devnet (writes); the board images build.
* **Not yet run on a board:** the firmware itself (ML-DSA-87 needs about 350 KiB of stack, so signing runs on a
  short-lived 416 KiB thread; the chip has 2 MiB of SRAM shared with the kernel and services: check with
  `make flash` and a transfer), and the two buttons on PB1 / PB2 (`SCREEN=panel`, as in Quazar). The panel build has run on a board: it shows the home screen and answers the host.
* The dev build draws the screen on the laptop, so malware there could fake the screen and the button. The real
  wallet is the panel build.
* No Chrome extension (Quazar had one); the website is the companion. Staking messages are shown and signed by the
  device, but the website has no staking page yet. One message per transaction; no off-chain message signing yet.
* Mainnet is reachable from the website. Nothing here has been tried with real funds. Use the testnet first.

See [CLAUDE.md](CLAUDE.md) for working notes and [firmware/dabao-app-qorebit/README.md](firmware/dabao-app-qorebit/README.md)
for the device side.
