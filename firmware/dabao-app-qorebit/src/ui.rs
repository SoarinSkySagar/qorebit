//! Device screen and approval.
//!
//! The device decides every screen and **renders it itself** into a 240x280 colour framebuffer
//! (`screen.rs`, 16-colour palette, `embedded-graphics`). Signing asks for a decision with
//! [`Ui::ask`]: the request blocks until the user approves, rejects, or the timeout passes
//! (= reject).
//!
//! **Two buttons.** The left button is 0 / Reject, the right one is 1 / Approve. There is no tap or
//! hold: one press of a button is the answer.
//!
//! Backends:
//! * `vdisplay` (cargo feature, default on for now): the **virtual display**. The rendered
//!   framebuffer is read over USB by `tools/qorebit_display.py` (only the rows that changed,
//!   run-length encoded), which only maps palette indices to pixels in a window and sends back
//!   which button was pressed (`DISPLAY_POLL` / `DISPLAY_INPUT`). Development stand-in: the
//!   laptop could fake the window or the presses.
//! * `panel` (cargo feature): the ST7789 on the board and two buttons, PB1 (left) and PB2 (right)
//!   (`panel.rs`), show the same framebuffer and press the same inputs.
//! * none: without a display the device refuses every signing request (never auto-approves).
//!
//! Input screens ([`Ui::ask_button`]) take the two buttons as a bit: left = 0, right = 1 (PIN digits,
//! next / back, word-by-word entry).

use crate::screen::{self, Frame, Screen, Tone};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long an approval screen waits before rejecting.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a result screen (SIGNED, REJECTED, ...) stays before the idle screen returns.
pub const RESULT_TIME: Duration = Duration::from_secs(4);

pub const KIND_IDLE: u8 = 0;
pub const KIND_APPROVAL: u8 = 1;
pub const KIND_RESULT: u8 = 2;
/// A PIN entry screen (left = 0, right = 1).
pub const KIND_PIN: u8 = 3;
/// Any other screen waiting for a button (recovery words, confirmations, word entry).
pub const KIND_INPUT: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Reject,
    Timeout,
    /// No approval backend in this build.
    Unavailable,
}

struct State {
    /// Bumped on every visual change (new screen, countdown tick).
    seq: u32,
    /// Identifies the current screen; a button press must name it.
    id: u32,
    kind: u8,
    screen: Screen,
    idle: Screen,
    deadline: Option<Instant>,
    /// Seconds left last time the approval screen was rendered.
    shown_left: u8,
    decision: Option<(u32, bool)>,
    result_until: Option<Instant>,
    /// The last frame sent to the display and its seq, to send only changed rows next time.
    sent: Option<(u32, Frame)>,
    /// A Working screen is up (no result follows it, e.g. while the account is prepared).
    working: bool,
}

impl State {
    fn show(&mut self, screen: Screen) {
        let kind = match &screen {
            Screen::Approval(_) => KIND_APPROVAL,
            Screen::Result { .. } | Screen::Working { .. } => KIND_RESULT,
            Screen::Input(i) if i.pin => KIND_PIN,
            Screen::Input(_) => KIND_INPUT,
            _ => KIND_IDLE,
        };
        self.seq = self.seq.wrapping_add(1);
        self.id = self.id.wrapping_add(1);
        self.kind = kind;
        self.screen = screen;
        self.working = false;
        self.result_until = (kind == KIND_RESULT).then(|| Instant::now() + RESULT_TIME);
        if kind != KIND_APPROVAL {
            self.deadline = None;
        }
    }

    fn left(&self) -> u8 {
        self.deadline.map(|d| d.saturating_duration_since(Instant::now()).as_secs().min(255) as u8).unwrap_or(0)
    }

    /// Time-based changes: result screens expire, the countdown ticks.
    fn tick(&mut self) {
        if self.kind == KIND_RESULT && self.result_until.is_some_and(|t| Instant::now() >= t) {
            let idle = self.idle.clone();
            self.show(idle);
        }
        if self.kind == KIND_APPROVAL {
            let left = self.left();
            if left != self.shown_left {
                self.shown_left = left;
                self.seq = self.seq.wrapping_add(1);
            }
        }
    }

    fn render(&self) -> Frame {
        screen::render(&self.screen, self.shown_left, APPROVAL_TIMEOUT.as_secs() as u8)
    }

    /// Encode the current frame for a display that has frame `since`: only the changed rows
    /// when `since` is the frame we sent last, else the whole frame.
    fn update(&mut self, since: Option<u32>) -> Vec<u8> {
        let frame = self.render();
        let (y0, y1) = match &self.sent {
            Some((seq, old)) if since == Some(*seq) => frame.changed_rows(old).unwrap_or((0, 0)),
            _ => (0, screen::HEIGHT as usize),
        };
        let mut p = Vec::with_capacity(64 + 4096);
        for v in [screen::WIDTH as u16, screen::HEIGHT as u16, y0 as u16, y1 as u16] {
            p.extend_from_slice(&v.to_be_bytes());
        }
        for c in screen::PALETTE {
            p.extend_from_slice(&c);
        }
        p.extend_from_slice(&frame.encode_rows(y0, y1));
        self.sent = Some((self.seq, frame));
        p
    }
}

pub struct Ui {
    state: Mutex<State>,
    cv: Condvar,
}

static UI: OnceLock<Ui> = OnceLock::new();

/// Whether the companion app is talking to the device (a host request in the last few seconds).
static LINK: AtomicBool = AtomicBool::new(false);

pub fn link_up() -> bool {
    LINK.load(Ordering::Relaxed)
}

pub fn ui() -> &'static Ui {
    UI.get_or_init(|| Ui {
        state: Mutex::new(State {
            seq: 1,
            id: 1,
            kind: KIND_IDLE,
            screen: Screen::Boot,
            idle: Screen::Boot,
            deadline: None,
            shown_left: 0,
            decision: None,
            result_until: None,
            sent: None,
            working: false,
        }),
        cv: Condvar::new(),
    })
}

/// A DISPLAY_POLL / DISPLAY_INPUT answer.
pub struct Snapshot {
    pub seq: u32,
    pub id: u32,
    pub kind: u8,
    pub left: u8,
    /// Frame update (width, height, rows y0..y1, palette, RLE rows); `None` when the caller
    /// already has frame `seq`.
    pub frame: Option<Vec<u8>>,
}

impl Ui {
    /// A screen with buttons exists: the virtual display or the panel on the board.
    pub const fn enabled() -> bool {
        cfg!(any(feature = "vdisplay", feature = "panel"))
    }

    /// Show an input screen and wait for a button: `Some(true)` right (1), `Some(false)` left
    /// (0), `None` after `timeout` (or without a display). The screen stays until the caller
    /// shows the next one.
    pub fn ask_button(&self, screen: Screen, timeout: Option<Duration>) -> Option<bool> {
        if !Self::enabled() {
            return None;
        }
        let mut s = self.state.lock().unwrap();
        s.decision = None;
        s.show(screen);
        let id = s.id;
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if let Some((did, right)) = s.decision.take() {
                if did == id {
                    return Some(right);
                }
            }
            match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return None;
                    }
                    s = self.cv.wait_timeout(s, d - now).unwrap().0;
                }
                None => s = self.cv.wait(s).unwrap(),
            }
        }
    }

    /// A physical button (the panel build): right = 1 / Approve, left = 0 / Reject, on
    /// whatever screen is shown now.
    #[cfg_attr(not(feature = "panel"), allow(dead_code))]
    pub fn press(&self, right: bool) -> bool {
        let id = self.state.lock().unwrap().id;
        self.input(id, right).is_some()
    }

    /// The current frame and its sequence number (for the panel).
    #[cfg_attr(not(feature = "panel"), allow(dead_code))]
    pub fn frame(&self) -> (u32, Frame) {
        let mut s = self.state.lock().unwrap();
        s.tick();
        (s.seq, s.render())
    }

    /// Set the screen shown when nothing needs a decision.
    /// The companion app appeared or went away: the home screen's connection line follows.
    pub fn set_link(&self, up: bool) {
        LINK.store(up, Ordering::Relaxed);
        let mut s = self.state.lock().unwrap();
        if let Screen::Idle { home, .. } = &mut s.idle {
            home.app_seen = up;
        }
        if s.kind == KIND_IDLE {
            let idle = s.idle.clone();
            s.show(idle);
        }
    }

    pub fn set_idle(&self, idle: Screen) {
        let mut s = self.state.lock().unwrap();
        s.idle = idle;
        if s.kind == KIND_IDLE {
            let idle = s.idle.clone();
            s.show(idle);
        }
    }

    /// A short "working" screen (signing takes milliseconds, but the user sees that it ran).
    pub fn show_working(&self, word: &'static str, lines: Vec<String>) {
        let mut s = self.state.lock().unwrap();
        s.show(Screen::Working { word, lines, permille: 1000 });
        s.result_until = None;
        s.working = true;
    }

    /// Show a short result screen (`word` large in the tone's colour); idle returns after
    /// RESULT_TIME.
    pub fn show_result(&self, tone: Tone, word: impl Into<std::borrow::Cow<'static, str>>, lines: Vec<String>) {
        self.state.lock().unwrap().show(Screen::Result { tone, word: word.into(), lines });
    }

    /// Show an approval screen and block until the user decides or `timeout` passes.
    pub fn ask(&self, screen: Screen, timeout: Duration) -> Decision {
        if !Self::enabled() {
            return Decision::Unavailable;
        }
        let mut s = self.state.lock().unwrap();
        s.deadline = Some(Instant::now() + timeout);
        s.shown_left = s.left();
        s.decision = None;
        s.show(screen);
        let id = s.id;
        let deadline = s.deadline.unwrap();
        loop {
            if let Some((did, approve)) = s.decision.take() {
                if did == id {
                    // `input` has already switched the screen
                    return if approve { Decision::Approve } else { Decision::Reject };
                }
            }
            let now = Instant::now();
            if now >= deadline {
                s.show(Screen::Result {
                    tone: Tone::Warn,
                    word: "TIMED OUT".into(),
                    lines: vec!["No decision in 60 s".into(), "Nothing was signed".into()],
                });
                return Decision::Timeout;
            }
            s = self.cv.wait_timeout(s, deadline - now).unwrap().0;
        }
    }

    /// DISPLAY_POLL: the current screen; the frame only if it changed since `since`.
    pub fn poll(&self, since: Option<u32>) -> Snapshot {
        let mut s = self.state.lock().unwrap();
        s.tick();
        let frame = (since != Some(s.seq)).then(|| s.update(since));
        Snapshot { seq: s.seq, id: s.id, kind: s.kind, left: s.shown_left, frame }
    }

    /// DISPLAY_INPUT: the button answered screen `id` (`approve`: held long enough). Switches the
    /// screen at once (so the display reacts immediately) and returns the new frame; `None` if
    /// that screen is gone.
    pub fn input(&self, id: u32, approve: bool) -> Option<Snapshot> {
        let mut s = self.state.lock().unwrap();
        if s.id != id {
            return None;
        }
        if s.kind == KIND_PIN || s.kind == KIND_INPUT {
            // the flow waiting in ask_button decides the next screen
            s.decision = Some((id, approve));
            self.cv.notify_all();
            let frame = s.update(None);
            return Some(Snapshot { seq: s.seq, id: s.id, kind: s.kind, left: 0, frame: Some(frame) });
        }
        if s.kind != KIND_APPROVAL {
            return None;
        }
        s.decision = Some((id, approve));
        if approve {
            s.show(Screen::Result {
                tone: Tone::Good,
                word: "APPROVED".into(),
                lines: vec!["Signing on the chip".into(), "ML-DSA-87 + secp256k1".into()],
            });
            s.result_until = None; // stays until the signer shows its result
        } else {
            s.show(Screen::Result { tone: Tone::Bad, word: "REJECTED".into(), lines: vec!["Nothing was signed".into()] });
        }
        self.cv.notify_all();
        let frame = s.update(None);
        Some(Snapshot { seq: s.seq, id: s.id, kind: s.kind, left: 0, frame: Some(frame) })
    }
}
