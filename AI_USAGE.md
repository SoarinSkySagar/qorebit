# AI usage

Qorebit was built with an AI coding agent, **Claude Code** (Anthropic, model Claude Sonnet 5.5), working in the repository under the
direction of the author, Sagar Rana. This file is for QoreChain's "Build Before Q-Day" fest (Best AI-Native Build): it says who did what, how
the work was checked, and what the AI got wrong.

**In short: the author did the research, the system design and the planning, and directed and tested every step; the AI did the coding and the
implementation.** Most of the author's time went into understanding what had to be built, not into typing code.

## What the author did

- **The research, which took most of the time.** Before any code, the author studied the things the wallet depends on: how QoreChain works (its
  native lane, the hybrid secp256k1 + ML-DSA-87 signature, key registration, the networks, its kits and modules), how wallets work (seeds and
  derivation, accounts, what a hardware wallet must and must not trust, how transactions are decoded, shown and approved), and the Baochip-1x and
  Dabao board the wallet runs on. That research is what the design rests on.
- **The system design and the plan.** The author decided what Qorebit is and how its parts fit: a QoreChain hardware wallet on the Baochip with
  two buttons (left = reject, right = approve); the device decodes and signs only what it shows, with no API that signs a bare digest; a bridge
  and a companion website; an emulator path for people without hardware; QoreChain's own signature scheme, tested against its reference library
  and a local devnet; the same setup as the earlier Quazar wallet. The author also set the order of the work and what to add next (tokens,
  multisig, EVM accounts and Cross-VM, smart accounts, Sign In With QoreChain, the extensions and the provider for websites).
- **Micromanaged the build.** The author gave the AI the work in small, specific requests, set the constraints ("the board only labels multisig"
  and "there should be text specifying it's a multisig txn"; no board or screen changes that were not asked for), and corrected course whenever the
  result was not what they meant.
- **Tested everything by hand, as the user.** The author plugged in and flashed the Dabao, pressed the buttons, entered the PIN, tried the testnet
  flows, the extension in the browser and the demo website, and judged the UI and UX themselves: the purple wallet, the account menu, the device
  window. What they found went back to the AI as bug reports: the board's home balance flipping between accounts, an out-of-gas cross-VM call, the
  approval window saying "nothing to approve", the Firefox "No device answers".
- **Reviewed the code.** The author read the code themselves, alongside the AI's own review, and took bugs out.
- Made the project and naming decisions and decided what to commit and publish. Commits are the author's.

## What the AI did

- **All of the coding and implementation**, to the author's design: the `qorebit-signer` Rust crate (keys, protobuf decode, policy, hybrid
  signing, SIWQ), the Xous firmware app (protocol, screens, PIN, recovery, storage), the Python bridge and the virtual device window, the
  companion website (React, TypeScript), the Chrome and Firefox extensions and the `window.qorechain` provider, the demo website, the devnet
  scripts, the Makefile, and the tests.
- Wrote the documentation to match: the README, the walkthrough, the submission files and the `/setup` skill.
- Ran builds, tests and the emulator, flashed the board with the author at the keyboard, and fixed what failed: the ML-DSA-87 stack size (a
  416 KiB signing thread), transaction size limits, unknown message types, cross-VM gas, the app image size limit, the bridge refusing a Firefox
  origin.
- Read the author's earlier Quazar wallet and reused its Xous firmware, bridge and flashing structure (credited in the README).

## How correctness was checked (not taken on trust)

- The Rust signer is tested byte for byte against `@qorechain/qoresign` vectors (`make test`): keys, addresses, both signatures and the whole
  `TxRaw`, for v1 and v2 sign bytes, and the SIWQ signature. If the Rust and QoreSign disagree by one byte, the chain refuses the transaction,
  so the vectors are the oracle, not the AI's reading of a spec.
- `make test-emu` runs the firmware in the emulator and has a real local QoreChain node verify what the device signs (58 checks: key
  registration, transfers, reject and approve, wrong PIN, refusals, EVM keys, smart accounts, SIWQ, multisig).
- The companion has unit tests (31, including the site transaction parser) and scripted tests of the extension provider against a mocked
  browser; `web-ext lint` passes on the Firefox build with no errors.
- Golden bytes for the cross-VM and smart-account messages were taken from the node's own CLI, not written from memory.
- The device decodes every transaction itself and signs only what it shows; there is no API that signs a bare digest. Both signatures are
  verified before they leave the device.

## What the AI got wrong, and how it was caught

- Gave the signing path too little stack at first; `make stack` measured that ML-DSA-87 needs about 350 KiB, and the thread was sized to 416 KiB.
- Shipped a cross-VM call with the default gas; it ran out of gas on the testnet. A headroom rule was added after the author reported it.
- Chose a cross-VM example whose spender was the signer's own address; the chain refused it ("Cannot set to own account").
- Let the website race itself: a stale browser tab sent another account's balance to the board, so the board's home screen flipped between the
  native and the EVM balance. The author noticed it on the board; the fix is a visible-tab-only update.
- Routed the approval popup as if it were a website, so the demo page said "nothing to approve"; the author's test found it.
- Emptied a test file with a careless script edit; the file was restored from git and the test suite re-run.
- Documentation drifted: the README still said nothing had run on a board after it had. This revision corrects it.

## Known limits

- This is a research prototype and has not been audited. Do not use it with real funds.
- Signing, transfers, tokens, Cross-VM and smart accounts were run on a Dabao and the testnet; the panel build's own display and buttons are the
  least tested part ([HARDWARE.md](HARDWARE.md)). The emulator path is what others can run (`make demo`).
- Mainnet is reachable but nothing was sent to it. Not built: raw EVM transactions, off-chain message signing, smart-account key linking.
