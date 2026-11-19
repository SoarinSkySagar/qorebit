//! Qorebit on the dabao.
//!
//! A hardware wallet for QoreChain on the Baochip-1x: it holds the keys, shows every transaction
//! on its own screen, and signs only after a **hold** of its one button (a tap rejects) and the
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
#[cfg(all(target_os = "xous", feature = "board-dabao"))]
mod prog;
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
use std::sync::atomic::{AtomicU8, Ordering};
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

