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

/// Forgot the PIN: the 24 words of *this* wallet, then a new PIN. `None` if cancelled;
/// `Some(None)` if the words belong to another wallet.
pub fn forgot_pin(io: &mut dyn Io, entropy: &[u8; 32]) -> Option<Option<u8>> {
    let mut lines = vec![
        (None, "Enter this wallet's 24 recovery words, one by one. If they match, you choose a new PIN."),
        (Some(Tone::Warn), "The wallet stays as it is. About 12 presses a word."),
    ];
    if LAPTOP_SCREEN {
        lines.push((Some(Tone::Bad), "This build draws the screen on the laptop: the laptop sees the words."));
    }
    if !confirm(io, "NEW PIN", "Forgot your PIN?", &lines, ("CANCEL", Btn::Bad), ("START", Btn::Good)) {
        return None;
    }
    let m = enter_phrase(io, "NEW PIN")?;
    if m.to_entropy_array().0[..32] != entropy[..] {
        io.result(Tone::Bad, "NO MATCH", vec!["Those words are another wallet's".into(), "The PIN is unchanged".into()]);
        return Some(None);
    }
    Some(Some(choose_pin(io, "Words match. Choose a new PIN")?))
}

// ---------------------------------------------------------------- passphrase

/// 0 = done, then the printable ASCII characters from space to `~`.
fn charset_label(i: usize) -> String {
    match i {
        0 => "DONE".into(),
        1 => "space".into(),
        _ => format!("'{}'", (b' ' + i as u8 - 1) as char),
    }
}

/// Enter a passphrase on the device. `None` if cancelled; an empty passphrase is the
/// standard wallet.
pub fn passphrase(io: &mut dyn Io) -> Option<String> {
    let mut lines = vec![
        (None, "A passphrase opens a separate wallet from the same 24 words: other accounts, other funds."),
        (Some(Tone::Warn), "It isn't stored. Enter it after every start; DONE at once goes back to the standard wallet."),
    ];
    if LAPTOP_SCREEN {
        lines.push((Some(Tone::Bad), "The laptop draws this screen and sees the passphrase."));
    }
    if !confirm(io, "PASSPHRASE", "Use a passphrase", &lines, ("CANCEL", Btn::Bad), ("START", Btn::Good)) {
        return None;
    }
    loop {
        let mut p = String::new();
        loop {
            if p.len() == MAX_PASSPHRASE {
                break;
            }
            let (mut lo, mut hi) = (0usize, 96usize);
            while hi - lo > 1 {
                let mid = (lo + hi) / 2;
                let screen = input(
                    "PASSPHRASE",
                    format!("Passphrase: {p}_"),
                    Body::Pick {
                        above: format!("between {} and {}", charset_label(lo), charset_label(hi - 1)),
                        word: charset_label(mid),
                        below: "tap: earlier. hold: this one or later. Order: space ! ... 0-9 ... A-Z ... a-z ... ~".into(),
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
            if lo == 0 {
                break;
            }
            p.push((b' ' + lo as u8 - 1) as char);
        }
        let shown = if p.is_empty() { "(none: the standard wallet)".to_string() } else { p.clone() };
        let use_it = io.ask(
            input("PASSPHRASE", "Use this passphrase?", Body::Text(vec![(None, shown)]), ("REDO", Btn::Neutral), ("USE IT", Btn::Good)),
            Some(STEP_TIMEOUT),
        )?;
        if use_it {
            return Some(p);
        }
    }
}

// ---------------------------------------------------------------- wipe

/// Two confirmations before erasing the wallet.
pub fn confirm_wipe(io: &mut dyn Io) -> bool {
    confirm(
        io,
        "ERASE",
        "Erase this wallet?",
        &[
            (None, "The seed and the PIN are deleted, and a new wallet is made."),
            (Some(Tone::Bad), "Without your 24 words, the funds on this wallet are lost."),
        ],
        ("KEEP", Btn::Good),
        ("ERASE", Btn::Bad),
    ) && confirm(
        io,
        "ERASE",
        "Really erase?",
        &[(Some(Tone::Bad), "This can't be undone.")],
        ("KEEP", Btn::Good),
        ("ERASE NOW", Btn::Bad),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Presses chosen by looking at each screen; records results.
    struct Script<F: FnMut(&Screen) -> Option<bool>> {
        answer: F,
        results: Vec<&'static str>,
        screens: usize,
    }
    impl<F: FnMut(&Screen) -> Option<bool>> Io for Script<F> {
        fn ask(&mut self, screen: Screen, _: Option<Duration>) -> Option<bool> {
            self.screens += 1;
            (self.answer)(&screen)
        }
        fn result(&mut self, _: Tone, word: &'static str, _: Vec<String>) {
            self.results.push(word);
        }
    }
    fn script<F: FnMut(&Screen) -> Option<bool>>(answer: F) -> Script<F> {
        Script { answer, results: vec![], screens: 0 }
    }

    fn bits(pin: u8) -> impl FnMut(&Screen) -> Option<bool> {
        let mut k = 0;
        move |_| {
            let b = pin >> (7 - k % 8) & 1 == 1;
            k += 1;
            Some(b)
        }
    }

    #[test]
    fn pin_is_eight_presses_msb_first() {
        let mut io = script(bits(0b1011_0010));
        assert_eq!(enter_pin(&mut io, "x", None, None), Some(0b1011_0010));
        assert_eq!(io.screens, 8);
    }

    #[test]
    fn forgot_pin_needs_this_wallets_words() {
        let target: Vec<usize> = mnemonic().word_indices().collect();
        let list = Language::English.word_list();
        let answer = |target: Vec<usize>| {
            move |s: &Screen| {
                let Screen::Input(i) = s else { panic!() };
                match &i.body {
                    Body::Pick { word, .. } if i.heading.ends_with("where is yours?") => {
                        let n: usize = i.heading[5..].split(' ').next().unwrap().parse().unwrap();
                        Some(target[n - 1] >= list.iter().position(|w| w == word).unwrap())
                    }
                    // the new PIN, twice: 0x5a
                    Body::Pin { entered, .. } => Some(0x5a >> (7 - entered) & 1 == 1),
                    _ => Some(true),
                }
            }
        };
        let mut io = script(answer(target.clone()));
        assert_eq!(forgot_pin(&mut io, &[0x5a; 32]), Some(Some(0x5a)));
        // another wallet's words: no new PIN
        let other: Vec<usize> = Mnemonic::from_entropy(&[1; 32]).unwrap().word_indices().collect();
        let mut io = script(answer(other));
        assert_eq!(forgot_pin(&mut io, &[0x5a; 32]), Some(None));
        assert_eq!(io.results, vec!["NO MATCH"]);
    }

    #[test]
    fn choose_pin_repeats_until_both_match() {
        // first pair differs, second matches
        let seq = [0x0Fu8, 0xF0, 0x55, 0x55];
        let mut k = 0;
        let mut io = script(move |_| {
            let b = seq[k / 8] >> (7 - k % 8) & 1 == 1;
            k += 1;
            Some(b)
        });
        assert_eq!(choose_pin(&mut io, "x"), Some(0x55));
    }

    fn mnemonic() -> Mnemonic {
        Mnemonic::from_entropy(&[0x5a; 32]).unwrap()
    }

    #[test]
    fn restore_finds_every_word_by_bisection() {
        let target: Vec<usize> = mnemonic().word_indices().collect();
        let list = Language::English.word_list();
        let mut io = script(|s: &Screen| {
            let Screen::Input(i) = s else { panic!() };
            match &i.body {
                // "Word n of 24: where is yours?": right if my word >= the shown one
                Body::Pick { word, .. } if i.heading.ends_with("where is yours?") => {
                    let n: usize = i.heading[5..].split(' ').next().unwrap().parse().unwrap();
                    let shown = list.iter().position(|w| w == word).unwrap();
                    Some(target[n - 1] >= shown)
                }
                _ => Some(true), // start, "is this your word?", restore
            }
        });
        let m = restore(&mut io).expect("restored");
        assert_eq!(m.to_entropy_array().0[..32], [0x5a; 32]);
        // 24 words x (11 picks + 1 check) + start + final confirmation
        assert_eq!(io.screens, 24 * 12 + 2);
    }

    #[test]
    fn restore_with_a_wrong_word_offers_a_retry() {
        let list = Language::English.word_list();
        let mut target: Vec<usize> = mnemonic().word_indices().collect();
        target[23] = (target[23] + 1) % 2048; // breaks the checksum (almost surely)
        assert!(Mnemonic::parse_normalized(&target.iter().map(|&i| list[i]).collect::<Vec<_>>().join(" ")).is_err());
        let mut io = script(|s: &Screen| {
            let Screen::Input(i) = s else { panic!() };
            match &i.body {
                Body::Pick { word, .. } if i.heading.ends_with("where is yours?") => {
                    let n: usize = i.heading[5..].split(' ').next().unwrap().parse().unwrap();
                    Some(target[n - 1] >= list.iter().position(|w| w == word).unwrap())
                }
                _ if i.heading == "These words don't check out" => Some(false), // cancel
                _ => Some(true),
            }
        });
        assert!(restore(&mut io).is_none());
    }

    #[test]
    fn passphrase_by_bisection() {
        let want = b"Tr0ub4dor & 3";
        let mut io = script(move |s: &Screen| {
            let Screen::Input(i) = s else { panic!() };
            match &i.body {
                Body::Pick { word, .. } => {
                    // the shown entry: 0 DONE, 1 space, then the character
                    let shown = match word.as_str() {
                        "DONE" => 0,
                        "space" => 1,
                        w => (w.as_bytes()[1] - b' ' + 1) as usize,
                    };
                    // the heading is "Passphrase: <typed so far>_"
                    let typed = i.heading.len() - "Passphrase: _".len();
                    let mine = want.get(typed).map(|&c| (c - b' ' + 1) as usize).unwrap_or(0);
                    Some(mine >= shown)
                }
                _ => Some(true),
            }
        });
        assert_eq!(passphrase(&mut io).as_deref(), Some("Tr0ub4dor & 3"));
    }

    #[test]
    fn phrase_pages_back_and_forth() {
        // show, next, back, next x3, done
        let presses = [true, true, false, true, true, true, true];
        let mut k = 0;
        let mut io = script(move |_| {
            let p = presses.get(k).copied();
            k += 1;
            p
        });
        assert!(show_phrase(&mut io, &mnemonic()));
        assert_eq!(io.results, vec!["DONE"]);
    }

    #[test]
    fn wipe_needs_two_rights() {
        assert!(confirm_wipe(&mut script(|_| Some(true))));
        let mut k = 0;
        assert!(!confirm_wipe(&mut script(move |_| {
            k += 1;
            Some(k == 1)
        })));
    }
}
