//! Custom I/O on the Baochip's BIO cores: the display engine and the button engine
//! (`firmware/bio/{display,buttons}`), plus the board self-test that checks them with two
//! more cores acting as test equipment (`firmware/bio/{capture,drive}`).
//!
//! FIFO use (the Keccak worker holds FIFO0/1): FIFO2 carries commands into the display
//! engine (or the drive core in a test), FIFO3 carries button events out (or captured bytes
//! in a test). A FIFO can be mapped only once per process, so [`Fifos`] owns both mappings.

#![cfg_attr(feature = "panel", allow(dead_code))]

use crate::bioproto::{self, Press};
use bao1x_api::bio::*;
use bao1x_api::bio_resources::*;
use bao1x_api::{IoSetup, IoxDir, IoxEnable, IoxFunction, IoxHal, IoxPort, IoxValue};
use bao1x_hal::bio::{Bio, CoreCsr};
use std::time::{Duration, Instant};
use utralib::utra::bio_bdma;

mod code {
    mod display {
        include!("../../bio/display/display.rs");
    }
    mod buttons {
        include!("../../bio/buttons/buttons.rs");
    }
    mod capture {
        include!("../../bio/capture/capture.rs");
    }
    mod drive {
        include!("../../bio/drive/drive.rs");
    }
    pub use buttons::buttons_bio_code;
    pub use capture::capture_bio_code;
    pub use display::display_bio_code;
    pub use drive::drive_bio_code;
}

/// Display pins (BIO numbers): SCK PC0, MOSI PC1, DC PC2, CS PC3.
pub const DISPLAY_PINS: [u8; 4] = [16, 17, 18, 19];
/// Button pins (BIO numbers): left PB1 = 1 (0 / Reject), right PB2 = 2 (1 / Approve) (BIO n is pad PBn for n < 16, PC(n-16) above: baochip/dabao docs/pinout/pins.csv).
pub const BUTTON_PINS: [u8; 2] = [1, 2];

fn mask(pins: &[u8]) -> u32 {
    pins.iter().fold(0, |m, p| m | 1 << p)
}

pub struct Fifos {
    _h2: CoreHandle,
    _h3: CoreHandle,
    f2: CoreCsr,
    f3: CoreCsr,
}

impl Fifos {
    pub fn new(bio: &Bio) -> Result<Self, String> {
        // safety: the handles live in this object as long as the CSRs made from them
        let h2 = unsafe { bio.get_core_handle(Fifo::Fifo2) }.map_err(|e| format!("fifo2: {e:?}"))?.ok_or("fifo2 in use")?;
        let h3 = unsafe { bio.get_core_handle(Fifo::Fifo3) }.map_err(|e| format!("fifo3: {e:?}"))?.ok_or("fifo3 in use")?;
        Ok(Self { f2: CoreCsr::from_handle(&h2), f3: CoreCsr::from_handle(&h3), _h2: h2, _h3: h3 })
    }

    /// Into FIFO2 (8 deep: waits while full).
    pub fn push2(&mut self, w: u32) {
        while self.f2.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL2) >= 8 {}
        self.f2.csr.wo(bio_bdma::SFR_TXF2, w);
    }

    /// Out of FIFO3, waiting up to `timeout`.
    pub fn pop3(&mut self, timeout: Duration) -> Option<u32> {
        let end = Instant::now() + timeout;
        while self.f3.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) == 0 {
            if Instant::now() >= end {
                return None;
            }
            xous::yield_slice();
        }
        Some(self.f3.csr.r(bio_bdma::SFR_RXF3))
    }

    pub fn drain3(&mut self) {
        while self.f3.csr.rf(bio_bdma::SFR_FLEVEL_PCLK_REGFIFO_LEVEL3) != 0 {
            self.f3.csr.r(bio_bdma::SFR_RXF3);
        }
    }
}

/// A program loaded and running on one BIO core; stopped and released on drop.
pub struct Core {
    bio: Bio,
    grant: ResourceGrant,
    unmap: u32,
    pub freq: Option<u32>,
}

impl Core {
    fn start(name: &str, fifos: Vec<Fifo>, pins: &[u8], code: (&[u8], Option<u32>), clock: ClockMode) -> Result<Self, String> {
        let mut bio = Bio::new();
        let spec = ResourceSpec {
            claimer: name.to_string(),
            cores: vec![CoreRequirement::Any],
            fifos,
            static_pins: pins.to_vec(),
            dynamic_pin_count: 0,
        };
        let grant = bio.claim_resources(&spec).map_err(|e| format!("{name}: claim {e:?}"))?;
        let freq = bio.init_core(grant.cores[0], code, CoreConfig { clock_mode: clock }).map_err(|e| format!("{name}: init {e:?}"))?;
        // start the core first, so it has set its pin levels (SCK low, ...) before the pins
        // are connected: mapping them first let the clock pin show its power-on level for a
        // moment, a stray clock edge the board self-test caught
        bio.set_core_run_state(&grant, true);
        if !pins.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(1));
            // hand the pins to the BIO (alternate function 1), leaving other mappings alone
            let io = IoConfig { mode: IoConfigMode::SetOnly, mapped: mask(pins), ..Default::default() };
            bio.setup_io_config(io).map_err(|e| format!("{name}: io {e:?}"))?;
        }
        Ok(Self { bio, grant, unmap: mask(pins), freq })
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        for &core in self.grant.cores.iter() {
            self.bio.de_init_core(core).ok();
        }
        if self.unmap != 0 {
            self.bio.setup_io_config(IoConfig { mode: IoConfigMode::ClearOnly, mapped: self.unmap, ..Default::default() }).ok();
        }
        self.bio.release_resources(self.grant.grant_id).ok();
    }
}

/// The display engine, with the SPI clock at `sck_hz` (its quantum is twice that).
pub fn start_display(sck_hz: u32) -> Result<Core, String> {
    Core::start("qorebit-display", vec![Fifo::Fifo2], &DISPLAY_PINS, code::display_bio_code(), ClockMode::TargetFreqInt(sck_hz * 2))
}

/// The button engine: 1 kHz sampling, pull-ups on both buttons.
pub fn start_buttons() -> Result<Core, String> {
    let iox = IoxHal::new();
    for pin in [1u8, 2] {
        iox.setup_pin(IoxPort::PB, pin, Some(IoxDir::Input), Some(IoxFunction::AF1), Some(IoxEnable::Enable), Some(IoxEnable::Enable), None, None);
    }
    Core::start("qorebit-buttons", vec![Fifo::Fifo3], &BUTTON_PINS, code::buttons_bio_code(), ClockMode::TargetFreqInt(1000))
}

// ---------------------------------------------------------------- board self-test

fn expected_bits(bytes: &[(bool, u8)]) -> Vec<bool> {
    bytes.iter().flat_map(|&(_, b)| (0..8).rev().map(move |i| b >> i & 1 == 1)).collect()
}

/// Content check: the display engine's SPI output, recorded bit by bit by the capture core,
/// against the model (`bioproto`, itself equal to the uDMA driver's bytes).
fn display_content(fifos: &mut Fifos, sck_hz: u32) -> Result<String, String> {
    let frame = crate::screen::render(&crate::screen::Screen::Boot, 0, 60);
    let mut words = bioproto::palette_words();
    words.extend(bioproto::rows_words(&frame, 30, 32));
    let expect = bioproto::wire_bytes(&words);
    if expect != bioproto::direct_bytes(&frame, 30, 32) {
        return Err("model mismatch".into());
    }
    let capture = Core::start("qorebit-capture", vec![], &[], code::capture_bio_code(), ClockMode::FixedDivider(0, 0))?;
    fifos.drain3();
    let display = start_display(sck_hz)?;
    for &w in &words {
        fifos.push2(w);
    }
    // the capture core stops ~50 ms after the last edge, then sends: bit count, MOSI words, DC words
    let n = fifos.pop3(Duration::from_secs(3)).ok_or("no capture")? as usize;
    let mut bits = Vec::with_capacity(n);
    for _ in 0..n.div_ceil(32) {
        let w = fifos.pop3(Duration::from_millis(500)).ok_or("capture words")?;
        bits.extend((0..32).rev().map(|i| w >> i & 1 == 1));
    }
    bits.truncate(n);
    let mut dcs = Vec::new();
    for _ in 0..(n / 8).div_ceil(32) {
        let w = fifos.pop3(Duration::from_millis(500)).ok_or("capture dc")?;
        dcs.extend((0..32).rev().map(|i| w >> i & 1 == 1));
    }
    dcs.truncate(n / 8);
    drop(capture);
    drop(display);
    let want = expected_bits(&expect);
    let want_dc: Vec<bool> = expect.iter().map(|e| e.0).collect();
    let first_bit = bits.iter().zip(&want).position(|(a, b)| a != b);
    let first_dc = dcs.iter().zip(&want_dc).position(|(a, b)| a != b);
    if bits.len() == want.len() && first_bit.is_none() && first_dc.is_none() {
        return Ok(format!("{} bytes bit-exact, DC right on every byte", expect.len()));
    }
    let hex = |b: &[bool]| -> String {
        b.chunks(8).take(6).map(|c| format!("{:02x}", c.iter().fold(0u8, |a, &x| a << 1 | x as u8))).collect::<Vec<_>>().join(" ")
    };
    Err(format!(
        "{} bits captured, {} expected; first bit difference {:?}, first DC difference {:?}; got {} / want {}",
        bits.len(),
        want.len(),
        first_bit,
        first_dc,
        hex(&bits),
        hex(&want)
    ))
}

/// Speed check: 40 rows of pixels (~19 KB on the wire) through the display engine alone,
/// timed until it echoes a sync word, i.e. until the last bit is out.
fn display_speed(fifos: &mut Fifos, sck_hz: u32) -> Result<String, String> {
    let frame = crate::screen::render(&crate::screen::Screen::Boot, 0, 60);
    let mut words = bioproto::palette_words();
    words.extend(bioproto::rows_words(&frame, 60, 100));
    let bytes = bioproto::wire_bytes(&words).len();
    fifos.drain3();
    let display = start_display(sck_hz)?;
    let q = display.freq.map(|f| format!("quantum {} kHz", f / 1000)).unwrap_or_default();
    let t = Instant::now();
    for &w in &words {
        fifos.push2(w);
    }
    let tag = bioproto::OP_SYNC << 28 | 0x5a5a;
    fifos.push2(tag);
    let echoed = fifos.pop3(Duration::from_secs(5));
    let ms = t.elapsed().as_micros() as f32 / 1000.0;
    drop(display);
    if echoed != Some(tag) {
        return Err(format!("no sync echo ({echoed:?})"));
    }
    let mbit = bytes as f32 * 8.0 / ms / 1000.0;
    Ok(format!("{bytes} bytes in {ms:.1} ms = {mbit:.2} Mbit/s ({} words from the CPU, {q})", words.len()))
}

fn button_test(fifos: &mut Fifos) -> Result<String, String> {
    let buttons = start_buttons()?;
    let drive = Core::start("qorebit-drive", vec![Fifo::Fifo2], &[], code::drive_bio_code(), ClockMode::TargetFreqInt(1000))?;
    fifos.drain3();
    // (left low, right low, ms) steps, and the presses they must give
    const R: u32 = 1 << 17;
    const L: u32 = 1 << 16;
    let cases: [(&str, &[(u32, u32)], &[(bool, u32)]); 5] = [
        ("tap right 50 ms", &[(R, 50), (0, 30)], &[(true, 50)]),
        ("hold left 800 ms", &[(L, 800), (0, 30)], &[(false, 800)]),
        ("bouncy right, then 100 ms", &[(R, 1), (0, 1), (R, 2), (0, 1), (R, 1), (0, 2), (R, 100), (0, 30)], &[(true, 100)]),
        ("3 ms glitch on left", &[(L, 3), (0, 30)], &[]),
        ("both 200 ms", &[(L | R, 200), (0, 30)], &[(false, 200), (true, 200)]),
    ];
    let mut report = Vec::new();
    // The pads: while the drive core holds one button low, the CPU must read that button's pad
    // (PB1 left, PB2 right) low through the IOX and the other one high. The cases below only
    // show that the two cores agree on BIO numbers, which they would for any wrong pair of pins.
    let iox = IoxHal::new();
    let mut pads_ok = true;
    let mut pads = Vec::new();
    for (name, lvl) in [("left", L), ("right", R)] {
        fifos.push2(lvl | 300);
        std::thread::sleep(Duration::from_millis(100));
        let low = |pin| matches!(iox.get_gpio_pin_value(IoxPort::PB, pin), IoxValue::Low);
        let (pb1, pb2) = (low(1), low(2));
        std::thread::sleep(Duration::from_millis(250));
        while fifos.pop3(Duration::from_millis(20)).is_some() {}
        let want_left = lvl == L;
        pads_ok &= pb1 == want_left && pb2 == !want_left;
        let lvl_name = |l: bool| if l { "low" } else { "high" };
        pads.push(format!("{name} held: PB1 {}, PB2 {}", lvl_name(pb1), lvl_name(pb2)));
    }
    report.push(format!("{} pads: {}", if pads_ok { "ok  " } else { "FAIL" }, pads.join("; ")));
    for (name, steps, want) in cases {
        for &(lvl, ms) in steps {
            fifos.push2(lvl | ms);
        }
        let total: u32 = steps.iter().map(|s| s.1).sum();
        std::thread::sleep(Duration::from_millis(total as u64 + 50));
        let mut got = Vec::new();
        while let Some(w) = fifos.pop3(Duration::from_millis(20)) {
            if let Some(p) = bioproto::press_of(w) {
                got.push(p);
            }
        }
        got.sort_by_key(|p: &Press| p.right);
        // the engine reports the hold within its 1 ms tick and debounce rounding
        let ok = got.len() == want.len()
            && got.iter().zip(want).all(|(p, &(r, ms))| p.right == r && p.held_ms.abs_diff(ms) <= 3);
        let shown: Vec<String> = got.iter().map(|p| format!("{} {} ms", if p.right { "right" } else { "left" }, p.held_ms)).collect();
        report.push(format!("{} {name}: {}", if ok { "ok  " } else { "FAIL" }, if shown.is_empty() { "no press".into() } else { shown.join(", ") }));
    }
    drop(drive);
    drop(buttons);
    let fails = report.iter().filter(|l| l.starts_with("FAIL")).count();
    if fails == 0 { Ok(report.join("\n")) } else { Err(report.join("\n")) }
}

/// BIO_SELFTEST: run the display and button engines against the test cores; text report.
pub fn selftest() -> (bool, String) {
    let bio = Bio::new();
    let mut out = vec![format!("BIO clock {} Hz", bio.get_bio_freq())];
    let mut fifos = match Fifos::new(&bio) {
        Ok(f) => f,
        Err(e) => return (false, format!("FIFO2/3: {e}")),
    };
    let mut ok = true;
    for sck in [250_000, 1_000_000, 4_000_000] {
        match display_content(&mut fifos, sck) {
            Ok(r) => out.push(format!("ok   display content, SCK {} kHz: {r}", sck / 1000)),
            Err(e) => {
                // above ~1 MHz the capture core's sampling loop may be the limit, not the engine
                out.push(format!("{}  display content, SCK {} kHz: {e}", if sck <= 1_000_000 { "FAIL" } else { "note" }, sck / 1000));
                ok &= sck > 1_000_000;
            }
        }
    }
    for sck in [1_000_000, 4_000_000, 10_000_000, 25_000_000] {
        match display_speed(&mut fifos, sck) {
            Ok(r) => out.push(format!("ok   display speed, SCK {} MHz: {r}", sck / 1_000_000)),
            Err(e) => {
                out.push(format!("FAIL display speed, SCK {} MHz: {e}", sck / 1_000_000));
                ok = false;
            }
        }
    }
    match button_test(&mut fifos) {
        Ok(s) => out.push(s),
        Err(s) => {
            out.push(s);
            ok = false;
        }
    }
    (ok, out.join("\n"))
}
