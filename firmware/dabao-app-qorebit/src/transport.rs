//! Host transport: the dabao vendor HID interface (`usb-bao1x/generic-hid`, 64-byte
//! reports) on the device, a TCP socket on 127.0.0.1:`QOREBIT_PORT` (default 7610) in the
//! emulator. Frames are handed to the app as bodies; its reply frame is sent back.

use crate::protocol::{self, Status};

pub trait Handler {
    /// Answer one request body with a complete response frame.
    fn handle(&mut self, body: &[u8]) -> Vec<u8>;
}

#[cfg(not(target_os = "xous"))]
pub fn serve(mut app: impl Handler) -> ! {
    use std::io::{Read, Write};
    let port: u16 = std::env::var("QOREBIT_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7610);
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("can't bind the emulator socket");
    log::info!("emulator transport: listening on 127.0.0.1:{port}");
    loop {
        let Ok((mut stream, peer)) = listener.accept() else { continue };
        log::info!("host connected from {peer}");
        loop {
            let mut prefix = [0u8; 4];
            if stream.read_exact(&mut prefix).is_err() {
                break;
            }
            let reply = match protocol::body_len(prefix) {
                Some(n) => {
                    let mut body = vec![0u8; n];
                    if stream.read_exact(&mut body).is_err() {
                        break;
                    }
                    app.handle(&body)
                }
                None => {
                    stream.write_all(&protocol::error(Status::BadRequest, "bad length")).ok();
                    break;
                }
            };
            if stream.write_all(&reply).is_err() {
                break;
            }
        }
        log::info!("host disconnected");
    }
}

/// Gap between HID reports of one reply (endpoint poll interval is 5 ms).
#[cfg(target_os = "xous")]
const REPORT_GAP_MS: u64 = 8;

#[cfg(target_os = "xous")]
pub fn serve(mut app: impl Handler) -> ! {
    use protocol::{Assembled, Reassembler};
    let usb = usb_bao1x::UsbHid::new();
    let mut rx = Reassembler::default();
    log::info!("USB HID transport ready");
    loop {
        let report = match usb.hid_wait_incoming() {
            Ok(r) => r,
            Err(e) => {
                log::warn!("HID receive: {:?}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };
        let reply = match rx.push(&report) {
            Assembled::Pending => continue,
            Assembled::Body(body) => app.handle(&body),
            Assembled::Invalid => protocol::error(Status::BadRequest, "bad length"),
        };
        // usb-bao1x queues a report and writes it to the endpoint at once, dropping it if the
        // previous report hasn't been collected yet (the error is discarded). The endpoint is
        // polled every 5 ms, so multi-report replies must be paced or they lose reports.
        let reports = protocol::to_reports(&reply);
        for (i, r) in reports.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(REPORT_GAP_MS));
            }
            if let Err(e) = usb.hid_send(*r) {
                log::warn!("HID send: {:?}", e);
                break;
            }
        }
    }
}
