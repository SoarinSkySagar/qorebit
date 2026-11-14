//! How much stack does a signature need? `cargo run --release --example stack -- <kib> <mode>` runs one
//! part on a thread with that stack; below the need it crashes. Modes: keygen, sign (raw fips204),
//! wallet, account, pq-public, pq-sign, register-proof, tx-sign (this crate). Each mode is its own
//! function, so one mode's locals do not sit in another's frame.
use fips204::ml_dsa_87;
use fips204::traits::{KeyGen, SerDes, Signer};
use qorebit_signer::tx::{self, Mode, Request, SignBytes};
use qorebit_signer::{Account, Wallet};

#[inline(never)]
fn keygen() -> u8 {
    ml_dsa_87::KG::keygen_from_seed(&[3u8; 32]).0.into_bytes()[0]
}
#[inline(never)]
fn sign() -> u8 {
    let (_pk, sk) = ml_dsa_87::KG::keygen_from_seed(&[3u8; 32]);
    sk.try_sign_with_seed(&[0u8; 32], b"m", &[]).unwrap()[0]
}
#[inline(never)]
fn wallet() -> Wallet {
    Wallet::new(&[7u8; 32], "")
}
#[inline(never)]
fn account(w: &Wallet) -> Account {
    w.account(0).unwrap()
}
#[inline(never)]
fn pq_public(a: &mut Account) -> u8 {
    a.pq_public()[0]
}
#[inline(never)]
fn pq_sign(a: &mut Account) -> u8 {
    a.pq_sign(b"m").unwrap()[0]
}
#[inline(never)]
fn register_proof(a: &mut Account) -> u8 {
    tx::register_proof(a, "qorechain-local").unwrap()[0]
}
#[inline(never)]
fn tx_sign(a: &mut Account) -> usize {
    let body = [0x0a, 0x00];
    let r = Request { chain_id: "x", account_number: 1, body: &body, auth_info: &[], version: SignBytes::V2, include_public_key: true, mode: Mode::Hybrid };
    tx::sign(a, &r).unwrap().tx_bytes.len()
}

fn main() {
    let kib: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(256);
    let what = std::env::args().nth(2).unwrap_or_else(|| "tx-sign".into());
    std::thread::Builder::new()
        .stack_size(kib * 1024)
        .spawn(move || {
            let out = match what.as_str() {
                "keygen" => keygen() as usize,
                "sign" => sign() as usize,
                "wallet" => wallet().entropy()[0] as usize,
                "account" => account(&wallet()).address[0] as usize,
                m => {
                    let w = wallet();
                    let mut a = account(&w);
                    match m {
                        "pq-public" => pq_public(&mut a) as usize,
                        "pq-sign" => pq_sign(&mut a) as usize,
                        "register-proof" => register_proof(&mut a) as usize,
                        _ => tx_sign(&mut a),
                    }
                }
            };
            println!("{what} ok ({out})");
        })
        .unwrap()
        .join()
        .unwrap();
}
