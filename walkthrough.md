# Qorebit walkthrough: run it yourself

Qorebit is a hardware wallet for QoreChain with **two buttons**. You do not need our hardware: everything runs in an
**emulator** with the device's screen drawn in a window on your computer. If you do have a Baochip Dabao board, sections
7 and 8 flash it. An agent can do all of this for you: run **`/setup`** in Claude Code (see section 10).

**Reviewing it for the QoreChain fest?** The five minute path: `make setup-emu`, `make demo`, then section 5 (PIN, faucet, register the key, send,
reject) and `make test-emu` in section 9. What the fest asks for and how Qorebit answers it: [SUBMISSION.md](SUBMISSION.md); the video script: [DEMO.md](DEMO.md).

Tested on Linux x86-64. The emulator and website also run on macOS (the local devnet node is Linux-only; use the
Testnet tab there).

## 1. What you need

`git`, `curl`, Python 3 with `venv`, Node 22+, Rust (rustup). `make install` installs Rust and Node if missing (no root)
and prints the commands for anything that needs `sudo`. Check with `make doctor`.

## 2. Get it

```bash
git clone --recursive <repo url> qorebit && cd qorebit
make install          # tools + project setup, about 10 minutes the first time (the Xous toolchain is the slow part)
```

Only want the emulator? `make setup-emu` skips the Xous toolchain and is much faster.

## 3. Run it: the emulator with a display (no hardware)

```bash
make demo
```

This starts, in one terminal: a one-node **local QoreChain** (`qorechain-local`), the **emulated device** with its
**screen window**, and the **wallet website** at <http://localhost:5173>. The first run compiles the firmware (a minute
or two). `Ctrl+C` stops the window, emulator and website; `make devnet-down` stops the chain.

No Linux x86-64 (so no local chain)? `make web-emu` starts the same without it; use the **Testnet** tab.

## 4. The two buttons

The device window shows the device's screen and two buttons. The **left** button = 0 = reject. The **right** button = 1 = approve.
One click is the answer. Click them, or use the Left / Right arrow keys (A / D; Esc rejects). The buttons are labelled REJECT and APPROVE and stay dim until the device asks for an answer.

The same two answers are used everywhere: approving a transaction, typing the 8-press PIN, confirming, and picking words
when restoring (left = "earlier", right = "this word or later").

## 5. First use, step by step

Open <http://localhost:5173>. The header shows the network (default: Local devnet) and the device status.

1. **Set the PIN.** Click *Set the PIN on the device*. On the device screen press the buttons 8 times (left = 0, right = 1),
   then the same 8 again. It is asked after every approval; 5 wrong tries lock signing.
2. **Get funds.** Click *Get 1,000 test QOR* (the local chain's faucet). On the testnet use QoreChain's faucet link.
3. **Register the post-quantum key.** QoreChain refuses transfers until an account's ML-DSA key is registered. Click
   *Register the post-quantum key*; on the device press the **right** button to approve (twice: the proof, then the transaction), then the PIN.
4. **Send.** Paste a `qor1…` address (account 2 is a good one: pick it in the account bar, copy its address, switch back),
   an amount, *Review on the device*. Read the screen: the amount, the address, the network. Press the **right** button to approve, enter
   the PIN. The website broadcasts it and shows it in *Activity*.
5. **Try saying no.** Send again and press the **left** button instead: "REJECTED", nothing is signed.

What to look for: the device decodes the transaction itself, so what its screen shows is what it signs, whatever the
website says. A transfer from another account, two messages in one transaction, or an unknown message type is refused or
marked **BLIND**.

## 6. The three networks

Switch in the header. All three go through the bridge (QoreChain's public endpoints allow no browser access).

| Tab | Chain id | Use |
|---|---|---|
| Mainnet | `qorechain-vladi` | real funds; an amber banner and **MAINNET** on the device screen. Read-only unless you fund it |
| Testnet | `qorechain-diana` | test QOR from QoreChain's faucet |
| Local devnet | `qorechain-local` | `make devnet`; one-click faucet |

## 6a. Sign in with QoreChain

Log in to a website with your account instead of a password (QoreChain's standard, SIWQ). The site gives you a **message**
("example.com wants you to sign in with your QoreChain account: qor1…", with a statement, the chain, a one-time nonce and times).
In the **Sign in with QoreChain** card:

1. Paste the site's message (or press *Make a test message* to try it). The card shows the same fields the board will.
2. *Sign in with the device.* The board shows **SIGN IN**: the site, your account number, the network, when it expires and the
   statement. Compare them, press the right button, enter your PIN.
3. The card checks the result itself (both signatures, the site, the nonce, the chain, the time) and gives you the **signature**
   (JSON) to paste back into the site. Nothing is sent from here.

What the board will not sign: anything that is not exactly a sign-in message (not a transaction, not a digest, not free text),
a sign-in for another account, a sign-in that lists *resources* (permissions its screen cannot show), a statement over 75
characters or a site name over 44 (it shows them in full or not at all). A sign-in signature is two signatures: secp256k1 over
the ADR-36 document and ML-DSA-87 over `qorechain-siwq-v1|` + the message, so it cannot be mistaken for a transaction signature.

## 6b. Tokens

The **Tokens** card lists tokens on the selected network that you hold or have added, with balances, and sends them. A token on
QoreChain is an ERC-20 contract shown in the bank module as a coin called `erc20:0x<contract address>`; the chain describes it
(symbol, name, decimals), so the app only needs the address. On the **Testnet** tab the three test tokens are listed by default:
**fUSD** (6 decimals), **fETH** (18) and **fBTC** (8), which the testnet faucet gives you.

- **Add a token:** *Add a token*, paste its contract address (`0x…`, any casing) or a coin denom (for example an `ibc/…` coin).
  The chain is asked first and only a coin that exists is added. The list is kept in this browser, per network. *Tokens this network
  lists* shows everything the chain describes, one click to add.
- **Send a token:** *Send*, who gets it and how much. It is an ordinary transfer signed on the device; the network fee is paid in QOR.
- **On the board:** the screen shows `Amount` in the token's **smallest unit** (1 fUSD is `1000000`) and `Token` as a short coin name
  (`erc20:0x9714..63CA`), with the recipient and the fee. The website tells you the number to expect before you approve.

## 6c. Multisig (shared accounts)

A **multisig** is an account owned by several people, where moving funds needs "N of M" of them to agree. QoreChain does it
with its group module: the multisig has an account of its own (a long `qor1…` address, 62 characters), and each step is
an ordinary transaction that **one member signs with their own device**: create it, *propose* a payment, *vote*, then *run* it.
Nothing is held by the website, and the board needs no multisig features: it only signs.

In the website, the **Multisig** card:

1. **Create.** Open *Create a multisig*, paste the other members' `qor1…` addresses (you are added for you), pick how many
   approvals are needed and the voting time, *Create with the device*.
2. **Fund it.** Copy the multisig's address and send QOR to it with the normal *Send* card.
3. **Propose a payment.** To, amount, a title, *Propose with the device*.
4. **Vote.** Each member opens the same page with their own device and votes Yes / No / Abstain.
5. **Run it.** Once enough members voted yes the proposal shows *Passed: ready to run*. Any member runs it (*Run it*), and the
   chain pays out.

On the board each of these shows a **MULTISIG** screen naming the action (Create multisig, Propose, Vote, Run proposal) with
a note that the details are on the website. The board signs them without reading the members or amounts, so read the
details on the website first.

Multisig works on the local devnet and the testnet. `make test-emu` runs the whole cycle with two device accounts.

## 6d. EVM accounts and Cross-VM calls

Above the balance, the account picker has two lanes: **Native** (Account 1..8) and **EVM** (EVM 1..8). An EVM account shows its `0x…` address
next to its `qor1…` one; they are the same account. Fund it, register its post-quantum key (as for any account), then open the **Cross-VM calls**
card. "Example: native call" makes a harmless call to a CW20 contract on the testnet (an allowance of 1 to the contract itself); from an EVM
account that is **EVM to Native**. The board shows CROSS-VM and the website shows what is called, so check it there before approving; the result
panel then shows the chain's `crossvm_request` and `crossvm_response`.

## 6e. Smart accounts and spending rules

The **Smart account** card creates a smart account (session based, multisig or social recovery) and edits its spending rules: a limit per
transaction and per day in QOR, which coins may move, and which EVM calls are allowed (at most 5 rules). Both are signed on the board, which
shows SMART ACCT. The rules limit linked keys and relayers; they do not limit what you send yourself. Mainnet has smart accounts switched off.

## 6f. The browser extension (side panel) and websites

The same wallet also runs as a Chrome / Brave / Edge extension: the toolbar button opens it in the browser's side panel,
next to whatever page you are on. It is the companion app built for the panel; it has no other powers than the website.

```bash
make extension        # builds extension/dist
```

Then open `chrome://extensions`, switch on **Developer mode**, click **Load unpacked** and choose `extension/dist`.
Start the bridge as usual (`make demo`, `make web-emu` or `make board-virtual`), then click the Qorebit button in the
toolbar. Chrome 114 or newer is needed (side panel).

What it can do: talk to the local bridge (`http://127.0.0.1:7630`) and keep the selected account in its own storage. It reads
nothing from pages. Two tiny scripts run on web pages only to offer them `window.qorechain` (below); `make test` checks the manifest.
The bridge accepts the extension by its fixed id (`extension/EXTENSION_ID`, pinned by the key in `extension/manifest.json`), not by a
pattern, so another extension cannot use the device. Approval of every transaction still happens on the device.

**Firefox.** `make extension-firefox` builds `extension/dist-firefox`. Open `about:debugging#/runtime/this-firefox`, **Load Temporary Add-on**,
and choose `extension/dist-firefox/manifest.json` (Firefox 128 or newer; it opens as a sidebar, the toolbar button toggles it, and a temporary
add-on is removed when Firefox closes; `STORE.md` covers a signed, permanent install). Firefox gives every install its own address, so the first
time the wallet says "the bridge does not know this wallet yet" and shows a command: start the bridge with it,
`QOREBIT_EXTENSION_ORIGINS=moz-extension://<id> make board-panel`.

**The password lock.** The first time you open the extension it asks you to create a password (at least 8 characters, with a strength
bar). After that it opens on the password field with the keyboard already in it: type and press Enter. Once unlocked it stays open for the
browser session (the side panel and the approval window share it; closing the browser locks it again). The lock icon at the top locks it
now; **Device → Wallet password** changes or removes the password. Five wrong tries start a growing wait. The password only locks the
wallet's pages in your browser: the keys are on the device, behind its PIN. If you forget it, remove the extension and load it again
(nothing is lost). The website has no password lock.

**The wallet's screens** (the website is the same): the account menu (Native 1..8, EVM 1..8) and the network menu are at the top;
**Home** (balance, Send, Receive, Tokens, recent activity), **Activity**, **Apps** (Sign in with QoreChain, Multisig, Smart account,
Cross-VM calls, each its own screen) and **Device** (PIN, recovery words, passphrase, restore, erase) are in the bar at the bottom.

**Websites: Sign In With QoreChain without pasting.** A page can call `window.qorechain`:

```js
const [address] = await window.qorechain.request({ method: "qor_requestAccounts" });   // asks you to connect the site
const chainId   = await window.qorechain.request({ method: "qor_chainId" });
const signature = await window.qorechain.request({ method: "qor_signIn", params: [{ message }] }); // SIWQ, signed on the device
const { hash, height } = await window.qorechain.request({   // a transaction, signed on the device and sent by the wallet
  method: "qor_sendTransaction",
  params: [{ messages: [{ typeUrl: "/cosmos.bank.v1beta1.MsgSend", value: { toAddress: "qor1…", amount: [{ denom: "uqor", amount: "100000" }] } }], memo: "order 42" }],
});
```

**Sending a transaction from a site.** `qor_sendTransaction` takes exactly one message (the device signs one per transaction): a send as
`{ toAddress, amount }` (the wallet fills in your account), or any message as `{ typeUrl, value: "<base64 protobuf>" }`. The Qorebit window
shows the site, the amount and recipient (sends and staking are decoded; other types show their type and bytes with a warning), the memo,
the network and the fee, and checks that the account exists, its post-quantum key is registered and the balance covers it. On Approve the device
shows the transaction itself and asks for the button and PIN; the wallet then sends it and answers the site with `{ hash, height }` once it is
in a block, or an error (code 4001 if you reject, on the window or on the device). The site must be connected first. To try it,
run `make demo-site` and open http://localhost:8080 with the extension installed: **Sign in with QoreChain** connects the site and signs a real sign-in message on the device (the page builds the message the way `createSiwqMessage` does), then **Send with the wallet** asks for a transaction.

`qor_requestAccounts` opens a small Qorebit window ("Connect to this site?"). `qor_signIn` opens the same window with the site and the
statement, then the device asks for its button and your PIN. The wallet refuses a message whose site is not the requesting site, one for
another account, and anything the device cannot show in full. The returned object is the SIWQ signature (`ml-dsa-87-hybrid`) that
`verifySiwq` from `@qorechain/qoresign` checks. Sign-in uses native accounts. Without the extension, the **Sign in** app still takes a pasted message.

## 7. With a Dabao board and the display on your laptop (virtual display)

The board does the cryptography and keeps the keys; the screen and button are the window on your laptop (a development
stand-in: malware on the laptop could fake them).

```bash
make flash-virtual    # build + flash. When asked: unplug the Dabao, hold PROG, plug it back in
make board-virtual    # the window + website (re-run this any time; no reflash needed)
```

The board is open hardware around a verifiable chip (README, "Open hardware and a verifiable chip"): with it in its bootloader, `make audit` hashes its boot code and compares it with Baochip's published hashes (read-only).

The board's PROG button only enters the bootloader. If `make flash-virtual` says the board is not in its bootloader,
repeat the plug-in with PROG held. `make security` shows the chip's state; `make info` its firmware.

## 8. With a Dabao and its own display and button (no window)

For a board wired with the ST7789 panel and a button on PB1 (pin table and bring-up steps in [`HARDWARE.md`](HARDWARE.md)):

```bash
make flash-panel      # builds the panel firmware (screen and button stay on the device) and flashes
make board-panel      # the bridge and website only
```

`make bio-test` checks the display and button engines without the panel. The panel build is the least tested path; signing itself has been run on a board (testnet transfers, Cross-VM, smart accounts).

## 9. Checks you can run

```bash
make test             # the signer against QoreChain's own library (byte for byte), firmware, website tests
make devnet && make test-emu   # the real thing: firmware in the emulator signs, a real QoreChain node verifies
make screens          # every device screen as an image, in screens/
make stack            # how much memory the post-quantum signing needs
```

`make test-emu` prints 58 `ok` lines: key registration, transfers, left rejects and right approves, a wrong PIN,
the transactions the device must refuse, EVM-lane keys, tokens, multisig, smart accounts and Sign In With QoreChain. A real
QoreChain node (the local devnet) verifies every signature the firmware makes.

## 10. Set it up with an agent

In Claude Code (or any agent that reads `.claude/skills`), open this folder and run **`/setup`**. It checks the tools, starts
the local chain, runs the emulator with its window and the website, and stops at the PIN for you. Variants: `/setup emu`
(never touch a board), `/setup nodevnet` (testnet only), `/setup board` (flash a Dabao, virtual display), `/setup panel`.

## 11. Troubleshooting

| Symptom | Fix |
|---|---|
| `port 7610/7620/7630/5173 is already in use` | an earlier run is still going: Ctrl+C it, or find it with `ss -ltnp` |
| the window does not open (no `DISPLAY`) | run on a desktop session; over SSH use `ssh -X`. `make test-emu` needs no window |
| website says *The bridge is not running* | start it with `make demo` / `make web-emu` |
| *No device answers* | the emulator is still compiling (wait), or the board is not running Qorebit (`make boot`) |
| `make devnet` fails | the node release is Linux x86-64 only; use the Testnet tab. `devnet/node.log` has details |
| *post-quantum key not registered* | click *Register the post-quantum key* (section 5, step 3) |
| *this account has no funds* | faucet button (devnet) or QoreChain's faucet link (testnet) |
| `SemVer::from_git: no major version` while flashing | `make submodules` fetches the tags the image signer needs |
| the board stays in its bootloader | `make boot`, or press PROG |
| PIN locked | *Forgot PIN* in the website's Device card: enter your 24 words on the device |
| Firefox extension: *the bridge does not know this wallet* or *No device answers* | Firefox gives each install its own address: start the bridge with the command the wallet shows, `QOREBIT_EXTENSION_ORIGINS=moz-extension://<id> make board-panel` |
| the demo site says *nothing to approve* | reload the extension (it must be the current build: `make extension`), then connect again |
| the extension asks for a password | the password you set in the extension (the keys are on the device); closing the browser locks it again |
| the board never starts after a flash | the app image is over the board's 893,696-byte limit (`build.sh` fails above it), or boot wait is off: `make boot` |

## 12. Stopping and resetting

`Ctrl+C` in the terminal; `make devnet-down` for the chain. `make emu-reset` forgets the emulator's wallet (a new device
next start). The emulator keeps its wallet in `firmware/dabao-app-qorebit/target/qorebit-state.bin`: not secret, test use only.
