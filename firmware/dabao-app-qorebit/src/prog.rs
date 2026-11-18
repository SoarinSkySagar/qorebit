//! The Dabao's PROG button as the wallet's one button.
//!
//! PROG is on PC13 (the boot switch, shared with the USB SE0 control; see
//! `bao1x-hal/src/board/dabao.rs`). After boot `usb-bao1x` leaves PC13 an input with its
//! pull-up, so the pin reads high, and low while PROG is held. This thread polls it through the
//! bao1x HAL server, times each press, and gives the screen shown now the same answer the
//! window's button gives: a tap (shorter than `ui::HOLD_MS`) is 0 / Reject, a hold is 1 /
//! Approve (`Ui::press`). The answer comes when the button is let go.
//!
//! On the Dabao, pressing PROG can also disturb the USB link for a moment (the schematic notes
//! the button disconnects USB); the bridge reconnects on its own and the job on the chip keeps
//! waiting for its answer.

use crate::ui;
use bao1x_api::{IoxHal, IoxPort, IoxValue};
use std::time::{Duration, Instant};

const PORT: IoxPort = IoxPort::PC;
const PIN: u8 = 13;
const POLL: Duration = Duration::from_millis(10);
/// Consecutive low reads that count as a press, high reads as a release (debounce: 3 x 10 ms).
const DEBOUNCE: u32 = 3;
/// Longest press that still counts; a stuck button is not an answer.
const STUCK: Duration = Duration::from_secs(20);

pub fn spawn() {
    std::thread::Builder::new()
        .name("prog-button".into())
        .spawn(|| {
            let iox = IoxHal::new();
            // a press counts only after the button was seen released (held at boot: ignored)
            let mut armed = false;
            let (mut low, mut high) = (0u32, 0u32);
            let mut down: Option<Instant> = None;
            loop {
                let pressed = matches!(iox.get_gpio_pin_value(PORT, PIN), IoxValue::Low);
                if pressed {
                    low += 1;
                    high = 0;
                    if armed && low == DEBOUNCE && down.is_none() {
                        down = Some(Instant::now() - POLL * (DEBOUNCE - 1));
                    }
                } else {
                    high += 1;
                    low = 0;
                    if high == DEBOUNCE {
                        armed = true;
                        if let Some(t) = down.take() {
                            let held = t.elapsed().saturating_sub(POLL * (DEBOUNCE - 1));
                            if held < STUCK {
                                let ms = held.as_millis() as u32;
                                let answered = ui::ui().press(ms);
                                log::info!("PROG held {ms} ms: {}{}", if ms >= ui::HOLD_MS { "hold (1)" } else { "tap (0)" }, if answered { "" } else { ", no screen was waiting" });
                            }
                        }
                    }
                }
                std::thread::sleep(POLL);
            }
        })
        .expect("prog button thread");
}
