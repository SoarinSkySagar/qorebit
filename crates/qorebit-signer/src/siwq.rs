//! Sign In With QoreChain (SIWQ), as `@qorechain/qoresign` defines it.
//!
//! A website asks the wallet to sign a text message (a domain, the account, a statement, a chain id, a nonce and
//! times). The device parses the text **strictly** (it must be in exactly the canonical form QoreSign renders: no
//! other text can be signed through this door), shows the parts that matter, and after the user approves makes
//! the two signatures of a QoreChain account:
//!
//! 1. **secp256k1** over `sha256(ADR-36 sign document)`: the Keplr-style `sign/MsgSignData` document for the
//!    account and the message, serialised with sorted keys and no whitespace (compact, low-S, RFC 6979);
//! 2. **ML-DSA-87** over `"qorechain-siwq-v1|" + message` (deterministic, empty context).
//!
//! Neither can be mistaken for a transaction signature: the first is a JSON document, the second has its own domain.

use crate::keys::{Account, KeyError, PQ_SIG_LEN};
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SIWQ_PQ_DOMAIN: &str = "qorechain-siwq-v1|";
const HEADER: &str = " wants you to sign in with your QoreChain account:";
/// Longest message the device takes.
pub const MAX_MESSAGE: usize = 2048;
/// Longest statement the device shows in full (three lines of 25 characters).
pub const MAX_STATEMENT: usize = 75;

#[derive(Debug, PartialEq, Eq)]
pub enum SiwqError {
    /// Not a valid SIWQ message (the reason is for the host).
    Malformed(&'static str),
    /// A valid message the device will not sign.
    Refused(&'static str),
    Key(KeyError),
}

impl From<KeyError> for SiwqError {
    fn from(e: KeyError) -> Self {
        SiwqError::Key(e)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fields {
    pub scheme: Option<String>,
    pub domain: String,
    pub address: String,
    pub statement: Option<String>,
    pub uri: String,
    pub chain_id: String,
    pub nonce: String,
    pub issued_at: String,
    pub expiration_time: Option<String>,
    pub not_before: Option<String>,
    pub request_id: Option<String>,
    pub resources: Vec<String>,
}

fn bad<T>(why: &'static str) -> Result<T, SiwqError> {
    Err(SiwqError::Malformed(why))
}

fn is_uri(s: &str) -> bool {
    // ^[a-zA-Z][a-zA-Z0-9+.-]*:\S+$
    let Some(colon) = s.find(':') else { return false };
    let (scheme, rest) = (&s[..colon], &s[colon + 1..]);
    let mut c = scheme.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '+' | '.' | '-'))
        && !rest.is_empty()
        && !rest.chars().any(char::is_whitespace)
}

/// `YYYY-MM-DDTHH:MM:SS[.fraction](Z|+HH:MM|-HH:MM)` with real calendar values (QoreSign's `parseSiwqTimestamp`).
pub fn is_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let num = |i: usize, n: usize| -> Option<u32> {
        (i..i + n).all(digit).then(|| s[i..i + n].parse().ok()).flatten()
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' {
        return false;
    }
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = (num(0, 4), num(5, 2), num(8, 2), num(11, 2), num(14, 2), num(17, 2)) else {
        return false;
    };
    if !(1..=12).contains(&mo) || h > 23 || mi > 59 || se > 59 {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if d < 1 || d > days[(mo - 1) as usize] {
        return false;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while digit(i) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    match b.get(i) {
        Some(b'Z') => i + 1 == b.len(),
        Some(b'+') | Some(b'-') => {
            i + 6 == b.len() && b[i + 3] == b':' && matches!((num(i + 1, 2), num(i + 4, 2)), (Some(oh), Some(om)) if oh <= 23 && om <= 59)
        }
        _ => false,
    }
}

fn validate(f: &Fields) -> Result<(), SiwqError> {
    if f.domain.is_empty() || f.domain.chars().any(|c| c.is_whitespace() || c == '/') {
        return bad("domain must be a host (and optional port) without spaces or slashes");
    }
    if let Some(s) = &f.scheme {
        let mut c = s.chars();
        if !(c.next().is_some_and(|f| f.is_ascii_alphabetic()) && c.all(|x| x.is_ascii_alphanumeric() || matches!(x, '+' | '.' | '-'))) {
            return bad("invalid scheme");
        }
    }
    if !f.address.starts_with("qor1") || f.address.len() < 10 || !f.address.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) {
        return bad("address must be the bech32 qor1 form");
    }
    if f.statement.as_ref().is_some_and(|s| s.is_empty() || s.contains('\n')) {
        return bad("statement must be a non-empty single line");
    }
    if !is_uri(&f.uri) {
        return bad("uri must be an absolute URI");
    }
    if f.chain_id.is_empty() || !f.chain_id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
        return bad("invalid chain id");
    }
    if f.nonce.len() < 8 || !f.nonce.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return bad("nonce must be at least 8 alphanumeric characters");
    }
    if !is_timestamp(&f.issued_at) {
        return bad("issuedAt must be ISO 8601");
    }
    if f.expiration_time.as_ref().is_some_and(|t| !is_timestamp(t)) {
        return bad("expirationTime must be ISO 8601");
    }
    if f.not_before.as_ref().is_some_and(|t| !is_timestamp(t)) {
        return bad("notBefore must be ISO 8601");
    }
    if f.request_id.as_ref().is_some_and(|r| r.is_empty() || r.contains('\n')) {
        return bad("requestId must be a non-empty single line");
    }
    if f.resources.iter().any(|r| !is_uri(r)) {
        return bad("resources must be URIs");
    }
    Ok(())
}

/// The canonical message text (QoreSign's `createSiwqMessage`).
pub fn render(f: &Fields) -> String {
    let mut out = String::new();
    if let Some(s) = &f.scheme {
        out.push_str(s);
        out.push_str("://");
    }
    out.push_str(&f.domain);
    out.push_str(HEADER);
    out.push('\n');
    out.push_str(&f.address);
    out.push_str("\n\n");
    if let Some(s) = &f.statement {
        out.push_str(s);
        out.push('\n');
    }
    out.push('\n');
    out.push_str("URI: ");
    out.push_str(&f.uri);
    out.push_str("\nVersion: 1\nChain ID: ");
    out.push_str(&f.chain_id);
    out.push_str("\nNonce: ");
    out.push_str(&f.nonce);
    out.push_str("\nIssued At: ");
    out.push_str(&f.issued_at);
    for (label, v) in [("Expiration Time", &f.expiration_time), ("Not Before", &f.not_before), ("Request ID", &f.request_id)] {
        if let Some(v) = v {
            out.push('\n');
            out.push_str(label);
            out.push_str(": ");
            out.push_str(v);
        }
    }
    if !f.resources.is_empty() {
        out.push_str("\nResources:");
        for r in &f.resources {
            out.push_str("\n- ");
            out.push_str(r);
        }
    }
    out
}

/// Parse a sign-in message. It must re-render to exactly the same text, so nothing else is signed.
pub fn parse(text: &str) -> Result<Fields, SiwqError> {
    if text.len() > MAX_MESSAGE {
        return Err(SiwqError::Refused("the sign-in message is too long for the device"));
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let header = lines.first().copied().unwrap_or("");
    let Some(origin) = header.strip_suffix(HEADER) else { return bad("missing SIWQ header line") };
    let (scheme, domain) = match origin.find("://") {
        Some(p) => (Some(origin[..p].to_string()), origin[p + 3..].to_string()),
        None => (None, origin.to_string()),
    };
    let address = lines.get(1).copied().unwrap_or("").to_string();
    if lines.get(2) != Some(&"") {
        return bad("expected an empty line after the address");
    }
    let mut i = 3;
    let mut statement = None;
    // the statement line, if there is one (the next line is not empty)
    if lines.get(i) != Some(&"") {
        statement = lines.get(i).map(|l| (*l).to_string());
        i += 1;
    }
    if lines.get(i) != Some(&"") {
        return bad("expected an empty line before the fields");
    }
    i += 1;
    let mut pos = i;
    let mut take = |label: &str, optional: bool| -> Result<Option<String>, SiwqError> {
        if let Some(l) = lines.get(pos) {
            if let Some(v) = l.strip_prefix(label).and_then(|r| r.strip_prefix(": ")) {
                pos += 1;
                return Ok(Some(v.to_string()));
            }
        }
        if optional { Ok(None) } else { bad("a required field is missing") }
    };
    let uri = take("URI", false)?.unwrap_or_default();
    let version = take("Version", false)?.unwrap_or_default();
    let chain_id = take("Chain ID", false)?.unwrap_or_default();
    let nonce = take("Nonce", false)?.unwrap_or_default();
    let issued_at = take("Issued At", false)?.unwrap_or_default();
    let expiration_time = take("Expiration Time", true)?;
    let not_before = take("Not Before", true)?;
    let request_id = take("Request ID", true)?;
    let mut resources = Vec::new();
    if lines.get(pos) == Some(&"Resources:") {
        pos += 1;
        while let Some(r) = lines.get(pos).and_then(|l| l.strip_prefix("- ")) {
            resources.push(r.to_string());
            pos += 1;
        }
    }
    if pos != lines.len() {
        return bad("unexpected trailing content");
    }
    if version != "1" {
        return bad("version must be 1");
    }
    let f = Fields { scheme, domain, address, statement, uri, chain_id, nonce, issued_at, expiration_time, not_before, request_id, resources };
    validate(&f)?;
    if render(&f) != text {
        return bad("message is not in canonical form");
    }
    Ok(f)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..4 {
            if k <= c.len() {
                out.push(T[((n >> (18 - 6 * k)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The ADR-36 sign document the classical key signs (as Keplr's `signArbitrary` builds it).
pub fn adr36_sign_bytes(signer: &str, data: &[u8]) -> Vec<u8> {
    let mut s = String::from(r#"{"account_number":"0","chain_id":"","fee":{"amount":[],"gas":"0"},"memo":"","msgs":[{"type":"sign/MsgSignData","value":{"data":""#);
    s.push_str(&base64(data));
    s.push_str(r#"","signer":""#);
    s.push_str(signer);
    s.push_str(r#""}}],"sequence":"0"}"#);
    s.into_bytes()
}

/// The bytes the ML-DSA-87 key signs.
pub fn pq_sign_bytes(message: &str) -> Vec<u8> {
    let mut v = SIWQ_PQ_DOMAIN.as_bytes().to_vec();
    v.extend_from_slice(message.as_bytes());
    v
}

/// What the device refuses to sign in for `acct`: the account in the message must be this one, and a sign-in
/// that lists resources (permissions the screen cannot show) is refused.
pub fn check(f: &Fields, acct: &Account) -> Result<(), SiwqError> {
    if f.address != acct.address_bech {
        return Err(SiwqError::Refused("the sign-in is for another account"));
    }
    if !f.resources.is_empty() {
        return Err(SiwqError::Refused("this sign-in lists resources the device cannot show"));
    }
    if f.statement.as_ref().is_some_and(|s| s.chars().count() > MAX_STATEMENT) {
        return Err(SiwqError::Refused("the statement is too long for the device to show in full (75 characters at most)"));
    }
    if f.domain.chars().count() > 44 {
        return Err(SiwqError::Refused("the site name is too long for the device to show in full"));
    }
    Ok(())
}

/// Both signatures of a sign-in message (checked by the key code before they are returned).
pub fn sign(acct: &mut Account, message: &str) -> Result<([u8; 64], Box<[u8; PQ_SIG_LEN]>), SiwqError> {
    let f = parse(message)?;
    check(&f, acct)?;
    // the ML-DSA signature first: the heavy one
    let pq = acct.pq_sign(&pq_sign_bytes(message))?;
    let classical = acct.classical_sign(&adr36_sign_bytes(&acct.address_bech.clone(), message.as_bytes()))?;
    Ok((classical, pq))
}
