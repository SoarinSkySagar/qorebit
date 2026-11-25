//! Qorebit on the dabao.
//!
//! A hardware wallet for QoreChain on the Baochip-1x: it holds the keys, shows every transaction
//! on its own screen, and signs only after the **right** button is pressed (the left one rejects) and the
//! signing PIN. QoreChain's native lane needs a hybrid signature on every transaction, so the
//! device makes both: ML-DSA-87 (FIPS 204) and secp256k1 (`qorebit-signer`, checked byte for byte
//! against QoreChain's own `@qorechain/qoresign`). It answers a host over USB HID, or over TCP
//! in the emulator. See `protocol.rs` for the commands.

#[cfg(target_os = "xous")]
mod bioio;
#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
mod bioproto;
mod flows;
#[cfg(all(target_os = "xous", feature = "panel"))]
mod panel;
mod protocol;
mod screen;
mod security;
mod storage;
mod transport;
mod ui;

use flows::Io;
use protocol::{Status, MSG_TYPE};
use qorebit_signer::tx::{self, Mode, Msg, SignBytes, TxError};
use qorebit_signer::{keys, mnemonic_from_entropy, Account, KeyError, Wallet};
use screen::{Row, Tone};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Current job (see protocol.rs), 0 when idle.
static JOB: AtomicU8 = AtomicU8::new(0);

pub const VERSION: &str = concat!("qorebit ", env!("CARGO_PKG_VERSION"));

/// Seed slots, PIN record and PIN counter.
#[cfg(target_os = "xous")]
type Store = storage::Keystore;
#[cfg(not(target_os = "xous"))]
type Store = storage::HostFile;

/// ML-DSA-87 keeps big arrays on the stack: the heaviest signing path needs about 350 KiB (measured
/// with `cargo run --release -p qorebit-signer --example stack`). Crypto runs on a thread made for the
/// purpose, with room to spare, so that memory is the chip's again when the signature is done
/// (the SRAM is 2 MiB, shared with the kernel and every service) instead of staying committed to
/// the worker thread.
const CRYPTO_STACK: usize = 416 * 1024;

fn crypto<R: Send>(f: impl FnOnce() -> R + Send) -> Result<R, Vec<u8>> {
    std::thread::scope(|s| {
        let h = std::thread::Builder::new()
            .name("qorebit-crypto".into())
            .stack_size(CRYPTO_STACK)
            .spawn_scoped(s, f)
            .map_err(|e| protocol::error(Status::SigningFailed, &format!("not enough memory to sign: {e}")))?;
        h.join().map_err(|_| protocol::error(Status::SigningFailed, "the signing thread crashed"))
    })
}

/// When the companion app last sent the device a request (milliseconds since the app started).
static LAST_HOST_MS: AtomicU32 = AtomicU32::new(0);
/// The companion polls every few seconds; this long without a request means it is gone.
const LINK_TIMEOUT_MS: u32 = 15_000;

/// The home screen shows whether the companion app is connected: called often (the display loop, the display
/// polls) it notices when the app has gone quiet for `LINK_TIMEOUT_MS`.
pub(crate) fn link_tick() {
    let last = LAST_HOST_MS.load(Ordering::Relaxed);
    let up = last != 0 && host_clock_ms().wrapping_sub(last) < LINK_TIMEOUT_MS;
    if up != ui::link_up() {
        ui::ui().set_link(up);
    }
}

fn host_clock_ms() -> u32 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    // 32 bits (the chip has no 64-bit atomics): wraps after 49 days, which `wrapping_sub` below survives;
    // never 0, so that "no request yet" is distinguishable
    (START.get_or_init(Instant::now).elapsed().as_millis() as u32).max(1)
}

/// Chains the device knows by name. Any other chain id is signed for too (a private chain, a
/// new network), but the screen says it is unknown.
const MAINNET: &str = screen::MAINNET;
const TESTNET: &str = "qorechain-diana";
const DEVNET: &str = "qorechain-local";

struct App {
    wallet: Wallet,
    origin: storage::SeedOrigin,
    security: security::State,
    store: Store,
    /// The keystore's key wrapping (device only).
    #[cfg(target_os = "xous")]
    chip_key: storage::Keystore,
    /// The account the idle screen shows.
    idle_account: u32,
    /// What the app last sent for the home screen (account, balance, address, check).
    home: Option<(u32, screen::Home)>,
}

/// `abcdefgh..uvwxyz`: an address or validator, short enough for a row.
fn short(s: &str) -> String {
    if s.len() <= 18 {
        s.to_string()
    } else {
        format!("{}..{}", &s[..8], &s[s.len() - 6..])
    }
}

fn coins_text(coins: &[tx::Coin]) -> String {
    if coins.is_empty() {
        "none".into()
    } else {
        coins.iter().map(|c| c.pretty()).collect::<Vec<_>>().join(" + ")
    }
}

/// A big seven-segment amount for one `uqor` coin: "1.234567" QOR.
fn big_qor(c: &tx::Coin) -> Option<(String, &'static str)> {
    let p = c.pretty();
    (c.denom == "uqor").then(|| (p.trim_end_matches(" QOR").to_string(), "QOR")).filter(|(n, _)| !n.contains(' '))
}

/// What the approval screen shows for `view`. Everything the device signs is on it, or the
/// heading says BLIND.
fn describe(view: &tx::View, acct: &Account, chain: &str, mode: Mode) -> screen::Approval {
    let mut rows = Vec::new();
    if chain == MAINNET {
        rows.push(Row::Warn("MAINNET: real funds".into()));
    } else if chain != TESTNET && chain != DEVNET {
        rows.push(Row::Warn("Unknown network".into()));
    }
    let from = Row::Kv("From", format!("Account {}", acct.index + 1));
    let (tag, heading, big): (&'static str, &'static str, _) = match &view.msgs[0] {
        Msg::Send { to, amount, .. } => {
            rows.push(Row::Addr("To", to.clone()));
            let big = (amount.len() == 1).then(|| big_qor(&amount[0])).flatten();
            if big.is_none() {
                rows.push(Row::Kv("Amount", coins_text(amount)));
            }
            rows.push(from);
            ("SEND", "Send", big)
        }
        Msg::Delegate { validator, amount, .. } => {
            rows.push(Row::Kv("Validator", short(validator)));
            rows.push(from);
            ("STAKE", "Delegate to a validator", big_qor(amount))
        }
        Msg::Undelegate { validator, amount, .. } => {
            rows.push(Row::Kv("Validator", short(validator)));
            rows.push(from);
            ("UNSTAKE", "Undelegate from a validator", big_qor(amount))
        }
        Msg::WithdrawReward { validator, .. } => {
            rows.push(Row::Kv("Validator", short(validator)));
            rows.push(from);
            ("REWARDS", "Claim staking rewards", None)
        }
        Msg::RegisterPqKey { account, .. } => {
            rows.push(Row::Addr("Account", account.clone()));
            rows.push(Row::Kv("Key", "ML-DSA-87 (post-quantum)".into()));
            if mode == Mode::ClassicalOnly {
                rows.push(Row::Kv("Signed", "classical only".into()));
            }
            ("REGISTER", "Register your post-quantum key", None)
        }
        // a multisig (the chain's group module) message: the board says so and names the action; the details
        // (members, amounts, the proposal) are read on the website, so the screen says that too
        Msg::Other { type_url, len, id } if type_url.starts_with("/cosmos.group.v1.") => {
            let action = match type_url.rsplit('.').next().unwrap_or("") {
                "MsgCreateGroupWithPolicy" | "MsgCreateGroup" | "MsgCreateGroupPolicy" => "Create multisig",
                "MsgSubmitProposal" => "Propose",
                "MsgVote" => "Vote",
                "MsgExec" => "Run proposal",
                "MsgWithdrawProposal" => "Withdraw proposal",
                "MsgUpdateGroupMembers" | "MsgUpdateGroupAdmin" | "MsgUpdateGroupMetadata" => "Change members",
                "MsgUpdateGroupPolicyAdmin" | "MsgUpdateGroupPolicyDecisionPolicy" | "MsgUpdateGroupPolicyMetadata" => "Change policy",
                "MsgLeaveGroup" => "Leave multisig",
                _ => "Multisig action",
            };
            rows.push(Row::Kv("Action", action.to_string()));
            rows.push(Row::Warn("Details: see website".into()));
            rows.push(Row::Kv("Size / id", format!("{len} bytes / {}", id.iter().map(|b| format!("{b:02x}")).collect::<String>())));
            rows.push(from);
            ("MULTISIG", "Multisig transaction", None)
        }
        Msg::Other { type_url, len, id } => {
            let name = type_url.rsplit('.').next().unwrap_or(type_url);
            rows.push(Row::Kv("Type", name.to_string()));
            rows.push(Row::Kv("Size / id", format!("{len} bytes / {}", id.iter().map(|b| format!("{b:02x}")).collect::<String>())));
            rows.push(from);
            ("BLIND", "BLIND: a message the device can't show", None)
        }
    };
    rows.push(Row::Kv("Max fee", coins_text(&view.fee)));
    if !view.fee_granter.is_empty() {
        rows.push(Row::Kv("Fee paid by", short(&view.fee_granter)));
    }
    rows.push(Row::Kv("Network", chain.to_string()));
    if !view.memo.is_empty() {
        rows.push(Row::Text(view.memo.replace('\n', " ")));
    }
    screen::Approval { tag, heading, big, rows }
}

/// A SIGN_TX request, parsed.
struct SignTx<'a> {
    account: u32,
    flags: u8,
    account_number: u64,
    chain: &'a str,
    body: &'a [u8],
    auth_info: &'a [u8],
}

fn parse_sign_tx(p: &[u8]) -> Option<SignTx<'_>> {
    // account u32, flags u8, account number u64, chain id (len u8), body len u32, body, auth info
    let account = u32::from_be_bytes(p.get(0..4)?.try_into().ok()?);
    let flags = *p.get(4)?;
    let account_number = u64::from_be_bytes(p.get(5..13)?.try_into().ok()?);
    let n = *p.get(13)? as usize;
    let chain = core::str::from_utf8(p.get(14..14 + n)?).ok()?;
    let at = 14 + n;
    let blen = u32::from_be_bytes(p.get(at..at + 4)?.try_into().ok()?) as usize;
    let body = p.get(at + 4..at + 4 + blen)?;
    let auth_info = p.get(at + 4 + blen..)?;
    Some(SignTx { account, flags, account_number, chain, body, auth_info })
}

impl App {
    fn account(&self, index: u32) -> Result<Account, Vec<u8>> {
        self.wallet.account(index).map_err(|_| protocol::error(Status::BadRequest, "no such account (0..8)"))
    }

    fn get_account(&mut self, p: &[u8]) -> Vec<u8> {
        let Some(index) = p.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) else {
            return protocol::error(Status::BadRequest, "expected account u32");
        };
        match self.account(index) {
            Ok(a) => {
                let mut out = Vec::with_capacity(53 + 43);
                out.extend_from_slice(&a.address);
                out.extend_from_slice(&a.public);
                out.extend_from_slice(a.address_bech.as_bytes());
                protocol::response(Status::Ok, &out)
            }
            Err(e) => e,
        }
    }

    fn get_pq_public_key(&mut self, p: &[u8]) -> Vec<u8> {
        let Some(index) = p.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap())) else {
            return protocol::error(Status::BadRequest, "expected account u32");
        };
        match self.account(index) {
            Ok(mut a) => match crypto(move || a.pq_public().to_vec()) {
                Ok(pk) => protocol::response(Status::Ok, &pk),
                Err(e) => e,
            },
            Err(e) => e,
        }
    }

    /// Show `screen`, wait for the right button (the left one or the timeout rejects), then the
    /// signing PIN. `Ok` only when the user approved and the PIN was right.
    fn approve(&mut self, screen: screen::Approval) -> Result<(), Vec<u8>> {
        if !ui::Ui::enabled() {
            return Err(protocol::error(Status::NoApprovalUi, "no display in this build: refusing to sign"));
        }
        // the signing PIN: refused up front without one, or when locked
        if let Some(e) = self.pin_refusal() {
            return Err(e);
        }
        JOB.store(8, Ordering::Relaxed);
        log::info!("waiting for approval: {}", screen.heading);
        let decision = ui::ui().ask(screen::Screen::Approval(screen), ui::APPROVAL_TIMEOUT);
        let r = match decision {
            ui::Decision::Approve => match flows::check_pin(&mut flows::DeviceIo, &mut self.store, "Your PIN, to sign") {
                flows::PinOk::Right => {
                    log::info!("approved on the device, PIN right");
                    Ok(())
                }
                p => Err(Self::pin_failure(p)),
            },
            // the screen already shows REJECTED / TIMED OUT
            ui::Decision::Reject => Err(protocol::error(Status::Rejected, "rejected on the device")),
            ui::Decision::Timeout => Err(protocol::error(Status::Rejected, "no decision within 60 s: rejected")),
            ui::Decision::Unavailable => Err(protocol::error(Status::NoApprovalUi, "no display in this build: refusing to sign")),
        };
        JOB.store(2, Ordering::Relaxed);
        r
    }

    /// The result screen and idle screen after a signing attempt.
    fn signed_screen(&mut self, r: &Result<tx::Signed, TxError>, ms: u32, account: u32) {
        match r {
            Ok(_) => ui::ui().show_result(
                Tone::Good,
                "SIGNED",
                vec![format!("{ms} ms on the chip"), "Both signatures checked".into()],
            ),
            Err(TxError::Key(KeyError::Fault)) => {
                ui::ui().show_result(Tone::Bad, "FAULT", vec!["Signature withheld".into(), "Self-check failed".into()]);
                log::error!("fault detected while signing: signature withheld");
            }
            Err(_) => ui::ui().show_result(Tone::Bad, "FAILED", vec!["Nothing was signed".into()]),
        }
        self.refresh_idle(account);
    }

    /// SIGN_TX: the device decodes the transaction itself, shows it, and signs what it decoded.
    fn sign_tx(&mut self, payload: &[u8]) -> Vec<u8> {
        let Some(req) = parse_sign_tx(payload) else {
            return protocol::error(Status::BadRequest, "expected account u32, flags u8, account number u64, chain id, body, auth info");
        };
        let mut acct = match self.account(req.account) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let mode = if req.flags & protocol::FLAG_CLASSICAL_ONLY != 0 { Mode::ClassicalOnly } else { Mode::Hybrid };
        let view = match tx::decode(req.body, req.auth_info) {
            Ok(v) => v,
            Err(e) => return Self::refusal(e),
        };
        let checked = match crypto(|| tx::check(&view, &mut acct, req.chain, mode)) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if let Err(e) = checked {
            return Self::refusal(e);
        }
        let screen = describe(&view, &acct, req.chain, mode);
        if let Err(e) = self.approve(screen) {
            return e;
        }
        ui::ui().show_working("SIGNING", vec!["ML-DSA-87 + secp256k1".into()]);
        let request = tx::Request {
            chain_id: req.chain,
            account_number: req.account_number,
            body: req.body,
            auth_info: req.auth_info,
            version: if req.flags & protocol::FLAG_SIGN_BYTES_V1 != 0 { SignBytes::V1 } else { SignBytes::V2 },
            include_public_key: req.flags & protocol::FLAG_INCLUDE_PQ_KEY != 0,
            mode,
        };
        let t = Instant::now();
        let r = match crypto(|| tx::sign(&mut acct, &request)) {
            Ok(r) => r,
            Err(e) => return e,
        };
        let ms = t.elapsed().as_millis() as u32;
        self.signed_screen(&r, ms, req.account);
        match r {
            Ok(s) => {
                log::info!("signed in {ms} ms: {} bytes", s.tx_bytes.len());
                let mut out = Vec::with_capacity(8 + s.tx_bytes.len() + 64 + keys::PQ_SIG_LEN);
                out.extend_from_slice(&ms.to_be_bytes());
                out.extend_from_slice(&(s.tx_bytes.len() as u32).to_be_bytes());
                out.extend_from_slice(&s.tx_bytes);
                out.extend_from_slice(&s.classical);
                if let Some(pq) = &s.pq {
                    out.extend_from_slice(&pq[..]);
                }
                protocol::response(Status::Ok, &out)
            }
            Err(TxError::Key(KeyError::Fault)) => protocol::error(Status::SigningFailed, "fault detected while signing: the signature was withheld"),
            Err(e) => protocol::error(Status::SigningFailed, &format!("signing failed: {e:?}")),
        }
    }

    /// REGISTER_PROOF: the account's ML-DSA key signs its registration message for a chain, so
    /// the app can build the key-registration transaction (which `SIGN_TX` then checks and
    /// signs classical-only).
    fn register_proof(&mut self, p: &[u8]) -> Vec<u8> {
        let (Some(index), Some(n)) = (p.get(..4).map(|b| u32::from_be_bytes(b.try_into().unwrap())), p.get(4).map(|&n| n as usize)) else {
            return protocol::error(Status::BadRequest, "expected account u32, chain id (len u8)");
        };
        let Some(chain) = p.get(5..5 + n).and_then(|c| core::str::from_utf8(c).ok()).filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_graphic())) else {
            return protocol::error(Status::BadRequest, "bad chain id");
        };
        let mut acct = match self.account(index) {
            Ok(a) => a,
            Err(e) => return e,
        };
        let mut rows = Vec::new();
        if chain == MAINNET {
            rows.push(Row::Warn("MAINNET: real funds".into()));
        } else if chain != TESTNET && chain != DEVNET {
            rows.push(Row::Warn("Unknown network".into()));
        }
        rows.push(Row::Addr("Account", acct.address_bech.clone()));
        rows.push(Row::Kv("Key", "ML-DSA-87 (post-quantum)".into()));
        rows.push(Row::Kv("Network", chain.to_string()));
        let screen = screen::Approval { tag: "REGISTER", heading: "Prove your post-quantum key", big: None, rows };
        if let Err(e) = self.approve(screen) {
            return e;
        }
        ui::ui().show_working("SIGNING", vec!["Proof of possession".into()]);
        let t = Instant::now();
        let r = match crypto(|| tx::register_proof(&mut acct, chain)) {
            Ok(r) => r,
            Err(e) => return e,
        };
        let ms = t.elapsed().as_millis() as u32;
        self.refresh_idle(index);
        match r {
            Ok(proof) => {
                ui::ui().show_result(Tone::Good, "SIGNED", vec![format!("{ms} ms on the chip"), "Proof of possession".into()]);
                let mut out = ms.to_be_bytes().to_vec();
                out.extend_from_slice(&proof[..]);
                protocol::response(Status::Ok, &out)
            }
            Err(e) => {
                ui::ui().show_result(Tone::Bad, "FAILED", vec!["Nothing was signed".into()]);
                protocol::error(Status::SigningFailed, &format!("signing failed: {e:?}"))
            }
        }
    }

    fn refusal(e: TxError) -> Vec<u8> {
        let (status, msg) = match e {
            TxError::Malformed(w) => (Status::BadRequest, format!("can't read the transaction: {w}")),
            TxError::Refused(w) => (Status::Refused, w),
            TxError::Key(k) => (Status::SigningFailed, format!("{k:?}")),
        };
        log::warn!("refused: {msg}");
        ui::ui().show_result(Tone::Warn, "REFUSED", vec![msg.chars().take(30).collect(), "Nothing was signed".into()]);
        protocol::error(status, &msg)
    }

    /// SET_HOME: the app's view of an account (balance, address, network) for the home screen.
    /// The address is checked against this device's own key.
    fn set_home(&mut self, p: &[u8]) -> Vec<u8> {
        let bad = || protocol::error(Status::BadRequest, "expected account u32, balance u128, chain id (len u8), address (len u8)");
        if p.len() < 22 {
            return bad();
        }
        let account = u32::from_be_bytes(p[0..4].try_into().unwrap());
        let balance = u128::from_be_bytes(p[4..20].try_into().unwrap());
        let n = p[20] as usize;
        let Some(chain) = p.get(21..21 + n).and_then(|c| core::str::from_utf8(c).ok()) else { return bad() };
        let Some(&m) = p.get(21 + n) else { return bad() };
        let Some(addr) = p.get(22 + n..22 + n + m as usize).and_then(|a| core::str::from_utf8(a).ok()) else { return bad() };
        let Ok(acct) = self.account(account) else { return protocol::error(Status::BadRequest, "no such account") };
        let ok = acct.address_bech == addr;
        if !ok {
            log::warn!("SET_HOME: the app's address for account {} is not this device's", account + 1);
        }
        self.home = Some((
            account,
            screen::Home {
                balance: Some(balance),
                address: Some(addr.to_string()),
                address_ok: Some(ok),
                network: chain.chars().take(24).collect(),
                app_seen: true,
            },
        ));
        self.refresh_idle(account);
        protocol::response(Status::Ok, &[ok as u8])
    }

    fn refresh_idle(&mut self, account: u32) {
        self.idle_account = account;
        self.security.pin_tries = storage::pin_tries_left(&mut self.store);
        self.security.pin_set = storage::pin_is_set(&mut self.store);
        let home = match &self.home {
            Some((a, h)) if *a == account => screen::Home { app_seen: ui::link_up(), ..h.clone() },
            Some((_, h)) => screen::Home { app_seen: ui::link_up(), network: h.network.clone(), ..Default::default() },
            None => screen::Home { app_seen: ui::link_up(), ..Default::default() },
        };
        // without a SET_HOME for this account the device still knows its own address
        let home = if home.address.is_none() {
            screen::Home { address: self.wallet.account(account).ok().map(|a| a.address_bech), address_ok: Some(true), ..home }
        } else {
            home
        };
        ui::ui().set_idle(screen::Screen::Idle {
            account,
            home,
            trust: self.security.trust(),
            passphrase: self.security.passphrase,
        });
    }

    // ------------------------------------------------------------ PIN, recovery words, wipe

    /// Use `entropy` as the wallet seed (no passphrase) from now on.
    fn use_entropy(&mut self, entropy: zeroize::Zeroizing<[u8; 32]>, origin: storage::SeedOrigin) {
        self.wallet = Wallet::new(&entropy, "");
        self.origin = origin;
        self.security.passphrase = false;
    }

    /// Store `entropy` as the seed (wrapped on the device) and switch to it.
    fn install_entropy(&mut self, entropy: zeroize::Zeroizing<[u8; 32]>, origin: storage::SeedOrigin) -> bool {
        #[cfg(target_os = "xous")]
        let wrap: Option<&mut dyn storage::SeedWrap> = Some(&mut self.chip_key);
        #[cfg(not(target_os = "xous"))]
        let wrap: Option<&mut dyn storage::SeedWrap> = None;
        match storage::replace_entropy(&mut self.store, wrap, &entropy) {
            Some(protection) => {
                self.security.seed = protection;
                self.use_entropy(entropy, origin);
                true
            }
            None => {
                log::error!("could not store the new seed");
                false
            }
        }
    }

    /// After a wipe: a fresh seed from the TRNG; the companion then asks for a new PIN.
    fn new_wallet(&mut self, io: &mut dyn flows::Io) {
        storage::wipe_wallet(&mut self.store);
        if !self.install_entropy(storage::fresh_entropy(), storage::SeedOrigin::Created) {
            io.result(Tone::Bad, "FAILED", vec!["Couldn't store a new seed".into()]);
        }
    }

    /// SET_PIN: choose the signing PIN, or change it (the current one first).
    fn set_pin(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        let new = if storage::pin_is_set(&mut self.store) {
            match flows::change_pin(&mut io, &mut self.store) {
                None => return Self::cancelled(),
                Some(None) => return protocol::error(Status::Rejected, "wrong PIN: unchanged"),
                Some(Some(p)) => p,
            }
        } else {
            match flows::choose_pin(&mut io, "Choose a signing PIN") {
                None => return Self::cancelled(),
                Some(p) => p,
            }
        };
        self.store_pin(&mut io, new)
    }

    /// RESET_PIN: forgot the PIN: this wallet's 24 words on the device, then a new PIN.
    fn reset_pin(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        match flows::forgot_pin(&mut io, self.wallet.entropy()) {
            None => Self::cancelled(),
            Some(None) => protocol::error(Status::Rejected, "those words are another wallet's: PIN unchanged"),
            Some(Some(p)) => self.store_pin(&mut io, p),
        }
    }

    fn store_pin(&mut self, io: &mut dyn flows::Io, pin: u8) -> Vec<u8> {
        let reply = if storage::set_pin(&mut self.store, pin) {
            io.result(Tone::Good, "PIN SET", vec!["Asked for every signature".into(), format!("{} wrong in a row lock signing", storage::PIN_TRIES)]);
            protocol::response(Status::Ok, b"PIN set")
        } else {
            io.result(Tone::Bad, "FAILED", vec!["Couldn't store the PIN".into()]);
            protocol::error(Status::StoreFailed, "could not store the PIN")
        };
        self.refresh_idle(self.idle_account);
        reply
    }

    fn no_display() -> Option<Vec<u8>> {
        (!ui::Ui::enabled()).then(|| protocol::error(Status::NoApprovalUi, "no display in this build"))
    }

    fn cancelled() -> Vec<u8> {
        protocol::error(Status::Rejected, "cancelled on the device")
    }

    /// SHOW_PHRASE: the PIN again, then the 24 words on the device's screen.
    fn show_phrase(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        if let Some(e) = self.pin_refusal() {
            return e;
        }
        let reply = match flows::check_pin(&mut io, &mut self.store, "Your PIN, to show the recovery words") {
            flows::PinOk::Right => {
                let m = mnemonic_from_entropy(self.wallet.entropy());
                if flows::show_phrase(&mut io, &m) { protocol::response(Status::Ok, b"shown") } else { Self::cancelled() }
            }
            r => Self::pin_failure(r),
        };
        self.refresh_idle(self.idle_account);
        reply
    }

    /// RESTORE: 24 words entered on the device replace the seed.
    fn restore(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        let reply = match flows::restore(&mut io) {
            Some(m) => {
                let (bytes, len) = m.to_entropy_array();
                if len != 32 {
                    protocol::error(Status::BadRequest, "only 24-word phrases")
                } else {
                    let mut e = zeroize::Zeroizing::new([0u8; 32]);
                    e.copy_from_slice(&bytes[..32]);
                    if self.install_entropy(e, storage::SeedOrigin::Restored) {
                        io.result(Tone::Good, "RESTORED", vec!["Your wallet is back".into()]);
                        protocol::response(Status::Ok, b"restored")
                    } else {
                        io.result(Tone::Bad, "FAILED", vec!["Couldn't store the seed".into()]);
                        protocol::error(Status::StoreFailed, "could not store the restored seed")
                    }
                }
            }
            None => Self::cancelled(),
        };
        self.home = None;
        self.refresh_idle(0);
        reply
    }

    /// WIPE: erase the wallet, make a new one and a new PIN.
    fn wipe(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        let reply = if flows::confirm_wipe(&mut io) {
            self.new_wallet(&mut io);
            protocol::response(Status::Ok, b"wiped: new wallet")
        } else {
            Self::cancelled()
        };
        self.home = None;
        self.refresh_idle(0);
        reply
    }

    /// Refuse before anything is shown: no PIN yet, or signing locked.
    fn pin_refusal(&mut self) -> Option<Vec<u8>> {
        if !storage::pin_is_set(&mut self.store) {
            return Some(protocol::error(Status::NoPin, "no signing PIN yet: set one first (the companion's setup, or SET_PIN)"));
        }
        (storage::pin_tries_left(&mut self.store) == 0)
            .then(|| protocol::error(Status::PinLocked, "PIN locked after 5 wrong tries: reset it with your 24 recovery words"))
    }

    fn pin_failure(r: flows::PinOk) -> Vec<u8> {
        match r {
            flows::PinOk::Wrong(n) => protocol::error(Status::Rejected, &format!("wrong PIN ({n} tries left): nothing was signed")),
            flows::PinOk::Locked => {
                protocol::error(Status::PinLocked, "PIN locked after 5 wrong tries: reset it with your 24 recovery words")
            }
            _ => protocol::error(Status::Rejected, "no PIN entered: nothing was signed"),
        }
    }

    /// PASSPHRASE: switch to the wallet of the 24 words plus a passphrase (until restart).
    fn passphrase(&mut self) -> Vec<u8> {
        if let Some(e) = Self::no_display() {
            return e;
        }
        let mut io = flows::DeviceIo;
        let reply = match flows::passphrase(&mut io) {
            Some(p) => {
                let entropy = *self.wallet.entropy();
                self.wallet = Wallet::new(&entropy, &p);
                self.security.passphrase = !p.is_empty();
                drop(zeroize::Zeroizing::new(p));
                io.result(
                    Tone::Good,
                    if self.security.passphrase { "PASSPHRASE" } else { "STANDARD" },
                    vec![if self.security.passphrase { "Passphrase wallet in use".into() } else { "Standard wallet in use".into() }],
                );
                protocol::response(Status::Ok, &[self.security.passphrase as u8])
            }
            None => Self::cancelled(),
        };
        self.home = None;
        self.refresh_idle(0);
        reply
    }

    fn info(&mut self) -> Vec<u8> {
        fn approval_text() -> String {
            #[cfg(all(target_os = "xous", feature = "panel"))]
            return format!("the panel and button on the board (panel: {})", panel::status());
            #[allow(unreachable_code)]
            if ui::Ui::enabled() {
                "virtual display (vdisplay; dev only, the laptop draws the screen)".into()
            } else {
                "none: signing is refused".into()
            }
        }
        let state = if cfg!(target_os = "xous") {
            format!("keystore slots, PIN attempts in one-way counter {}", storage::PIN_OWC)
        } else {
            "host file (emulator, not secret, not one-way)".into()
        };
        let text = format!(
            "{VERSION}\nseed: {} (TRNG entropy, keystore slots {}..={})\nstate: {state}\nscheme: QoreChain hybrid: secp256k1 + ML-DSA-87 (FIPS 204), on the CPU\nbuttons: two; left (PB1) is 0 / reject, right (PB2) is 1 / approve\napproval: {}\nfault checks: each signature verified with its public key before release\n{}",
            match self.origin {
                storage::SeedOrigin::Created => "created on this boot",
                storage::SeedOrigin::Loaded => "loaded from the keystore",
                storage::SeedOrigin::Replaced => "NEW: the stored seed no longer unwraps on this chip",
                storage::SeedOrigin::Restored => "restored from 24 words on this device",
            },
            storage::SEED_SLOT,
            storage::WRAP_SLOT + 1,
            approval_text(),
            self.security.describe(),
        );
        protocol::response(Status::Ok, text.as_bytes())
    }

    /// BIO_SELFTEST: the BIO display and button engines against the BIO test cores.
    fn bio_selftest() -> Vec<u8> {
        #[cfg(all(target_os = "xous", not(feature = "panel")))]
        {
            let (ok, report) = bioio::selftest();
            log::info!("BIO self-test:\n{report}");
            return protocol::response(if ok { Status::Ok } else { Status::SigningFailed }, report.as_bytes());
        }
        #[allow(unreachable_code)]
        protocol::error(Status::NoHardware, "BIO self-test: on the board only, and not in a panel build (the panel owns the pins)")
    }
}

impl transport::Handler for App {
    fn handle(&mut self, body: &[u8]) -> Vec<u8> {
        if body.len() < 2 || body[0] != MSG_TYPE {
            return protocol::error(Status::BadRequest, "not a Qorebit message");
        }
        let payload = &body[2..];
        match body[1] {
            protocol::CMD_GET_VERSION => protocol::response(Status::Ok, VERSION.as_bytes()),
            protocol::CMD_GET_ACCOUNT => self.get_account(payload),
            protocol::CMD_GET_PQ_PUBLIC_KEY => self.get_pq_public_key(payload),
            protocol::CMD_REGISTER_PROOF => self.register_proof(payload),
            protocol::CMD_SIGN_TX => self.sign_tx(payload),
            protocol::CMD_GET_INFO => self.info(),
            protocol::CMD_GET_SECURITY => {
                self.security.pin_tries = storage::pin_tries_left(&mut self.store);
                self.security.pin_set = storage::pin_is_set(&mut self.store);
                protocol::response(Status::Ok, &self.security.encode())
            }
            protocol::CMD_SHOW_PHRASE => self.show_phrase(),
            protocol::CMD_RESTORE => self.restore(),
            protocol::CMD_WIPE => self.wipe(),
            protocol::CMD_PASSPHRASE => self.passphrase(),
            protocol::CMD_SET_PIN => self.set_pin(),
            protocol::CMD_RESET_PIN => self.reset_pin(),
            protocol::CMD_BIO_SELFTEST => Self::bio_selftest(),
            protocol::CMD_SET_HOME => self.set_home(payload),
            _ => protocol::error(Status::UnknownCommand, "unknown command"),
        }
    }
}

fn job_of(cmd: u8) -> u8 {
    match cmd {
        protocol::CMD_GET_ACCOUNT | protocol::CMD_GET_PQ_PUBLIC_KEY => 1,
        protocol::CMD_SIGN_TX | protocol::CMD_REGISTER_PROOF => 2,
        protocol::CMD_BIO_SELFTEST => 12,
        protocol::CMD_SET_HOME => 3,
        protocol::CMD_GET_INFO | protocol::CMD_GET_SECURITY => 7,
        protocol::CMD_SHOW_PHRASE
        | protocol::CMD_RESTORE
        | protocol::CMD_WIPE
        | protocol::CMD_PASSPHRASE
        | protocol::CMD_SET_PIN
        | protocol::CMD_RESET_PIN => 11,
        _ => 9,
    }
}

/// Transport-side front end: forwards requests to the worker thread and answers progress
/// while a job runs, so a slow job (one waiting for the user's button) is visible and
/// distinguishable from a hang.
struct Dispatcher {
    to_worker: mpsc::Sender<Vec<u8>>,
    from_worker: mpsc::Receiver<Vec<u8>>,
    busy: bool,
    /// The running job is the boot job, whose reply nobody waits for.
    boot_job: bool,
    started: Instant,
}

impl Dispatcher {
    fn progress(&self, status: Status) -> Vec<u8> {
        let mut p = vec![JOB.load(Ordering::Relaxed)];
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&(self.started.elapsed().as_millis() as u32).to_be_bytes());
        protocol::response(status, &p)
    }

    fn wait(&mut self, timeout: Duration) -> Vec<u8> {
        match self.from_worker.recv_timeout(timeout) {
            Ok(reply) => {
                self.busy = false;
                self.boot_job = false;
                reply
            }
            Err(_) => self.progress(Status::Pending),
        }
    }
}

impl transport::Handler for Dispatcher {
    fn handle(&mut self, body: &[u8]) -> Vec<u8> {
        if body.len() < 2 || body[0] != MSG_TYPE {
            return protocol::error(Status::BadRequest, "not a Qorebit message");
        }
        // the companion app is the one that asks for accounts, security and home screens: note that it is
        // there (the bridge's own screen polling and presses do not count)
        if !matches!(body[1], protocol::CMD_GET_VERSION | protocol::CMD_DISPLAY_POLL | protocol::CMD_DISPLAY_INPUT | protocol::CMD_RESULT) {
            LAST_HOST_MS.store(host_clock_ms(), Ordering::Relaxed);
            if !ui::link_up() {
                ui::ui().set_link(true);
            }
        }
        // version always answers at once: a liveness check even while a job runs
        if body[1] == protocol::CMD_GET_VERSION {
            return protocol::response(Status::Ok, VERSION.as_bytes());
        }
        // the screen is read and the button pressed while a job waits for a decision
        if body[1] == protocol::CMD_DISPLAY_POLL {
            link_tick();
            let p = &body[2..];
            let since = (p.len() >= 4).then(|| u32::from_be_bytes(p[0..4].try_into().unwrap()));
            return protocol::response(Status::Ok, &encode_snapshot(ui::ui().poll(since)));
        }
        if body[1] == protocol::CMD_DISPLAY_INPUT {
            let p = &body[2..];
            if p.len() != 5 || !ui::Ui::enabled() {
                return protocol::error(Status::BadRequest, "expected screen id u32, decision u8");
            }
            let id = u32::from_be_bytes(p[0..4].try_into().unwrap());
            let right = p[4] != 0;
            return match ui::ui().input(id, right) {
                Some(snap) => protocol::response(Status::Ok, &encode_snapshot(snap)),
                None => protocol::error(Status::BadRequest, "that screen is no longer shown"),
            };
        }
        if self.boot_job && self.from_worker.try_recv().is_ok() {
            self.busy = false;
            self.boot_job = false;
        }
        if body[1] == protocol::CMD_RESULT {
            return if self.busy { self.wait(Duration::from_millis(50)) } else { protocol::error(Status::BadRequest, "no job pending") };
        }
        if self.busy {
            return self.progress(Status::Busy);
        }
        JOB.store(job_of(body[1]), Ordering::Relaxed);
        self.started = Instant::now();
        self.busy = true;
        if self.to_worker.send(body.to_vec()).is_err() {
            self.busy = false;
            return protocol::error(Status::SigningFailed, "worker thread gone");
        }
        self.wait(Duration::from_millis(200))
    }
}

fn main() -> ! {
    log_server::init_wait().unwrap();
    log::set_max_level(log::LevelFilter::Info);
    log::info!("{VERSION}, PID {}", xous::process::id());

    let (to_worker, worker_rx) = mpsc::channel::<Vec<u8>>();
    let (worker_tx, from_worker) = mpsc::channel::<Vec<u8>>();
    JOB.store(6, Ordering::Relaxed);
    std::thread::Builder::new()
        .name("qorebit-worker".into())
        // flows, screens and the protocol; the signatures run on their own thread (`crypto`)
        .stack_size(256 * 1024)
        .spawn(move || {
            // Seed: TRNG entropy kept in keystore app slots (host file in the emulator).
            #[cfg(target_os = "xous")]
            let mut slots = storage::Keystore::open();
            #[cfg(not(target_os = "xous"))]
            let mut slots = storage::HostFile::open();
            // the device keeps the seed wrapped under the chip's master key (storage.rs)
            #[cfg(target_os = "xous")]
            let mut chip_key = storage::Keystore::open();
            #[cfg(target_os = "xous")]
            let wrap: Option<&mut dyn storage::SeedWrap> = Some(&mut chip_key);
            #[cfg(not(target_os = "xous"))]
            let wrap: Option<&mut dyn storage::SeedWrap> = None;
            let (entropy, origin, protection) = storage::load_or_create_entropy(&mut slots, wrap).expect("seed storage");
            match origin {
                storage::SeedOrigin::Created => log::info!("new wallet: seed created from the chip TRNG and stored"),
                storage::SeedOrigin::Loaded => log::info!("wallet seed loaded from the keystore"),
                storage::SeedOrigin::Replaced => {
                    log::error!("the stored seed no longer unwraps on this chip: a NEW wallet was created")
                }
                storage::SeedOrigin::Restored => {}
            }
            #[cfg(target_os = "xous")]
            let security = security::read(&slots, protection);
            #[cfg(not(target_os = "xous"))]
            let security = security::read(protection);
            match security.trust() {
                security::Trust::Secure => log::info!("chip: secure boot"),
                t => log::warn!("chip: {t:?}: {}", security.describe().replace('\n', "; ")),
            }
            let mut app = App {
                wallet: Wallet::new(&entropy, ""),
                origin,
                security,
                store: slots,
                #[cfg(target_os = "xous")]
                chip_key,
                idle_account: 0,
                home: None,
            };
            let t = Instant::now();
            let a = app.account(0).ok().map(|a| a.address_bech);
            log::info!("account 1: {} ({} ms)", a.unwrap_or_default(), t.elapsed().as_millis());
            app.refresh_idle(0);
            worker_tx.send(protocol::response(Status::Ok, b"boot ready")).ok();
            for body in worker_rx {
                let reply = transport::Handler::handle(&mut app, &body);
                JOB.store(0, Ordering::Relaxed);
                if worker_tx.send(reply).is_err() {
                    break;
                }
            }
        })
        .expect("worker thread");
    // the screen and the button on the board
    #[cfg(all(target_os = "xous", feature = "panel"))]
    panel::spawn();
    // the boot job counts as pending until its reply is collected
    transport::serve(Dispatcher { to_worker, from_worker, busy: true, boot_job: true, started: Instant::now() })
}

/// seq u32, screen id u32, kind u8, seconds left u8, vdisplay u8, has_frame u8, then
/// with a frame: width u16, height u16, rows y0 u16, y1 u16, palette 16 x RGB, RLE pixels of y0..y1
fn encode_snapshot(s: ui::Snapshot) -> Vec<u8> {
    let mut p = Vec::with_capacity(12 + s.frame.as_ref().map_or(0, Vec::len));
    p.extend_from_slice(&s.seq.to_be_bytes());
    p.extend_from_slice(&s.id.to_be_bytes());
    p.push(s.kind);
    p.push(s.left);
    p.push(ui::Ui::enabled() as u8);
    p.push(s.frame.is_some() as u8);
    if let Some(f) = s.frame {
        p.extend_from_slice(&f);
    }
    p
}
