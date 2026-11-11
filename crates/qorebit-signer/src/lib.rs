//! QoreChain hybrid signing for the Qorebit hardware wallet.
//!
//! QoreChain's native lane requires **every transaction to carry two signatures**: secp256k1
//! over the Cosmos `SignDoc`, and ML-DSA-87 (FIPS 204) over the "hybrid sign bytes". This crate
//! derives the keys from the wallet's 24 words (`keys`), reads a transaction the way the device
//! must show it (`tx`), and makes both signatures (`tx::sign`). Everything is checked against
//! `@qorechain/qoresign` in `tests/`.
#![no_std]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod keys;
pub mod proto;
pub mod tx;

pub use bip39::{Language, Mnemonic};
pub use keys::{Account, KeyError, Wallet};

/// The 24 recovery words of 32 bytes of entropy.
pub fn mnemonic_from_entropy(entropy: &[u8; 32]) -> Mnemonic {
    Mnemonic::from_entropy(entropy).expect("32 bytes of entropy make a 24-word phrase")
}
