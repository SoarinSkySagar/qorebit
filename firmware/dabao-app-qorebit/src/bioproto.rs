//! The command words the BIO display engine (`firmware/bio/display/main.c`) takes, and a
//! model of the bytes it puts on the SPI wire. Platform independent, so the encoding is unit
//! tested on the host; the board self-test (`bioio.rs`) checks the real core against the
//! same model, byte for byte.

use crate::screen::{Frame, HEIGHT, PALETTE, WIDTH};

pub const OP_CMD: u32 = 0x1;
pub const OP_DATA: u32 = 0x2;
pub const OP_RUN: u32 = 0x3;
pub const OP_PAL: u32 = 0x4;
#[cfg_attr(not(feature = "panel"), allow(dead_code))]
pub const OP_SYNC: u32 = 0x6;

/// Rows of the panel's glass. The 1.69" 240x280 ST7789 module; 240 for a 240x240 one.
pub const PANEL_ROWS: usize = 280;
/// The 240-row frame sits in the middle of the glass: panel row = frame row + ROW_OFFSET.
pub const ROW_OFFSET: usize = (PANEL_ROWS - HEIGHT as usize) / 2;
/// Rows of the ST7789's frame memory (it has 320 for any panel up to 240x320), and the row where a
/// 240x280 panel's glass starts in it. The Adafruit ST7789 driver uses the same 20 for a 240x280
/// panel; if the picture sits too high or too low, this is the number to tune.
pub const GRAM_ROWS: usize = 320;
pub const GRAM_Y: usize = 20;
/// Palette index the colour fills borrow (the caller reloads the palette afterwards).
#[cfg_attr(not(feature = "panel"), allow(dead_code))]
pub const FILL_INDEX: u32 = 15;

/// CASET over the full width, RASET over frame-memory rows `py0..py1` (16-bit: more than 256
/// rows), RAMWR: (DC, byte) per byte on the wire.
pub fn window_bytes(py0: usize, py1: usize) -> Vec<(bool, u8)> {
    let (w, last) = (WIDTH as usize - 1, py1 - 1);
    vec![
        (false, 0x2A), (true, 0), (true, 0), (true, (w >> 8) as u8), (true, w as u8),
        (false, 0x2B), (true, (py0 >> 8) as u8), (true, py0 as u8), (true, (last >> 8) as u8), (true, last as u8),
        (false, 0x2C),
    ]
}

fn window_words(py0: usize, py1: usize) -> Vec<u32> {
    window_bytes(py0, py1).into_iter().map(|(dc, b)| if dc { data(b) } else { cmd(b) }).collect()
}

/// A button event from the BIO button engine (`firmware/bio/buttons/main.c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Press {
    /// false: left (Reject / 0), true: right (Approve / 1)
    pub right: bool,
    pub held_ms: u32,
}

/// Decode a word from FIFO3: a button event (bit 31 set) or something else.
pub fn press_of(w: u32) -> Option<Press> {
    (w & 0x8000_0000 != 0).then(|| Press { right: (w >> 24) & 1 == 1, held_ms: w & 0x00ff_ffff })
}

pub fn rgb565(c: [u8; 3]) -> u16 {
    (c[0] as u16 >> 3) << 11 | (c[1] as u16 >> 2) << 5 | c[2] as u16 >> 3
}

pub fn cmd(c: u8) -> u32 {
    OP_CMD << 28 | c as u32
}

pub fn data(d: u8) -> u32 {
    OP_DATA << 28 | d as u32
}

/// Load the device palette into the engine.
pub fn palette_words() -> Vec<u32> {
    PALETTE.iter().enumerate().map(|(i, c)| OP_PAL << 28 | (i as u32) << 16 | rgb565(*c) as u32).collect()
}

/// Rows `y0..y1` of `frame`: set the window (at frame-memory rows `GRAM_Y + ROW_OFFSET + y`), start the memory
/// write, then the pixels as colour runs (4-bit index + count), which the engine expands to RGB565.
pub fn rows_words(frame: &Frame, y0: usize, y1: usize) -> Vec<u32> {
    let w = WIDTH as usize;
    let mut out = window_words(GRAM_Y + ROW_OFFSET + y0, GRAM_Y + ROW_OFFSET + y1);
    let px = &frame.0[y0 * w..y1 * w];
    let mut i = 0;
    while i < px.len() {
        let c = px[i];
        let mut n = 1;
        while i + n < px.len() && px[i + n] == c && n < 0xffff {
            n += 1;
        }
        out.push(OP_RUN << 28 | (c as u32 & 15) << 16 | n as u32);
        i += n;
    }
    out
}

/// Frame-memory rows `py0..py1` in one RGB565 colour, through palette entry FILL_INDEX (the panel
/// self-test's colour fills and the black bands above and below the frame).
#[cfg_attr(not(feature = "panel"), allow(dead_code))]
pub fn fill_words(rgb: u16, py0: usize, py1: usize) -> Vec<u32> {
    let mut out = vec![OP_PAL << 28 | FILL_INDEX << 16 | rgb as u32];
    out.extend(window_words(py0, py1));
    let mut left = (py1 - py0) * WIDTH as usize;
    while left > 0 {
        let n = left.min(0xffff);
        out.push(OP_RUN << 28 | FILL_INDEX << 16 | n as u32);
        left -= n;
    }
    out
}

/// What the engine sends for `words`: (DC, byte) per byte on the wire.
pub fn wire_bytes(words: &[u32]) -> Vec<(bool, u8)> {
    let mut pal = [0u16; 16];
    let mut out = Vec::new();
    for &w in words {
        match w >> 28 {
            OP_CMD => out.push((false, w as u8)),
            OP_DATA => out.push((true, w as u8)),
            OP_RUN => {
                let c = pal[(w >> 16) as usize & 15];
                for _ in 0..(w & 0xffff) {
                    out.push((true, (c >> 8) as u8));
                    out.push((true, c as u8));
                }
            }
            OP_PAL => pal[(w >> 16) as usize & 15] = w as u16,
            _ => {}
        }
    }
    out
}

/// The same rows as the uDMA driver (`panel.rs`) sends them: window commands, then every
/// pixel as RGB565 high byte first. The engine must put exactly this on the wire.
pub fn direct_bytes(frame: &Frame, y0: usize, y1: usize) -> Vec<(bool, u8)> {
    let w = WIDTH as usize;
    let mut out = window_bytes(GRAM_Y + ROW_OFFSET + y0, GRAM_Y + ROW_OFFSET + y1);
    for &p in &frame.0[y0 * w..y1 * w] {
        let c = rgb565(PALETTE[p as usize & 15]);
        out.push((true, (c >> 8) as u8));
        out.push((true, c as u8));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screen::{render, Screen};

    #[test]
    fn runs_expand_to_the_direct_pixels() {
        let f = render(&Screen::Boot, 0, 60);
        let mut words = palette_words();
        words.extend(rows_words(&f, 0, 240));
        assert_eq!(wire_bytes(&words), direct_bytes(&f, 0, 240));
        // a few rows mid-frame
        words = palette_words();
        words.extend(rows_words(&f, 100, 103));
        assert_eq!(wire_bytes(&words), direct_bytes(&f, 100, 103));
    }

    #[test]
    fn runs_are_compact() {
        let f = render(&Screen::Boot, 0, 60);
        let words = rows_words(&f, 0, 240);
        // a 115,200-byte frame in a few hundred words: 4-bit colour + run length
        assert!(words.len() < 1000, "{} words", words.len());
    }

    #[test]
    fn frame_rows_sit_in_the_middle_of_the_glass() {
        assert_eq!(ROW_OFFSET + HEIGHT as usize + ROW_OFFSET, PANEL_ROWS);
        let f = render(&Screen::Boot, 0, 60);
        let bytes = direct_bytes(&f, 230, 240);
        // RASET: frame-memory rows 270..279 (GRAM_Y + ROW_OFFSET + 230 ..), both bytes of each
        assert_eq!(&bytes[5..11], &[(false, 0x2B), (true, 1), (true, 14), (true, 1), (true, 23), (false, 0x2C)]);
        // CASET: columns 0..239
        assert_eq!(&bytes[0..5], &[(false, 0x2A), (true, 0), (true, 0), (true, 0), (true, 239)]);
    }

    #[test]
    fn fills_cover_every_pixel() {
        let words = fill_words(0xF800, 0, GRAM_ROWS);
        let wire = wire_bytes(&words);
        let window = window_bytes(0, GRAM_ROWS);
        assert_eq!(&wire[..window.len()], &window[..]);
        let px = &wire[window.len()..];
        assert_eq!(px.len(), 2 * WIDTH as usize * GRAM_ROWS, "every pixel, past one 16-bit run");
        assert!(px.chunks(2).all(|c| c == [(true, 0xF8), (true, 0x00)]));
        // the last frame-memory row (319) needs the high byte of RASET
        assert_eq!(&window[5..10], &[(false, 0x2B), (true, 0), (true, 0), (true, 1), (true, 63)]);
    }

    #[test]
    fn button_events() {
        assert_eq!(press_of(0x8100_0032), Some(Press { right: true, held_ms: 50 }));
        assert_eq!(press_of(0x8000_0320), Some(Press { right: false, held_ms: 800 }));
        assert_eq!(press_of(0x6000_0001), None, "a display sync word is not a press");
    }
}
