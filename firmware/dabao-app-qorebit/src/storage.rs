//! Persistent wallet state on the Baochip-1x:
//!
//! * **Seed**: 32 bytes of BIP-39 entropy (a 24-word phrase) created on first boot from the
//!   chip TRNG (the TRNG service via `getrandom`, plus the kernel's hardware entropy pool via
//!   server IDs, mixed with SHA-256). Stored in keystore application slots `SEED_SLOT` (the
//!   entropy) and `SEED_SLOT + 1` (`MAGIC` + SHA-256 check, written last so a half-written
//!   seed is never used).
//!   **Wrapped (device):** the entropy is stored AES-KWP wrapped by the keystore service under
//!   the chip's master key (`WRAP_SLOT`, `WRAP_SLOT + 1`) and never in plain. The master key
//!   is derived inside the keystore from the chip's root seed, its anti-readout "nuisance"
//!   keys, its UUID and its lifecycle (`dev` / `sec` / `tampered`), so the blob is useless
//!   read out of the RRAM or copied to another chip, and if a secure chip is ever put into
//!   developer mode (to run firmware that could read it) the master key changes and the seed
//!   can no longer be unwrapped. A plain seed from older firmware is wrapped on the next boot
//!   and its slots are wiped.
//! * **No signature counters**: ML-DSA and ECDSA signing keep no state, so signing writes
//!   nothing. The one one-way counter (`PIN_OWC`) counts PIN attempts.
//!
//! The emulator keeps both in host files instead (not secret, not one-way).


use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAGIC: &[u8; 8] = b"QORE0001";
/// Keystore application slots of the Qorebit wallet: 320..=325 (the Quazar wallet uses 300..=305, the
/// burner 132..=133, BIO config 0..=123), so one board can hold either without touching the other.
pub const SEED_SLOT: usize = 320;
/// The one-way counter of PIN attempts (application counters are 128..=255; Quazar uses 232).
pub const OWC_BASE: usize = 233;

pub type Slot = [u8; 32];

pub trait Slots {
    fn read(&mut self, i: usize) -> Option<Slot>;
    fn write(&mut self, i: usize, data: &Slot) -> bool;
}

fn header(entropy: &[u8; 32]) -> Slot {
    let mut h = [0u8; 32];
    h[..8].copy_from_slice(MAGIC);
    h[8..].copy_from_slice(&Sha256::digest(entropy)[..24]);
    h
}

/// Where the seed came from this boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedOrigin {
    Loaded,
    Created,
    /// A wrapped seed existed but no longer unwraps (the chip's lifecycle changed): a new
    /// seed was created in its place.
    Replaced,
    /// Restored from 24 words entered on the device.
    Restored,
}

/// Slots of the wrapped seed: the first 32 bytes of the 40-byte AES-KWP blob, then its last
/// 8 bytes, `MAGIC_WRAPPED` and 16 bytes of SHA-256 of the entropy (written last).
pub const WRAP_SLOT: usize = SEED_SLOT + 2;
pub const MAGIC_WRAPPED: &[u8; 8] = b"QORE0002";
pub const WRAPPED_LEN: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "xous"), allow(dead_code))]
pub enum UnwrapError {
    /// The blob doesn't unwrap under this chip's master key (definitive).
    Integrity,
    /// The keystore couldn't be asked (transient: don't touch the stored seed).
    Unavailable,
}

/// Key wrapping under a key that never leaves the chip (the keystore's AES-KWP).
pub trait SeedWrap {
    fn wrap(&mut self, entropy: &[u8; 32]) -> Option<[u8; WRAPPED_LEN]>;
    fn unwrap(&mut self, blob: &[u8; WRAPPED_LEN]) -> Result<Zeroizing<[u8; 32]>, UnwrapError>;
}

fn check16(entropy: &[u8; 32]) -> [u8; 16] {
    let mut h = Sha256::new();
    h.update(b"qorebit/seed-check/v1");
    h.update(entropy);
    h.finalize()[..16].try_into().unwrap()
}

/// Load the stored entropy, or create, store and return a fresh one.
///
/// With `wrap` (the device) the seed is kept wrapped: a wrapped seed is loaded, a plain seed
/// from older firmware is wrapped and its plain slots wiped, a new seed is stored wrapped
/// only. Returns `None` if the seed can't be stored or the keystore can't be reached (then
/// nothing stored is changed).
pub fn load_or_create_entropy(
    slots: &mut impl Slots,
    wrap: Option<&mut dyn SeedWrap>,
) -> Option<(Zeroizing<[u8; 32]>, SeedOrigin, crate::security::SeedProtection)> {
    use crate::security::SeedProtection;
    let Some(w) = wrap else {
        let plain = if cfg!(target_os = "xous") { SeedProtection::Raw } else { SeedProtection::HostFile };
        return load_or_create_plain(slots).map(|(e, o)| (e, o, plain));
    };
    let mut origin = SeedOrigin::Created;
    if let Some(tail) = slots.read(WRAP_SLOT + 1).filter(|t| &t[8..16] == MAGIC_WRAPPED) {
        let blob = wrapped_blob(&slots.read(WRAP_SLOT)?, &tail);
        match w.unwrap(&blob) {
            Ok(e) if check16(&e)[..] == tail[16..] => {
                wipe_plain(slots);
                return Some((e, SeedOrigin::Loaded, SeedProtection::Wrapped));
            }
            Ok(_) | Err(UnwrapError::Integrity) => {
                log::error!("the wrapped seed doesn't unwrap on this chip (its lifecycle changed?): creating a new seed");
                origin = SeedOrigin::Replaced;
            }
            Err(UnwrapError::Unavailable) => {
                log::error!("keystore unavailable: not touching the stored seed");
                return None;
            }
        }
    } else if let Some(e) = read_plain(slots) {
        // older firmware kept the seed in plain: wrap it, then wipe the plain copy
        if store_wrapped(slots, w, &e) {
            wipe_plain(slots);
            log::info!("seed moved from plain keystore slots to a chip-wrapped blob");
            return Some((e, SeedOrigin::Loaded, SeedProtection::Wrapped));
        }
        log::error!("could not wrap the seed: keeping it in plain");
        return Some((e, SeedOrigin::Loaded, SeedProtection::Raw));
    }
    let e = fresh_entropy();
    if !store_wrapped(slots, w, &e) {
        log::error!("could not store the new seed");
        return None;
    }
    wipe_plain(slots);
    Some((e, origin, SeedProtection::Wrapped))
}

fn wrapped_blob(head: &Slot, tail: &Slot) -> [u8; WRAPPED_LEN] {
    let mut blob = [0u8; WRAPPED_LEN];
    blob[..32].copy_from_slice(head);
    blob[32..].copy_from_slice(&tail[..8]);
    blob
}

/// Wrap, store (marker last), read back and unwrap again: true only if the stored blob gives
/// back `e`.
fn store_wrapped(slots: &mut impl Slots, w: &mut dyn SeedWrap, e: &[u8; 32]) -> bool {
    let Some(blob) = w.wrap(e) else { return false };
    let mut tail = [0u8; 32];
    tail[..8].copy_from_slice(&blob[32..]);
    tail[8..16].copy_from_slice(MAGIC_WRAPPED);
    tail[16..].copy_from_slice(&check16(e));
    if !slots.write(WRAP_SLOT, blob[..32].try_into().unwrap()) || !slots.write(WRAP_SLOT + 1, &tail) {
        return false;
    }
    match (slots.read(WRAP_SLOT), slots.read(WRAP_SLOT + 1)) {
        (Some(h), Some(t)) if t == tail => matches!(w.unwrap(&wrapped_blob(&h, &t)), Ok(back) if back[..] == e[..]),
        _ => false,
    }
}

fn read_plain(slots: &mut impl Slots) -> Option<Zeroizing<[u8; 32]>> {
    let h = slots.read(SEED_SLOT + 1)?;
    if &h[..8] != MAGIC {
        return None;
    }
    let e = Zeroizing::new(slots.read(SEED_SLOT)?);
    if header(&e) == h {
        Some(e)
    } else {
        log::warn!("seed header does not match the stored entropy");
        None
    }
}

/// Zero the plain seed slots (if anything is in them).
fn wipe_plain(slots: &mut impl Slots) {
    for i in [SEED_SLOT, SEED_SLOT + 1] {
        if slots.read(i).is_some_and(|d| d.iter().any(|&b| b != 0)) && !slots.write(i, &[0; 32]) {
            log::error!("could not wipe keystore slot {i}");
        }
    }
}

fn load_or_create_plain(slots: &mut impl Slots) -> Option<(Zeroizing<[u8; 32]>, SeedOrigin)> {
    if let Some(e) = read_plain(slots) {
        return Some((e, SeedOrigin::Loaded));
    }
    let e = fresh_entropy();
    if !slots.write(SEED_SLOT, &e) || !slots.write(SEED_SLOT + 1, &header(&e)) {
        log::error!("could not store the new seed");
        return None;
    }
    // read back: only use what is really stored
    let back = Zeroizing::new(slots.read(SEED_SLOT)?);
    (back[..] == e[..] && slots.read(SEED_SLOT + 1)? == header(&e)).then_some((e, SeedOrigin::Created))
}

/// 32 bytes from the chip TRNG (TRNG service + kernel hardware entropy), mixed with SHA-256.
pub fn fresh_entropy() -> Zeroizing<[u8; 32]> {
    let mut pool = Zeroizing::new([0u8; 64]);
    getrandom::getrandom(&mut pool[..32]).expect("TRNG");
    // unit tests run without a Xous kernel to ask
    #[cfg(not(test))]
    for chunk in pool[32..].chunks_mut(16) {
        let id = xous::create_server_id().expect("kernel entropy").to_array();
        for (j, w) in id.iter().enumerate() {
            chunk[4 * j..4 * j + 4].copy_from_slice(&w.to_le_bytes());
        }
    }
    let mut h = Sha256::new();
    h.update(b"qorebit/seed/v1");
    h.update(&pool[..]);
    Zeroizing::new(h.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CounterError {
    /// No such counter (the emulator's host file).
    #[cfg_attr(target_os = "xous", allow(dead_code))]
    NoCounter,
    /// The keystore / counter hardware failed (e.g. a worn-out counter line).
    Hardware,
}

/// One-way counters: can only go up (hardware on the device, a host file in the emulator).
pub trait Owc {
    fn owc_get(&mut self, index: usize) -> Result<u32, CounterError>;
    fn owc_inc(&mut self, index: usize) -> Result<(), CounterError>;
}

// ---------------------------------------------------------------- PIN

/// The PIN: 8 binary digits, one per button press (left = 0, right = 1).
pub const PIN_BITS: u32 = 8;
/// Wrong PINs in a row before signing locks (until the PIN is reset with the 24 recovery
/// words). 256 PINs, 5 tries: a thief who guesses has a 5 in 256 (~2%) chance.
pub const PIN_TRIES: u32 = 5;
/// Slot of the PIN record (`PIN_MAGIC`, salt, hash) and of the counter snapshot taken at the
/// last right PIN.
pub const PIN_SLOT: usize = SEED_SLOT + 4;
pub const PIN_MAGIC: &[u8; 8] = b"QRPIN001";
/// One-way counter of PIN attempts. Every attempt advances
/// it *before* the PIN is checked, so cutting the power can't hide a wrong guess; the
/// attempts used are the counter minus its value at the last right PIN.
pub const PIN_OWC: usize = OWC_BASE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinCheck {
    Right,
    /// Wrong; this many tries are left before signing locks.
    Wrong(u32),
    /// No tries left: signing is locked until the PIN is reset with the recovery words.
    Locked,
    NotSet,
    /// The counter or the storage failed: nothing was decided.
    Failed,
}

fn pin_hash(salt: &[u8], pin: u8) -> [u8; 16] {
    let mut h = Sha256::new();
    h.update(b"qorebit/pin/v1");
    h.update(salt);
    h.update([pin]);
    h.finalize()[..16].try_into().unwrap()
}

pub fn pin_is_set(slots: &mut impl Slots) -> bool {
    slots.read(PIN_SLOT).is_some_and(|r| &r[..8] == PIN_MAGIC)
}

/// Tries left before signing locks (PIN_TRIES when the PIN isn't set or can't be read).
pub fn pin_tries_left<S: Slots + Owc>(s: &mut S) -> u32 {
    let (Some(snap), Ok(now)) = (s.read(PIN_SLOT + 1), s.owc_get(PIN_OWC)) else { return PIN_TRIES };
    let used = now.saturating_sub(u32::from_le_bytes(snap[..4].try_into().unwrap()));
    PIN_TRIES.saturating_sub(used)
}

/// Store a new PIN (and reset the tries).
pub fn set_pin<S: Slots + Owc>(s: &mut S, pin: u8) -> bool {
    let mut salt = [0u8; 8];
    getrandom::getrandom(&mut salt).expect("TRNG");
    let mut rec = [0u8; 32];
    rec[..8].copy_from_slice(PIN_MAGIC);
    rec[8..16].copy_from_slice(&salt);
    rec[16..].copy_from_slice(&pin_hash(&salt, pin));
    let Ok(now) = s.owc_get(PIN_OWC) else { return false };
    let mut snap = [0u8; 32];
    snap[..4].copy_from_slice(&now.to_le_bytes());
    s.write(PIN_SLOT + 1, &snap) && s.write(PIN_SLOT, &rec) && s.read(PIN_SLOT) == Some(rec)
}

/// Check a PIN. The attempt is counted first; a right PIN resets the count; with no tries
/// left the PIN isn't even compared (signing stays locked until a reset with the words).
pub fn check_pin<S: Slots + Owc>(s: &mut S, pin: u8) -> PinCheck {
    let Some(rec) = s.read(PIN_SLOT).filter(|r| &r[..8] == PIN_MAGIC) else { return PinCheck::NotSet };
    if pin_tries_left(s) == 0 {
        return PinCheck::Locked;
    }
    if s.owc_inc(PIN_OWC).is_err() {
        return PinCheck::Failed;
    }
    let Ok(now) = s.owc_get(PIN_OWC) else { return PinCheck::Failed };
    if pin_hash(&rec[8..16], pin)[..] == rec[16..] {
        let mut snap = [0u8; 32];
        snap[..4].copy_from_slice(&now.to_le_bytes());
        return if s.write(PIN_SLOT + 1, &snap) { PinCheck::Right } else { PinCheck::Failed };
    }
    match pin_tries_left(s) {
        0 => PinCheck::Locked,
        n => PinCheck::Wrong(n),
    }
}

/// Erase the seed (plain and wrapped) and the PIN. The signature counters stay: they only go
/// up, so keys of a restored seed are never reused below their old counts.
pub fn wipe_wallet(slots: &mut impl Slots) {
    for i in SEED_SLOT..PIN_SLOT + 2 {
        if !slots.write(i, &[0; 32]) {
            log::error!("could not erase keystore slot {i}");
        }
    }
}

/// Replace the seed with `entropy` (a restored phrase, or a fresh seed after a wipe), stored
/// the same way as a new seed.
pub fn replace_entropy(
    slots: &mut impl Slots,
    wrap: Option<&mut dyn SeedWrap>,
    entropy: &[u8; 32],
) -> Option<crate::security::SeedProtection> {
    use crate::security::SeedProtection;
    match wrap {
        Some(w) => {
            if !store_wrapped(slots, w, entropy) {
                return None;
            }
            wipe_plain(slots);
            Some(SeedProtection::Wrapped)
        }
        None => {
            let ok = slots.write(SEED_SLOT, entropy) && slots.write(SEED_SLOT + 1, &header(entropy));
            ok.then(|| if cfg!(target_os = "xous") { SeedProtection::Raw } else { SeedProtection::HostFile })
        }
    }
}

// ---------------------------------------------------------------- device

#[cfg(target_os = "xous")]
pub struct Keystore(xous::CID);

#[cfg(target_os = "xous")]
impl Keystore {
    pub fn open() -> Self {
        let xns = xous_names::XousNames::new().unwrap();
        Keystore(xns.request_connection_blocking(keystore_api::SERVER_NAME_KEYS).expect("keystore"))
    }

    fn app_key(&self, op: keystore_api::AppKey) -> keystore_api::AppKey {
        use keystore_api::AppKey;
        let Ok(mut buf) = xous_ipc::Buffer::into_buf(op) else { return AppKey::InternalError };
        if buf.lend_mut(self.0, keystore_api::Opcode::AppKeyOp as u32).is_err() {
            return AppKey::InternalError;
        }
        buf.to_original::<AppKey, _>().unwrap_or(AppKey::InternalError)
    }

    /// (status, value); status 4 = OneWayErr::None.
    fn owc(&self, opcode: keystore_api::Opcode, index: usize, magic: [usize; 3]) -> Result<u32, CounterError> {
        match xous::send_message(
            self.0,
            xous::Message::new_blocking_scalar(opcode as usize, index, magic[0], magic[1], magic[2]),
        ) {
            Ok(xous::Result::Scalar5(_, 4, value, _, _)) => Ok(value as u32),
            Ok(xous::Result::Scalar5(_, e, _, _, _)) => {
                log::error!("one-way counter {index}: error {e}");
                Err(CounterError::Hardware)
            }
            other => {
                log::error!("one-way counter {index}: {other:?}");
                Err(CounterError::Hardware)
            }
        }
    }

    pub fn owc_get(&self, index: usize) -> Result<u32, CounterError> {
        self.owc(keystore_api::Opcode::GetOneWayCounter, index, keystore_api::OWC_MAGIC_GET)
    }

    pub fn owc_inc(&self, index: usize) -> Result<(), CounterError> {
        self.owc(keystore_api::Opcode::IncOneWayCounter, index, keystore_api::OWC_MAGIC_INC).map(|_| ())
    }

    /// The keystore's view of developer mode (its reply is 0 for a developer chip).
    pub fn is_developer(&self) -> Option<bool> {
        match xous::send_message(
            self.0,
            xous::Message::new_blocking_scalar(keystore_api::Opcode::IsDeveloper as usize, 0, 0, 0, 0),
        ) {
            Ok(xous::Result::Scalar5(_, v, _, _, _)) => Some(v == 0),
            other => {
                log::error!("keystore IsDeveloper: {other:?}");
                None
            }
        }
    }
}

#[cfg(target_os = "xous")]
impl Owc for Keystore {
    fn owc_get(&mut self, index: usize) -> Result<u32, CounterError> {
        Keystore::owc_get(self, index)
    }
    fn owc_inc(&mut self, index: usize) -> Result<(), CounterError> {
        Keystore::owc_inc(self, index)
    }
}

#[cfg(target_os = "xous")]
impl Slots for Keystore {
    fn read(&mut self, i: usize) -> Option<Slot> {
        use keystore_api::{AppKey, APPKEY_GUARD};
        match self.app_key(AppKey::ReadRequest { guard: APPKEY_GUARD, index: i }) {
            AppKey::ReadResponse { data } => Some(data),
            e => {
                log::error!("keystore read {i}: {e:?}");
                None
            }
        }
    }

    fn write(&mut self, i: usize, data: &Slot) -> bool {
        use keystore_api::{AppKey, APPKEY_GUARD};
        match self.app_key(AppKey::Write { guard: APPKEY_GUARD, index: i, data: *data }) {
            AppKey::Success => true,
            e @ (AppKey::AccessDenied | AppKey::WriteError | AppKey::InternalError) => {
                log::error!("keystore write {i}: {e:?}");
                false
            }
            _ => false,
        }
    }
}

#[cfg(target_os = "xous")]
impl Keystore {
    fn kwp(&self, op: keystore_api::KeyWrapOp, input: &[u8], expected_len: usize) -> Result<Vec<u8>, UnwrapError> {
        use keystore_api::{KeyWrapper, KeywrapError, MAX_WRAP_DATA};
        let mut kw = KeyWrapper {
            data: [0u8; MAX_WRAP_DATA + 8],
            len: input.len() as u32,
            domain: "qorebit.seed".into(),
            op,
            // overwritten by the keystore: an unanswered request reads as a failure
            result: Some(KeywrapError::IntegrityCheckFailed),
            expected_len: expected_len as u32,
        };
        kw.data[..input.len()].copy_from_slice(input);
        let mut buf = xous_ipc::Buffer::into_buf(kw).map_err(|_| UnwrapError::Unavailable)?;
        buf.lend_mut(self.0, keystore_api::Opcode::AesKwp as u32).map_err(|_| UnwrapError::Unavailable)?;
        let mut ret = buf.to_original::<KeyWrapper, _>().map_err(|_| UnwrapError::Unavailable)?;
        let out = match ret.result {
            None => Ok(ret.data[..ret.len as usize].to_vec()),
            Some(KeywrapError::IntegrityCheckFailed) => Err(UnwrapError::Integrity),
            Some(e) => {
                log::error!("keystore key wrap: {e:?}");
                Err(UnwrapError::Integrity)
            }
        };
        zeroize::Zeroize::zeroize(&mut ret.data[..]);
        out
    }
}

#[cfg(target_os = "xous")]
impl SeedWrap for Keystore {
    fn wrap(&mut self, entropy: &[u8; 32]) -> Option<[u8; WRAPPED_LEN]> {
        let w = self.kwp(keystore_api::KeyWrapOp::Wrap, entropy, 0).ok()?;
        w.as_slice().try_into().ok()
    }
    fn unwrap(&mut self, blob: &[u8; WRAPPED_LEN]) -> Result<Zeroizing<[u8; 32]>, UnwrapError> {
        let mut v = self.kwp(keystore_api::KeyWrapOp::Unwrap, blob, 32)?;
        // KWP pads to 8 bytes: 32 bytes come back as 32
        let out = v.get(..32).map(|s| Zeroizing::new(s.try_into().unwrap())).ok_or(UnwrapError::Integrity);
        zeroize::Zeroize::zeroize(&mut v[..]);
        out
    }
}

// ---------------------------------------------------------------- emulator

/// Emulator: seed slots and counters in a host file (`QOREBIT_STATE` or
/// `target/qorebit-state.bin`). Not secret, not one-way.
#[cfg(not(target_os = "xous"))]
pub struct HostFile(std::path::PathBuf);

/// Slots SEED_SLOT.. (plain seed, wrapped seed, PIN, then 8 reserved), then the PIN counter.
#[cfg(not(target_os = "xous"))]
const HOST_SLOTS: usize = 6 + 8;
#[cfg(not(target_os = "xous"))]
const HOST_COUNTERS: usize = 1;
#[cfg(not(target_os = "xous"))]
const HOST_LEN: usize = 32 * HOST_SLOTS + 4 * HOST_COUNTERS;

#[cfg(not(target_os = "xous"))]
impl HostFile {
    pub fn open() -> Self {
        let path = std::env::var_os("QOREBIT_STATE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/qorebit-state.bin"));
        HostFile(path)
    }
    fn data(&self) -> Vec<u8> {
        match std::fs::read(&self.0) {
            Ok(d) if d.len() == HOST_LEN => d,
            _ => vec![0; HOST_LEN],
        }
    }
    fn save(&self, d: &[u8]) -> bool {
        if let Some(dir) = self.0.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&self.0, d).is_ok()
    }
    fn counter_offset(index: usize) -> Option<usize> {
        let k = index.checked_sub(OWC_BASE).filter(|k| *k < HOST_COUNTERS)?;
        Some(32 * HOST_SLOTS + 4 * k)
    }
}

#[cfg(not(target_os = "xous"))]
impl Slots for HostFile {
    fn read(&mut self, i: usize) -> Option<Slot> {
        let k = i.checked_sub(SEED_SLOT).filter(|k| *k < HOST_SLOTS)?;
        self.data()[32 * k..32 * k + 32].try_into().ok()
    }
    fn write(&mut self, i: usize, data: &Slot) -> bool {
        let Some(k) = i.checked_sub(SEED_SLOT).filter(|k| *k < HOST_SLOTS) else { return false };
        let mut d = self.data();
        d[32 * k..32 * k + 32].copy_from_slice(data);
        self.save(&d)
    }
}

#[cfg(not(target_os = "xous"))]
impl Owc for HostFile {
    fn owc_get(&mut self, index: usize) -> Result<u32, CounterError> {
        let o = Self::counter_offset(index).ok_or(CounterError::NoCounter)?;
        Ok(u32::from_le_bytes(self.data()[o..o + 4].try_into().unwrap()))
    }
    fn owc_inc(&mut self, index: usize) -> Result<(), CounterError> {
        let o = Self::counter_offset(index).ok_or(CounterError::NoCounter)?;
        let mut d = self.data();
        let v = u32::from_le_bytes(d[o..o + 4].try_into().unwrap()) + 1;
        d[o..o + 4].copy_from_slice(&v.to_le_bytes());
        if self.save(&d) { Ok(()) } else { Err(CounterError::Hardware) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::SeedProtection;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Mem(HashMap<usize, Slot>);
    impl Slots for Mem {
        fn read(&mut self, i: usize) -> Option<Slot> {
            Some(self.0.get(&i).copied().unwrap_or([0; 32]))
        }
        fn write(&mut self, i: usize, data: &Slot) -> bool {
            self.0.insert(i, *data);
            true
        }
    }

    /// Stand-in for the keystore: XOR with a chip key, plus an 8-byte tag.
    struct Chip {
        key: u8,
        up: bool,
    }
    impl SeedWrap for Chip {
        fn wrap(&mut self, e: &[u8; 32]) -> Option<[u8; WRAPPED_LEN]> {
            let mut b = [self.key; WRAPPED_LEN];
            for (o, x) in b.iter_mut().zip(e) {
                *o ^= x;
            }
            Some(b)
        }
        fn unwrap(&mut self, b: &[u8; WRAPPED_LEN]) -> Result<Zeroizing<[u8; 32]>, UnwrapError> {
            if !self.up {
                return Err(UnwrapError::Unavailable);
            }
            if b[32..].iter().any(|&t| t != self.key) {
                return Err(UnwrapError::Integrity);
            }
            let mut e = [0u8; 32];
            for (o, x) in e.iter_mut().zip(b) {
                *o = x ^ self.key;
            }
            Ok(Zeroizing::new(e))
        }
    }

    fn load(m: &mut Mem, c: &mut Chip) -> Option<([u8; 32], SeedOrigin, SeedProtection)> {
        load_or_create_entropy(m, Some(c as &mut dyn SeedWrap)).map(|(e, o, p)| (*e, o, p))
    }

    fn plain_is_empty(m: &mut Mem) -> bool {
        m.read(SEED_SLOT).unwrap() == [0; 32] && m.read(SEED_SLOT + 1).unwrap() == [0; 32]
    }

    #[test]
    fn new_seed_is_stored_wrapped_only() {
        let (mut m, mut c) = (Mem::default(), Chip { key: 0x5a, up: true });
        let (e, o, p) = load(&mut m, &mut c).unwrap();
        assert_eq!((o, p), (SeedOrigin::Created, SeedProtection::Wrapped));
        assert!(plain_is_empty(&mut m));
        assert_ne!(m.read(WRAP_SLOT).unwrap(), e, "never stored in plain");
        assert_eq!(load(&mut m, &mut c).unwrap(), (e, SeedOrigin::Loaded, SeedProtection::Wrapped));
    }

    #[test]
    fn plain_seed_is_migrated_and_wiped() {
        let mut m = Mem::default();
        let (old, o) = load_or_create_plain(&mut m).unwrap();
        assert_eq!(o, SeedOrigin::Created);
        let mut c = Chip { key: 0x33, up: true };
        let (e, o, p) = load(&mut m, &mut c).unwrap();
        assert_eq!((e, o, p), (*old, SeedOrigin::Loaded, SeedProtection::Wrapped), "same wallet after the move");
        assert!(plain_is_empty(&mut m));
        assert_eq!(load(&mut m, &mut c).unwrap().0, *old);
    }

    #[test]
    fn interrupted_migration_finishes_the_wipe() {
        let mut m = Mem::default();
        let (old, _) = load_or_create_plain(&mut m).unwrap();
        let mut c = Chip { key: 0x33, up: true };
        assert!(store_wrapped(&mut m, &mut c, &old)); // power lost before the wipe
        assert_eq!(load(&mut m, &mut c).unwrap().0, *old);
        assert!(plain_is_empty(&mut m));
    }

    #[test]
    fn a_lifecycle_change_makes_the_seed_unrecoverable() {
        let (mut m, mut c) = (Mem::default(), Chip { key: 1, up: true });
        let (e, ..) = load(&mut m, &mut c).unwrap();
        c.key = 2; // e.g. the chip entered developer mode: a different master key
        let (e2, o, _) = load(&mut m, &mut c).unwrap();
        assert_eq!(o, SeedOrigin::Replaced);
        assert_ne!(e, e2);
    }

    #[derive(Default)]
    struct MemOwc(Mem, HashMap<usize, u32>);
    impl Slots for MemOwc {
        fn read(&mut self, i: usize) -> Option<Slot> {
            self.0.read(i)
        }
        fn write(&mut self, i: usize, d: &Slot) -> bool {
            self.0.write(i, d)
        }
    }
    impl Owc for MemOwc {
        fn owc_get(&mut self, i: usize) -> Result<u32, CounterError> {
            Ok(*self.1.get(&i).unwrap_or(&0))
        }
        fn owc_inc(&mut self, i: usize) -> Result<(), CounterError> {
            *self.1.entry(i).or_default() += 1;
            Ok(())
        }
    }

    #[test]
    fn pin_right_wrong_and_reset() {
        let mut s = MemOwc::default();
        assert_eq!(check_pin(&mut s, 0b1010_1010), PinCheck::NotSet);
        assert!(set_pin(&mut s, 0b1010_1010));
        assert_eq!(check_pin(&mut s, 0b1010_1011), PinCheck::Wrong(4));
        assert_eq!(check_pin(&mut s, 0), PinCheck::Wrong(3));
        assert_eq!(check_pin(&mut s, 0b1010_1010), PinCheck::Right);
        assert_eq!(pin_tries_left(&mut s), PIN_TRIES, "a right PIN resets the count");
    }

    #[test]
    fn five_wrong_pins_lock_signing_until_a_reset() {
        let mut s = MemOwc::default();
        let mut c = Chip { key: 9, up: true };
        let (e, ..) = load_or_create_entropy(&mut s, Some(&mut c as &mut dyn SeedWrap)).unwrap();
        assert!(set_pin(&mut s, 7));
        for left in (1..PIN_TRIES).rev() {
            assert_eq!(check_pin(&mut s, 8), PinCheck::Wrong(left));
        }
        assert_eq!(check_pin(&mut s, 8), PinCheck::Locked);
        // locked: even the right PIN is refused, and isn't counted
        assert_eq!(check_pin(&mut s, 7), PinCheck::Locked);
        assert_eq!(s.owc_get(PIN_OWC).unwrap(), PIN_TRIES);
        // the wallet is still there
        assert_eq!(*load_or_create_entropy(&mut s, Some(&mut c as &mut dyn SeedWrap)).unwrap().0, *e);
        // a new PIN (after the recovery words) unlocks with all tries
        assert!(set_pin(&mut s, 42));
        assert_eq!(pin_tries_left(&mut s), PIN_TRIES);
        assert_eq!(check_pin(&mut s, 42), PinCheck::Right);
    }

    #[test]
    fn an_unreachable_keystore_changes_nothing() {
        let (mut m, mut c) = (Mem::default(), Chip { key: 7, up: true });
        load(&mut m, &mut c).unwrap();
        let before = m.0.clone();
        c.up = false;
        assert!(load(&mut m, &mut c).is_none());
        assert_eq!(m.0, before);
    }
}
