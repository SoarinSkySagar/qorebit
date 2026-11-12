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
                    if proto::find(&a, 1) != Some(SECP256K1_PUBKEY.as_bytes()) {
                        return refuse("the signer's key is not a secp256k1 key");
                    }
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

