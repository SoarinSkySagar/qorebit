# AI usage

Qorebit was built with an AI coding agent, **Claude Code** (Anthropic, model Claude Sonnet 5.5), working in the
repository under the direction of the author, Sagar Rana.

## What the AI did

- Wrote most of the code and docs: the `qorebit-signer` Rust crate, the Xous firmware app, the bridge and device
  window, the companion website, the devnet scripts, the Makefile, the README, the walkthrough and the `/setup` skill.
- Ran builds, tests and the emulator, and fixed what failed (for example the ML-DSA-87 stack size, the transaction
  size limits and unknown message types).
- Read the earlier Quazar wallet's setup and reused its Xous firmware, bridge and flashing structure.

## What the author did

- Chose the product: a QoreChain hardware wallet on the Baochip with one button (tap = reject, hold = approve), a
  companion app with a mainnet / testnet / local devnet switch, and an emulator path for people without hardware.
- Set the constraints: use QoreChain's own signature scheme, a local devnet for testing, and the same setup as Quazar.
- Reviewed the results, made the project and naming decisions, and decided what to commit and publish.

## How correctness was checked (not taken on trust)

- The Rust signer is tested byte for byte against `@qorechain/qoresign` vectors (`make test`).
- `make test-emu` runs the firmware in the emulator and has a real local QoreChain node verify the device's
  signatures (29 checks).
- The device decodes every transaction itself and signs only what it shows; there is no API that signs a bare digest.

## Known limits

- The board images build, but nothing has been run on a physical Dabao yet: ML-DSA memory use on the 2 MiB chip, the
  PROG button and the panel display are unverified on hardware.
- Nothing was sent to mainnet or testnet; transactions were run on the local devnet only.
- This is a research prototype and has not been audited. Do not use it with real funds.
