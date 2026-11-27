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
make test-emu             # firmware in the emulator + the devnet: the chain verifies the device's signatures (58 checks)
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

## Sign In With QoreChain (SIWQ)

- Device command `SIGN_SIWQ` (0x07), `crates/qorebit-signer/src/siwq.rs`: strict parse (the message must re-render to exactly the text,
  so nothing else can be signed), `check` (this account only, no resources, statement <= 75, site <= 44 characters), then secp256k1 over
  `sha256(ADR-36 document)` and ML-DSA-87 over `"qorechain-siwq-v1|" + message`. Byte-identical to `@qorechain/qoresign` (vectors from
  `tools/gen-vectors.mjs`, `tests/siwq.rs`; `make test-emu` verifies with `verifySiwq`).
- Companion: `SignIn.tsx` (paste a message or make a test one; the signature JSON is copied back into the site by hand; there is no
  provider API in the browser yet).

## Tokens

- A token is a bank coin `erc20:0x<contract, EIP-55 casing>` (the chain's denom metadata gives symbol, name and decimals: the largest
  `denom_units` exponent). Sending one is a plain `MsgSend`. `companion/src/tokens.ts`, `tokenUnits.ts`, `Tokens.tsx`.
- The board shows a non-QOR coin as two rows: `Amount` (raw, smallest unit) and `Token` (`denom_text`, e.g. `erc20:0x9714..63CA`); before
  this, a long denom pushed the amount off the left edge of the screen. QOR sends are unchanged.
- Verified on the testnet: 1 fUSD, 0.001 fETH and 0.0001 fBTC sent from account 1 to account 2.

## Multisig (the chain's group module): companion only

- The board only labels multisig: any `/cosmos.group.v1.*` message shows a MULTISIG screen with the action name and "Details: check the
  website" (no decoding of members, amounts or proposals; the signer treats it as `Msg::Other`). The user chose this: "on the board we
  simply need to sign", but "there should be text specifying it's a multisig txn". Everything multisig lives in `companion/src/multisig*.ts` and `Multisig.tsx`; `make test-emu` runs the whole
  cycle (create 2-of-2, fund, propose, two votes, run) against the node.
- A group policy account is 32 bytes (62-character `qor1…`): the screen wraps addresses onto three lines (`screen.rs::address`) and the
  companion accepts 20- and 32-byte addresses (`isQorAddress`).
- The chain marks a proposal accepted only when it is run or its voting time ends: use the live tally (`/proposals/{id}/tally`).
- The app image must fit the Dabao's app region (893,696 bytes); `build.sh` fails above it, and Cargo.toml optimises for size except the
  signature crates. An image over the limit flashes fine and never starts (the board then answers nothing over USB).

## EVM-lane accounts and Cross-VM

- `Wallet::account(index)`: 0..8 native (`m/44'/118'/0'/0/i`, sha256 digest, `secp256k1.PubKey`), 8..16 EVM lane (`m/44'/60'/0'/0/i`, keccak256 digest,
  `/cosmos.evm.crypto.v1.ethsecp256k1.PubKey`; the address is the keccak of the public key, the same 20 bytes as the `0x` address). The PQ key seed
  formula is unchanged (it uses the bech32 address). `check` refuses a transaction whose key type is not the account's. Vectors: the `evm` section.
- `MsgCrossVMCall` (`companion/src/crossVmMsg.ts`, verified against `qorechaind tx crossvm call`); the board labels `/qorechain.crossvm.v1.*` as CROSS-VM.
  An ERC-20 `transfer` through a cross-VM call reverts (the EVM caller is not the account that holds the balance): the card offers a generic call.
- The EVM raw-transaction path (the 0x…0901 precompile, the EVM window) is NOT implemented.
- The site sends the board's home balance only from the visible tab (`document.hidden`): another tab on another account used to overwrite it.

## Smart accounts (the chain's abstractaccount module): companion only

- `MsgCreateAbstractAccount` (owner 1, account_type 2) and `MsgUpdateSpendingRules` (owner, account_address, rules): encoders in
  `companion/src/smartAccountMsg.ts`, golden bytes from the node in `test/smartaccount.test.mjs`; the board labels `/qorechain.abstractaccount.v1.*`
  as SMART ACCT (`Msg::Other`, no decoding). `make test-emu` creates one and sets a rule on the devnet.
- The chain gives each new smart account its own address (older testnet ones have the owner's address): find them with `GET
  /qorechain/abstractaccount/v1/accounts` filtered by `owner`; rules at `/accounts/{address}/spending_rules`. Limits are in uqor.
- Rules bind linked keys (ed25519 authenticators) and relayers, not the owner's own direct sends. Linking keys and `execute-cosmos` are not built.

## The wallet UI and the extension provider

- `companion/src/App.tsx` is a shell (header with account and network dropdowns, tab bar Home / Activity / Apps / Device) around one screen per
  feature (`Shell.tsx`: icons, menus, tab bar). Purple theme = the board's palette (`styles.css`). The feature cards (Tokens, Multisig, Smart
  account, Cross-VM with its example buttons, Sign in) are unchanged inside their screens. The extension and the website are the same app.
- Extension (`extension/`): `inject.js` (MAIN world, `window.qorechain`: `qor_requestAccounts`, `qor_accounts`, `qor_chainId`, `qor_signIn`, `qor_sendTransaction`),
  `content.js` (relay; the site comes from the browser's `sender.origin`), `background.js` (connections in `chrome.storage.local`, one waiting request
  per site, an approval popup `index.html#/approve/<id>` rendered by `Approve.tsx`). The wallet writes `qorebit.state` (selected account, chain) to
  extension storage for it. `tools/test-extension-provider.mjs` tests the background against a mocked `chrome`; `check-extension.mjs` pins the
  permissions (`sidePanel`, `storage`) and the two content scripts. Not tried in a real Chrome by the author yet (the Firefox build was).

- Password lock (extension only): `lock.ts` (PBKDF2-SHA-256, 600k rounds, salted hash in `chrome.storage.local`; the unlocked flag in
  `chrome.storage.session`, so it lasts until the browser closes; wrong-try throttle), `LockScreen.tsx` (`LockGate` wraps the wallet and the
  approval window in `main.tsx`; autofocus + Enter; `PasswordCard` in Device). It gates the pages, not the keys. No reset in the UI on purpose.

- Firefox: `extension/manifest.firefox.json` (MV3, `background.scripts`, `sidebar_action`, gecko id, `data_collection_permissions: none`, host permissions for the
  pages the content scripts run on); `make extension-firefox` -> `extension/dist-firefox`; scripts use `browser ?? chrome` (`companion/src/ext.ts`).
  Its origin is a random `moz-extension://uuid`: the bridge takes exact origins from `extension/EXTENSION_ID` (one id per line, add the Chrome Web Store
  id) and `QOREBIT_EXTENSION_ORIGINS`; the app shows the command when the bridge answers 403. `make extension-zip` -> store zips (`STORE.md`). `web-ext lint`
  passes (0 errors; 2 innerHTML warnings are React's). The author ran it in a real Firefox (temporary add-on) against a board, after allowing its `moz-extension://` origin in the bridge.

- `qor_sendTransaction` (one message; `siteTx.ts` validates and describes it, `Approve.tsx::SendTx` checks the chain, shows the fee, the device signs, the wallet
  broadcasts and waits; same pipeline as `App.submit`, duplicated on purpose to keep the approval window self-contained). `extension/demo/` is a test page.
  Parsing and the background rules are tested; the whole flow (demo page -> extension -> board -> chain: connect, Sign In With QoreChain, send) was run by the author in a real browser.

## Open hardware, verifiable chip (a headline feature: keep it in the docs)

- Baochip-1x RTL and the Dabao KiCad design are CERN-OHL-W-2.0; Xous is Rust; this repo is MIT. The chip is "mostly open" (closed: bus fabric, analog IP, self-test
  details, pads) and designed for IRIS inspection; `make audit` (`tools/audit.py`) checks boot0/boot1 hashes only. Say "to our knowledge, possibly the first
  post-quantum hardware wallet on open-source, verifiable hardware": never state it as fact or claim the silicon was inspected. Text lives in README ("Open
  hardware and a verifiable chip"), SUBMISSION.md, HARDWARE.md.

## The fest this is entered in (QoreChain "Build Before Q-Day", build.qorechain.io)

- Read `SUBMISSION.md` first: criteria, category analysis, form text, checklist. `DEMO.md` = 3 minute video script, `PROOF.md` = testnet
  hashes (fill from real transactions only), `AI_USAGE.md` = the Best AI-Native Build log. Re-read build.qorechain.io `/rules`, `/prizes`,
  `/faq` before editing them: the organisers may change them (last read 27 Nov 2026).
- Deadline **2 Dec 2026 23:59 UTC**; no submit button, the project form is the submission. Judging: works as described 25%, QoreChain tech
  verified in the repo and on chain 25%, post-quantum correctness 20%, originality 15%, clarity / UX / docs 15%.
- Required: public repo with MIT / Apache-2.0 licence and README, a video up to 3 minutes, testnet `qor1…` addresses, honest kit and feature
  ticks (kits from dependency files, features from transactions of the listed addresses), libraries credited in the README, only commits
  from 4 Nov. Category: **Quantum Pass** (decided by the author, 27 Nov); `create-qorechain-dapp` is not used, which is the open question.
- Keep claims verifiable: do not write a transaction hash, address, test count or "verified on hardware" that was not seen. Keep README
  "Honest status", `AI_USAGE.md` and `SUBMISSION.md` section 7 in step with the code when a feature lands.

## Rules

- Approval of a transaction happens on the device, never only in the companion. Never add an API that signs a bare
  host-supplied digest (`SIGN_TX` decodes and shows what it signs).
- Secrets: never log or print key material or the mnemonic. `SecretKey`/seeds zeroize on drop.
- The faucet words in `devnet/devnet.sh` and `tools/fund.mjs` are public on purpose (local chain only): never use
  them, or any key from this repo's tests, anywhere else.
- Commit only when the user asks. End commit messages with the attribution line the session provides.
