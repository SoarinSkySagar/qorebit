//! Keys of a Qorebit wallet, derived exactly as QoreChain's wallets do (`@qorechain/qoresign`),
//! so the same 24 words give the same accounts in the QoreX wallet and on the device:
//!
//! * classical key: BIP-39 seed (with the optional passphrase) -> BIP-32 `m/44'/118'/0'/0/i`
//!   (secp256k1; the address is `bech32("qor", ripemd160(sha256(compressed key)))`);
//! * post-quantum key (ML-DSA-87, FIPS 204): the "canonical" key of an account, seeded with
//!   `SHAKE256("qorechain:pqc:v1|" + address + "|" + mnemonic, 32 bytes)`. It is bound to the
//!   address and does not use the passphrase.

use alloc::string::String;
use alloc::vec::Vec;
use fips204::ml_dsa_87;
use fips204::traits::{KeyGen, SerDes, Signer as _, Verifier as _};
use hmac::{Hmac, Mac};
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{Signature, SigningKey, VerifyingKey};
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{NonZeroScalar, Scalar};
use ripemd::Ripemd160;
use sha2::{Digest, Sha256, Sha512};
use zeroize::{Zeroize, Zeroizing};

pub const ACCOUNTS: u32 = 8;
pub const PQ_PUBLIC_LEN: usize = ml_dsa_87::PK_LEN;
pub const PQ_SIG_LEN: usize = ml_dsa_87::SIG_LEN;
pub const ADDRESS_PREFIX: &str = "qor";
pub const PQ_DERIVATION_DOMAIN: &str = "qorechain:pqc:v1";

#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    BadIndex,
    Derivation,
    Signing,
    /// A signature failed the check made before release (a fault while signing).
    Fault,
}

/// The wallet's BIP-39 material: the entropy (24 words) and the passphrase.
pub struct Wallet {
    entropy: Zeroizing<[u8; 32]>,
    seed: Zeroizing<[u8; 64]>,
    mnemonic: Zeroizing<String>,
}

impl Wallet {
    #[inline(never)]
    pub fn new(entropy: &[u8; 32], passphrase: &str) -> Self {
        let m = bip39::Mnemonic::from_entropy(entropy).expect("32 bytes of entropy");
        let seed = Zeroizing::new(m.to_seed_normalized(passphrase));
        let mut words = String::new();
        for (i, w) in m.words().enumerate() {
            if i > 0 {
                words.push(' ');
            }
            words.push_str(w);
        }
        Self { entropy: Zeroizing::new(*entropy), seed, mnemonic: Zeroizing::new(words) }
    }

    pub fn entropy(&self) -> &[u8; 32] {
        &self.entropy
    }

    /// The account `index` (0..[`ACCOUNTS`]).
    #[inline(never)]
    pub fn account(&self, index: u32) -> Result<Account, KeyError> {
        if index >= ACCOUNTS {
            return Err(KeyError::BadIndex);
        }
        let sk = bip32_secp256k1(&self.seed, index)?;
        let signing = SigningKey::from(&sk);
        let public = compressed(signing.verifying_key());
        let address = address_bytes(&public);
        let address_bech = bech32_address(&address);
        let pq_seed = pq_seed(&address_bech, &self.mnemonic);
        Ok(Account { index, signing, public, address, address_bech, pq_seed, pq: None })
    }
}

/// One account: both keys of a hybrid QoreChain signer.
pub struct Account {
    pub index: u32,
    signing: SigningKey,
    pub public: [u8; 33],
    pub address: [u8; 20],
    pub address_bech: String,
    pq_seed: Zeroizing<[u8; 32]>,
    /// The ML-DSA-87 key pair, derived on first use (a key generation is the heaviest step, so it is done once).
    pq: Option<(alloc::boxed::Box<[u8; PQ_PUBLIC_LEN]>, Zeroizing<Vec<u8>>)>,
}

impl Account {
    fn ensure_pq(&mut self) {
        if self.pq.is_none() {
            self.pq = Some(pq_keygen(&self.pq_seed));
        }
    }

    /// The ML-DSA-87 public key (2,592 bytes), computed once.
    pub fn pq_public(&mut self) -> &[u8; PQ_PUBLIC_LEN] {
        self.ensure_pq();
        &self.pq.as_ref().unwrap().0
    }

    /// ML-DSA-87 signature of `msg` (FIPS 204, empty context, deterministic: the same bytes
    /// `@qorechain/qoresign` makes), checked with the public key before it is returned.
    pub fn pq_sign(&mut self, msg: &[u8]) -> Result<alloc::boxed::Box<[u8; PQ_SIG_LEN]>, KeyError> {
        self.ensure_pq();
        let (pk, sk) = self.pq.as_ref().unwrap();
        let sig = pq_sign_raw(sk, msg)?;
        if !pq_verify(pk, msg, &sig) {
            return Err(KeyError::Fault);
        }
        Ok(sig)
    }

    /// Compact secp256k1 signature of `sha256(msg)`, low-S, RFC 6979 (what QoreSign's
    /// `classicalSign` makes for `secp256k1` keys), checked before it is returned.
    #[inline(never)]
    pub fn classical_sign(&self, msg: &[u8]) -> Result<[u8; 64], KeyError> {
        let digest = Sha256::digest(msg);
        let sig: Signature = self.signing.sign_prehash(&digest).map_err(|_| KeyError::Signing)?;
        let sig = sig.normalize_s().unwrap_or(sig);
        use k256::ecdsa::signature::hazmat::PrehashVerifier;
        if self.signing.verifying_key().verify_prehash(&digest, &sig).is_err() {
            return Err(KeyError::Fault);
        }
        Ok(sig.to_bytes().into())
    }
}

