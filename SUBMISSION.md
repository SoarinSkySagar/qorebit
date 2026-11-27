# Submission: QoreChain "Build Before Q-Day"

Qorebit is entered in QoreChain's **Build Before Q-Day** fest (<https://build.qorechain.io>). This file keeps what the organisers
ask for, how Qorebit answers each point, the text for the project form, and what is still open. The pages were read on
**27 Nov 2026**; the site says rules, prizes and dates can change until the event ends, so re-read `/rules`, `/prizes` and `/faq`
before you submit. Working notes for agents are in [CLAUDE.md](CLAUDE.md); the run-it-yourself guide is [walkthrough.md](walkthrough.md).

## 1. What the organisers say

**Dates (UTC).** Building from 4 Nov (only commits from 4 Nov count; ours start 10 Nov). **Submission deadline 2 Dec 2026, 23:59**; the
project is read-only after it and there is no submit button, so the form must be complete. Checks 3 to 5 Dec, jury 6 to 15 Dec, winners and
live demo show 16 Dec, payouts 17 to 22 Dec, follow-on QOR grants from January 2027. Workshop 4 (protocol building and submission clinic):
27 Nov, 16:30. The Q-Day Drill (17 to 19 Nov) is over.

**Judging.** Every category is scored on the same five criteria (FAQ):

| Criterion | Weight | Where Qorebit answers it |
|---|---|---|
| Works as described | 25% | `make test`, `make test-emu` (58 checks against a real node), the walkthrough; transactions signed by the board on the testnet |
| QoreChain tech usage, verified in the repo and on chain | 25% | QoreSign 0.2.0 and qorechain-pqc 0.2.3 in `package.json`; the hybrid signature, PQC key registration, Cross-VM, smart accounts and tokens as testnet transactions (section 4) |
| Post-quantum correctness and security | 20% | the Rust signer matches `@qorechain/qoresign` byte for byte; both signatures are verified on the device before release; the device decodes and shows what it signs; no API signs a bare digest |
| Originality | 15% | a QoreChain hardware wallet: keys and ML-DSA-87 on a two-button device with its own PIN, not a browser or phone wallet; built on **open-source, verifiable hardware** (the Baochip-1x RTL and the Dabao board are CERN-OHL-W-2.0, boot code checkable with `make audit`); to our knowledge possibly the first post-quantum hardware wallet on such hardware (unverified claim) |
| Clarity, UX and documentation | 15% | README, walkthrough, `/setup` skill, HARDWARE.md, this file, DEMO.md |

Jurors score by hand, check the repo items themselves and may ask finalists for a change (48 hours). Checks that must pass for any category:
public GitHub repo under **MIT or Apache-2.0** (the licence file must be detected by GitHub), a **README**, **video up to 3 minutes** showing the
project working, **at least one testnet `qor1…` address** the project uses, every team member registered, commits made during the fest,
kits and features ticked only if they are really used. A 16:9 **cover image** (at least 1200 x 675, JPG / PNG / WebP, up to 5 MB): the Start page calls it optional (one is made from the name), but the Rules page lists "a featured image" as required and the checks list it, so **make one**.
Open-source libraries, kits and templates **must be credited in the README** (see its Credits section). Pre-built or copied projects are disqualified: "Projects are built during the event" and "only commits made from 4 November count". The FAQ allows reusing your own idea and experience, and your own published libraries when credited: "start a fresh repository and build the QoreChain part during the fest." (Second reading of Rules, Start, FAQ and Prizes on 27 Nov: no country restriction, no ban or disclosure rule for AI-written code, and nothing about hardware projects. Everything runs on the testnet; People's Choice voting is on mainnet.)

**Kits and features are verified by the organisers.** A kit is "verified" when its package name appears in a dependency file. A network feature is
"verified" when transactions from the project's listed `qor1…` addresses show it. Checks run on save, on every push, daily and on "Check again" in
My space. Install the **QoreChain Build GitHub App** on the repo (My space) so commits show in the feed.

**Special awards** (on top of a category prize unless noted): Best AI-Native Build ($500: tick the option and keep `AI_USAGE.md`), Quantum Campus ($500,
every member a student), Regional ($200 x 5, only if no other prize), Top-Quality ($250 x 7, only if no other prize, jury score of at least 70/100),
People's Choice ($500 / $300 / $200, community vote 6 to 15 Dec with QoreX on mainnet, at least 10 QOR in the wallet), Q-Day Survivor (closed).

## 2. Which category

The six categories are Quantum Pass (1 to 2 people), Post-Quantum dApp (EVM, 1 to 3), Post-Quantum AI Agents (1 to 4), Encrypted Apps
(ML-KEM, 1 to 4), Cross-VM (1 to 4, prize still "to be announced") and Protocol Builders (rollups, **2 to 4 people, no solo**). A hardware wallet
is none of them exactly, and no page mentions hardware wallets. The honest fit:

| Category | Fit | Why |
|---|---|---|
| **Quantum Pass** (recommended) | closest | "a small original QoreChain app, tool, game or bot", solo allowed. Needs: connect QoreX, a hybrid-signed transaction on the testnet (done many times). **Gap: it asks for `create-qorechain-dapp`, which Qorebit does not use** |
| Cross-VM | partial | the wallet signs a verified EVM to Native `MsgCrossVMCall` on the testnet, but the category is about a contract calling a contract, and its prize is not announced |
| Protocol Builders, Post-Quantum dApp, AI Agents, Encrypted Apps | no | rollup / EVM dApp with audit / agent / ML-KEM requirements that Qorebit does not meet |

**Decided: Quantum Pass** (the author's choice, 27 Nov 2026). Form field **Category**: `Quantum Pass`; team size 1 (solo is fine, limit 2).

What Quantum Pass asks for, and where Qorebit stands:

| Requirement | Qorebit |
|---|---|
| A small original QoreChain app, tool, game or bot | a hardware wallet and its companion: original, built since 10 Nov |
| Connect QoreX (on Testnet) | **Open.** The organisers' answer (27 Nov): "the companion app needs to connect QoreX", so registration alone may not be enough. Nothing in the companion talks to QoreX yet; find out what QoreX exposes (provider or wallet standard) and build the connection, or ask what they accept |
| Deploy on the testnet | organisers confirmed (27 Nov): no contract needed; an app that runs against the testnet and sends real transactions from the listed `qor1…` addresses is fine |
| Complete one hybrid-signed transaction | yes in practice; the organisers want the tx hash on explore.qore.network in the README. **Open: copy one real hash from the wallet's Activity screen into README and PROOF.md** |
| Use `create-qorechain-dapp` | organisers (27 Nov): starting the companion from the `fullstack-web` starter counts; jurors check by hand, so keep the generated structure and put the exact command in the README. **Done (27 Nov):** the starter was generated with the command in the README and its skeleton merged into `companion/` (the wallet's code was copied on top, not generated). The README says exactly that |

Prizes: the jury scores the projects that pass, and the ten best win $150 each; then 20 prizes of $50 are drawn by a public script (drand
beacon) from the other passing projects, one entry per project. So passing every check matters more than any single feature: finish the
checklist in section 6. Quantum Pass is also where the follow-on Community Grant (up to $5,000 in QOR, from January 2027) applies.

**Still ask:** in the fest Discord or at workshop 4 (27 Nov, 16:30) whether a signing tool without `create-qorechain-dapp`
passes the Quantum Pass check, and say what Qorebit is before the deadline.

## 3. Text for the project form

**Name** (max 60): `Qorebit`

**Tagline** (max 120):
```
A two-button hardware wallet that signs QoreChain's hybrid secp256k1 + ML-DSA-87 transactions on the device.
```

**Description** (max 5000):
```
Qorebit is a hardware wallet for QoreChain on the Baochip-1x (Dabao board), with two buttons: left rejects, right approves.

Open hardware: Qorebit runs on the Baochip-1x, a mostly open-source 22 nm security chip (RTL under CERN-OHL-W-2.0) on the open Dabao board, with a verifiable boot chain: make audit compares the chip's boot code with Baochip's published hashes. Every layer from the chip's design to the website can be read. To our knowledge this may be the first post-quantum hardware wallet on open-source, verifiable hardware (we have not surveyed every wallet).

Why: QoreChain's native lane needs a hybrid signature on every transaction, secp256k1 plus ML-DSA-87. A browser or phone wallet holds those keys where malware can reach them. Qorebit keeps the 24-word seed on the chip (wrapped by the chip's keystore), derives both keys there, decodes every transaction itself, shows it on its own screen and signs only after the right button and a signing PIN. The website cannot sign anything on its own: there is no API that signs a bare digest.

What it does
- Hybrid signing: ML-DSA-87 over the v2 hybrid sign bytes, then secp256k1 over the SignDoc that includes it. Both signatures are verified on the device before they leave it. The Rust signer matches @qorechain/qoresign byte for byte (vectors regenerated from QoreSign).
- Accounts: eight native accounts (m/44'/118'/0'/0/i) and eight EVM-lane accounts (m/44'/60'/0'/0/i, eth_secp256k1) from the same words, so other QoreChain wallets restore the same accounts.
- PQC key registration with the proof of possession, sends, staking messages, tokens (fUSD, fETH, fBTC), multisig (the chain's group module), smart accounts and spending rules, and EVM to Native Cross-VM calls, all signed on the board, each labelled on its screen.
- Sign In With QoreChain (SIWQ): the board parses the site's message, shows site, network, expiry and statement, and signs both layers, matching QoreSign.
- A wallet website and Chrome and Firefox extensions (password lock, purple theme like the board) with window.qorechain for websites: connect, sign in, send a transaction. The wallet only forwards; approval is on the device.
- Networks: mainnet (qorechain-vladi), testnet (qorechain-diana) and a local devnet, one node on your computer.
- No hardware needed to try it: the firmware runs in the Xous emulator with the device's screen and two buttons in a window (make demo).

What it does not do: the chip is mostly open, not fully (bus fabric, analog blocks and pads are closed), and make audit checks boot code, not silicon. It is unaudited research code (do not use real funds), one message per transaction, no raw EVM transactions (the EVM window path), no off-chain message signing yet, no linking of keys or relayed execution for smart accounts.
```

**How it's made** (max 5000):
```
What we reused: the Xous firmware setup, the bridge and the flashing scripts follow the author's earlier Quazar wallet (credited in the README). Everything on QoreChain, the signer, the screens, the two-button input, the companion, the extensions and the tests were written during the fest (first commit 10 Nov).

What we're using
- The board: Baochip-1x (Dabao), Xous OS, Rust. The firmware is a Xous app; the signer is a no_std Rust crate (qorebit-signer): BIP-39/32 keys, protobuf decode of the transaction, a policy check, ML-DSA-87 and secp256k1, SIWQ.
- @qorechain/qoresign 0.2.0 and @qorechain/pqc 0.2.3 are the ground truth: tools/gen-vectors.mjs makes the test vectors, the Rust signer must match them byte for byte, and make test-emu has a real local QoreChain node verify what the firmware signs.
- The companion (React, TypeScript, Vite; it follows the create-qorechain-dapp fullstack-web starter and reads the chain with @qorechain/sdk) builds transactions and talks to a small Python bridge (USB HID, or TCP to the emulator). The browser extensions are the same app plus a provider for websites.

Hard parts
- ML-DSA-87 needs about 350 KiB of stack on a chip with 2 MiB of SRAM, so signing runs on a short-lived 416 KiB thread. The chip's Keccak engines (made for hash-based signatures) turned out slower than the CPU, so they are not used.
- The app image must fit the board's 893,696-byte app region or the board never starts; the build checks it.
- Trusting the screen: the device decodes the transaction itself and refuses what it cannot show in full (several messages, another account, unknown fields); unknown message types are marked BLIND.
- The chain's rules: a classical-only transaction is refused except the one that registers the PQ key, which carries a proof of possession; the classical signature covers the ML-DSA one, so the device must sign ML-DSA first.

How we checked it
- make test: signer vs QoreSign, firmware and companion unit tests. make test-emu: 58 checks, firmware in the emulator against the local node (registration, transfers, left rejects / right approves, wrong PIN, refused transactions, EVM keys, smart accounts, SIWQ, multisig).
- On a Dabao board and the testnet: transfers, tokens, Cross-VM (EVM account to a CW20 contract) and smart accounts, plus the Firefox extension and the demo website in a real browser.

We build this AI-natively: Claude Code wrote most of the code and docs under the author's direction; AI_USAGE.md logs it, including what it got wrong.
```

**Repository:** `https://github.com/SoarinSkySagar/qorebit` (MIT, `LICENSE`). **Video:** see [DEMO.md](DEMO.md) (a 3 minute script). **Demo:** none hosted
(the wallet needs a local bridge); judges can run `make demo`, no hardware. **Country** (Regional): the author's.

## 4. Kits and network features: tick only what is true

| Kit | Variant | Tick? |
|---|---|---|
| QoreChain SDK | TypeScript (`companion/package.json`, `@qorechain/sdk` 0.14.0, used in `companion/src/sdk.ts`) | yes |
| QoreSign | TypeScript (`companion/package.json`, `tools/package.json`, 0.2.0) | yes |
| qorechain-pqc | TypeScript (0.2.3, same files) | yes |
| create-qorechain-dapp | | yes, partly in the sense of a merge: the scaffolder (0.14.0, `fullstack-web`) was run and its skeleton merged into `companion/`, the wallet code copied on top (README has the command); jurors look by hand, so say what was done and no more. Only `companion/` (the website, which the extension's panel reuses) is starter-based; the extension's background, content and provider scripts and the demo page are our own |
| QoreX integrations, wallet adapter, RDK, QoreSeal, Asset Kit, QoreScan, MCP, Custody | | no |

The Rust signer implements ML-DSA-87 and the scheme itself, so no kit is named in `Cargo.toml`; the organisers' check will not see a Rust kit.

| Network feature | Tick? | Evidence needed |
|---|---|---|
| Hybrid ML-DSA-87 transactions | yes | every native transaction from a listed address |
| PQC key registration with proof of possession | yes | `MsgRegisterPQCKeyV2` from a listed address |
| Cross-VM | yes | `MsgCrossVMCall` from an EVM-lane address (verified once on the testnet) |
| Smart accounts | yes | `MsgCreateAbstractAccount` / `MsgUpdateSpendingRules` from a listed address |
| fUSD / fETH / fBTC | yes | the token sends (account 1 to 2) |
| Sign In With QoreChain | yes, but not provable on chain | it is an off-chain signature; jurors read the code (`siwq.rs`) and the demo |
| EVM window | **no** | raw EVM transactions are not built |
| AMM, IBC, QoreSeal registry, RDK rollup, sidechain, light node, Asset Kit | no | not used |

**Testnet addresses** (up to five `qor1…`): list the accounts that made the transactions above, from the wallet's account menu: Account 1 and 2 (sends,
tokens, key registration), one EVM account (Cross-VM), the owner of the smart account. Fill them in on the form and in [PROOF.md](PROOF.md) once
the transaction hashes are copied from the Activity screen.

## 5. Special awards for Qorebit

| Award | Choose | Notes |
|---|---|---|
| Best AI-Native Build | **yes** | tick the option on the form; `AI_USAGE.md` is current as of this file |
| Regional | pick the country | pays only if no other prize |
| Top-Quality | automatic | pays only if no other prize and the jury score is at least 70/100 |
| People's Choice | optional | needs a public post and 10 QOR on mainnet to vote |
| Quantum Campus | only if the author is a student | proof is asked from jury-picked candidates |

## 6. Checklist before 2 Dec 23:59 UTC

| Item | State |
|---|---|
| Public repo, MIT `LICENSE` | done (confirm GitHub shows the licence) |
| README with run steps, credits, honest limits | done |
| Commits from 4 Nov | done (first commit 10 Nov) |
| Kits and features ticked as in section 4 | to do, on the form |
| Install the QoreChain Build GitHub App | to do, My space |
| Registration complete (GitHub, details, X, QoreX on Testnet) | author to confirm |
| Testnet `qor1…` addresses on the form | to do |
| PROOF.md with transaction hashes | to do: copy them from the testnet explorer |
| Video, up to 3 minutes | to do: record with [DEMO.md](DEMO.md) |
| Cover image (treat as required: Rules say so, Start says optional) | to do: 16:9, at least 1200 x 675, up to 5 MB |
| Create the project on the form now (name, tagline, category, description) and link the repo | to do: commits only show once it is linked; the form is the submission |
| Choose the registration level to match the category (the sign-up asks for Quantum Pass, Intermediate, Advanced or Expert) | author to confirm |
| Check the raffle's excluded-country list when it is published (prizes cannot be paid where law or sanctions forbid) | before 16 Dec |
| AI-native option ticked | to do, on the form |
| Category: Quantum Pass chosen; ask whether `create-qorechain-dapp` is required for a signing tool | decided; the question is still to ask (section 2) |
| Short post in the fest Discord (what it does, category, features) | to do: feedback before judging |

## 7. What a juror may find (be honest about it)

* **Originality rule.** The rules disqualify "copied or pre-built projects" and count only commits from 4 Nov. This repository is new (first commit 10 Nov) and the reuse is the author's own earlier wallet, as the FAQ permits when credited; the README's Credits section names it. Say plainly in the form that the Xous setup, bridge and flashing follow Quazar and that the QoreChain signer, screens, input, companion and extensions are new. Do not call the whole project new.
* **Reused work.** The Xous firmware setup, the bridge and the flashing scripts follow the author's earlier Quazar wallet (Ethereum and leanSPHINCS).
  The chain, the signer, the screens, the input, the companion and everything on QoreChain are new since 10 Nov. The README's Credits say so.
* **No QoreX in the loop.** Qorebit is its own wallet; QoreX is only used for fest registration. The keys follow QoreSign's derivation, so the same
  words open the same accounts in QoreX (vectors check this), but no feature has been tested through QoreX itself.
* **"First" claim.** "May be the first post-quantum hardware wallet on open-source, verifiable hardware" is the author's belief, not a surveyed fact: word it as "to our knowledge" everywhere, and soften or drop it if a juror shows a counter-example. "Verifiable" means the chip is designed for inspection and publishes boot hashes; we ran `make audit` on the boot code only, and have not inspected the silicon.
* **Hardware.** Signing and the testnet flows were run on a Dabao. The panel build's own display and buttons are the least tested part
  ([HARDWARE.md](HARDWARE.md)). The emulator path is what a judge can run.
* **Not built:** raw EVM transactions, `qor_signMessage`, linking keys for smart accounts, the Firefox origin step being automatic, an audit.
