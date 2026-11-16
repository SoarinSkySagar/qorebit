//! The chip's own security state, read at boot and shown on the device and in the companion.
//!
//! The Baochip-1x keeps its lifecycle in one-way counters that only boot0 / boot1 can advance:
//!
//! * `DEVELOPER_MODE` (85): non-zero once the chip ran an image signed with the public
//!   developer key. The chip erased its secret-key area on that first boot, and from then on
//!   any developer-signed firmware can run, so anything this app stores (the seed) can be read
//!   by someone who reflashes the board. Qorebit images are developer-signed today (only
//!   Baochip holds the other boot0 keys), so a Dabao running Qorebit is always in this mode.
//! * `PARANOID_MODE` (65, duplicate 67): glitch detectors on a hair trigger.
//! * `POSSIBLE_ATTACKS` (66): glitch / tamper alarms counted by the hardening code; past a
//!   limit the chip wipes itself.
//! * `REQUIRE_PQ` (19, duplicate 44): every boot stage must carry a valid SLH-DSA signature
//!   as well as ed25519.
//!
//! The keystore service answers `IsDeveloper` from the same counter; both are read so a
//! disagreement shows up. The emulator has none of this and says so.

/// Where the seed is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedProtection {
    /// Plain entropy in keystore app slots.
    Raw,
    /// AES-KWP wrapped under the chip's master key (see `storage.rs`): useless off this chip,
    /// and lost if the chip's lifecycle changes (e.g. it enters developer mode).
    Wrapped,
    /// The emulator's host file.
    HostFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    /// Running on a Baochip (false: the emulator).
    pub chip: bool,
    /// The chip has booted developer-signed code (its secrets are not protected).
    pub developer: bool,
    pub paranoid: bool,
    pub require_pq: bool,
    pub attacks: u32,
    pub seed: SeedProtection,
    /// A PIN is set (it's asked on every start).
    pub pin_set: bool,
    /// Wrong PINs left before signing locks.
    pub pin_tries: u32,
    /// The wallet in use comes from the 24 words plus a passphrase.
    pub passphrase: bool,
}

/// The source revision this firmware was built from (`git describe`, set by build.sh).
pub const FIRMWARE_ID: &str = match option_env!("QOREBIT_GIT") {
    Some(s) => s,
    None => "unknown",
};

/// How the device describes its own trust level, worst first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trust {
    /// Glitch / tamper alarms were recorded.
    Attacked(u32),
    Emulator,
    /// Developer-signed firmware: the seed can be read by reflashing.
    Developer,
    Secure,
}

impl State {
    pub fn trust(&self) -> Trust {
        if self.attacks > 0 {
            Trust::Attacked(self.attacks)
        } else if !self.chip {
            Trust::Emulator
        } else if self.developer {
            Trust::Developer
        } else {
            Trust::Secure
        }
    }

    /// Wire format of CMD_GET_SECURITY: version 2, then chip u8, developer u8, paranoid u8,
    /// require_pq u8, attacks u32, seed u8, pin_set u8, pin_tries u8, passphrase u8, firmware id
    /// (UTF-8). (Version 1 ended with the seed byte and the firmware id.)
    pub fn encode(&self) -> Vec<u8> {
        let mut p = vec![2, self.chip as u8, self.developer as u8, self.paranoid as u8, self.require_pq as u8];
        p.extend_from_slice(&self.attacks.to_be_bytes());
        p.push(match self.seed {
            SeedProtection::Raw => 0,
            SeedProtection::Wrapped => 1,
            SeedProtection::HostFile => 2,
        });
        p.extend_from_slice(&[self.pin_set as u8, self.pin_tries.min(255) as u8, self.passphrase as u8]);
        p.extend_from_slice(FIRMWARE_ID.as_bytes());
        p
    }

    /// Lines for the `info` text.
    pub fn describe(&self) -> String {
        let pin = if self.pin_set { format!("set, {} wrong tries left before signing locks", self.pin_tries) } else { "not set".into() };
        let wallet = if self.passphrase { "passphrase wallet" } else { "standard (no passphrase)" };
        if !self.chip {
            return format!("security: emulator (no chip: nothing here is secret or one-way)\nPIN: {pin}\nwallet: {wallet}");
        }
        format!(
            "security: {}\nparanoid mode: {}\nPQ boot signatures required: {}\npossible attacks recorded: {}\nseed storage: {}\nPIN: {pin}\nwallet: {wallet}\nfirmware: {FIRMWARE_ID}",
            if self.developer {
                "DEVELOPER MODE (developer-signed firmware can run and read this device's storage)"
            } else {
                "secure boot (only Baochip-signed firmware runs)"
            },
            on(self.paranoid),
            on(self.require_pq),
            self.attacks,
            match self.seed {
                SeedProtection::Raw => "plain in keystore slots",
                SeedProtection::Wrapped => "wrapped with the chip's master key (AES-KWP)",
                SeedProtection::HostFile => "host file",
            }
        )
    }
}

fn on(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

/// Read the counters (device).
#[cfg(target_os = "xous")]
pub fn read(ks: &crate::storage::Keystore, seed: SeedProtection) -> State {
    use bao1x_api::{
        DEVELOPER_MODE, PARANOID_MODE, PARANOID_MODE_DUPE, POSSIBLE_ATTACKS, REQUIRE_PQ, REQUIRE_PQ_DUPE,
    };
    // an unreadable counter counts as the unsafe value
    let get = |i: usize, unsafe_value: u32| ks.owc_get(i).unwrap_or(unsafe_value);
    let counter_dev = get(DEVELOPER_MODE, 1) != 0;
    let keystore_dev = ks.is_developer().unwrap_or(true);
    if counter_dev != keystore_dev {
        log::warn!("developer mode: counter says {counter_dev}, keystore says {keystore_dev}");
    }
    State {
        chip: true,
        developer: counter_dev || keystore_dev,
        paranoid: get(PARANOID_MODE, 0) != 0 || get(PARANOID_MODE_DUPE, 0) != 0,
        require_pq: get(REQUIRE_PQ, 0) != 0 || get(REQUIRE_PQ_DUPE, 0) != 0,
        attacks: get(POSSIBLE_ATTACKS, 0),
        seed,
        pin_set: false,
        pin_tries: 0,
        passphrase: false,
    }
}

/// The emulator: no chip state.
#[cfg(not(target_os = "xous"))]
pub fn read(seed: SeedProtection) -> State {
    State {
        chip: false,
        developer: false,
        paranoid: false,
        require_pq: false,
        attacks: 0,
        seed,
        pin_set: false,
        pin_tries: 0,
        passphrase: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chip() -> State {
        State {
            chip: true,
            developer: false,
            paranoid: true,
            require_pq: true,
            attacks: 0,
            seed: SeedProtection::Wrapped,
            pin_set: true,
            pin_tries: 5,
            passphrase: false,
        }
    }

    #[test]
    fn trust_order() {
        assert_eq!(chip().trust(), Trust::Secure);
        assert_eq!(State { developer: true, ..chip() }.trust(), Trust::Developer);
        assert_eq!(State { chip: false, ..chip() }.trust(), Trust::Emulator);
        // alarms win over everything
        assert_eq!(State { developer: true, attacks: 2, ..chip() }.trust(), Trust::Attacked(2));
    }

    #[test]
    fn wire_format() {
        let p = State { developer: true, attacks: 3, ..chip() }.encode();
        assert_eq!(&p[..9], &[2, 1, 1, 1, 1, 0, 0, 0, 3]);
        assert_eq!(&p[9..13], &[1, 1, 5, 0]);
        assert_eq!(&p[13..], FIRMWARE_ID.as_bytes());
    }
}
