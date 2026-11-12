//! The signer against `@qorechain/qoresign` 0.2.0 (tests/vectors.json, made by
//! `node tools/gen-vectors.mjs`): same keys, same addresses, the same bytes signed, the same
//! signatures. If QoreChain's wallets and this device disagree on one byte, a transaction
//! signed here would be refused by the chain.

use qorebit_signer::tx::{self, Mode, Request, SignBytes};
use qorebit_signer::Wallet;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

fn h(b: &[u8]) -> String {
    hex::encode(b)
}

fn sha(b: &[u8]) -> String {
    h(&Sha256::digest(b))
}

/// The 24-word test phrase "abandon x23 art" is the all-zero-ish entropy: 0x00..00 with the
/// last word's checksum, i.e. entropy of 32 bytes of 0x00.
fn wallet(passphrase: &str) -> Wallet {
    Wallet::new(&[0u8; 32], passphrase)
}

#[test]
fn the_test_phrase_is_the_zero_entropy() {
    let m = bip39::Mnemonic::from_entropy(&[0u8; 32]).unwrap();
    assert_eq!(m.to_string(), s(&vectors()["mnemonic"]));
}

#[test]
fn accounts_match_qoresign() {
    let v = vectors();
    for a in v["accounts"].as_array().unwrap() {
        let w = wallet(s(&a["passphrase"]));
        let mut acct = w.account(a["index"].as_u64().unwrap() as u32).unwrap();
        assert_eq!(acct.address_bech, s(&a["address"]));
        assert_eq!(h(&acct.public), s(&a["classicalPublic"]));
        let pk = *acct.pq_public();
        assert_eq!(sha(&pk), s(&a["pqPublicSha256"]));
        assert_eq!(h(&pk[..32]), s(&a["pqPublicHead"]));
        // a deterministic ML-DSA-87 signature is byte-identical to noble's
        let msg = hex::decode(s(&a["pqMsg"])).unwrap();
        let sig = acct.pq_sign(&msg).unwrap();
        assert_eq!(sha(&sig[..]), s(&a["pqSigSha256"]));
    }
}

fn request<'a>(t: &'a Value, body: &'a [u8], auth: &'a [u8]) -> Request<'a> {
    Request {
        chain_id: s(&t["chainId"]),
        account_number: t["accountNumber"].as_u64().unwrap(),
        body,
        auth_info: auth,
        version: if s(&t["version"]) == "v1" { SignBytes::V1 } else { SignBytes::V2 },
        include_public_key: t["includePublicKey"].as_bool().unwrap(),
        mode: Mode::Hybrid,
    }
}

#[test]
fn hybrid_transactions_match_qoresign_byte_for_byte() {
    let v = vectors();
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    for t in v["txs"].as_array().unwrap() {
        let body = hex::decode(s(&t["bodyBytes"])).unwrap();
        let auth = hex::decode(s(&t["authInfoBytes"])).unwrap();
        let view = tx::decode(&body, &auth).unwrap();
        tx::check(&view, &mut acct, s(&t["chainId"]), Mode::Hybrid).unwrap_or_else(|e| panic!("{}: {e:?}", s(&t["name"])));
        let r = request(t, &body, &auth);
        // the bytes the ML-DSA key signs
        let b0 = tx::strip_extension(&body, tx::PQC_HYBRID_TYPE_URL).unwrap();
        let sb = tx::hybrid_sign_bytes(r.version, r.chain_id, &b0, &auth);
        assert_eq!(h(&sb), s(&t["pqSignBytes"]), "{}", s(&t["name"]));
        let out = tx::sign(&mut acct, &r).unwrap();
        assert_eq!(h(&out.classical), s(&t["classicalSig"]), "classical signature of {}", s(&t["name"]));
        assert_eq!(sha(&out.pq.as_ref().unwrap()[..]), s(&t["pqSigSha256"]), "ML-DSA signature of {}", s(&t["name"]));
        assert_eq!(sha(&out.tx_bytes), s(&t["txBytesSha256"]), "TxRaw of {}", s(&t["name"]));
    }
}

#[test]
fn key_registration_matches_qoresign() {
    let v = vectors();
    let reg = &v["register"];
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    let chain = s(&reg["chainId"]);
    let pk = *acct.pq_public();
    assert_eq!(String::from_utf8(tx::pop_message(chain, &acct.address_bech.clone(), &pk)).unwrap(), s(&reg["popMessage"]));
    let proof = tx::register_proof(&mut acct, chain).unwrap();
    assert_eq!(sha(&proof[..]), s(&reg["popSigSha256"]));
}

#[test]
fn what_the_device_refuses() {
    let v = vectors();
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    let t = &v["txs"][0];
    let body = hex::decode(s(&t["bodyBytes"])).unwrap();
    let auth = hex::decode(s(&t["authInfoBytes"])).unwrap();
    let view = tx::decode(&body, &auth).unwrap();
    // another account's message
    let mut other = w.account(1).unwrap();
    assert!(tx::check(&view, &mut other, "qorechain-local", Mode::Hybrid).is_err());
    // classical-only signing is for key registrations only
    assert!(tx::check(&view, &mut acct, "qorechain-local", Mode::ClassicalOnly).is_err());
    // two messages in one transaction
    let mut two = view.clone();
    two.msgs.push(two.msgs[0].clone());
    assert!(tx::check(&two, &mut acct, "qorechain-local", Mode::Hybrid).is_err());
    // trailing garbage / truncation never decodes
    assert!(tx::decode(&body[..body.len() - 1], &auth).is_err());
    assert!(tx::decode(&body, &auth[..auth.len() - 1]).is_err());
    // a second signer
    let mut auth2 = auth.clone();
    let first_signer = &auth[..auth.iter().position(|_| true).unwrap()];
    let _ = first_signer;
    auth2.extend_from_slice(&auth);
    assert!(tx::decode(&body, &auth2).is_err());
}

#[test]
fn display_amounts() {
    let c = |a: &str| tx::Coin { denom: "uqor".into(), amount: a.into() };
    assert_eq!(c("1234567").pretty(), "1.234567 QOR");
    assert_eq!(c("2000000").pretty(), "2 QOR");
    assert_eq!(c("50").pretty(), "0.00005 QOR");
    assert_eq!(tx::Coin { denom: "fusd".into(), amount: "5".into() }.pretty(), "5 fusd");
}

/// MsgRegisterPQCKeyV2 as QoreSign encodes it (account, public key, algorithm, classical key,
/// "hybrid", proof): the device must accept exactly its own registration, classical-only.
#[test]
fn registration_transaction_is_accepted_only_classical_and_only_for_its_own_key() {
    use qorebit_signer::proto;
    let v = vectors();
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    let chain = "qorechain-local";
    let pk = *acct.pq_public();
    let proof = tx::register_proof(&mut acct, chain).unwrap();
    let value = |account: &str, pk: &[u8], classical: &[u8], proof: &[u8]| {
        let mut m = Vec::new();
        proto::put_len(&mut m, 1, account.as_bytes());
        proto::put_len(&mut m, 2, pk);
        proto::put_uint(&mut m, 3, 1);
        proto::put_len(&mut m, 4, classical);
        proto::put_len(&mut m, 5, b"hybrid");
        proto::put_len(&mut m, 6, proof);
        m
    };
    let own = value(&acct.address_bech.clone(), &pk, &acct.public, &proof[..]);
    assert_eq!(sha(&own), s(&v["register"]["msgValueSha256"]));
    let body_of = |m: &[u8]| {
        let mut b = Vec::new();
        proto::put_len(&mut b, 1, &proto::any(tx::MSG_REGISTER_PQC_KEY_V2, m));
        b
    };
    let auth = hex::decode(s(&v["txs"][0]["authInfoBytes"])).unwrap();
    let view = tx::decode(&body_of(&own), &auth).unwrap();
    tx::check(&view, &mut acct, chain, Mode::ClassicalOnly).unwrap();
    assert!(tx::check(&view, &mut acct, chain, Mode::Hybrid).is_err());
    // the proof was made for another chain, another key, another account: all refused
    let other_chain = tx::register_proof(&mut acct, "qorechain-diana").unwrap();
    let mut other = w.account(1).unwrap();
    let other_pk = *other.pq_public();
    let cases = [
        value(&acct.address_bech.clone(), &pk, &acct.public, &other_chain[..]),
        value(&acct.address_bech.clone(), &other_pk, &acct.public, &proof[..]),
        value(&other.address_bech.clone(), &pk, &acct.public, &proof[..]),
        value(&acct.address_bech.clone(), &pk, &other.public, &proof[..]),
    ];
    for c in cases {
        let view = tx::decode(&body_of(&c), &auth).unwrap();
        assert!(tx::check(&view, &mut acct, chain, Mode::ClassicalOnly).is_err());
    }
    // and a classical-only signature goes out without an ML-DSA layer
    let body = body_of(&own);
    let r = Request { chain_id: chain, account_number: 7, body: &body, auth_info: &auth, version: SignBytes::V2, include_public_key: false, mode: Mode::ClassicalOnly };
    let out = tx::sign(&mut acct, &r).unwrap();
    assert!(out.pq.is_none());
}

#[test]
fn signing_speed_on_this_machine() {
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    let t = std::time::Instant::now();
    let _ = acct.pq_public();
    let kg = t.elapsed();
    let t = std::time::Instant::now();
    let _ = acct.pq_sign(&[7u8; 300]).unwrap();
    std::eprintln!("ML-DSA-87 keygen {kg:?}, sign + self-check {:?}", t.elapsed());
}

#[test]
fn an_unknown_message_is_shown_blind_whatever_its_bytes() {
    use qorebit_signer::proto;
    let v = vectors();
    let w = wallet("");
    let mut acct = w.account(0).unwrap();
    let auth = hex::decode(s(&v["txs"][0]["authInfoBytes"])).unwrap();
    let mut body = Vec::new();
    proto::put_len(&mut body, 1, &proto::any("/cosmos.bank.v1beta1.MsgMultiSend", &[1, 2, 3]));
    let view = tx::decode(&body, &auth).unwrap();
    assert!(matches!(&view.msgs[0], tx::Msg::Other { len: 3, .. }));
    tx::check(&view, &mut acct, "qorechain-local", Mode::Hybrid).unwrap();
}
