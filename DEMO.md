# Demo: the 3 minute video and the live show

The fest asks for a video of **at most 3 minutes** that shows the project working, and holds a live demo show on 16 Dec. This is the script for
both. Everything below runs from this repository; the emulator path needs no hardware, and a Dabao on the table makes it stronger.

## Before you record

```bash
make demo            # local QoreChain + emulated device and its window + website (http://localhost:5173)
# with a board instead:  make flash-virtual && make board-virtual    (or make flash-panel && make board-panel)
make demo-site       # second terminal: the demo website for window.qorechain, http://localhost:8080
make extension       # once: load extension/dist unpacked (chrome://extensions, Developer mode)
```

Set the PIN once before recording and fund account 1 (devnet faucet button, or the testnet faucet), register its post-quantum key, so the
recording is not spent on set-up. Screen size 1280 x 720 or larger; the device window beside the browser. Use the **testnet** for the part that
shows real transactions and the explorer (<https://explore.qore.network>); use the devnet only if you have no testnet funds.

## The 3 minute script

| Time | Show | Say |
|---|---|---|
| 0:00 | the board (or the device window) and the website side by side | "Qorebit is a hardware wallet for QoreChain with two buttons. Left rejects, right approves." |
| 0:15 | README top / the signing diagram | "Every QoreChain transaction needs two signatures: secp256k1 and ML-DSA-87. The device makes both. The website never holds a key." |
| 0:30 | Send 1 QOR to account 2 (*Review on the device*) | "The device decodes the transaction itself. What its screen shows is what it signs." |
| 0:50 | the device screen: amount, address, network; press right, then the PIN | "I read it, press approve, enter my PIN." |
| 1:10 | the transaction in Activity, then on the explorer | "The chain accepted it: hybrid ML-DSA-87 signature, verified on chain." |
| 1:25 | send again, press the **left** button | "Left means no. Nothing is signed." |
| 1:35 | Apps: Cross-VM example (EVM account), the **CROSS-VM** screen on the device | "EVM accounts from the same 24 words. This is an EVM to Native call: the board labels it and signs it." |
| 1:55 | Apps: Smart account (create or a rule), the **SMART ACCT** screen | "Smart accounts and spending rules: same device, same two buttons." |
| 2:10 | `http://localhost:8080` with the extension: **Sign in with QoreChain** | "A website asks to sign in. The extension opens an approval, the device shows the site and statement, I approve." |
| 2:35 | terminal: `make test-emu` result (58 ok) or `make test` | "The Rust signer matches QoreSign byte for byte, and a real QoreChain node verifies what the firmware signs." |
| 2:50 | README "Honest status" | "Research prototype, not audited. Built with Claude Code; AI_USAGE.md logs it." |

Keep each device shot long enough to read the screen. If a take runs over 3:00, cut the Smart account scene first.

## The live show (5 to 8 minutes)

1. The two buttons on a real board: approve, reject, the wrong PIN (the counter) and the lockout.
2. The same transaction on the testnet; open the hash in the explorer.
3. `make test-emu` live (about a minute) to show the chain verifying the device's signatures, or its saved output.
4. The extension and `make demo-site`: connect, sign in, send.
5. Questions: why the device decodes (no blind signing), why ML-DSA first (the classical signature covers it), what is still missing
   ([SUBMISSION.md](SUBMISSION.md), section 7).

## For reviewers without hardware

`make setup-emu && make demo` (see [walkthrough.md](walkthrough.md), sections 3 to 5). The window shows the device's screen and two buttons, REJECT
and APPROVE; click them or use the Left / Right arrow keys. With Claude Code, `/setup` does it all and stops at the PIN.
