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

**Open hardware, verifiable chip:** it runs on the open-source Baochip-1x silicon and the open Dabao board, so the whole stack from the RTL to the website can be read and checked ([details](#open-hardware-and-a-verifiable-chip)). To our knowledge this may be the first post-quantum hardware wallet built on open-source, verifiable hardware.

**QoreChain "Build Before Q-Day" entry.** What the fest asks for, how Qorebit answers it, the form text and the open items are in
[SUBMISSION.md](SUBMISSION.md); the 3 minute video script is [DEMO.md](DEMO.md); testnet transactions go in [PROOF.md](PROOF.md);
how AI was used is [AI_USAGE.md](AI_USAGE.md).

## Open hardware and a verifiable chip

Qorebit runs on the **Baochip-1x**, a security chip that is open down to the silicon, on the **Dabao** board, which is open too. So the wallet
is open source at every layer you can inspect: the signer and firmware (this repository, MIT), the OS (Xous, Rust), the board and the chip's design.

| Layer | What is open | Licence |
|---|---|---|
| Wallet: signer, firmware, bridge, website, extensions | this repository | MIT |
| OS | [Xous](https://github.com/betrusted-io/xous-core), a pure-Rust microkernel OS | open source (see `xous-core/`) |
| Chip | the Baochip-1x RTL (Verilog / SystemVerilog, with parts from SpinalHDL and LiteX): 22 nm TSMC, a 350 MHz RISC-V main core with an MMU, four 700 MHz BIO cores, 2 MiB SRAM, 4 MiB RRAM | CERN-OHL-W-2.0 ([baochip/baochip-1x](https://github.com/baochip/baochip-1x)) |
| Board | the Dabao's KiCad schematic and layout, STEP files and pinout | CERN-OHL-W-2.0 ([baochip/dabao](https://github.com/baochip/dabao)) |

**Verifiable.** The chip is meant to be checked, not trusted. It is designed to be compatible with **IRIS inspection**, a method for
non-destructively imaging a chip to compare it with its published design, and Baochip publishes the expected hashes of its boot code, built with
a reproducible toolchain. On your own board, `make audit` asks the bootloader to hash `boot0` and `boot1` as they sit in the chip and compares
them with those published hashes (read-only; it also prints the chip's lifecycle counters), and `make security` shows its security state. Qorebit
only uses what the chip makes verifiable: the key never leaves the chip's keystore, the seed is wrapped by a chip key, and the firmware you
flash is the firmware you can read and rebuild.

**What the chip gives a wallet.** A hardware true random number generator (the seed is made on the device), hardware-protected key slots (the
keystore that wraps the seed), one-way counters (the PIN's attempt counter cannot be rolled back), glitch sensors and ECC-protected RAM, and a
signed boot chain with optional hardening (`make harden`, irreversible).

**To our knowledge this may be the first post-quantum hardware wallet on open-source, verifiable hardware.** It signs QoreChain's ML-DSA-87
signatures on a chip whose design anyone can read. We have not surveyed every wallet, so treat that as a claim to check, not a fact.

**What "open" does not mean here.** The chip is *mostly* open, not fully: its designer lists the bus fabric, the analog blocks (PLL, USB, voltage
regulators, ADC), the built-in self-test details and the I/O pads as closed, and argues they are effectively wires. He also says open silicon is
"not perfect transparency". `make audit` checks the boot code, not the silicon; checking the silicon itself (IRIS) needs equipment and an expert,
and has not been done on our boards. The post-quantum signing here is our own firmware on the chip's CPU, not a hardware block. The Dabao board is
a development board: the real wallet is the panel build with its own display and buttons, and it is unaudited.

## Note: try it without the hardware (demo only)

An **emulator + virtual display** setup is included for anyone who wants to try the wallet without a Dabao board. The same firmware runs in the
emulator on your computer, with the device's screen and its two buttons (REJECT and APPROVE) drawn in a window, and a local QoreChain node
verifies the signatures. Anyone with this repository can start it with `make demo`.

**It only works as a demo. Do not use real funds with the emulator**: it keeps its keys in a file on your computer, and malware there could fake
the screen and the buttons. Use the local devnet or the testnet. Real funds belong only on a board with its own panel and buttons
(`make flash-panel`); even a Dabao with the virtual display (`make flash-virtual`) shows its screen on your computer, so treat it as development only.

## Quick start

Everything runs through `make` (`make help` lists every command).

```bash
git clone --recursive <this repo> && cd qorebit
make install      # Rust, Node, submodule, npm, Python venv, Xous toolchain (make setup-emu: emulator only, faster)
make demo         # NO HARDWARE NEEDED: local QoreChain + emulated device with its screen window + website

make flash-virtual && make board-virtual   # a Dabao, its screen as a window on your laptop
make flash-panel   && make board-panel     # a Dabao with its own panel + button, no window
```

**Step by step guide: [walkthrough.md](walkthrough.md).** In the website: **Set the PIN on the device**, choose the
network, get funds (the devnet has a one-click faucet, the testnet a link to QoreChain's), **register the
post-quantum key** once per account and network, then send. Pin connections for a board: [HARDWARE.md](HARDWARE.md).
Browser extension: `make extension` (Chrome, Brave, Edge: side panel) or `make extension-firefox` (Firefox: sidebar); see the
walkthrough and [extension/STORE.md](extension/STORE.md) (publishing). A demo website for `window.qorechain`: `make demo-site`.

With Claude Code, run **`/setup`** (or `/setup emu`): it does the steps above and waits for you at the PIN.

### Sign in with QoreChain

Log in to a website with the hardware wallet (QoreChain's SIWQ standard). The board parses the site's message itself, shows
the site, network, expiry and statement, and signs both layers (secp256k1 over the ADR-36 document, ML-DSA-87 over a
domain-prefixed message). It signs nothing that is not a canonical sign-in message, and nothing for another account. Matches
QoreSign byte for byte (`crates/qorebit-signer/src/siwq.rs`); with the browser extension a site gets the signature straight from the wallet (`window.qorechain`), no pasting; see [walkthrough.md](walkthrough.md), sections 6a and 6f.

### Tokens

The companion lists the tokens on the selected network (on the testnet: the test tokens fUSD, fETH, fBTC), adds any token by
contract address or coin denom after asking the chain about it, and sends them. A token transfer is an ordinary bank transfer
signed on the device, which shows the amount in the token's smallest unit and the coin name on separate rows (see
[walkthrough.md](walkthrough.md), section 6b).

### Multisig

Shared accounts ("N of M must agree") use QoreChain's group module, so the board needs nothing special: creating a
multisig, proposing a payment, voting and running it are each an ordinary transaction that one member signs with their
own device. The companion app builds and explains them (the **Multisig** card); the board signs them and its screen says
**MULTISIG** and names the action (create, propose, vote, run), but not the details, so check those on the website
before you approve. See [walkthrough.md](walkthrough.md) (section 6c).

### EVM accounts and Cross-VM calls

QoreChain has a second account type that works with EVM tools: coin type 60, an `eth_secp256k1` key, the same 20 bytes as a `0x…`
address and a `qor1…` address, signed over a Keccak-256 digest. The device derives eight of them (EVM 1..8) from the same 24 words, next to
the eight native accounts, and refuses a transaction whose key type is not the account's. From an EVM account a **Cross-VM call** runs a
native (CosmWasm) contract (**EVM to Native**); from a native account it goes the other way. The board says **CROSS-VM**, names the action and
leaves the details to the website, like multisig. Verified on the testnet (EVM account to a CW20 contract: `evm → cosmwasm`, executed).

### Smart accounts and spending rules

The companion's **Smart account** card creates one of the chain's `abstractaccount` accounts (session based, multisig, social recovery) and
edits its spending rules (per transaction and daily limits, coins, EVM calls). Both are ordinary transactions the owner signs; the board says
**SMART ACCT** and leaves the details to the website. The rules govern what linked keys and relayers spend for the account, not what you
send yourself with the device. Linking keys and relayed execution are not in the wallet yet.

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
| `SUBMISSION.md`, `DEMO.md`, `PROOF.md`, `AI_USAGE.md` | the QoreChain fest: criteria and form text, the video script, testnet hashes, AI log |
| `extension/` | Chrome and Firefox extensions, `window.qorechain`, `demo/` (the demo website), `STORE.md` |
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

* **Verified:** the Rust signer against `@qorechain/qoresign` (keys, addresses, both signatures, the whole `TxRaw`, v1 and v2);
  the firmware in the emulator against a real node (`make test-emu`, 58 checks: registration, transfers, reject / approve, PIN lockout,
  refusals, EVM keys, smart accounts, SIWQ, multisig); the website and the extensions against the emulator, the devnet and the testnet.
* **Run on a Dabao board:** signing (ML-DSA-87 needs about 350 KiB of stack, so it runs on a short-lived 416 KiB thread on the chip's 2 MiB of
  SRAM), transfers, tokens, Cross-VM calls and smart accounts on the testnet, the demo website with the Chrome and Firefox extensions. The
  panel build (own ST7789 display and the buttons on PB1 / PB2, [HARDWARE.md](HARDWARE.md)) is the least tested part.
* The dev build draws the screen on the laptop, so malware there could fake the screen and the button. The real wallet is the panel build.
* One message per transaction. Not built: raw EVM transactions (the EVM window), off-chain message signing (`qor_signMessage`), linking keys
  and relayed execution for smart accounts, a website staking page (staking messages are shown and signed by the device).
* The extension has been run by the author in a real browser against the demo site (connect, Sign In With QoreChain, send; the Firefox build
  with a board). The Chrome build is tested by scripts and `web-ext`-style checks; neither is published (see `extension/STORE.md`).
  Firefox needs one extra step the first time (the bridge must be told its address).
* Mainnet is reachable from the website. Nothing here has been tried with real funds. Use the testnet first. Not audited.

## Credits

* **Quazar** (the author's earlier Ethereum + leanSPHINCS wallet) gave the setup this reuses: the Xous firmware app structure, the bridge,
  the virtual display, the emulator and the flashing scripts. Everything on QoreChain, the signer, the screens, the two-button input, the
  companion app, the extensions and the tests are new in this repository (first commit 10 Nov 2026).
* **Xous** (`xous-core/`, the `btchip/xous-core` fork, branch `devcon8-workshop`) and the **Baochip-1x / Dabao** by the Baochip project.
* **`@qorechain/qoresign` 0.2.0, `@qorechain/pqc` 0.2.3** (QoreChain): the reference the signer is tested against.
* **`@qorechain/sdk` 0.14.0 and `create-qorechain-dapp` 0.14.0** (QoreChain): `companion/` was started from the scaffolder's `fullstack-web` starter.
  The command we ran (into a scratch folder):
  ```
  npx create-qorechain-dapp@0.14.0 qorebit-companion --template fullstack-web --network testnet --package-manager npm --no-install --yes
  ```
  We then merged the starter's skeleton into `companion/` (its `.env` / `.env.example`, the `/lcd` dev proxy in `vite.config.ts`, `src/vite-env.d.ts`,
  its README as `STARTER-README.md` and its screen unchanged as `src/starter/App.tsx`) and built the wallet UI on top (everything else under
  `src/`). The wallet uses the SDK the way the starter does: `createClient` pointed at the chain's REST (through the bridge's proxy, where the
  starter uses its dev server), for the **Chain status** panel on Home. Signing never goes through the SDK: it stays on the device.
  The browser extension's side panel is this companion built for a browser panel (`extension/build.sh`); the extension's own background, content and
  provider scripts and the demo page are our own and were not generated by the scaffolder.
* **`apps/web`: the generated starter app.** Created with `npx create-qorechain-dapp@0.14.0 apps/web --template fullstack-web --network testnet --package-manager npm --no-install --yes`,
  structure kept; the only additions are `src/WalletConnect.tsx` (connect **QoreX** via `window.qorex.native`, or **Qorebit** via `window.qorechain`, and send one
  `MsgSend` that the wallet signs with both keys) and two lines in `src/App.tsx`. Built and type-checked; the QoreX path has not been run against a real QoreX yet.
* Rust crates `fips204` (ML-DSA), `k256` (secp256k1), RustCrypto's `sha2`, `sha3`, `ripemd`, `hmac`, plus `bech32`, `bip39` and `zeroize`; React, Vite
  and TypeScript in the companion. See `Cargo.lock` and `companion/package.json`.
* The code was written with Claude Code (Anthropic) under the author's direction: [AI_USAGE.md](AI_USAGE.md).

See [CLAUDE.md](CLAUDE.md) for working notes and [firmware/dabao-app-qorebit/README.md](firmware/dabao-app-qorebit/README.md)
for the device side.
