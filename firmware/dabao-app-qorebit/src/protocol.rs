//! Wire protocol between the host and the Qorebit app.
//!
//! Frame (both directions): `length (u32 BE) || body`, `body = MSG_TYPE (0x51 'Q') || ...`.
//! The length counts the body. Over USB HID the frame is cut into 64-byte reports (the last
//! one zero-padded); over the emulator's TCP socket it is sent as is.
//!
//! Request body:  `0x51 || command (u8) || payload`
//! Response body: `0x51 || status (u8) || payload`   (status 0 = ok, else an error code and a
//!                                                     UTF-8 message as payload)
//!
//! | cmd  | name           | request payload                   | response payload |
//! |------|----------------|-----------------------------------|------------------|
//! | 0x01 | GET_VERSION    | -                                 | UTF-8 version, e.g. `qorebit 0.1.0` |
//! | 0x02 | GET_ACCOUNT    | account u32                       | address (20), secp256k1 public key (33), bech32 address (UTF-8) |
//! | 0x03 | GET_PQ_PUBLIC_KEY | account u32                    | ML-DSA-87 public key (2,592) |
//! | 0x04 | REGISTER_PROOF | account u32, chain id (len u8, UTF-8) | sign_ms u32, proof of possession (4,627): the account's ML-DSA key signs its own registration message for that chain (approval + PIN on the device) |
//! | 0x05 | GET_INFO       | -                                 | UTF-8: seed origin, scheme, approval, chip security state |
//! | 0x06 | SIGN_TX        | account u32, flags u8, account number u64, chain id (len u8, UTF-8), body len u32, TxBody, AuthInfo (rest) | sign_ms u32, TxRaw len u32, TxRaw, secp256k1 signature (64), ML-DSA-87 signature (4,627, absent in classical-only mode) |
//! | 0x07 | SIGN_SIWQ      | account u32, the Sign In With QoreChain message (UTF-8, canonical form) | sign_ms u32, secp256k1 signature (64) over the ADR-36 document, ML-DSA-87 signature (4,627) over `qorechain-siwq-v1|` + message (approval + PIN on the device; only this account's sign-in, no resources, short statement) |
//! | 0x13 | SET_HOME       | account u32, balance u128 (uqor), chain id (len u8), address (len u8, bech32) | checked u8 (1 the address is this device's, 0 it is not, 2 unknown) |
//! | 0x09 | GET_SECURITY   | -                                 | version u8 (2), chip u8, developer u8, paranoid u8, require_pq u8, possible_attacks u32, seed storage u8 (0 plain, 1 wrapped with the chip key, 2 host file), pin_set u8, pin_tries_left u8, passphrase u8, firmware id (UTF-8 `git describe`) |
//! | 0x0A | SHOW_PHRASE    | -                                 | "shown": the PIN again, then the 24 words on the device screen (never sent to the host) |
//! | 0x0B | RESTORE        | -                                 | "restored": 24 words entered on the device replace the seed |
//! | 0x0C | WIPE           | -                                 | "wiped": seed and PIN erased (two confirmations), a new seed made (then SET_PIN) |
//! | 0x0D | PASSPHRASE     | -                                 | passphrase_active u8: a passphrase entered on the device selects that wallet until restart |
//! | 0x0E | SET_PIN        | -                                 | "PIN set": choose the signing PIN on the device (twice), or change it (the current PIN first) |
//! | 0x0F | RESET_PIN      | -                                 | "PIN set": forgot it: this wallet's 24 words on the device, then a new PIN |
//! | 0x12 | BIO_SELFTEST   | -                                 | UTF-8 report: the BIO display and button engines checked by two test cores (board only, not in a panel build) |
//! | 0x30 | DISPLAY_POLL   | [since_seq u32]                   | snapshot (below); the frame only if it changed since `since_seq` |
//! | 0x31 | DISPLAY_INPUT  | screen id u32, decision u8 (1 approve, 0 reject) | snapshot with the new frame (error if that screen is gone) |
//! | 0x20 | RESULT         | -                                 | the pending job's response, or PENDING again |
//!
//! SIGN_TX flags: bit 0 classical-only (only for a key registration), bit 1 carry the ML-DSA public
//! key in the PQC extension, bit 2 v1 hybrid sign bytes (chains that have not switched to v2).
//! The host sends the unsigned body and auth info; the device decodes them itself, shows what
//! they do, and signs only after the right button on the approval screen and the right PIN.
//!
//! **Signing PIN.** Every signature asks for the 8-bit PIN on the device after Approve (left = 0,
//! right = 1); NO_PIN (13) before one is set, PIN_LOCKED (14) after 5 wrong in a row.
//!
//! Snapshot: `seq u32, screen id u32, kind u8 (0 idle, 1 approval, 2 result, 3 PIN entry,
//! 4 other input: left = 0, right = 1), seconds left u8, vdisplay u8, has_frame u8,
//! [frame update]`. The frame is the device-rendered 240x280 16-colour screen (RLE rows).
//!
//! Long jobs: requests run on a worker thread. If the answer isn't ready within ~200 ms the
//! device replies status PENDING (7) with `job u8, 0 u32, elapsed_ms u32`; the host then polls
//! RESULT until it gets the real response. A new command while a job runs is refused with BUSY
//! (8) and the same progress payload. Jobs: 1 account key, 2 sign, 3 home screen, 6 boot,
//! 8 waiting for approval on the device, 11 a flow on the device (PIN, words, restore, wipe,
//! passphrase). Signing first shows an approval screen (`ui.rs`): REJECTED (9) on the left button or a
//! 60 s timeout, NO_APPROVAL_UI (10) in a build without a display.
//!
//! All integers big-endian.

pub const MSG_TYPE: u8 = 0x51;
/// Largest accepted body: a TxBody and an AuthInfo of up to 10 KiB each, plus the header.
pub const MAX_LEN: usize = 24 * 1024;

pub const CMD_GET_VERSION: u8 = 0x01;
pub const CMD_GET_ACCOUNT: u8 = 0x02;
pub const CMD_GET_PQ_PUBLIC_KEY: u8 = 0x03;
pub const CMD_REGISTER_PROOF: u8 = 0x04;
pub const CMD_GET_INFO: u8 = 0x05;
pub const CMD_SIGN_TX: u8 = 0x06;
pub const CMD_SIGN_SIWQ: u8 = 0x07;
pub const CMD_GET_SECURITY: u8 = 0x09;
pub const CMD_SHOW_PHRASE: u8 = 0x0A;
pub const CMD_RESTORE: u8 = 0x0B;
pub const CMD_WIPE: u8 = 0x0C;
pub const CMD_PASSPHRASE: u8 = 0x0D;
pub const CMD_SET_PIN: u8 = 0x0E;
pub const CMD_RESET_PIN: u8 = 0x0F;
pub const CMD_BIO_SELFTEST: u8 = 0x12;
pub const CMD_SET_HOME: u8 = 0x13;
pub const CMD_RESULT: u8 = 0x20;
pub const CMD_DISPLAY_POLL: u8 = 0x30;
pub const CMD_DISPLAY_INPUT: u8 = 0x31;

/// SIGN_TX flag bits.
pub const FLAG_CLASSICAL_ONLY: u8 = 1;
pub const FLAG_INCLUDE_PQ_KEY: u8 = 2;
pub const FLAG_SIGN_BYTES_V1: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Ok = 0,
    BadRequest = 1,
    UnknownCommand = 2,
    SigningFailed = 4,
    StoreFailed = 5,
    NoHardware = 6,
    Pending = 7,
    Busy = 8,
    Rejected = 9,
    NoApprovalUi = 10,
    /// The device read the transaction and refuses to sign it (the message says why).
    Refused = 11,
    /// No signing PIN set yet.
    NoPin = 13,
    /// 5 wrong PINs in a row: signing is locked until the PIN is reset with the words.
    PinLocked = 14,
}

pub fn frame(body: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(4 + body.len());
    f.extend_from_slice(&(body.len() as u32).to_be_bytes());
    f.extend_from_slice(body);
    f
}

pub fn response(status: Status, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(2 + payload.len());
    b.push(MSG_TYPE);
    b.push(status as u8);
    b.extend_from_slice(payload);
    frame(&b)
}

pub fn error(status: Status, msg: &str) -> Vec<u8> {
    response(status, msg.as_bytes())
}

/// Body length from a 4-byte prefix; `None` if 0 or above [`MAX_LEN`].
pub fn body_len(prefix: [u8; 4]) -> Option<usize> {
    match u32::from_be_bytes(prefix) as usize {
        0 => None,
        n if n > MAX_LEN => None,
        n => Some(n),
    }
}

/// Cut a frame into 64-byte HID reports (last one zero-padded).
#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
pub fn to_reports(frame: &[u8]) -> Vec<[u8; 64]> {
    frame
        .chunks(64)
        .map(|c| {
            let mut r = [0u8; 64];
            r[..c.len()].copy_from_slice(c);
            r
        })
        .collect()
}

/// Reassembles a frame from 64-byte HID reports.
#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
#[derive(Default)]
pub struct Reassembler {
    buf: Vec<u8>,
    need: Option<usize>,
}

#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
pub enum Assembled {
    Pending,
    Body(Vec<u8>),
    /// Bad length prefix: the transfer can't be delimited; start over with the next report.
    Invalid,
}

#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
impl Reassembler {
    pub fn push(&mut self, report: &[u8; 64]) -> Assembled {
        self.buf.extend_from_slice(report);
        if self.need.is_none() {
            match body_len([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) {
                Some(n) => self.need = Some(n),
                None => {
                    self.buf.clear();
                    return Assembled::Invalid;
                }
            }
        }
        let n = self.need.unwrap();
        if self.buf.len() < 4 + n {
            return Assembled::Pending;
        }
        let body = self.buf[4..4 + n].to_vec();
        self.buf.clear();
        self.need = None;
        Assembled::Body(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_round_trip() {
        let body: Vec<u8> = (0..6600u32).map(|i| i as u8).collect();
        let f = frame(&body);
        let mut r = Reassembler::default();
        let reports = to_reports(&f);
        assert_eq!(reports.len(), (f.len() + 63) / 64);
        let mut out = None;
        for (i, rep) in reports.iter().enumerate() {
            match r.push(rep) {
                Assembled::Pending => assert!(i + 1 < reports.len()),
                Assembled::Body(b) => out = Some(b),
                Assembled::Invalid => panic!("invalid"),
            }
        }
        assert_eq!(out.unwrap(), body);
    }

    #[test]
    fn rejects_bad_lengths() {
        assert_eq!(body_len([0, 0, 0, 0]), None);
        assert_eq!(body_len((MAX_LEN as u32 + 1).to_be_bytes()), None);
        assert_eq!(body_len(2u32.to_be_bytes()), Some(2));
    }
}
