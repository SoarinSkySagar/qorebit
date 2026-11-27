# Proof: transactions on the testnet

The fest checks features against the chain for the `qor1…` addresses listed on the project form, and jurors read this file. **Fill it from the
wallet's Activity screen and the explorer (<https://explore.qore.network>), one row per real transaction. Do not invent or round anything;
leave a row empty until it exists.** Network: `qorechain-diana` (testnet).

## Addresses on the project form

| Role | Account in the wallet | `qor1…` address |
|---|---|---|
| Sends, tokens, key registration | Account 1 | |
| Receiver | Account 2 | |
| Cross-VM caller (EVM lane) | EVM 1 | |
| Smart account owner | | |

## Transactions

| What | Signed by | Hash | Height |
|---|---|---|---|
| Register the post-quantum key (`MsgRegisterPQCKeyV2`, classical-only with the proof of possession) | Account 1 | | |
| Send QOR (hybrid secp256k1 + ML-DSA-87) | Account 1 | | |
| Token send (fUSD / fETH / fBTC) | Account 1 | | |
| Cross-VM call, EVM to Native (`MsgCrossVMCall`) | EVM 1 | | |
| Create a smart account (`MsgCreateAbstractAccount`) | | | |
| Set spending rules (`MsgUpdateSpendingRules`) | | | |

## Off-chain

| What | Where to see it |
|---|---|
| Sign In With QoreChain | `crates/qorebit-signer/src/siwq.rs`, `tests/siwq.rs`; `make test-emu` verifies with `verifySiwq`; `extension/demo/` |
| Byte-for-byte match with QoreSign | `make test-rust`, `crates/qorebit-signer/tests/vectors.json` (`make vectors`) |
| The chain verifies the firmware's signatures | `make devnet && make test-emu` (58 checks) |
