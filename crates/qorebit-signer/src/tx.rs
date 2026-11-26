//! Reading and signing a QoreChain transaction.
//!
//! The host (the companion app) sends the unsigned `TxBody` and `AuthInfo` bytes, the chain id
//! and the account number. The device decodes them itself ([`decode`]), refuses what it can't
//! show completely ([`check`]), and after the user approves makes the two signatures
//! ([`sign`]):
//!
//! 1. **ML-DSA-87** over the hybrid sign bytes, `"qorechain-pqc-hybrid-v2" | BE64(len chain id)
//!    | chain id | BE32(len B0) | B0 | BE32(len AuthInfo) | AuthInfo` (v1 on chains that have
//!    not switched: no tag and no chain id), where B0 is the body without the PQC extension;
//! 2. **secp256k1** over `SignDoc{body with the PQC extension, auth info, chain id, account
//!    number}` (SIGN_MODE_DIRECT), SHA-256, low-S, so it also covers the ML-DSA signature.
//!
//! The PQC extension is `/qorechain.pqc.v1.PQCHybridSignature{algorithm 1, signature,
//! optional public key}` among the body's extension options. A key-registration transaction
//! (`MsgRegisterPQCKeyV2`) is the one transaction signed classical-only.

use crate::keys::{self, Account, KeyError, PQ_PUBLIC_LEN, PQ_SIG_LEN};
use crate::proto::{self, Field, Wire};
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use sha2::{Digest, Sha256};

pub const PQC_HYBRID_TYPE_URL: &str = "/qorechain.pqc.v1.PQCHybridSignature";
pub const MSG_REGISTER_PQC_KEY_V2: &str = "/qorechain.pqc.v1.MsgRegisterPQCKeyV2";
pub const MSG_SEND: &str = "/cosmos.bank.v1beta1.MsgSend";
pub const MSG_DELEGATE: &str = "/cosmos.staking.v1beta1.MsgDelegate";
pub const MSG_UNDELEGATE: &str = "/cosmos.staking.v1beta1.MsgUndelegate";
pub const MSG_WITHDRAW_REWARD: &str = "/cosmos.distribution.v1beta1.MsgWithdrawDelegatorReward";
pub const SECP256K1_PUBKEY: &str = "/cosmos.crypto.secp256k1.PubKey";
/// The public key type of an EVM-lane (eth_secp256k1) account.
pub const ETHSECP256K1_PUBKEY: &str = "/cosmos.evm.crypto.v1.ethsecp256k1.PubKey";
pub const POP_DOMAIN: &str = "qorechain-pqc-pop-v1";
pub const HYBRID_V2_DOMAIN: &str = "qorechain-pqc-hybrid-v2";
pub const ALGORITHM_ML_DSA_87: u64 = 1;
/// Longest body or auth info the device takes.
pub const MAX_PART: usize = 10 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum TxError {
    /// Not a transaction the device can read to the end.
    Malformed(&'static str),
    /// Readable, but something the device refuses to sign.
    Refused(String),
    Key(KeyError),
}

impl From<KeyError> for TxError {
    fn from(e: KeyError) -> Self {
        TxError::Key(e)
    }
}

fn refuse<T>(why: impl Into<String>) -> Result<T, TxError> {
    Err(TxError::Refused(why.into()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignBytes {
    V1,
    V2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Hybrid,
    /// Only for the transaction that registers the account's ML-DSA key.
    ClassicalOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

impl Coin {
    /// "1.5 QOR" for `uqor` (1 QOR = 10^6 uqor), "<amount> <denom>" for anything else.
    pub fn pretty(&self) -> String {
        if self.denom == "uqor" {
            if let Ok(v) = self.amount.parse::<u128>() {
                let (whole, frac) = (v / 1_000_000, v % 1_000_000);
                return if frac == 0 {
                    format!("{whole} QOR")
                } else {
                    let f = format!("{frac:06}");
                    format!("{whole}.{} QOR", f.trim_end_matches('0'))
                };
            }
        }
        format!("{} {}", self.amount, self.denom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Send { from: String, to: String, amount: Vec<Coin> },
    Delegate { delegator: String, validator: String, amount: Coin },
    Undelegate { delegator: String, validator: String, amount: Coin },
    WithdrawReward { delegator: String, validator: String },
    RegisterPqKey { account: String, public_key: Vec<u8>, algorithm: u64, classical: Vec<u8>, key_type: String, proof: Vec<u8> },
    /// Any other message: the device can show only its type and a short fingerprint (BLIND).
    Other { type_url: String, len: usize, id: [u8; 4] },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub msgs: Vec<Msg>,
    pub memo: String,
    pub timeout_height: u64,
    pub fee: Vec<Coin>,
    pub gas_limit: u64,
    pub fee_payer: String,
    pub fee_granter: String,
    pub sequence: u64,
    pub signer_public: Option<Vec<u8>>,
    /// `Some(true)` when the signer's public key is an eth_secp256k1 key, `Some(false)` for a secp256k1 one.
    pub signer_eth: Option<bool>,
}

fn string(b: &[u8]) -> Result<String, TxError> {
    proto::utf8(b).map(ToString::to_string).ok_or(TxError::Malformed("invalid UTF-8"))
}

fn coin(b: &[u8]) -> Result<Coin, TxError> {
    let fs = proto::fields(b).ok_or(TxError::Malformed("coin"))?;
    Ok(Coin { denom: string(proto::find(&fs, 1).unwrap_or(&[]))?, amount: string(proto::find(&fs, 2).unwrap_or(&[]))? })
}

fn coins(fs: &[Field], num: u32) -> Result<Vec<Coin>, TxError> {
    fs.iter().filter(|f| f.num == num && f.wire == Wire::Len).map(|f| coin(f.bytes)).collect()
}

fn text(fs: &[Field], num: u32) -> Result<String, TxError> {
    string(proto::find(fs, num).unwrap_or(&[]))
}

fn message(type_url: &str, value: &[u8]) -> Result<Msg, TxError> {
    // a message the device can't show is not parsed at all: it is shown BLIND, by type and fingerprint
    let known = [MSG_SEND, MSG_DELEGATE, MSG_UNDELEGATE, MSG_WITHDRAW_REWARD, MSG_REGISTER_PQC_KEY_V2];
    let fs = if known.contains(&type_url) { proto::fields(value).ok_or(TxError::Malformed("message"))? } else { Vec::new() };
    Ok(match type_url {
        MSG_SEND => Msg::Send { from: text(&fs, 1)?, to: text(&fs, 2)?, amount: coins(&fs, 3)? },
        MSG_DELEGATE | MSG_UNDELEGATE => {
            let (delegator, validator) = (text(&fs, 1)?, text(&fs, 2)?);
            let amount = coin(proto::find(&fs, 3).unwrap_or(&[]))?;
            if type_url == MSG_DELEGATE {
                Msg::Delegate { delegator, validator, amount }
            } else {
                Msg::Undelegate { delegator, validator, amount }
            }
        }
        MSG_WITHDRAW_REWARD => Msg::WithdrawReward { delegator: text(&fs, 1)?, validator: text(&fs, 2)? },
        MSG_REGISTER_PQC_KEY_V2 => Msg::RegisterPqKey {
            account: text(&fs, 1)?,
            public_key: proto::find(&fs, 2).unwrap_or(&[]).to_vec(),
            algorithm: fs.iter().find(|f| f.num == 3 && f.wire == Wire::Varint).map_or(0, |f| f.varint),
            classical: proto::find(&fs, 4).unwrap_or(&[]).to_vec(),
            key_type: text(&fs, 5)?,
            proof: proto::find(&fs, 6).unwrap_or(&[]).to_vec(),
        },
        _ => {
            let h = Sha256::digest(value);
            Msg::Other { type_url: type_url.to_string(), len: value.len(), id: h[..4].try_into().unwrap() }
        }
    })
}

/// The body without any `PQCHybridSignature` extension (B0).
pub fn strip_extension(body: &[u8], type_url: &str) -> Result<Vec<u8>, TxError> {
    let fs = proto::fields(body).ok_or(TxError::Malformed("body"))?;
    let mut out = Vec::with_capacity(body.len());
    for f in &fs {
        if f.num == 1023 && f.wire == Wire::Len {
            let a = proto::fields(f.bytes).ok_or(TxError::Malformed("extension"))?;
            if proto::find(&a, 1) == Some(type_url.as_bytes()) {
                continue;
            }
        }
        out.extend_from_slice(f.raw);
    }
    Ok(out)
}

/// Add an extension option in canonical order: after the existing ones, before any
/// non-critical extension options (field numbers above 1023).
pub fn append_extension(body: &[u8], any: &[u8]) -> Result<Vec<u8>, TxError> {
    let fs = proto::fields(body).ok_or(TxError::Malformed("body"))?;
    let at = fs.iter().position(|f| f.num > 1023).unwrap_or(fs.len());
    let mut out = Vec::with_capacity(body.len() + any.len() + 8);
    for f in &fs[..at] {
        out.extend_from_slice(f.raw);
    }
    proto::put_len(&mut out, 1023, any);
    for f in &fs[at..] {
        out.extend_from_slice(f.raw);
    }
    Ok(out)
}

/// Read a transaction (the PQC extension, if present, is ignored: it is made fresh at signing).
pub fn decode(body: &[u8], auth_info: &[u8]) -> Result<View, TxError> {
    if body.len() > MAX_PART || auth_info.len() > MAX_PART {
        return refuse("transaction too large for the device");
    }
    let bfs = proto::fields(body).ok_or(TxError::Malformed("body"))?;
    let mut v = View {
        msgs: Vec::new(),
        memo: String::new(),
        timeout_height: 0,
        fee: Vec::new(),
        gas_limit: 0,
        fee_payer: String::new(),
        fee_granter: String::new(),
        sequence: 0,
        signer_public: None,
        signer_eth: None,
    };
    for f in &bfs {
        match (f.num, f.wire) {
            (1, Wire::Len) => {
                let a = proto::fields(f.bytes).ok_or(TxError::Malformed("message"))?;
                let url = proto::utf8(proto::find(&a, 1).unwrap_or(&[])).ok_or(TxError::Malformed("type url"))?;
                v.msgs.push(message(url, proto::find(&a, 2).unwrap_or(&[]))?);
            }
            (2, Wire::Len) => v.memo = string(f.bytes)?,
            (3, Wire::Varint) => v.timeout_height = f.varint,
            (1023, Wire::Len) => {
                let a = proto::fields(f.bytes).ok_or(TxError::Malformed("extension"))?;
                if proto::find(&a, 1) != Some(PQC_HYBRID_TYPE_URL.as_bytes()) {
                    return refuse("the transaction carries an extension option the device can't show");
                }
            }
            _ => return refuse(format!("the transaction body has a field the device can't show (field {})", f.num)),
        }
    }
    let afs = proto::fields(auth_info).ok_or(TxError::Malformed("auth info"))?;
    let mut signers = 0;
    for f in &afs {
        match (f.num, f.wire) {
            (1, Wire::Len) => {
                signers += 1;
                let s = proto::fields(f.bytes).ok_or(TxError::Malformed("signer info"))?;
                if let Some(pk) = proto::find(&s, 1) {
                    let a = proto::fields(pk).ok_or(TxError::Malformed("public key"))?;
                    let url = proto::find(&a, 1);
                    if url != Some(SECP256K1_PUBKEY.as_bytes()) && url != Some(ETHSECP256K1_PUBKEY.as_bytes()) {
                        return refuse("the signer's key is not a secp256k1 or eth_secp256k1 key");
                    }
                    v.signer_eth = Some(url == Some(ETHSECP256K1_PUBKEY.as_bytes()));
                    let k = proto::fields(proto::find(&a, 2).unwrap_or(&[])).ok_or(TxError::Malformed("public key"))?;
                    v.signer_public = Some(proto::find(&k, 1).unwrap_or(&[]).to_vec());
                }
                // mode_info.single.mode must be SIGN_MODE_DIRECT (1)
                let mode = proto::find(&s, 2)
                    .and_then(proto::fields)
                    .and_then(|m| proto::find(&m, 1).and_then(proto::fields))
                    .and_then(|single| single.iter().find(|x| x.num == 1 && x.wire == Wire::Varint).map(|x| x.varint));
                if mode != Some(1) {
                    return refuse("only direct signing (SIGN_MODE_DIRECT) is supported");
                }
                v.sequence = s.iter().find(|x| x.num == 3 && x.wire == Wire::Varint).map_or(0, |x| x.varint);
            }
            (2, Wire::Len) => {
                let fe = proto::fields(f.bytes).ok_or(TxError::Malformed("fee"))?;
                v.fee = coins(&fe, 1)?;
                v.gas_limit = fe.iter().find(|x| x.num == 2 && x.wire == Wire::Varint).map_or(0, |x| x.varint);
                v.fee_payer = text(&fe, 3)?;
                v.fee_granter = text(&fe, 4)?;
            }
            _ => return refuse("the auth info has a field the device can't show (tip?)"),
        }
    }
    if signers != 1 {
        return refuse("exactly one signer is supported");
    }
    if v.msgs.is_empty() {
        return refuse("the transaction has no message");
    }
    Ok(v)
}

/// What the device accepts to sign for `acct`: one message, from this account, shown
/// completely or marked blind, in the right signing mode.
pub fn check(v: &View, acct: &mut Account, chain_id: &str, mode: Mode) -> Result<(), TxError> {
    if chain_id.is_empty() || chain_id.len() > 64 || !chain_id.bytes().all(|b| b.is_ascii_graphic()) {
        return refuse("bad chain id");
    }
    if v.msgs.len() != 1 {
        return refuse("one message per transaction: the device can only show one");
    }
    if let Some(pk) = &v.signer_public {
        if pk[..] != acct.public[..] {
            return refuse("the transaction names another signer key than this account's");
        }
    }
    if let Some(eth) = v.signer_eth {
        if eth != (acct.kind == keys::KeyKind::Eth) {
            return refuse("the transaction's key type is not this account's (EVM accounts use eth_secp256k1 keys)");
        }
    }
    if !v.fee_payer.is_empty() && v.fee_payer != acct.address_bech {
        return refuse("another account pays the fee: it would have to sign too");
    }
    let me = acct.address_bech.clone();
    let signer_ok = |who: &str| if who == me { Ok(()) } else { refuse("the message is not from this account") };
    match (&v.msgs[0], mode) {
        (Msg::Send { from, .. }, Mode::Hybrid) => signer_ok(from),
        (Msg::Delegate { delegator, .. } | Msg::Undelegate { delegator, .. } | Msg::WithdrawReward { delegator, .. }, Mode::Hybrid) => {
            signer_ok(delegator)
        }
        (Msg::Other { .. }, Mode::Hybrid) => Ok(()),
        (Msg::RegisterPqKey { .. }, Mode::Hybrid) => refuse("a key registration is signed classical-only"),
        (Msg::RegisterPqKey { account, public_key, algorithm, classical, key_type, proof }, Mode::ClassicalOnly) => {
            signer_ok(account)?;
            let own_pq = *acct.pq_public();
            let proof: Result<[u8; PQ_SIG_LEN], _> = proof.as_slice().try_into();
            let ok = public_key[..] == own_pq[..]
                && *algorithm == ALGORITHM_ML_DSA_87
                && classical[..] == acct.public[..]
                && key_type == "hybrid"
                && proof.is_ok_and(|p| keys::pq_verify(&own_pq, &pop_message(chain_id, &me, &own_pq), &p));
            if ok { Ok(()) } else { refuse("the registration is not this account's own key (or its proof is wrong)") }
        }
        (_, Mode::ClassicalOnly) => refuse("classical-only signing is only for a key registration"),
    }
}

/// `qorechain-pqc-pop-v1|<chain id>|1|<account>|<hex public key>`.
pub fn pop_message(chain_id: &str, account: &str, public: &[u8; PQ_PUBLIC_LEN]) -> Vec<u8> {
    let mut m = format!("{POP_DOMAIN}|{chain_id}|{ALGORITHM_ML_DSA_87}|{account}|").into_bytes();
    m.extend_from_slice(&keys::vec_hex(public));
    m
}

/// The proof of possession a key registration carries: the account's ML-DSA key signs its own
/// registration message for this chain.
pub fn register_proof(acct: &mut Account, chain_id: &str) -> Result<Box<[u8; PQ_SIG_LEN]>, KeyError> {
    let public = *acct.pq_public();
    let msg = pop_message(chain_id, &acct.address_bech.clone(), &public);
    acct.pq_sign(&msg)
}

pub fn hybrid_sign_bytes(version: SignBytes, chain_id: &str, b0: &[u8], auth_info: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(b0.len() + auth_info.len() + chain_id.len() + 48);
    if version == SignBytes::V2 {
        m.extend_from_slice(HYBRID_V2_DOMAIN.as_bytes());
        m.extend_from_slice(&(chain_id.len() as u64).to_be_bytes());
        m.extend_from_slice(chain_id.as_bytes());
    }
    m.extend_from_slice(&(b0.len() as u32).to_be_bytes());
    m.extend_from_slice(b0);
    m.extend_from_slice(&(auth_info.len() as u32).to_be_bytes());
    m.extend_from_slice(auth_info);
    m
}

pub fn sign_doc(body: &[u8], auth_info: &[u8], chain_id: &str, account_number: u64) -> Vec<u8> {
    let mut d = Vec::with_capacity(body.len() + auth_info.len() + chain_id.len() + 16);
    proto::put_len(&mut d, 1, body);
    proto::put_len(&mut d, 2, auth_info);
    proto::put_len(&mut d, 3, chain_id.as_bytes());
    proto::put_uint(&mut d, 4, account_number);
    d
}

pub fn tx_raw(body: &[u8], auth_info: &[u8], signature: &[u8]) -> Vec<u8> {
    let mut t = Vec::with_capacity(body.len() + auth_info.len() + signature.len() + 16);
    proto::put_len(&mut t, 1, body);
    proto::put_len(&mut t, 2, auth_info);
    // repeated bytes keep an empty entry, so write the signature explicitly
    proto::put_varint(&mut t, 3 << 3 | 2);
    proto::put_varint(&mut t, signature.len() as u64);
    t.extend_from_slice(signature);
    t
}

pub struct Request<'a> {
    pub chain_id: &'a str,
    pub account_number: u64,
    pub body: &'a [u8],
    pub auth_info: &'a [u8],
    pub version: SignBytes,
    /// Carry the ML-DSA public key inside the extension (the chain also has it registered).
    pub include_public_key: bool,
    pub mode: Mode,
}

pub struct Signed {
    /// The broadcastable `TxRaw`.
    pub tx_bytes: Vec<u8>,
    pub classical: [u8; 64],
    pub pq: Option<Box<[u8; PQ_SIG_LEN]>>,
}

/// Make both signatures. Call [`decode`] and [`check`] first and show the user what they found.
pub fn sign(acct: &mut Account, r: &Request) -> Result<Signed, TxError> {
    let b0 = strip_extension(r.body, PQC_HYBRID_TYPE_URL)?;
    let (body, pq) = match r.mode {
        Mode::ClassicalOnly => (b0, None),
        Mode::Hybrid => {
            let msg = hybrid_sign_bytes(r.version, r.chain_id, &b0, r.auth_info);
            let sig = acct.pq_sign(&msg)?;
            let mut ext = Vec::with_capacity(PQ_SIG_LEN + 16);
            proto::put_uint(&mut ext, 1, ALGORITHM_ML_DSA_87);
            proto::put_len(&mut ext, 2, &sig[..]);
            if r.include_public_key {
                let pk = *acct.pq_public();
                proto::put_len(&mut ext, 3, &pk);
            }
            let any = proto::any(PQC_HYBRID_TYPE_URL, &ext);
            (append_extension(&b0, &any)?, Some(sig))
        }
    };
    let doc = sign_doc(&body, r.auth_info, r.chain_id, r.account_number);
    let classical = acct.classical_sign(&doc)?;
    Ok(Signed { tx_bytes: tx_raw(&body, r.auth_info, &classical), classical, pq })
}
