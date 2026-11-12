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

