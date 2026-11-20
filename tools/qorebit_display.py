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


def poll_screen(dev, screen, status):
    while True:
        try:
            since = screen.get("seq") if screen.get("connected") and "frame" in screen else None
            st, p = dev.call(CMD_DISPLAY_POLL, struct.pack(">I", since) if since is not None else b"")
            if st == 0 and len(p) >= 14:
                apply_snapshot(screen, p)
                status.pop("error", None)
            else:
                status["error"] = f"device answered {st}: {p[:60]!r}"
        except Exception as e:
            screen.update(connected=False)
            screen.pop("frame", None)
            status["error"] = f"device: {e}"
            time.sleep(1)
        time.sleep(POLL_S)


def frame_to_surface(pygame, frame):
    """Palette indices -> an 8-bit surface with the chip's palette."""
    surf = pygame.image.frombuffer(frame["px"], (frame["w"], frame["h"]), "P")
    surf.set_palette(frame["palette"])
    return surf


# ---------------------------------------------------------------- HTTP API for browsers

class DeviceError(Exception):
    def __init__(self, status, text):
        super().__init__(text)
        self.status = status


def poll_result(dev):
    """CMD_RESULT, riding out a short USB drop (pressing PROG on the Dabao can disconnect USB
    for a moment): the job keeps running on the chip and its reply waits there."""
    for attempt in range(25):
        try:
            return dev.call(CMD_RESULT)
        except Exception:
            time.sleep(0.4)  # Device reopens the link on the next call
    return dev.call(CMD_RESULT)


def call_job(dev, cmd, payload=b"", wait=180):
    """Run a device command to completion, following PENDING / BUSY."""
    st, p = dev.call(cmd, payload)
    deadline = time.time() + wait
    while st in (PENDING, BUSY) and time.time() < deadline:
        time.sleep(0.2)
        if st == BUSY:  # another job is running: wait for it, then send ours
            st2, _ = poll_result(dev)
            if st2 not in (PENDING, BUSY):
                st, p = dev.call(cmd, payload)
            continue
        st, p = poll_result(dev)
    if st != 0:
        raise DeviceError(st, f"{STATUS_TEXT.get(st, 'error ' + str(st))}: {p.decode(errors='replace')}")
    return p


def unhex(h):
    return bytes.fromhex(h[2:] if h.startswith("0x") else h)


def acct(params):
    return struct.pack(">I", int(params.get("account", 0)))


SEED_STORAGE = {0: "plain", 1: "wrapped", 2: "hostfile"}


def decode_security(p):
    """CMD_GET_SECURITY: the chip's lifecycle counters, PIN and wallet state."""
    if len(p) < 13 or p[0] != 2:
        raise DeviceError(1, "unknown security report format")
    attacks, = struct.unpack(">I", p[5:9])
    return {"chip": bool(p[1]), "developerMode": bool(p[2]), "paranoid": bool(p[3]), "requirePq": bool(p[4]),
            "possibleAttacks": attacks, "seedStorage": SEED_STORAGE.get(p[9], "unknown"),
            "pinSet": bool(p[10]), "pinTriesLeft": p[11], "passphrase": bool(p[12]),
            "firmware": p[13:].decode(errors="replace")}


def api(dev, method, params):
    # the signing PIN: choose / change it, or reset it with the recovery words (on the device)
    if method == "device.setPin":
        call_job(dev, CMD_SET_PIN, wait=900)
        return {"pinSet": True}
    if method == "device.resetPin":
        call_job(dev, CMD_RESET_PIN, wait=3600)
        return {"pinSet": True}
    if method == "device.info":
        return {"text": call_job(dev, CMD_INFO).decode(errors="replace")}
    if method == "device.security":
        return decode_security(call_job(dev, CMD_SECURITY))
    # recovery flows run on the device's screen and button; nothing secret comes back
    if method == "device.showPhrase":
        call_job(dev, CMD_SHOW_PHRASE, wait=900)
        return {"shown": True}
    if method == "device.restore":
        call_job(dev, CMD_RESTORE, wait=3600)
        return {"restored": True}
    if method == "device.wipe":
        call_job(dev, CMD_WIPE, wait=900)
        return {"wiped": True}
    if method == "device.passphrase":
        p = call_job(dev, CMD_PASSPHRASE, wait=1800)
        return {"passphrase": bool(p[:1] == b"\x01")}
    if method == "device.status":
        call_job(dev, CMD_VERSION)
        return {}
    if method == "device.account":
        p = call_job(dev, CMD_ACCOUNT, acct(params))
        return {"address": "0x" + p[:20].hex(), "publicKey": "0x" + p[20:53].hex(), "bech32": p[53:].decode()}
    if method == "device.pqPublicKey":
        p = call_job(dev, CMD_PQ_PUBKEY, acct(params))
        return {"publicKey": "0x" + p[:PQ_PK_LEN].hex()}
    if method == "device.home":
        # the home screen: balance (uqor) + address + network; the device checks the address against its key
        chain, addr = params["chainId"].encode(), params["address"].encode()
        body = (acct(params) + int(params["balance"]).to_bytes(16, "big") + bytes([len(chain)]) + chain
                + bytes([len(addr)]) + addr)
        p = call_job(dev, CMD_SET_HOME, body)
        return {"addressChecked": {0: False, 1: True}.get(p[0]) if p else None}
    if method == "device.registerProof":
        chain = params["chainId"].encode()
        p = call_job(dev, CMD_REGISTER_PROOF, acct(params) + bytes([len(chain)]) + chain, wait=900)
        ms, = struct.unpack(">I", p[:4])
        return {"ms": ms, "proof": "0x" + p[4:4 + PQ_SIG_LEN].hex()}
    if method == "device.signTx":
        chain = params["chainId"].encode()
        body, auth = unhex(params["body"]), unhex(params["authInfo"])
        flags = ((FLAG_CLASSICAL_ONLY if params.get("classicalOnly") else 0)
                 | (FLAG_INCLUDE_PQ_KEY if params.get("includePublicKey") else 0)
                 | (FLAG_SIGN_BYTES_V1 if params.get("signBytes") == "v1" else 0))
        req = (acct(params) + bytes([flags]) + struct.pack(">Q", int(params["accountNumber"])) + bytes([len(chain)]) + chain
               + struct.pack(">I", len(body)) + body + auth)
        p = call_job(dev, CMD_SIGN_TX, req, wait=900)
        ms, n = struct.unpack(">II", p[:8])
        tx = p[8:8 + n]
        rest = p[8 + n:]
        return {"ms": ms, "txBytes": "0x" + tx.hex(), "classicalSignature": "0x" + rest[:64].hex(),
                "pqSignature": ("0x" + rest[64:64 + PQ_SIG_LEN].hex()) if len(rest) >= 64 + PQ_SIG_LEN else None}
    raise DeviceError(1, f"unknown method {method}")


def allowed_origin(origin):
    if not origin:
        return None
    for host in ("http://localhost", "http://127.0.0.1"):
        if origin == host or origin.startswith(host + ":"):
            return origin
    return None


def load_networks():
    with open(os.path.join(ROOT, "tools", "networks.json")) as f:
        return json.load(f)


NETWORKS = load_networks()
# what the browser may ask a chain endpoint for through /proxy: reads and the broadcast, nothing else
PROXY_PREFIXES = {"rest": ("/cosmos/", "/qorechain/", "/cosmwasm/"), "rpc": ("/status", "/abci_info", "/block", "/tx")}
MAX_PROXY_BODY = 256 * 1024


def proxy(method, net, kind, rest, body):
    """Forward one request to the chosen network's REST or RPC endpoint (an allowlist of three)."""
    n = NETWORKS.get(net)
    if n is None or kind not in ("rest", "rpc"):
        return 404, b'{"error":"unknown network"}'
    if method not in ("GET", "POST") or not rest.startswith(PROXY_PREFIXES[kind]):
        return 403, b'{"error":"not allowed through the bridge"}'
    req = urllib.request.Request(n[kind].rstrip("/") + rest, data=body if method == "POST" else None, method=method,
                                 headers={"Content-Type": "application/json", "User-Agent": "qorebit-bridge"})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()
    except Exception as e:
        return 502, json.dumps({"error": f"{net} {kind} unreachable: {e}"}).encode()


def fund_devnet(address, amount):
    """Send test QOR from the local devnet's faucet (devnet/fund.mjs signs as a software wallet)."""
    out = subprocess.run(["node", os.path.join(ROOT, "tools", "fund.mjs"), address, str(amount)],
                         capture_output=True, text=True, timeout=120, cwd=os.path.join(ROOT, "tools"))
    if out.returncode != 0:
        raise DeviceError(1, (out.stderr or out.stdout).strip()[-400:])
    return {"result": out.stdout.strip()}


