# Qorebit: notes for coding agents

QoreChain hardware wallet on the Baochip-1x / Dabao board, **two buttons** (left PB1 = 0 / reject, right PB2 = 1 / approve).
Derived from the Quazar wallet (`~/work/baochip/quazar`, Ethereum + leanSPHINCS): same Xous firmware setup, bridge,
emulator and flashing; the chain, the signature scheme and the input are Qorebit's own. See `README.md` first.

## Repos and boundaries

- This repo holds all wallet code. `xous-core/` is a submodule (btchip/xous-core, `devcon8-workshop`): never
  commit inside it. `devnet/bin` (the QoreChain node release) and `devnet/.home` (chain data) are gitignored.
- Ground truth for the signature scheme is `@qorechain/qoresign` 0.2.0 / `@qorechain/pqc` 0.2.3 (pinned in
  `tools/package.json`). If the Rust signer and QoreSign disagree by one byte, the chain refuses the transaction:
  fix the Rust, or regenerate `crates/qorebit-signer/tests/vectors.json` on purpose (`make vectors`) and say so.
- Chain reference (networks, kits, rules): `~/work/cloister/docs/qorechain-reference.md`.

## Commands

```bash
make test                 # signer (vs QoreSign), firmware, companion unit tests; no devnet needed
make devnet               # fetch the node release (sha256 checked) + start a fresh local chain
make test-emu             # firmware in the emulator + the devnet: the chain verifies the device's signatures
make web-emu              # emulator + bridge + device window + website (http://localhost:5173)
make firmware-device      # board images (loader/xous/apps.uf2); make flash to flash
make stack                # ML-DSA-87 stack need (the firmware gives signing 416 KiB)
```

## The scheme (do not change without the vectors and a real node agreeing)

- Native lane: every tx needs **secp256k1 + ML-DSA-87**; a classical-only tx is refused, except the one that
  registers the PQ key (`MsgRegisterPQCKeyV2`, signed classical-only, carries a proof of possession).
- ML-DSA-87 signs `hybridSignBytes`: v2 = `"qorechain-pqc-hybrid-v2" | BE64(len chain) | chain | BE32(len B0) | B0 |
  BE32(len auth) | auth`, v1 = the same without the tag and chain id (chains that have not switched; all three
  current networks are v2). B0 = body without the `/qorechain.pqc.v1.PQCHybridSignature` extension (field 1023).
- secp256k1 signs `sha256(SignDoc{body incl. the PQC extension, auth, chain id, account number})`, compact, low-S,
  RFC 6979. So the classical signature covers the ML-DSA one: the device makes ML-DSA first.
- ML-DSA-87 signatures are deterministic (`rnd = 0`, empty context), as QoreSign makes them.
- Keys: BIP-39 seed (passphrase applies to the classical key only) → BIP-32 `m/44'/118'/0'/0/i`; PQ seed =
  `SHAKE256("qorechain:pqc:v1|" + bech32 address + "|" + mnemonic)`. Address = `bech32("qor", ripemd160(sha256(pk)))`.
- Proto3 canonical encoding: zero/empty fields are omitted (`proto.rs`).
- Networks: mainnet `qorechain-vladi`, testnet `qorechain-diana`, local `qorechain-local`; fee floor 0.1 uqor/gas;
  1 QOR = 10^6 uqor. The testnet/mainnet public REST has no CORS: browsers go through the bridge's `/proxy`.

## Firmware (`firmware/dabao-app-qorebit`)

- Build: `./build.sh emu | device` (via `make`). `vdisplay` (default) shows the screen in a window; `panel` is the
  ST7789 + buttons on PB1 / PB2 (`panel.rs`, uDMA SPI; it has booted on a board). PROG only enters the bootloader.
- Two buttons: right = 1 / approve, left = 0 / reject, no tap or hold. DISPLAY_INPUT carries `decision u8`.
  `flows.rs` talks to the screen through `Io` (`ask` → `Some(true)` right / `Some(false)` left); unit tests script it.
- State: keystore slots **320..=325** and one-way counter **233** (Quazar uses 300..=305 and 232: one board can hold
  either wallet). Seed is AES-KWP wrapped by the keystore (`domain "qorebit.seed"`); the emulator uses a host file
  (`QOREBIT_STATE`, default `target/qorebit-state.bin`; `tools/make-emu-state.mjs` writes a known wallet).
- Signing path: decode → `check` (policy) → approval screen (right button) → PIN → sign on the `crypto` thread (416 KiB stack,
  ML-DSA-87 needs ~350 KiB) → both signatures verified before release. Device refuses: >1 message, a message not
  from this account, another signer key, classical-only for anything but the registration, unknown body fields.
- The Keccak engines (combohash, BIO) are gone: ML-DSA on the CPU is the fast path. BIO is only for the panel.
- Stop processes by exact PID / process group only, never broad `pkill -f` patterns.

## Multisig (the chain's group module)

- Messages the device reads: `MsgCreateGroupWithPolicy` (threshold policy, <= 4 members, weight 1 each, no duplicates, admin = this
  account), `MsgSubmitProposal` (proposer = this account only, exactly one inner message from the group policy account: send, delegate,
  undelegate, claim; anything else inside shows BLIND), `MsgVote` (voter = this account), `MsgExec` (executor = this account).
  Everything else about a group is not signed unless it shows BLIND. Test vectors for them come from `cosmjs-types`
  (`tools/gen-vectors.mjs`, `crates/qorebit-signer/tests/group.rs`); `make test-emu` runs the whole cycle against the node.
- A group policy account is 32 bytes (62-character `qor1…`): addresses wrap onto three lines on the screen (`screen.rs::address`).
- The chain marks a proposal accepted only when it is run or its voting time ends: use the live tally (`/proposals/{id}/tally`).

## Rules

- Approval of a transaction happens on the device, never only in the companion. Never add an API that signs a bare
  host-supplied digest (`SIGN_TX` decodes and shows what it signs).
- Secrets: never log or print key material or the mnemonic. `SecretKey`/seeds zeroize on drop.
- The faucet words in `devnet/devnet.sh` and `tools/fund.mjs` are public on purpose (local chain only): never use
  them, or any key from this repo's tests, anywhere else.
- Commit only when the user asks. End commit messages with the attribution line the session provides.
