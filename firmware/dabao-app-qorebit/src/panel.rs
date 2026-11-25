//! The real screen and buttons on the board (cargo feature `panel`): a 240x280 ST7789 IPS
//! panel on SPI and two push buttons.
//!
//! **Untested on hardware**: written against the ST7789 datasheet and xous-core's SH1107 driver
//! (`bao1x-hal/src/sh1107.rs`, same SPI master and pins as the Baosec display); check it once
//! the panel is wired. Until then the default build keeps the virtual display.
//!
//! Wiring (firmware README): SPI2 on PC0 (SCK), PC1 (MOSI), PC3 (CS0); DC on PC2; RES on PB4;
//! backlight on PB5; buttons from PB1 (left, 0 / Reject) and PB2 (right, 1 / Approve) to GND,
//! with the chip's pull-ups.
//!
//! The UI renders the same 16-colour framebuffer as for the virtual display (`screen.rs`); this
//! thread sends the rows that changed and turns button presses into the same inputs as the
//! window's buttons (`Ui::press`).
//!
//! **Custom I/O on the BIO cores (first choice).** One BIO core is the display engine: the CPU
//! sends it 4-bit colour runs (`bioproto.rs`) and the core expands them to RGB565 and clocks
//! the SPI itself, DC and chip select included. Another BIO core is the button engine: it
//! samples both buttons every millisecond, debounces them and reports each press with how
//! long it was held. The CPU never touches a pixel bit or polls a button, so the screen and
//! the buttons keep working while it signs. If the BIO can't be claimed, the uDMA SPI master
//! and CPU polling below take over.

use crate::screen::{Frame, HEIGHT, PALETTE, WIDTH};
use crate::{bioproto, ui};
use bao1x_api::{
    HalOpcode, IoSetup, IoxDir, IoxDriveStrength, IoxEnable, IoxFunction, IoxHal, IoxPort, IoxValue,
    PeriphId, SpimChannel, SERVER_NAME_BAO1X_HAL,
};
use bao1x_hal::udma::{Spim, SpimClkPha, SpimClkPol, SpimCs, SpimEndian};
use std::time::Duration;

const SPI_PORT: IoxPort = IoxPort::PC;
const SCK: u8 = 0;
const MOSI: u8 = 1;
const DC: u8 = 2;
const CS: u8 = 3;
const CTRL_PORT: IoxPort = IoxPort::PB;
const RES: u8 = 4;
const BLK: u8 = 5;
const BTN_LEFT: u8 = 1;
const BTN_RIGHT: u8 = 2;
/// SPI clock of the uDMA path: 4 MHz for bring-up over breadboard jumpers (16 MHz on short wires).
const SPI_HZ: u32 = 4_000_000;
/// Skip the BIO engines: draw through the uDMA SPI and poll the buttons (`panel-udma` feature).
const FORCE_UDMA: bool = cfg!(feature = "panel-udma");
/// Rows per SPI transfer: 16 rows x 240 px x 2 bytes = 7,680 bytes.
const ROWS_PER_CHUNK: usize = 16;
const CHUNK_BYTES: usize = ROWS_PER_CHUNK * WIDTH as usize * 2;
const POLL: Duration = Duration::from_millis(15);
/// SPI clock of the BIO display engine. 2 MHz for bring-up over breadboard jumpers (10 MHz was
/// blank there); raise it once the wiring is on a board or short wires. The engine runs
/// 25 MHz in the board self-test.
const BIO_SCK_HZ: u32 = 2_000_000;
/// Consecutive pressed reads before a press counts (debounce: 3 x 15 ms).
const PRESS_READS: u32 = 3;

struct Panel {
    spim: Spim,
    iox: IoxHal,
}

impl Panel {
    fn new() -> Option<Self> {
        let iox = IoxHal::new();
        let out = |port, pin, f| iox.setup_pin(port, pin, Some(IoxDir::Output), Some(f), None, None, Some(IoxEnable::Enable), Some(IoxDriveStrength::Drive2mA));
        out(SPI_PORT, SCK, IoxFunction::AF2);
        out(SPI_PORT, MOSI, IoxFunction::AF2);
        out(SPI_PORT, CS, IoxFunction::AF2);
        out(SPI_PORT, DC, IoxFunction::Gpio);
        out(CTRL_PORT, RES, IoxFunction::Gpio);
        out(CTRL_PORT, BLK, IoxFunction::Gpio);
        for pin in [BTN_LEFT, BTN_RIGHT] {
            iox.setup_pin(CTRL_PORT, pin, Some(IoxDir::Input), Some(IoxFunction::Gpio), Some(IoxEnable::Enable), Some(IoxEnable::Enable), None, None);
        }
        // ungate the SPI master's clock (bao1x HAL server)
        let xns = xous_names::XousNames::new().ok()?;
        let hal = xns.request_connection_blocking(SERVER_NAME_BAO1X_HAL).ok()?;
        xous::send_message(
            hal,
            xous::Message::new_blocking_scalar(HalOpcode::ConfigureUdmaClock as usize, PeriphId::from(SpimChannel::Channel2) as u32 as usize, 1, 0, 0),
        )
        .ok()?;
        // safety: the clock is on, and the channel is ours (nothing else uses SPI2 on a Dabao)
        let mut spim = unsafe {
            Spim::new(
                SpimChannel::Channel2,
                SPI_HZ,
                bao1x_api::PERCLK / 2,
                SpimClkPol::LeadingEdgeRise,
                SpimClkPha::CaptureOnLeading,
                SpimCs::Cs0,
                0,
                0,
                None,
                CHUNK_BYTES,
                0,
                None,
                None,
            )?
        };
        spim.set_endianness(SpimEndian::MsbFirst);
        Some(Panel { spim, iox })
    }

    fn send(&mut self, data: &[u8]) {
        self.spim.tx_data_async(data, true, false);
        self.spim.tx_data_await(false);
    }

    fn command(&mut self, cmd: u8, args: &[u8]) {
        self.iox.set_gpio_pin_value(SPI_PORT, DC, IoxValue::Low);
        self.send(&[cmd]);
        if !args.is_empty() {
            self.iox.set_gpio_pin_value(SPI_PORT, DC, IoxValue::High);
            self.send(args);
        }
    }

    /// ST7789 power-up: hardware reset, wake, 16-bit colour, inverted (IPS), display on.
    fn init(&mut self) {
        let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::High);
        sleep(5);
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::Low);
        sleep(20);
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::High);
        sleep(150);
        self.command(0x01, &[]); // SWRESET
        sleep(150);
        self.command(0x11, &[]); // SLPOUT
        sleep(120);
        self.command(0x3A, &[0x55]); // COLMOD: 16 bits per pixel
        self.command(0x36, &[0x00]); // MADCTL: top to bottom, left to right, RGB
        self.command(0x21, &[]); // INVON: IPS panels show true colours inverted
        self.command(0x13, &[]); // NORON
        self.command(0x29, &[]); // DISPON
        self.iox.set_gpio_pin_value(CTRL_PORT, BLK, IoxValue::High);
    }

    /// Send (DC, byte) pairs, one SPI transfer per run of equal DC.
    fn wire(&mut self, bytes: &[(bool, u8)]) {
        for run in bytes.chunk_by(|a, b| a.0 == b.0) {
            self.iox.set_gpio_pin_value(SPI_PORT, DC, if run[0].0 { IoxValue::High } else { IoxValue::Low });
            let b: Vec<u8> = run.iter().map(|p| p.1).collect();
            self.send(&b);
        }
    }

    /// Send rows y0..y1 of `f` (to panel rows ROW_OFFSET + y).
    fn draw_rows(&mut self, f: &Frame, y0: usize, y1: usize) {
        let w = WIDTH as usize;
        let rgb565: Vec<[u8; 2]> = PALETTE.iter().map(|c| bioproto::rgb565(*c).to_be_bytes()).collect();
        self.wire(&bioproto::window_bytes(bioproto::GRAM_Y + bioproto::ROW_OFFSET + y0, bioproto::GRAM_Y + bioproto::ROW_OFFSET + y1));
        self.iox.set_gpio_pin_value(SPI_PORT, DC, IoxValue::High);
        let mut buf = Vec::with_capacity(CHUNK_BYTES);
        for y in y0..y1 {
            for &p in &f.0[y * w..(y + 1) * w] {
                buf.extend_from_slice(&rgb565[p as usize & 15]);
            }
            if buf.len() >= CHUNK_BYTES || y + 1 == y1 {
                self.send(&buf);
                buf.clear();
            }
        }
    }

    /// The whole glass in one RGB565 colour.
    fn fill(&mut self, rgb: u16) {
        self.wire(&bioproto::window_bytes(0, bioproto::GRAM_ROWS));
        self.iox.set_gpio_pin_value(SPI_PORT, DC, IoxValue::High);
        let chunk: Vec<u8> = std::iter::repeat_n(rgb.to_be_bytes(), CHUNK_BYTES / 2).flatten().collect();
        let mut left = WIDTH as usize * bioproto::GRAM_ROWS * 2;
        while left > 0 {
            let n = left.min(chunk.len());
            self.send(&chunk[..n]);
            left -= n;
        }
    }

    fn pressed(&self, pin: u8) -> bool {
        matches!(self.iox.get_gpio_pin_value(CTRL_PORT, pin), IoxValue::Low)
    }
}

/// The display and button engines on BIO cores.
struct BioPanel {
    fifos: crate::bioio::Fifos,
    _display: crate::bioio::Core,
    _buttons: crate::bioio::Core,
    iox: IoxHal,
    pending: Vec<bioproto::Press>,
    syncs: u32,
    missed: u32,
}

impl BioPanel {
    fn new() -> Option<Self> {
        let bio = bao1x_hal::bio::Bio::new();
        set_status("claiming BIO FIFOs");
        let fifos = crate::bioio::Fifos::new(&bio).map_err(|e| set_status(format!("BIO FIFOs failed: {e}"))).ok()?;
        set_status("starting the BIO display engine");
        let display = crate::bioio::start_display(BIO_SCK_HZ).map_err(|e| set_status(format!("BIO display failed: {e}"))).ok()?;
        set_status("starting the BIO button engine");
        let buttons = crate::bioio::start_buttons().map_err(|e| set_status(format!("BIO buttons failed: {e}"))).ok()?;
        let iox = IoxHal::new();
        for pin in [RES, BLK] {
            iox.setup_pin(CTRL_PORT, pin, Some(IoxDir::Output), Some(IoxFunction::Gpio), None, None, Some(IoxEnable::Enable), Some(IoxDriveStrength::Drive2mA));
        }
        Some(Self { fifos, _display: display, _buttons: buttons, iox, pending: Vec::new(), syncs: 0, missed: 0 })
    }

    /// Wait until everything queued so far is on the wire (button events that arrive
    /// meanwhile are kept).
    fn sync(&mut self) {
        self.syncs = (self.syncs + 1) & 0xffff;
        let tag = bioproto::OP_SYNC << 28 | self.syncs;
        self.fifos.push2(tag);
        while let Some(w) = self.fifos.pop3(Duration::from_secs(1)) {
            if w == tag {
                return;
            }
            if let Some(p) = bioproto::press_of(w) {
                self.pending.push(p);
            }
        }
        self.missed += 1;
        log::warn!("panel: BIO display engine didn't answer a sync");
    }

    fn command(&mut self, c: u8, args: &[u8]) {
        self.fifos.push2(bioproto::cmd(c));
        for &a in args {
            self.fifos.push2(bioproto::data(a));
        }
    }

    /// The same power-up as `Panel::init`, through the engine.
    fn init(&mut self) {
        let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::High);
        sleep(5);
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::Low);
        sleep(20);
        self.iox.set_gpio_pin_value(CTRL_PORT, RES, IoxValue::High);
        sleep(150);
        for w in bioproto::palette_words() {
            self.fifos.push2(w);
        }
        for (c, args, wait) in [(0x01u8, &[][..], 150), (0x11, &[][..], 120), (0x3A, &[0x55u8][..], 0), (0x36, &[0x00][..], 0), (0x21, &[][..], 0), (0x13, &[][..], 0), (0x29, &[][..], 0)] {
            self.command(c, args);
            if wait > 0 {
                self.sync();
                sleep(wait);
            }
        }
        self.sync();
        self.iox.set_gpio_pin_value(CTRL_PORT, BLK, IoxValue::High);
    }

    fn draw_rows(&mut self, f: &Frame, y0: usize, y1: usize) {
        for w in bioproto::rows_words(f, y0, y1) {
            self.fifos.push2(w);
        }
    }

    /// The whole glass in one RGB565 colour (borrows a palette entry; `restore_palette` after).
    fn fill(&mut self, rgb: u16) {
        for w in bioproto::fill_words(rgb, 0, bioproto::GRAM_ROWS) {
            self.fifos.push2(w);
        }
        self.sync();
    }

    fn restore_palette(&mut self) {
        for w in bioproto::palette_words() {
            self.fifos.push2(w);
        }
    }

    fn presses(&mut self) -> Vec<bioproto::Press> {
        let mut out = std::mem::take(&mut self.pending);
        while let Some(w) = self.fifos.pop3(Duration::ZERO) {
            if let Some(p) = bioproto::press_of(w) {
                out.push(p);
            }
        }
        out
    }
}

/// Wiring check at power-up: red, green, blue and white over the whole frame memory (so it shows
/// whatever the panel's row offset), twice, then black (which also clears the bands above and
/// below the frame on a taller panel). Wrong colours or a
/// shifted picture point at the wiring or the panel settings, before the wallet draws anything.
/// What the panel thread is doing, for `info` (the log only goes to the UART pins).
static STATUS: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

fn set_status(s: impl Into<String>) {
    let s = s.into();
    log::info!("panel: {s}");
    if let Ok(mut g) = STATUS.lock() {
        *g = s;
    }
}

/// The panel thread's last step, for CMD_GET_INFO.
pub fn status() -> String {
    STATUS.lock().map(|g| if g.is_empty() { "not started".into() } else { g.clone() }).unwrap_or_default()
}

const TEST_FILLS: [(&str, u16); 4] = [("red", 0xF800), ("green", 0x07E0), ("blue", 0x001F), ("white", 0xFFFF)];
const TEST_FILL_MS: u64 = 700;
const TEST_ROUNDS: usize = 2;

fn colour_test(mut fill: impl FnMut(u16)) {
    for round in 1..=TEST_ROUNDS {
        for (name, rgb) in TEST_FILLS {
            set_status(format!("colour test {round}/{TEST_ROUNDS}: {name}"));
            fill(rgb);
            std::thread::sleep(Duration::from_millis(TEST_FILL_MS));
        }
    }
    fill(0x0000);
}

fn run_bio(mut panel: BioPanel) -> ! {
    set_status("BIO: ST7789 init");
    panel.init();
    set_status("BIO: colour test");
    colour_test(|rgb| panel.fill(rgb));
    panel.restore_palette();
    let mut frames = 0u32;
    log::info!("panel: ST7789 on a BIO display engine (SPI {} MHz), buttons on a BIO button engine", BIO_SCK_HZ / 1_000_000);
    let mut shown: Option<(u32, Frame)> = None;
    loop {
        let (seq, frame) = ui::ui().frame();
        if shown.as_ref().map(|(s, _)| *s) != Some(seq) {
            let rows = match &shown {
                Some((_, old)) => frame.changed_rows(old),
                None => Some((0, HEIGHT as usize)),
            };
            if let Some((y0, y1)) = rows {
                panel.draw_rows(&frame, y0, y1);
            }
            shown = Some((seq, frame));
            frames += 1;
            set_status(format!("BIO: running, {frames} frames drawn, {} syncs missed", panel.missed));
        }
        for p in panel.presses() {
            set_status(format!("BIO: running, {frames} frames drawn, last press {}", if p.right { "right" } else { "left" }));
            ui::ui().press(p.right);
        }
        std::thread::sleep(POLL);
    }
}

/// Start the panel thread: draws every new frame and reads the two buttons.
pub fn spawn() {
    std::thread::Builder::new()
        .name("panel".into())
        .spawn(|| {
            set_status("thread started");
            if !FORCE_UDMA {
                if let Some(panel) = BioPanel::new() {
                    run_bio(panel);
                }
                log::warn!("panel: BIO engines unavailable: uDMA SPI and CPU-polled buttons");
            }
            let bio_err = if FORCE_UDMA { "uDMA SPI2 (BIO skipped)".to_string() } else { status() };
            let Some(mut panel) = Panel::new() else {
                set_status(format!("{bio_err}; then SPI2 or the HAL server unavailable: no screen"));
                return;
            };
            set_status(format!("{bio_err}; uDMA: ST7789 init"));
            panel.init();
            set_status(format!("{bio_err}; uDMA: colour test"));
            colour_test(|rgb| panel.fill(rgb));
            set_status(format!("{bio_err}; uDMA: running"));
            log::info!("panel: ST7789 240x{} on SPI2, buttons on PB{BTN_LEFT} / PB{BTN_RIGHT}", bioproto::PANEL_ROWS);
            let mut shown: Option<(u32, Frame)> = None;
            // a press counts only after its button was seen released (held at boot: ignored)
            let mut held = [0u32; 2];
            let mut released = [false; 2];
            // what `info` shows: each pin's level right now and the presses counted (wiring check)
            let mut presses = [0u32; 2];
            let mut shown_status = ([false; 2], [0u32; 2]);
            loop {
                let (seq, frame) = ui::ui().frame();
                if shown.as_ref().map(|(s, _)| *s) != Some(seq) {
                    let rows = match &shown {
                        Some((_, old)) => frame.changed_rows(old),
                        None => Some((0, HEIGHT as usize)),
                    };
                    if let Some((y0, y1)) = rows {
                        panel.draw_rows(&frame, y0, y1);
                    }
                    shown = Some((seq, frame));
                }
                let mut low = [false; 2];
                for (k, pin) in [BTN_LEFT, BTN_RIGHT].into_iter().enumerate() {
                    low[k] = panel.pressed(pin);
                    if low[k] {
                        held[k] += 1;
                        if released[k] && held[k] == PRESS_READS {
                            released[k] = false;
                            presses[k] += 1;
                            ui::ui().press(k == 1);
                        }
                    } else {
                        held[k] = 0;
                        released[k] = true;
                    }
                }
                if (low, presses) != shown_status {
                    shown_status = (low, presses);
                    let lvl = |l: bool| if l { "LOW (pressed)" } else { "high" };
                    set_status(format!(
                        "{bio_err}; uDMA: running, left PB{BTN_LEFT} {} / {} presses, right PB{BTN_RIGHT} {} / {} presses",
                        lvl(low[0]), presses[0], lvl(low[1]), presses[1]
                    ));
                }
                crate::link_tick();
                std::thread::sleep(POLL);
            }
        })
        .expect("panel thread");
}
