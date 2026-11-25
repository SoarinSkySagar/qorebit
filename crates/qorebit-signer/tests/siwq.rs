//! Sign In With QoreChain against `@qorechain/qoresign` (tests/vectors.json): the same message text, the same ADR-36
//! document, the same two signatures. What the device refuses is tested too.

use qorebit_signer::siwq::{self, SiwqError};
use qorebit_signer::Wallet;
use serde_json::Value;
use sha2::{Digest, Sha256};

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors.json")).unwrap()
}
fn case(name: &str) -> Value {
    vectors()["siwq"]["cases"][name].clone()
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}
fn wallet() -> Wallet {
    Wallet::new(&[0u8; 32], "")
}

#[test]
fn messages_parse_and_render_back_to_the_same_text() {
    for name in ["full", "minimal", "request-id", "resources", "other-account"] {
        let c = case(name);
        let f = siwq::parse(s(&c["message"])).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(siwq::render(&f), s(&c["message"]), "{name}");
    }
    let f = siwq::parse(s(&case("full")["message"])).unwrap();
    assert_eq!(f.domain, "app.example.com");
    assert_eq!(f.scheme.as_deref(), Some("https"));
    assert_eq!(f.statement.as_deref(), Some("Sign in to Example"));
    assert_eq!(f.chain_id, "qorechain-diana");
    assert_eq!(f.expiration_time.as_deref(), Some("2026-11-26T10:10:00Z"));
    let m = siwq::parse(s(&case("minimal")["message"])).unwrap();
    assert_eq!((m.scheme, m.statement, m.expiration_time), (None, None, None));
    assert_eq!(m.domain, "localhost:3000");
    assert_eq!(siwq::parse(s(&case("resources")["message"])).unwrap().resources.len(), 2);
}

#[test]
fn the_documents_and_signatures_match_qoresign_byte_for_byte() {
    let w = wallet();
    let mut acct = w.account(0).unwrap();
    for name in ["full", "minimal", "request-id"] {
        let c = case(name);
        let msg = s(&c["message"]);
        assert_eq!(hex::encode(siwq::adr36_sign_bytes(&acct.address_bech, msg.as_bytes())), s(&c["adr36"]), "{name}: ADR-36 document");
        assert_eq!(hex::encode(siwq::pq_sign_bytes(msg)), s(&c["pqBytes"]), "{name}: ML-DSA bytes");
        let (classical, pq) = siwq::sign(&mut acct, msg).unwrap();
        assert_eq!(hex::encode(classical), s(&c["classicalSignature"]), "{name}: secp256k1 signature");
        assert_eq!(hex::encode(Sha256::digest(&pq[..])), s(&c["pqSignatureSha256"]), "{name}: ML-DSA signature");
    }
}

#[test]
fn only_this_accounts_sign_in_is_signed_and_no_resources() {
    let w = wallet();
    let mut acct = w.account(0).unwrap();
    assert_eq!(siwq::sign(&mut acct, s(&case("other-account")["message"])), Err(SiwqError::Refused("the sign-in is for another account")));
    assert!(matches!(siwq::sign(&mut acct, s(&case("resources")["message"])), Err(SiwqError::Refused(_))));
    let mut other = w.account(1).unwrap();
    assert!(siwq::sign(&mut other, s(&case("full")["message"])).is_err(), "account 2 does not sign account 1's sign-in");
}

#[test]
fn nothing_but_a_canonical_sign_in_message_is_signed() {
    let w = wallet();
    let mut acct = w.account(0).unwrap();
    let good = s(&case("full")["message"]).to_string();
    // text that is not a sign-in message at all (a transaction, a digest, free text)
    for junk in ["hello", "", "0xdeadbeef", "{\"msgs\":[]}"] {
        assert!(matches!(siwq::sign(&mut acct, junk), Err(SiwqError::Malformed(_))), "{junk:?}");
    }
    // the right fields in a non-canonical form: extra blank line, CRLF, trailing text, trailing newline, a changed version
    for bad in [
        good.replace("\n\n", "\n\n\n"),
        good.replace('\n', "\r\n"),
        format!("{good}\nextra"),
        format!("{good}\n"),
        good.replace("Version: 1", "Version: 2"),
        good.replace("Nonce: abc12345XYZ", "Nonce: short"),
        good.replace("Issued At: 2026-11-26T10:00:00Z", "Issued At: 2026-13-26T10:00:00Z"),
        good.replace("Issued At: 2026-11-26T10:00:00Z", "Issued At: 2026-02-30T10:00:00Z"),
        good.replace("https://app.example.com wants", "https://app.example.com/path wants"),
        good.replace("URI: https://app.example.com/login", "URI: not a uri"),
        good.replace(" QoreChain account:", " account:"),
    ] {
        assert!(matches!(siwq::sign(&mut acct, &bad), Err(SiwqError::Malformed(_))), "{bad:?}");
    }
    // too long
    let long = good.replace("Sign in to Example", &"x".repeat(3000));
    assert!(matches!(siwq::sign(&mut acct, &long), Err(SiwqError::Refused(_))));
}

#[test]
fn timestamps_follow_the_calendar() {
    for ok in ["2026-11-26T10:00:00Z", "2024-02-29T23:59:59.999+05:30", "2026-01-01T00:00:00-12:00", "2000-02-29T00:00:00Z"] {
        assert!(siwq::is_timestamp(ok), "{ok}");
    }
    for bad in ["2026-11-26", "2026-11-26 10:00:00Z", "2026-11-31T10:00:00Z", "2023-02-29T10:00:00Z", "2026-11-26T24:00:00Z", "2026-11-26T10:60:00Z", "2026-11-26T10:00:60Z", "2026-11-26T10:00:00", "2026-11-26T10:00:00+24:00", "2026-11-26T10:00:00.Z", "1900-02-29T00:00:00Z"] {
        assert!(!siwq::is_timestamp(bad), "{bad}");
    }
}
