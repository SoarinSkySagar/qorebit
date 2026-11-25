//! On-chip rendering of the device screen.
//!
//! The screen is a **240x240 colour panel** (ST7789-class TFT) drawn with a 16-colour palette:
//! one palette index per pixel, rendered here on the Baochip with `embedded-graphics`.
//!
//! * now (`vdisplay`): the chip run-length encodes the rows that changed and sends them over
//!   USB; the laptop window only maps indices to the palette colours and shows the pixels;
//! * later (real panel): the same frame goes to the TFT, each index expanded to RGB565.
//!
//! Numbers (amounts, the account number, test codes) use seven-segment digits like a digital
//! clock, drawn as polygons so they stay sharp at any size; text uses the X11 bitmap fonts.

use embedded_graphics::{
    mono_font::{
        ascii::{FONT_6X10, FONT_7X13, FONT_9X18_BOLD},
        MonoFont, MonoTextStyle,
    },
    pixelcolor::raw::RawU8,
    prelude::*,
    primitives::{CornerRadii, PrimitiveStyle, Rectangle, RoundedRectangle, Triangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

pub const WIDTH: u32 = 240;
pub const HEIGHT: u32 = 240;
pub const FRAME_LEN: usize = (WIDTH * HEIGHT) as usize;

/// A palette index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pal(pub u8);

impl PixelColor for Pal {
    type Raw = RawU8;
}

pub const BG: Pal = Pal(0);
pub const WHITE: Pal = Pal(1);
pub const INK: Pal = Pal(2);
pub const MUTED: Pal = Pal(3);
pub const LINE: Pal = Pal(4);
pub const GHOST: Pal = Pal(5);
pub const GREEN: Pal = Pal(6);
pub const RED: Pal = Pal(7);
pub const AMBER: Pal = Pal(8);
pub const GREEN_D: Pal = Pal(9);
pub const RED_D: Pal = Pal(10);
pub const AMBER_D: Pal = Pal(11);
pub const ADDR2: Pal = Pal(12);

/// RGB for each palette index (sent with every frame update, so the window needs no copy).
pub const PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00], // BG
    [0xFF, 0xFF, 0xFF], // WHITE navbar
    [0xEC, 0xEE, 0xF1], // INK
    [0x86, 0x8C, 0x96], // MUTED labels
    [0x2B, 0x2F, 0x36], // LINE dividers, empty bar
    [0x16, 0x19, 0x1D], // GHOST unlit segments
    [0x2E, 0xE0, 0x7A], // GREEN approve
    [0xFF, 0x4A, 0x4A], // RED reject
    [0xFF, 0xB3, 0x1F], // AMBER warning
    [0x0A, 0x33, 0x1C], // GREEN_D
    [0x45, 0x10, 0x12], // RED_D
    [0x42, 0x30, 0x05], // AMBER_D
    [0xB8, 0xC6, 0xFF], // ADDR2 alternate address groups
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
];

/// 240x240 framebuffer, one palette index per pixel, row-major.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame(pub Vec<u8>);

impl Frame {
    pub fn new() -> Self {
        Frame(vec![BG.0; FRAME_LEN])
    }

    /// The rows `y0..y1` run-length encoded: one byte `color << 4 | (run - 1)` for runs of
    /// 1..=15; a low nibble of 15 means a long run whose length is `16 + next byte`.
    pub fn encode_rows(&self, y0: usize, y1: usize) -> Vec<u8> {
        let px = &self.0[y0 * WIDTH as usize..y1 * WIDTH as usize];
        let mut out = Vec::new();
        let mut i = 0;
        while i < px.len() {
            let c = px[i];
            let mut n = 1;
            while i + n < px.len() && px[i + n] == c && n < 16 + 255 {
                n += 1;
            }
            if n < 16 {
                out.push(c << 4 | (n as u8 - 1));
            } else {
                out.push(c << 4 | 15);
                out.push((n - 16) as u8);
            }
            i += n;
        }
        out
    }

    /// First and last+1 rows that differ from `old` (`None` if identical).
    pub fn changed_rows(&self, old: &Frame) -> Option<(usize, usize)> {
        let w = WIDTH as usize;
        let rows = HEIGHT as usize;
        let differs = |y: usize| self.0[y * w..(y + 1) * w] != old.0[y * w..(y + 1) * w];
        let y0 = (0..rows).find(|&y| differs(y))?;
        let y1 = (y0..rows).rev().find(|&y| differs(y)).unwrap() + 1;
        Some((y0, y1))
    }
}

impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(WIDTH, HEIGHT)
    }
}

impl DrawTarget for Frame {
    type Color = Pal;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Pal>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && p.x < WIDTH as i32 && p.y < HEIGHT as i32 {
                self.0[p.y as usize * WIDTH as usize + p.x as usize] = c.0;
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Pal) -> Result<(), Self::Error> {
        let a = area.intersection(&self.bounding_box());
        if let Some(br) = a.bottom_right() {
            for y in a.top_left.y..=br.y {
                let row = y as usize * WIDTH as usize;
                self.0[row + a.top_left.x as usize..=row + br.x as usize].fill(color.0);
            }
        }
        Ok(())
    }
}

/// Draws everything at 2x (for the big result words).
struct Double<'a>(&'a mut Frame, Point);

impl OriginDimensions for Double<'_> {
    fn size(&self) -> Size {
        Size::new(WIDTH / 2, HEIGHT / 2)
    }
}

impl DrawTarget for Double<'_> {
    type Color = Pal;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Pal>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            let q = Point::new(self.1.x + p.x * 2, self.1.y + p.y * 2);
            self.0.fill_solid(&Rectangle::new(q, Size::new(2, 2)), c)?;
        }
        Ok(())
    }
}

// ------------------------------------------------------------------ what can be shown

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Good,
    Bad,
    Warn,
}

/// One block of an approval screen, drawn top to bottom until the space runs out.
#[derive(Clone, Debug)]
pub enum Row {
    /// Label above a full bech32 address (`qor1...`), in two lines of groups of four.
    Addr(&'static str, String),
    /// Label left, value right, one line.
    Kv(&'static str, String),
    /// Wrapped text (a message).
    Text(String),
    /// Amber warning line.
    Warn(String),
}

#[derive(Clone, Debug)]
pub struct Approval {
    /// Right side of the navbar ("SEND", "MESSAGE", ...).
    pub tag: &'static str,
    /// Small line under the navbar ("Send", "Contract call", ...).
    pub heading: &'static str,
    /// Big seven-segment value and its unit ("0.0100", "ETH").
    pub big: Option<(String, &'static str)>,
    pub rows: Vec<Row>,
}

/// Look of a button hint on an input screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn {
    Neutral,
    Good,
    Bad,
}

/// The middle of an input screen.
#[derive(Clone, Debug)]
pub enum Body {
    /// PIN boxes: `entered` of `total` filled (the digits themselves are never shown), and an
    /// optional line under them.
    Pin { entered: u8, total: u8, note: Option<(Tone, String)> },
    /// Numbered recovery words.
    Words(Vec<(usize, String)>),
    /// Lines of text, wrapped; a tone colours a line (None = normal ink).
    Text(Vec<(Option<Tone>, String)>),
    /// One big word, a small line above and one below (word-by-word entry).
    Pick { above: String, word: String, below: String },
}

/// A screen that waits for a button: the left one (0, the left hint) or the right one (1, the right hint).
#[derive(Clone, Debug)]
pub struct Input {
    pub tag: &'static str,
    pub heading: String,
    pub body: Body,
    pub left: (String, Btn),
    pub right: (String, Btn),
    /// A PIN entry screen (the display link reports it apart from other input screens).
    pub pin: bool,
}

/// What the home screen knows about the shown account, from the app (`SET_HOME`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Home {
    /// Balance in uqor (1 QOR = 10^6 uqor), as the app read it on chain.
    pub balance: Option<u128>,
    /// The account's bech32 address, as the app sent it.
    pub address: Option<String>,
    /// `Some(true)`: the device computed the same address from its own key; `Some(false)`: it
    /// did not (the address is not shown); `None`: not checked.
    pub address_ok: Option<bool>,
    /// The chain the app is on (`qorechain-vladi`, `qorechain-diana`, `qorechain-local`).
    pub network: String,
    /// The app has sent the home screen at least once since the device started.
    pub app_seen: bool,
}

/// `uqor` as QOR with 4 decimals (truncated), for the seven-segment balance.
pub fn qor4(uqor: u128) -> String {
    format!("{}.{:04}", uqor / 1_000_000, (uqor % 1_000_000) / 100)
}

/// Mainnet moves real funds: screens say so.
pub const MAINNET: &str = "qorechain-vladi";

#[derive(Clone, Debug)]
pub enum Screen {
    Boot,
    Idle { account: u32, home: Home, trust: crate::security::Trust, passphrase: bool },
    Input(Input),
    Approval(Approval),
    Result { tone: Tone, word: std::borrow::Cow<'static, str>, lines: Vec<String> },
    /// Work in progress on the chip (signing): a band like a result, plus a progress bar.
    Working { word: &'static str, lines: Vec<String>, permille: u16 },
}

// ------------------------------------------------------------------ drawing helpers

const M: i32 = 12; // side margin
const NAV_H: i32 = 26;
const ACTIONS_Y: i32 = 205; // top of the left / right button hints
const BAR_Y: i32 = 196; // countdown bar

fn fill(f: &mut Frame, x: i32, y: i32, w: i32, h: i32, c: Pal) {
    if w > 0 && h > 0 {
        f.fill_solid(&Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32)), c).ok();
    }
}

fn text_at(f: &mut Frame, s: &str, font: &MonoFont, c: Pal, x: i32, y: i32, align: Alignment) {
    let style = MonoTextStyle::new(font, c);
    let ts = TextStyleBuilder::new().alignment(align).baseline(Baseline::Top).build();
    Text::with_text_style(s, Point::new(x, y), style, ts).draw(f).ok();
}

fn text_w(s: &str, font: &MonoFont) -> i32 {
    s.chars().count() as i32 * (font.character_size.width + font.character_spacing) as i32
}

fn navbar(f: &mut Frame, tag: &str) {
    fill(f, 0, 0, WIDTH as i32, NAV_H, WHITE);
    text_at(f, "QOREBIT", &FONT_9X18_BOLD, BG, M - 2, 4, Alignment::Left);
    text_at(f, tag, &FONT_7X13, BG, WIDTH as i32 - M + 2, 7, Alignment::Right);
}

fn divider(f: &mut Frame, y: i32) {
    fill(f, M, y, WIDTH as i32 - 2 * M, 1, LINE);
}

/// Wrap `s` to lines of at most `cols` characters, breaking at spaces where possible.
fn wrap(s: &str, cols: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > cols {
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
            }
            out.push(word.drain(..cols).collect());
        }
        let word: String = word.into_iter().collect();
        let need = line.chars().count() + word.chars().count() + usize::from(!line.is_empty());
        if need > cols && !line.is_empty() {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

// ------------------------------------------------------------------ seven-segment digits

/// Segments a..g lit for a character (bit 0 = a ... bit 6 = g).
fn segments(c: char) -> Option<u8> {
    Some(match c {
        '0' | 'O' => 0b0111111,
        '1' => 0b0000110,
        '2' => 0b1011011,
        '3' => 0b1001111,
        '4' => 0b1100110,
        '5' | 'S' => 0b1101101,
        '6' => 0b1111101,
        '7' => 0b0000111,
        '8' => 0b1111111,
        '9' => 0b1101111,
        'A' | 'a' => 0b1110111,
        'B' | 'b' => 0b1111100,
        'C' | 'c' => 0b0111001,
        'D' | 'd' => 0b1011110,
        'E' | 'e' => 0b1111001,
        'F' | 'f' => 0b1110001,
        '-' => 0b1000000,
        ' ' => 0,
        _ => return None,
    })
}

#[derive(Clone, Copy)]
struct Seg {
    h: i32, // digit height
    w: i32, // digit width
    t: i32, // stroke
    gap: i32,
    /// Fixed-width digits (a clock with unlit "8"s behind); otherwise a "1" is narrow, so
    /// amounts don't read as "0.0 100".
    fixed: bool,
}

impl Seg {
    fn new(h: i32) -> Self {
        let t = (h / 7).max(3);
        Seg { h, w: h * 11 / 20, t, gap: (t / 4).max(1), fixed: false }
    }

    /// Horizontal advance of a character.
    fn adv(&self, c: char) -> i32 {
        match c {
            '.' | ':' => self.t + self.t * 2 / 3 + 2,
            '1' if !self.fixed => 2 * self.t + self.t / 2,
            _ => self.w + self.t,
        }
    }

    fn width(&self, s: &str) -> i32 {
        s.chars().map(|c| self.adv(c)).sum::<i32>() - self.t
    }

    /// A segment as a hexagon between two points on its centre line.
    fn bar(&self, f: &mut Frame, x0: i32, y0: i32, x1: i32, y1: i32, c: Pal) {
        let h = self.t / 2;
        let style = PrimitiveStyle::with_fill(c);
        if y0 == y1 {
            // horizontal
            let (a, b) = (x0 + self.gap, x1 - self.gap);
            fill(f, a + h, y0 - h, b - a - 2 * h + 1, self.t, c);
            Triangle::new(Point::new(a, y0), Point::new(a + h, y0 - h), Point::new(a + h, y0 - h + self.t - 1))
                .into_styled(style)
                .draw(f)
                .ok();
            Triangle::new(Point::new(b, y0), Point::new(b - h, y0 - h), Point::new(b - h, y0 - h + self.t - 1))
                .into_styled(style)
                .draw(f)
                .ok();
        } else {
            let (a, b) = (y0 + self.gap, y1 - self.gap);
            fill(f, x0 - h, a + h, self.t, b - a - 2 * h + 1, c);
            Triangle::new(Point::new(x0, a), Point::new(x0 - h, a + h), Point::new(x0 - h + self.t - 1, a + h))
                .into_styled(style)
                .draw(f)
                .ok();
            Triangle::new(Point::new(x0, b), Point::new(x0 - h, b - h), Point::new(x0 - h + self.t - 1, b - h))
                .into_styled(style)
                .draw(f)
                .ok();
        }
    }

    fn digit(&self, f: &mut Frame, x: i32, y: i32, bits: u8, on: Pal, off: Pal) {
        let h = self.t / 2;
        let (l, r) = (x + h, x + self.w - h);
        let (top, mid, bot) = (y + h, y + self.h / 2, y + self.h - h);
        let segs = [
            (l, top, r, top),
            (r, top, r, mid),
            (r, mid, r, bot),
            (l, bot, r, bot),
            (l, mid, l, bot),
            (l, top, l, mid),
            (l, mid, r, mid),
        ];
        for (i, &(x0, y0, x1, y1)) in segs.iter().enumerate() {
            let c = if bits >> i & 1 == 1 { on } else { off };
            if c != BG {
                self.bar(f, x0, y0, x1, y1, c);
            }
        }
    }

    /// Draw `s` with its top-left at (x, y). Unlit segments are drawn in `off` (the faint
    /// "88:88" of a real LCD); pass BG to skip them.
    fn draw(&self, f: &mut Frame, s: &str, x: i32, y: i32, on: Pal, off: Pal) {
        let mut x = x;
        for c in s.chars() {
            match c {
                '.' => fill(f, x, y + self.h - self.t, self.t, self.t, on),
                ':' => {
                    fill(f, x, y + self.h * 3 / 10 - self.t / 2, self.t, self.t, on);
                    fill(f, x, y + self.h * 7 / 10 - self.t / 2, self.t, self.t, on);
                }
                // narrow "1": its right-hand segments drawn at the left edge
                '1' if !self.fixed => self.digit(f, x + self.t + self.t / 4 - self.w, y, segments('1').unwrap(), on, BG),
                c => self.digit(f, x, y, segments(c).unwrap_or(0), on, off),
            }
            x += self.adv(c);
        }
    }
}

/// The largest seven-segment size (from `sizes`) at which `s` fits in `max_w`.
fn fit_seg(s: &str, max_w: i32, sizes: &[i32]) -> Seg {
    sizes.iter().map(|&h| Seg::new(h)).find(|g| g.width(s) <= max_w).unwrap_or(Seg::new(*sizes.last().unwrap()))
}

// ------------------------------------------------------------------ screens

fn idle(f: &mut Frame, account: u32, home: &Home, trust: crate::security::Trust, passphrase: bool) {
    navbar(f, "");
    // accounts are numbered from 1, as in the companion app
    text_at(f, &format!("Account {}", account + 1), &FONT_7X13, MUTED, M, 36, Alignment::Left);
    if !home.network.is_empty() {
        let c = if home.network == MAINNET { AMBER } else { MUTED };
        text_at(f, &home.network, &FONT_7X13, c, WIDTH as i32 - M, 36, Alignment::Right);
    }
    // the balance, large, with its unit
    let (s, on) = match home.balance {
        Some(w) => (qor4(w), INK),
        None => ("-.----".to_string(), MUTED),
    };
    let unit_w = 3 * 7 + 6;
    let seg = fit_seg(&s, WIDTH as i32 - 2 * M - unit_w, &[52, 44, 36, 28]);
    seg.draw(f, &s, M, 56, on, BG);
    text_at(f, "QOR", &FONT_7X13, MUTED, M + seg.width(&s) + 6, 56 + seg.h - 13, Alignment::Left);

    divider(f, 124);
    text_at(f, "Address", &FONT_6X10, MUTED, M, 134, Alignment::Left);
    match (&home.address, home.address_ok) {
        (Some(_), Some(false)) => {
            text_at(f, "The app's address is not", &FONT_7X13, RED, M, 150, Alignment::Left);
            text_at(f, "this device's account", &FONT_7X13, RED, M, 168, Alignment::Left);
        }
        (Some(a), _) => address(f, a, 150),
        (None, _) => text_at(f, "-", &FONT_7X13, GHOST, M, 150, Alignment::Left),
    }

    // one status line, only when there is something to say
    use crate::security::Trust;
    let status = match trust {
        Trust::Attacked(n) => Some((RED, format!("{n} glitch/tamper alarm{}", if n == 1 { "" } else { "s" }))),
        _ if !home.app_seen => Some((MUTED, "Waiting for the Qorebit app".to_string())),
        _ if passphrase => Some((AMBER, "Passphrase wallet".to_string())),
        _ => None,
    };
    if let Some((color, line)) = status {
        fill(f, M, 222, 6, 6, color);
        text_at(f, &line, &FONT_6X10, color, M + 12, 220, Alignment::Left);
    }
}

fn boot(f: &mut Frame) {
    navbar(f, "");
    text_at(f, "Starting", &FONT_9X18_BOLD, INK, WIDTH as i32 / 2, 100, Alignment::Center);
    text_at(f, "deriving the signing key", &FONT_6X10, MUTED, WIDTH as i32 / 2, 124, Alignment::Center);
}

/// A bech32 address in two lines of groups of four characters, alternate groups tinted so the
/// eye can follow it ("qor1 r5v5 srda 7xfth3 ..."): 6 groups, then the rest.
fn address(f: &mut Frame, a: &str, y: i32) {
    let groups: Vec<&str> = a.as_bytes().chunks(4).map(|c| core::str::from_utf8(c).unwrap_or("?")).collect();
    let gw = text_w("0000", &FONT_7X13) + 6;
    for (i, g) in groups.iter().enumerate() {
        let (line, col) = (i as i32 / 6, i as i32 % 6);
        let c = if i % 2 == 0 { INK } else { ADDR2 };
        text_at(f, g, &FONT_7X13, c, M + col * gw, y + line * 15, Alignment::Left);
    }
}

fn approval(f: &mut Frame, a: &Approval, left: u8, timeout: u8) {
    navbar(f, a.tag);
    let need = |row: &Row| match row {
        Row::Addr(..) => 41,
        Row::Text(t) => 15 * wrap(t, 30).len().clamp(1, 5) as i32 + 2,
        _ => 15,
    };
    let limit = BAR_Y - 6;
    let mut y = NAV_H + 8;
    // a heading starting with BLIND warns that the screen can't show everything being signed
    let heading_color = if a.heading.starts_with("BLIND") { AMBER } else { MUTED };
    text_at(f, a.heading, &FONT_7X13, heading_color, M, y, Alignment::Left);
    y += 18;
    if let Some((value, unit)) = &a.big {
        // the amount gets the height the rows leave, within 18..=44 px
        let rows_h: i32 = a.rows.iter().map(need).sum();
        let room = limit - y - 17 - rows_h;
        let sizes: Vec<i32> = [44, 40, 36, 32, 28, 24, 20, 18].into_iter().filter(|&h| h <= room).collect();
        let sizes = if sizes.is_empty() { vec![18] } else { sizes };
        let unit_w = text_w(unit, &FONT_9X18_BOLD);
        let seg = fit_seg(value, WIDTH as i32 - 2 * M - unit_w - 8, &sizes);
        seg.draw(f, value, M, y, INK, BG);
        let ux = M + seg.width(value) + 8;
        text_at(f, unit, &FONT_9X18_BOLD, MUTED, ux, y + seg.h - 17, Alignment::Left);
        y += seg.h + 10;
    }
    divider(f, y);
    y += 7;
    for row in &a.rows {
        let need = need(row);
        if y + need > limit {
            text_at(f, "...", &FONT_7X13, MUTED, M, y.min(limit - 13), Alignment::Left);
            break;
        }
        match row {
            Row::Addr(label, addr) => {
                text_at(f, label, &FONT_6X10, MUTED, M, y, Alignment::Left);
                address(f, addr, y + 13);
            }
            Row::Kv(k, v) => {
                text_at(f, k, &FONT_7X13, MUTED, M, y, Alignment::Left);
                text_at(f, v, &FONT_7X13, INK, WIDTH as i32 - M, y, Alignment::Right);
            }
            Row::Text(t) => {
                let lines = wrap(t, 30);
                fill(f, M, y, 2, 15 * lines.len().clamp(1, 5) as i32 - 2, LINE);
                for (i, l) in lines.iter().take(5).enumerate() {
                    let l = if i == 4 && lines.len() > 5 { format!("{}...", &l[..l.len().min(26)]) } else { l.clone() };
                    text_at(f, &l, &FONT_7X13, INK, M + 8, y + i as i32 * 15, Alignment::Left);
                }
            }
            Row::Warn(t) => {
                fill(f, M, y + 3, 6, 6, AMBER);
                text_at(f, t, &FONT_7X13, AMBER, M + 12, y, Alignment::Left);
            }
        }
        y += need;
    }

    // countdown: shrinking bar, green, then amber, then red
    let full = WIDTH as i32 - 2 * M - 30;
    let w = if timeout > 0 { full * left as i32 / timeout as i32 } else { 0 };
    let tone = match left {
        0..=9 => RED,
        10..=19 => AMBER,
        _ => GREEN,
    };
    fill(f, M, BAR_Y, full, 4, LINE);
    fill(f, M, BAR_Y, w, 4, tone);
    text_at(f, &format!("{left}s"), &FONT_6X10, tone, WIDTH as i32 - M, BAR_Y - 3, Alignment::Right);

    // the left button rejects, the right one approves
    buttons(f, ("REJECT", Btn::Bad), ("APPROVE", Btn::Good));
}

/// The two buttons: "LEFT" and "RIGHT", each with what it does here.
fn buttons(f: &mut Frame, left: (&str, Btn), right: (&str, Btn)) {
    let bw = (WIDTH as i32 - 2 * 8 - 8) / 2;
    for (x, tag, (label, b)) in [(8, "LEFT", left), (8 + bw + 8, "RIGHT", right)] {
        let (fg, bg) = match b {
            Btn::Good => (GREEN, GREEN_D),
            Btn::Bad => (RED, RED_D),
            Btn::Neutral => (INK, LINE),
        };
        RoundedRectangle::new(
            Rectangle::new(Point::new(x, ACTIONS_Y), Size::new(bw as u32, 32)),
            CornerRadii::new(Size::new(6, 6)),
        )
        .into_styled(PrimitiveStyle::with_fill(bg))
        .draw(f)
        .ok();
        text_at(f, tag, &FONT_6X10, MUTED, x + bw / 2, ACTIONS_Y + 3, Alignment::Center);
        text_at(f, label, &FONT_7X13, fg, x + bw / 2, ACTIONS_Y + 15, Alignment::Center);
    }
}

fn tone_color(t: Tone) -> Pal {
    match t {
        Tone::Good => GREEN,
        Tone::Bad => RED,
        Tone::Warn => AMBER,
    }
}

fn input(f: &mut Frame, i: &Input) {
    navbar(f, i.tag);
    let mut y = NAV_H + 8;
    for line in wrap(&i.heading, 30).iter().take(2) {
        text_at(f, line, &FONT_7X13, MUTED, M, y, Alignment::Left);
        y += 15;
    }
    y += 6;
    match &i.body {
        Body::Pin { entered, total, note } => {
            let n = *total as i32;
            let (bw, gap) = (22, 4);
            let x0 = (WIDTH as i32 - (n * bw + (n - 1) * gap)) / 2;
            let by = y + 22;
            for k in 0..n {
                let x = x0 + k * (bw + gap);
                let filled = k < *entered as i32;
                RoundedRectangle::new(Rectangle::new(Point::new(x, by), Size::new(bw as u32, 30)), CornerRadii::new(Size::new(4, 4)))
                    .into_styled(PrimitiveStyle::with_fill(if filled { GHOST } else { BG }))
                    .draw(f)
                    .ok();
                RoundedRectangle::new(Rectangle::new(Point::new(x, by), Size::new(bw as u32, 30)), CornerRadii::new(Size::new(4, 4)))
                    .into_styled(PrimitiveStyle::with_stroke(if k == *entered as i32 { INK } else { LINE }, 2))
                    .draw(f)
                    .ok();
                if filled {
                    fill(f, x + bw / 2 - 3, by + 12, 6, 6, INK);
                }
            }
            text_at(f, "left = 0    right = 1", &FONT_6X10, MUTED, WIDTH as i32 / 2, by + 44, Alignment::Center);
            if let Some((tone, t)) = note {
                for (k, l) in wrap(t, 30).iter().take(2).enumerate() {
                    text_at(f, l, &FONT_7X13, tone_color(*tone), WIDTH as i32 / 2, by + 66 + k as i32 * 15, Alignment::Center);
                }
            }
        }
        Body::Words(words) => {
            for (k, (n, w)) in words.iter().take(6).enumerate() {
                let wy = y + k as i32 * 23;
                text_at(f, &format!("{n:>2}"), &FONT_9X18_BOLD, MUTED, M, wy, Alignment::Left);
                text_at(f, w, &FONT_9X18_BOLD, INK, M + 34, wy, Alignment::Left);
            }
        }
        Body::Text(lines) => {
            for (tone, t) in lines {
                for l in wrap(t, 30) {
                    if y > ACTIONS_Y - 16 {
                        break;
                    }
                    text_at(f, &l, &FONT_7X13, tone.map(tone_color).unwrap_or(INK), M, y, Alignment::Left);
                    y += 15;
                }
                y += 5;
            }
        }
        Body::Pick { above, word, below } => {
            text_at(f, above, &FONT_7X13, MUTED, WIDTH as i32 / 2, y + 6, Alignment::Center);
            let w2 = text_w(word, &FONT_9X18_BOLD);
            let mut d = Double(f, Point::new((WIDTH as i32 - 2 * w2) / 2, y + 32));
            Text::with_baseline(word, Point::zero(), MonoTextStyle::new(&FONT_9X18_BOLD, INK), Baseline::Top).draw(&mut d).ok();
            for (k, l) in wrap(below, 30).iter().take(3).enumerate() {
                text_at(f, l, &FONT_6X10, MUTED, WIDTH as i32 / 2, y + 82 + k as i32 * 12, Alignment::Center);
            }
        }
    }
    buttons(f, (&i.left.0, i.left.1), (&i.right.0, i.right.1));
}

fn result(f: &mut Frame, tone: Tone, word: &str, lines: &[String]) {
    let (fg, bg) = match tone {
        Tone::Good => (GREEN, GREEN_D),
        Tone::Bad => (RED, RED_D),
        Tone::Warn => (AMBER, AMBER_D),
    };
    navbar(f, "");
    // a full-width band in the result colour, the word at 2x
    fill(f, 0, 62, WIDTH as i32, 64, bg);
    fill(f, 0, 62, WIDTH as i32, 3, fg);
    fill(f, 0, 123, WIDTH as i32, 3, fg);
    let w2 = text_w(word, &FONT_9X18_BOLD);
    let mut d = Double(f, Point::new((WIDTH as i32 - 2 * w2) / 2, 76));
    Text::with_baseline(word, Point::zero(), MonoTextStyle::new(&FONT_9X18_BOLD, fg), Baseline::Top).draw(&mut d).ok();
    for (i, l) in lines.iter().take(4).enumerate() {
        let c = if i == 0 { INK } else { MUTED };
        text_at(f, l, &FONT_7X13, c, WIDTH as i32 / 2, 142 + i as i32 * 18, Alignment::Center);
    }
}

fn working(f: &mut Frame, word: &str, lines: &[String], permille: u16) {
    result(f, Tone::Good, word, lines);
    // the progress bar under the lines, and the percentage
    let (x, y, w) = (M + 8, 196, WIDTH as i32 - 2 * (M + 8));
    fill(f, x, y, w, 8, LINE);
    fill(f, x, y, w * permille.min(1000) as i32 / 1000, 8, GREEN);
    text_at(f, &format!("{}%", permille / 10), &FONT_7X13, MUTED, WIDTH as i32 / 2, y + 14, Alignment::Center);
}

/// Draw `screen` into a fresh frame. `left` / `timeout`: approval countdown in seconds.
pub fn render(screen: &Screen, left: u8, timeout: u8) -> Frame {
    let mut f = Frame::new();
    match screen {
        Screen::Boot => boot(&mut f),
        Screen::Idle { account, home, trust, passphrase } => {
            idle(&mut f, *account, home, *trust, *passphrase)
        }
        Screen::Input(i) => input(&mut f, i),
        Screen::Approval(a) => approval(&mut f, a, left, timeout),
        Screen::Result { tone, word, lines } => result(&mut f, *tone, word, lines),
        Screen::Working { word, lines, permille } => working(&mut f, word, lines, *permille),
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDR: &str = "qor1r5v5srda7xfth3hn2s26txvrcrntldju0qscza";

    fn demo_home() -> Home {
        Home {
            balance: Some(1_234_567_000),
            address: Some(ADDR.to_string()),
            address_ok: Some(true),
            network: "qorechain-local".into(),
            app_seen: true,
        }
    }

    #[test]
    fn balances() {
        assert_eq!(qor4(1_234_567_000), "1234.5670");
        assert_eq!(qor4(0), "0.0000");
        assert_eq!(qor4(50), "0.0000");
        assert_eq!(qor4(500), "0.0005");
    }

    fn decode(enc: &[u8], n: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < enc.len() {
            let (c, r) = (enc[i] >> 4, enc[i] & 15);
            let run = if r == 15 {
                i += 1;
                16 + enc[i] as usize
            } else {
                r as usize + 1
            };
            out.extend(std::iter::repeat(c).take(run));
            i += 1;
        }
        assert_eq!(out.len(), n);
        out
    }

    #[test]
    fn rle_round_trip_and_diff() {
        let a = render(&Screen::Idle { account: 0, home: Home::default(), trust: crate::security::Trust::Secure, passphrase: false }, 0, 60);
        let enc = a.encode_rows(0, HEIGHT as usize);
        assert_eq!(decode(&enc, FRAME_LEN), a.0);
        assert!(enc.len() < 12_000, "idle frame is {} bytes", enc.len());
        let ap = Approval { tag: "SEND", heading: "Send", big: Some(("1.5".into(), "QOR")), rows: vec![] };
        let b = render(&Screen::Approval(ap.clone()), 59, 60);
        let c = render(&Screen::Approval(ap), 58, 60);
        let (y0, y1) = c.changed_rows(&b).unwrap();
        assert!(y0 >= BAR_Y as usize - 4 && y1 <= BAR_Y as usize + 8, "countdown changes only its rows: {y0}..{y1}");
    }

    #[test]
    fn an_address_fits_its_two_lines() {
        // 43 characters in groups of four: 6 per line at 34 px, inside the 216 px between margins
        assert_eq!(ADDR.len(), 42);
        assert!(6 * (text_w("0000", &FONT_7X13) + 6) - 6 <= WIDTH as i32 - 2 * M);
        assert!(ADDR.as_bytes().chunks(4).count() <= 12);
    }

    /// `QOREBIT_SCREENS=dir cargo test dump_screens` writes every screen as a PPM for review.
    #[test]
    fn dump_screens() {
        let Ok(dir) = std::env::var("QOREBIT_SCREENS") else { return };
        let to = "qor1w4rn5hw6gz5pxzpdwdsrvw5mrmf9h7wwkt9hca".to_string();
        let send = Approval {
            tag: "SEND",
            heading: "Send",
            big: Some(("1.234567".into(), "QOR")),
            rows: vec![
                Row::Addr("To", to.clone()),
                Row::Kv("From", "Account 1".into()),
                Row::Kv("Max fee", "0.05 QOR".into()),
                Row::Kv("Network", "qorechain-local".into()),
            ],
        };
        let mainnet = Approval {
            tag: "SEND",
            heading: "Send",
            big: Some(("250".into(), "QOR")),
            rows: vec![
                Row::Warn("MAINNET: real funds".into()),
                Row::Addr("To", to.clone()),
                Row::Kv("From", "Account 1".into()),
                Row::Kv("Network", "qorechain-vladi".into()),
            ],
        };
        let stake = Approval {
            tag: "STAKE",
            heading: "Delegate to a validator",
            big: Some(("250".into(), "QOR")),
            rows: vec![
                Row::Kv("Validator", "qorvaloper1erha..jns5y3ka".into()),
                Row::Kv("From", "Account 1".into()),
                Row::Kv("Max fee", "0.05 QOR".into()),
                Row::Kv("Network", "qorechain-diana".into()),
            ],
        };
        let register = Approval {
            tag: "REGISTER",
            heading: "Register your post-quantum key",
            big: None,
            rows: vec![
                Row::Addr("Account", ADDR.to_string()),
                Row::Kv("Key", "ML-DSA-87".into()),
                Row::Kv("Signed", "classical only".into()),
                Row::Kv("Network", "qorechain-diana".into()),
            ],
        };
        let blind = Approval {
            tag: "BLIND",
            heading: "BLIND: a message the device can't show",
            big: None,
            rows: vec![
                Row::Kv("Type", "MsgPostRollupData".into()),
                Row::Kv("Size / id", "1204 bytes / 9f3a01c2".into()),
                Row::Kv("From", "Account 1".into()),
                Row::Kv("Network", "qorechain-diana".into()),
            ],
        };
        let pin = Screen::Input(Input {
            tag: "PIN",
            heading: "Your PIN, to sign".into(),
            body: Body::Pin { entered: 3, total: 8, note: Some((Tone::Warn, "4 tries left before signing locks".into())) },
            left: ("0".into(), Btn::Neutral),
            right: ("1".into(), Btn::Neutral),
            pin: true,
        });
        let shots = [
            ("1-idle", Screen::Idle { account: 0, home: demo_home(), trust: crate::security::Trust::Secure, passphrase: false }, 0),
            ("2-send", Screen::Approval(send), 47),
            ("3-mainnet-send", Screen::Approval(mainnet), 40),
            ("4-delegate", Screen::Approval(stake), 33),
            ("5-register", Screen::Approval(register), 52),
            ("6-blind", Screen::Approval(blind), 12),
            ("7-approved", Screen::Result { tone: Tone::Good, word: "APPROVED".into(), lines: vec!["Signing on the chip".into(), "ML-DSA-87 + secp256k1".into()] }, 0),
            ("8-signed", Screen::Result { tone: Tone::Good, word: "SIGNED".into(), lines: vec!["12 ms on the chip".into(), "Checked before release".into()] }, 0),
            ("9-rejected", Screen::Result { tone: Tone::Bad, word: "REJECTED".into(), lines: vec!["Nothing was signed".into()] }, 0),
            ("10-timeout", Screen::Result { tone: Tone::Warn, word: "TIMED OUT".into(), lines: vec!["No decision in 60 s".into(), "Nothing was signed".into()] }, 0),
            ("11-boot", Screen::Boot, 0),
            ("12-pin", pin, 0),
            ("13-idle-waiting", Screen::Idle { account: 0, home: Home::default(), trust: crate::security::Trust::Developer, passphrase: false }, 0),
            (
                "14-restore-pick",
                Screen::Input(Input {
                    tag: "RESTORE",
                    heading: "Word 3 of 24: where is yours?".into(),
                    body: Body::Pick {
                        above: "between abandon and zoo".into(),
                        word: "lab".into(),
                        below: "left: my word comes earlier. right: it's this word or later".into(),
                    },
                    left: ("< EARLIER".into(), Btn::Neutral),
                    right: ("LATER >".into(), Btn::Neutral),
                    pin: false,
                }),
                0,
            ),
            (
                "15-wipe",
                Screen::Input(Input {
                    tag: "ERASE",
                    heading: "Erase this wallet?".into(),
                    body: Body::Text(vec![
                        (None, "The seed and the PIN are deleted, and a new wallet is made.".into()),
                        (Some(Tone::Bad), "Without your 24 words, the funds on this wallet are lost.".into()),
                    ]),
                    left: ("KEEP".into(), Btn::Good),
                    right: ("ERASE".into(), Btn::Bad),
                    pin: false,
                }),
                0,
            ),
        ];
        for (name, sc, left) in shots {
            let f = render(&sc, left, 60);
            let mut ppm = format!("P6 {WIDTH} {HEIGHT} 255\n").into_bytes();
            for &i in &f.0 {
                ppm.extend_from_slice(&PALETTE[i as usize]);
            }
            std::fs::write(format!("{dir}/{name}.ppm"), ppm).unwrap();
        }
    }

    #[test]
    fn wraps() {
        assert_eq!(wrap("hello world foo", 11), vec!["hello world", "foo"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}
