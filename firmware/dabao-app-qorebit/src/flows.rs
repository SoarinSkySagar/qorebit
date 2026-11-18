//! Device-side flows that only use the one button: a tap is 0, a hold is 1.
//!
//! * **Signing PIN**: 8 binary digits, one per press (256 PINs):
//!   tap = 0, hold = 1. Set from the companion's
//!   first-time setup, asked after you approve each signature. 5 wrong in a row lock signing
//!   until the PIN is reset with the 24 recovery words (`storage.rs`: the attempts live on a
//!   one-way counter, so a power cut doesn't reset them). Change it with the old PIN.
//! * **Forgot PIN**: the 24 words entered on the device (like a restore); if they are this
//!   wallet's, a new PIN.
//! * **Recovery words**: the 24 BIP-39 words, 6 per page, after the PIN.
//! * **Restore**: each word by binary search over the 2,048-word list: the device shows a word
//!   from the middle of what's left, you answer "my word comes earlier" (tap) or "this word or
//!   later" (hold); 11 answers find any word. Then the BIP-39 checksum is checked.
//! * **Passphrase**: the same search over the printable ASCII characters (7 answers each).
//! * **Wipe**: two confirmations, then a new seed (the companion then asks for a new PIN).
//!
//! The flows talk to the screen through [`Io`], so tests can drive them with scripted presses.

use crate::screen::{Body, Btn, Input, Screen, Tone};
use crate::storage::{self, Owc, PinCheck, Slots, PIN_BITS};
use qorebit_signer::{Language, Mnemonic};
use std::time::Duration;

/// How long a screen in a flow you started waits for a press before the flow is cancelled.
pub const STEP_TIMEOUT: Duration = Duration::from_secs(120);
pub const WORDS: usize = 24;
pub const MAX_PASSPHRASE: usize = 32;

pub trait Io {
    /// Show `screen`; `Some(true)` = hold, `Some(false)` = tap, `None` = timed out.
    fn ask(&mut self, screen: Screen, timeout: Option<Duration>) -> Option<bool>;
    fn result(&mut self, tone: Tone, word: &'static str, lines: Vec<String>);
}

/// The device's own screen and buttons (virtual display or panel).
pub struct DeviceIo;

impl Io for DeviceIo {
    fn ask(&mut self, screen: Screen, timeout: Option<Duration>) -> Option<bool> {
        crate::ui::ui().ask_button(screen, timeout)
    }
    fn result(&mut self, tone: Tone, word: &'static str, lines: Vec<String>) {
        crate::ui::ui().show_result(tone, word, lines)
    }
}

/// The virtual display draws on the laptop: anything shown or typed there, the laptop sees.
pub const LAPTOP_SCREEN: bool = cfg!(feature = "vdisplay") && !cfg!(feature = "panel");

fn input(tag: &'static str, heading: impl Into<String>, body: Body, left: (&str, Btn), right: (&str, Btn)) -> Screen {
    Screen::Input(Input {
        tag,
        heading: heading.into(),
        body,
        left: (left.0.into(), left.1),
        right: (right.0.into(), right.1),
        pin: false,
    })
}

fn text(lines: &[(Option<Tone>, &str)]) -> Body {
    Body::Text(lines.iter().map(|(t, s)| (*t, s.to_string())).collect())
}

/// A yes / no question: true only for a hold.
fn confirm(io: &mut dyn Io, tag: &'static str, heading: &str, lines: &[(Option<Tone>, &str)], no: (&str, Btn), yes: (&str, Btn)) -> bool {
    io.ask(input(tag, heading, text(lines), no, yes), Some(STEP_TIMEOUT)) == Some(true)
}

// ---------------------------------------------------------------- PIN

/// Read 8 binary digits. `None` if a press timed out.
pub fn enter_pin(io: &mut dyn Io, heading: &str, note: Option<(Tone, String)>, timeout: Option<Duration>) -> Option<u8> {
    let mut pin = 0u8;
    for k in 0..PIN_BITS as u8 {
        let screen = Screen::Input(Input {
            tag: "PIN",
            heading: heading.into(),
            body: Body::Pin { entered: k, total: PIN_BITS as u8, note: note.clone() },
            left: ("0".into(), Btn::Neutral),
            right: ("1".into(), Btn::Neutral),
            pin: true,
        });
        pin = pin << 1 | io.ask(screen, timeout)? as u8;
    }
    Some(pin)
}

/// Choose a signing PIN (entered twice). `None` if a press timed out.
pub fn choose_pin(io: &mut dyn Io, heading: &str) -> Option<u8> {
    let mut note = Some((Tone::Warn, "8 presses: tap is 0, hold is 1. Asked for every signature.".to_string()));
    loop {
        let a = enter_pin(io, heading, note.take(), Some(STEP_TIMEOUT))?;
        let b = enter_pin(io, "Enter the same PIN again", None, Some(STEP_TIMEOUT))?;
        if a == b {
            return Some(a);
        }
        note = Some((Tone::Bad, "The two PINs differ. Choose again.".into()));
    }
}

/// What a PIN check before signing (or showing the words) came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinOk {
    Right,
    /// Wrong (the screen said so); tries left.
    Wrong(u32),
    /// Signing is locked: reset the PIN with the recovery words.
    Locked,
    /// No PIN entered in time, or the check failed.
    Cancelled,
}

/// Ask for the PIN and check it. A wrong PIN counts as a try.
pub fn check_pin<S: Slots + Owc>(io: &mut dyn Io, store: &mut S, why: &str) -> PinOk {
    let left = storage::pin_tries_left(store);
    if left == 0 {
        io.result(Tone::Bad, "PIN LOCKED", vec!["5 wrong PINs in a row".into(), "Reset it with your 24 words".into()]);
        return PinOk::Locked;
    }
    let note = (left < storage::PIN_TRIES).then(|| (Tone::Warn, format!("{left} {} left before signing locks", if left == 1 { "try" } else { "tries" })));
    let Some(pin) = enter_pin(io, why, note, Some(STEP_TIMEOUT)) else { return PinOk::Cancelled };
    match storage::check_pin(store, pin) {
        PinCheck::Right | PinCheck::NotSet => PinOk::Right,
        PinCheck::Wrong(n) => {
            io.result(Tone::Bad, "WRONG PIN", vec![format!("{n} {} left", if n == 1 { "try" } else { "tries" }), "Nothing was done".into()]);
            PinOk::Wrong(n)
        }
        PinCheck::Locked => {
            io.result(Tone::Bad, "PIN LOCKED", vec!["5 wrong PINs in a row".into(), "Reset it with your 24 words".into()]);
            PinOk::Locked
        }
        PinCheck::Failed => PinOk::Cancelled,
    }
}

// ---------------------------------------------------------------- recovery words

/// Show the 24 words, 6 a page. `false` if cancelled or timed out.
pub fn show_phrase(io: &mut dyn Io, m: &Mnemonic) -> bool {
    let mut warn: Vec<(Option<Tone>, &str)> = vec![
        (None, "Your 24 recovery words come next."),
        (Some(Tone::Warn), "Anyone who sees them can take your funds. Write them on paper. Don't type or photograph them."),
    ];
    if LAPTOP_SCREEN {
        warn.push((Some(Tone::Bad), "This build draws the screen on the laptop: the laptop sees the words."));
    }
    if !confirm(io, "BACKUP", "Back up your wallet", &warn, ("CANCEL", Btn::Bad), ("SHOW", Btn::Good)) {
        return false;
    }
    let words: Vec<&str> = m.words().collect();
    let pages = words.len().div_ceil(6);
    let mut page = 0;
    loop {
        let first = page * 6;
        let list = words[first..(first + 6).min(words.len())].iter().enumerate().map(|(k, w)| (first + k + 1, w.to_string())).collect();
        let last = page + 1 == pages;
        let screen = input(
            "BACKUP",
            format!("Words {}-{} of {}", first + 1, (first + 6).min(words.len()), words.len()),
            Body::Words(list),
            (if page == 0 { "CANCEL" } else { "< BACK" }, Btn::Neutral),
            (if last { "DONE" } else { "NEXT >" }, if last { Btn::Good } else { Btn::Neutral }),
        );
        match io.ask(screen, Some(STEP_TIMEOUT)) {
            Some(true) if last => {
                io.result(Tone::Good, "DONE", vec!["Keep the words offline".into(), "They restore every account".into()]);
                return true;
            }
            Some(true) => page += 1,
            Some(false) if page > 0 => page -= 1,
            _ => return false,
        }
    }
}

/// Binary search for one word of the list. `None` if a press timed out.
fn pick_word(io: &mut dyn Io, tag: &'static str, n: usize) -> Option<usize> {
    let list = Language::English.word_list();
    let (mut lo, mut hi) = (0usize, list.len());
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        let screen = input(
            tag,
            format!("Word {n} of {WORDS}: where is yours?"),
            Body::Pick {
                above: format!("between {} and {}", list[lo], list[hi - 1]),
                word: list[mid].to_string(),
                below: "tap: my word comes earlier. hold: it's this word or later".into(),
            },
            ("< EARLIER", Btn::Neutral),
            ("LATER >", Btn::Neutral),
        );
        if io.ask(screen, Some(STEP_TIMEOUT))? {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// Restore from 24 words entered on the device. `None` if cancelled or timed out.
pub fn restore(io: &mut dyn Io) -> Option<Mnemonic> {
    let mut lines = vec![
        (None, "Enter your 24 recovery words, one by one, on this device."),
        (Some(Tone::Warn), "The wallet on this device now is replaced. Without its own words, its funds are lost."),
    ];
    if LAPTOP_SCREEN {
        lines.push((Some(Tone::Bad), "This build draws the screen on the laptop: the laptop sees the words."));
    }
    if !confirm(io, "RESTORE", "Restore a wallet", &lines, ("CANCEL", Btn::Bad), ("START", Btn::Good)) {
        return None;
    }
    let m = enter_phrase(io, "RESTORE")?;
    confirm(
        io,
        "RESTORE",
        "The words check out",
        &[(None, "Replace this device's wallet with the restored one?")],
        ("CANCEL", Btn::Bad),
        ("RESTORE", Btn::Good),
    )
    .then_some(m)
}

/// The 24 words, entered one by one; a wrong checksum offers to start over. `None` if
/// cancelled or timed out.
fn enter_phrase(io: &mut dyn Io, tag: &'static str) -> Option<Mnemonic> {
    let list = Language::English.word_list();
    loop {
        let mut words = Vec::with_capacity(WORDS);
        while words.len() < WORDS {
            let n = words.len() + 1;
            let i = pick_word(io, tag, n)?;
            let check = input(
                tag,
                format!("Word {n} of {WORDS}"),
                Body::Pick { above: "Is this your word?".into(), word: list[i].to_string(), below: String::new() },
                ("REDO", Btn::Neutral),
                ("YES >", Btn::Good),
            );
            if io.ask(check, Some(STEP_TIMEOUT))? {
                words.push(list[i]);
            }
        }
        match Mnemonic::parse_normalized(&words.join(" ")) {
            Ok(m) => return Some(m),
            Err(_) => {
                let again = confirm(
                    io,
                    tag,
                    "These words don't check out",
                    &[(Some(Tone::Bad), "The BIP-39 checksum is wrong: at least one word is wrong.")],
                    ("CANCEL", Btn::Bad),
                    ("START OVER", Btn::Neutral),
                );
                if !again {
                    return None;
                }
            }
        }
    }
}

// ---------------------------------------------------------------- PIN changes

/// Change the PIN: the current one, then a new one twice. `None` if cancelled; `Some(None)`
/// if the current PIN was wrong (or signing is locked).
pub fn change_pin<S: Slots + Owc>(io: &mut dyn Io, store: &mut S) -> Option<Option<u8>> {
    match check_pin(io, store, "Your current PIN") {
        PinOk::Right => {}
        PinOk::Cancelled => return None,
        _ => return Some(None),
    }
    Some(Some(choose_pin(io, "Choose a new PIN")?))
}

