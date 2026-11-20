#!/usr/bin/env python3
"""Qorebit virtual display and bridge: the hardware-wallet screen and its one button in a window on
the laptop, for firmware built with `--vdisplay` (the default until a real panel).

The screen is rendered **on the chip** into a 240x240 colour framebuffer (16-colour palette,
ST7789-class panel). The chip sends the rows that changed, run-length encoded, with the
palette; this window only maps indices to colours. The real panel gets the same frame.

**One button.** Tap it (shorter than the chip's hold time) to say 0 / Reject, hold it to say
1 / Approve. The window times the press and sends the length; the chip decides.
Space or the mouse on the button; Escape taps. The Dabao's PROG button is the same button.

The tool is also the bridge:
  * the device: it owns the USB HID connection (or the emulator's TCP socket);
  * 127.0.0.1:7620 for other tools (raw frames);
  * http://127.0.0.1:7630 for the companion app in a browser: `/rpc` (device calls), `/networks`,
    `/proxy/<network>/<rest|rpc>/...` (QoreChain's public endpoints send no CORS headers, so the
    browser reaches mainnet, testnet and the local devnet through here, and only those), and
    `/devnet/fund` (test QOR from the local devnet's faucet).

    tools/.venv/bin/python tools/qorebit_display.py            # Dabao over USB HID
    tools/.venv/bin/python tools/qorebit_display.py --emu      # emulator on 127.0.0.1:7610

DEV ONLY: the laptop draws this screen and sends the button presses, so malware on the laptop
could fake both. The real wallet uses a display and a button on the device.
"""
import argparse, json, os, signal, socket, struct, subprocess, sys, threading, time
import urllib.error, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MSG = 0x51
CMD_VERSION, CMD_ACCOUNT, CMD_PQ_PUBKEY, CMD_REGISTER_PROOF, CMD_INFO, CMD_SIGN_TX = 0x01, 0x02, 0x03, 0x04, 0x05, 0x06
CMD_SECURITY = 0x09
CMD_SHOW_PHRASE, CMD_RESTORE, CMD_WIPE, CMD_PASSPHRASE = 0x0A, 0x0B, 0x0C, 0x0D
CMD_SET_PIN, CMD_RESET_PIN = 0x0E, 0x0F
CMD_SET_HOME, CMD_RESULT = 0x13, 0x20
CMD_DISPLAY_POLL, CMD_DISPLAY_INPUT = 0x30, 0x31
PENDING, BUSY = 7, 8
STATUS_TEXT = {1: "bad request", 2: "unknown command", 4: "signing failed", 5: "storage failed",
               6: "no hardware engine", 9: "rejected", 10: "no approval UI", 11: "refused by the device",
               13: "no PIN", 14: "PIN locked"}
PQ_SIG_LEN, PQ_PK_LEN = 4627, 2592
SCALE = 2                  # 240x240 panel shown at 2x
POLL_S = 0.1
DEFAULT_HOLD_MS = 700      # until the chip says (every snapshot carries it)
FLAG_CLASSICAL_ONLY, FLAG_INCLUDE_PQ_KEY, FLAG_SIGN_BYTES_V1 = 1, 2, 4


# ---------------------------------------------------------------- device transports

class HidDevice:
    VID, PID = 0x1D50, 0x6197

    def __init__(self):
        try:
            import hidraw as hid  # kernel hidraw backend (pip hidapi's `hid` uses libusb)
        except ImportError:
            import hid
        self.d = hid.device()
        self.d.open(self.VID, self.PID)

    def exchange_frame(self, frame):
        for off in range(0, len(frame), 64):
            self.d.write(b"\0" + frame[off:off + 64].ljust(64, b"\0"))
        first = bytes(self.d.read(64, 5000))
        if not first:
            raise TimeoutError("no reply from the device")
        buf = first
        n = struct.unpack(">I", buf[:4])[0]
        while len(buf) < 4 + n:
            chunk = bytes(self.d.read(64, 2000))
            if not chunk:
                raise TimeoutError(f"incomplete reply ({len(buf) - 4} of {n} bytes)")
            buf += chunk
        return buf[:4 + n]


class TcpDevice:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port))

    def exchange_frame(self, frame):
        self.s.sendall(frame)
        head = self._read(4)
        return head + self._read(struct.unpack(">I", head)[0])

    def _read(self, n):
        out = b""
        while len(out) < n:
            c = self.s.recv(n - len(out))
            if not c:
                raise ConnectionError("device closed the connection")
            out += c
        return out


class Device:
    """Serialises access: the window's polling and the bridged clients share one channel."""

    def __init__(self, emu_port):
        self.emu_port = emu_port
        self.lock = threading.Lock()
        self.dev = None

    def _open(self):
        self.dev = TcpDevice(self.emu_port) if self.emu_port else HidDevice()

    def exchange_frame(self, frame):
        with self.lock:
            try:
                if self.dev is None:
                    self._open()
                return self.dev.exchange_frame(frame)
            except Exception:
                self.dev = None  # reconnect next time (e.g. the board was replugged)
                raise

    def call(self, cmd, payload=b""):
        body = bytes([MSG, cmd]) + payload
        r = self.exchange_frame(struct.pack(">I", len(body)) + body)[4:]
        return r[1], r[2:]


# ---------------------------------------------------------------- bridge for other tools

def serve_clients(dev, port, status):
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", port))
    srv.listen(4)

    def client(conn):
        with conn:
            while True:
                head = b""
                while len(head) < 4:
                    c = conn.recv(4 - len(head))
                    if not c:
                        return
                    head += c
                n = struct.unpack(">I", head)[0]
                body = b""
                while len(body) < n:
                    c = conn.recv(n - len(body))
                    if not c:
                        return
                    body += c
                try:
                    conn.sendall(dev.exchange_frame(head + body))
                except Exception as e:
                    status["error"] = f"device: {e}"
                    return

    while True:
        conn, _ = srv.accept()
        threading.Thread(target=client, args=(conn,), daemon=True).start()


def decode_rle(enc, n):
    """byte = color << 4 | (run - 1) for runs 1..15; low nibble 15 = run of 16 + next byte."""
    out = bytearray(n)
    pos = i = 0
    while i < len(enc) and pos < n:
        b = enc[i]
        c, r = b >> 4, b & 15
        if r == 15:
            i += 1
            r = 16 + enc[i]
        else:
            r += 1
        out[pos:pos + r] = bytes([c]) * r
        pos += r
        i += 1
    return bytes(out)


def apply_snapshot(screen, p):
    """seq u32, id u32, kind u8, left u8, vdisplay u8, has_frame u8, hold_ms u16, then with a
    frame: width u16, height u16, rows y0 u16, y1 u16, palette 16 x RGB, RLE indices of y0..y1"""
    seq, sid, kind, left, vdisp, has, hold = struct.unpack(">IIBBBBH", p[:14])
    upd = dict(seq=seq, id=sid, kind=kind, left=left, vdisplay=bool(vdisp), hold_ms=hold, connected=True)
    if has and len(p) >= 14 + 8 + 48:
        w, h, y0, y1 = struct.unpack(">HHHH", p[14:22])
        palette = [tuple(p[22 + 3 * k:25 + 3 * k]) for k in range(16)]
        old = screen.get("frame")
        if y0 == 0 and y1 == h:
            px = bytearray(decode_rle(p[70:], w * h))
        elif old is not None and len(old["px"]) == w * h:
            px = bytearray(old["px"])
            px[y0 * w:y1 * w] = decode_rle(p[70:], (y1 - y0) * w)
        else:
            px = None  # a partial update without a base frame: ask for a full one
        if px is not None:
            upd["frame"] = {"w": w, "h": h, "px": bytes(px), "palette": palette}
        else:
            screen.pop("frame", None)
            upd.pop("seq")
    screen.update(upd)


