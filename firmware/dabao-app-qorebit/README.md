# dabao-app-qorebit

Qorebit on the dabao: a Xous app that signs QoreChain transactions (secp256k1 + ML-DSA-87, `qorebit-signer`) and
answers a host over USB HID, or over TCP in the emulator. Structure follows `ethereum-app/dabao-app-eth-burner`
and the Quazar wallet.

**Two buttons.** The left one is 0 / reject, the right one is 1 / approve; one press is the answer. Every screen is
answered with them: approvals, the 8-press signing PIN, word-by-word recovery, confirmations. Sources of the buttons:
the window of the bridge (dev build), or PB1 (left) and PB2 (right) on the panel build (`panel.rs`).

**Signing.** `SIGN_TX` takes the unsigned `TxBody` and `AuthInfo`; the device decodes them (`qorebit-signer`'s
`tx::decode`), applies its policy (`tx::check`), shows them, waits for the right button, asks the PIN, then signs on a
short-lived thread with a 416 KiB stack (ML-DSA-87 needs about 350 KiB) and verifies both signatures before
releasing them. `REGISTER_PROOF` makes the proof of possession a new account's key registration carries.

**Seed and PIN (on the device):** on first boot the app creates 32 bytes of entropy from the chip TRNG, has the
keystore wrap it (AES-KWP under the chip's master key, which never leaves the keystore) and stores only the
wrapped blob in keystore application slots 322/323 (320/321 are the plain-seed slots, wiped when found). The PIN
record is in 324/325 and its attempt count on one-way counter 233 (Quazar: 300..=305 and 232, so one board can hold
either). The emulator keeps both in a host file (`QOREBIT_STATE`, default `target/qorebit-state.bin`).

**Accounts:** 16 from the one seed, derived on demand: 0..8 native (`BIP-32 m/44'/118'/0'/0/i`, sha256 digest) and 8..16 EVM lane
(`m/44'/60'/0'/0/i`, keccak256 digest, `eth_secp256k1`), each with its own ML-DSA key from the account's address. `check` refuses a
transaction whose key type is not the account's.

**Other commands:** `SIGN_SIWQ` (0x07, Sign In With QoreChain, strict parse, this account only). Multisig, Cross-VM and smart-account messages
are labelled MULTISIG / CROSS-VM / SMART ACCT on the screen and signed without decoding their fields (`Msg::Other`).

**Size:** the app image must fit the Dabao's 893,696-byte app region; `build.sh` fails above it (a larger image flashes but never starts).

**Build:** `./build.sh emu` (host binary for `cargo xtask dabao-emu`), `./build.sh device` (riscv binary and
`loader/xous/apps.uf2` in xous-core's target dir). Options `--vdisplay[=false]`, `--panel`, `--panel-udma`; use
`make` (see the root `Makefile`). The wire protocol is documented in `src/protocol.rs`.

**Wiring for the panel build** (as in Quazar): SPI2 on PC0 (SCK), PC1 (MOSI), PC3 (CS0); DC on PC2; RES on PB4;
backlight on PB5; the buttons from PB1 (left) and PB2 (right) to GND (the chip's pull-ups). The full pin table, a diagram and the
bring-up steps are in [`HARDWARE.md`](../../HARDWARE.md).
