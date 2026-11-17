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

/// A screen that waits for the one button: a tap (0, the left hint) or a hold (1, the right hint).
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
const ACTIONS_Y: i32 = 205; // top of the tap / hold hints
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

