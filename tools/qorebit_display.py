#!/usr/bin/env python3
"""Qorebit virtual display and bridge: the hardware-wallet screen and its two buttons in a window on
the laptop, for firmware built with `--vdisplay` (the default until a real panel).

The screen is rendered **on the chip** into a 240x280 colour framebuffer (16-colour palette,
ST7789-class panel). The chip sends the rows that changed, run-length encoded, with the
palette; this window only maps indices to colours. The real panel gets the same frame.

**Two buttons.** The left one says 0 / Reject, the right one says 1 / Approve: one click is the answer.
Mouse on a button, or the Left / Right arrow keys (A and D work too); Escape rejects.

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
could fake both. The real wallet uses a display and two buttons on the device.
"""
import argparse, json, os, signal, socket, struct, subprocess, sys, threading, time
import urllib.error, urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MSG = 0x51
CMD_VERSION, CMD_ACCOUNT, CMD_PQ_PUBKEY, CMD_REGISTER_PROOF, CMD_INFO, CMD_SIGN_TX = 0x01, 0x02, 0x03, 0x04, 0x05, 0x06
CMD_SIGN_SIWQ = 0x07
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
SCALE = 2                  # the 240x280 panel shown at 2x
POLL_S = 0.1
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
    """seq u32, id u32, kind u8, left u8, vdisplay u8, has_frame u8, then with a
    frame: width u16, height u16, rows y0 u16, y1 u16, palette 16 x RGB, RLE indices of y0..y1"""
    seq, sid, kind, left, vdisp, has = struct.unpack(">IIBBBB", p[:12])
    upd = dict(seq=seq, id=sid, kind=kind, left=left, vdisplay=bool(vdisp), connected=True)
    if has and len(p) >= 12 + 8 + 48:
        w, h, y0, y1 = struct.unpack(">HHHH", p[12:20])
        palette = [tuple(p[20 + 3 * k:23 + 3 * k]) for k in range(16)]
        old = screen.get("frame")
        if y0 == 0 and y1 == h:
            px = bytearray(decode_rle(p[68:], w * h))
        elif old is not None and len(old["px"]) == w * h:
            px = bytearray(old["px"])
            px[y0 * w:y1 * w] = decode_rle(p[68:], (y1 - y0) * w)
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
            if st == 0 and len(p) >= 12:
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


# One device job at a time. A second request that arrives while a job waits for the user (an approval, a
# PIN) would be told BUSY and then collect that job's RESULT, stealing the answer meant for the first
# caller. The website polls the device every few seconds, so this really happened: the signature the user
# approved on the device never reached the website. So concurrent callers are refused, not queued.
_job_lock = threading.Lock()


def call_job(dev, cmd, payload=b"", wait=180):
    """Run a device command to completion, following PENDING / BUSY. Refuses (BUSY) while another job runs."""
    if not _job_lock.acquire(timeout=3):
        raise DeviceError(BUSY, "the device is busy with another request: finish it on the device first")
    try:
        return _run_job(dev, cmd, payload, wait)
    finally:
        _job_lock.release()


def _run_job(dev, cmd, payload, wait):
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
    if method == "device.signSiwq":
        # Sign In With QoreChain: the device parses the message itself and signs both layers (approval + PIN on the device)
        message = str(params["message"]).encode()
        p = call_job(dev, CMD_SIGN_SIWQ, acct(params) + message, wait=900)
        ms, = struct.unpack(">I", p[:4])
        return {"ms": ms, "classicalSignature": "0x" + p[4:68].hex(), "pqSignature": "0x" + p[68:68 + PQ_SIG_LEN].hex()}
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


# Outgoing lookups ask for IPv4 first. On many networks the resolver waits about 4.5 s for the IPv6 (AAAA)
# answer of QoreChain's public endpoints before it answers at all; a send makes several calls in a row.
# Only if there is no IPv4 address does the original lookup run.
_getaddrinfo = socket.getaddrinfo


def _ipv4_first(host, port, family=0, *args, **kwargs):
    if family in (0, socket.AF_UNSPEC):
        try:
            return _getaddrinfo(host, port, socket.AF_INET, *args, **kwargs)
        except socket.gaierror:
            pass
    return _getaddrinfo(host, port, family, *args, **kwargs)


socket.getaddrinfo = _ipv4_first


def _extension_origin():
    """The wallet browser extension's origin (its id is pinned by the key in extension/manifest.json)."""
    try:
        with open(os.path.join(ROOT, "extension", "EXTENSION_ID")) as f:
            return "chrome-extension://" + f.read().strip()
    except OSError:
        return None


EXTENSION_ORIGIN = _extension_origin()


def allowed_origin(origin):
    if not origin:
        return None
    if EXTENSION_ORIGIN and origin == EXTENSION_ORIGIN:
        return origin
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


def serve_http(dev, port, status):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *a):
            pass

        def _headers(self, code, origin, ctype="application/json"):
            self.send_response(code)
            if origin:
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Access-Control-Allow-Headers", "content-type")
                self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
            self.send_header("Content-Type", ctype)
            self.end_headers()

        def _origin_ok(self):
            origin = self.headers.get("Origin")
            ok = allowed_origin(origin)
            if origin and not ok:
                self._headers(403, None)
                self.wfile.write(b'{"error":"origin not allowed"}')
                return None
            return ok or ""

        def do_OPTIONS(self):
            self._headers(204, allowed_origin(self.headers.get("Origin")))

        def _proxy(self, method):
            ok = self._origin_ok()
            if ok is None:
                return
            parts = self.path.split("/", 4)  # '', 'proxy', net, kind, rest
            if len(parts) < 5:
                self._headers(404, ok)
                self.wfile.write(b'{"error":"use /proxy/<network>/<rest|rpc>/<path>"}')
                return
            body = self.rfile.read(min(int(self.headers.get("Content-Length", 0)), MAX_PROXY_BODY)) if method == "POST" else b""
            code, data = proxy(method, parts[2], parts[3], "/" + parts[4], body)
            self._headers(code, ok)
            self.wfile.write(data)

        def do_GET(self):
            if self.path.startswith("/proxy/"):
                return self._proxy("GET")
            ok = self._origin_ok()
            if ok is None:
                return
            self._headers(200, ok)
            if self.path.startswith("/networks"):
                self.wfile.write(json.dumps({k: {x: v for x, v in n.items() if x not in ("rest", "rpc")} for k, n in NETWORKS.items()}).encode())
            else:
                self.wfile.write(json.dumps({"qorebit": "bridge", "device": "connected" if status.get("connected") else "?"}).encode())

        def do_POST(self):
            if self.path.startswith("/proxy/"):
                return self._proxy("POST")
            ok = self._origin_ok()
            if ok is None:
                return
            try:
                req = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))) or b"{}")
                if self.path.startswith("/devnet/fund"):
                    result = fund_devnet(str(req["address"]), int(req.get("amount", 1_000_000_000)))
                else:
                    result = api(dev, req.get("method", ""), req.get("params", {}))
                self._headers(200, ok)
                self.wfile.write(json.dumps({"result": result}).encode())
            except DeviceError as e:
                self._headers(200, ok)
                self.wfile.write(json.dumps({"error": {"code": e.status, "message": str(e)}}).encode())
            except Exception as e:
                self._headers(500, ok)
                self.wfile.write(json.dumps({"error": {"code": -1, "message": str(e)}}).encode())

    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()


# ---------------------------------------------------------------- window

def run_window(dev, screen, status, title):
    os.environ.setdefault("PYGAME_HIDE_SUPPORT_PROMPT", "1")
    import pygame

    pygame.init()
    PW, PH = 240 * SCALE, 280 * SCALE
    W, H = PW + 80, PH + 200
    win = pygame.display.set_mode((W, H))
    pygame.display.set_caption(title)
    clock = pygame.time.Clock()

    label = pygame.font.SysFont("dejavusansmono,monospace", 18, bold=True)
    small = pygame.font.SysFont("dejavusans,sans", 13)
    cache = {"frame": None, "surf": None}

    ox, oy = (W - PW) // 2, 40
    bw = 150
    btn_left = pygame.Rect(ox + PW // 2 - bw - 12, oy + PH + 28, bw, 70)
    btn_right = pygame.Rect(ox + PW // 2 + 12, oy + PH + 28, bw, 70)
    flash = {"text": "", "until": 0}
    lit = {"side": None, "until": 0}   # which button was pressed last (it lights up for a moment)

    def press(right):
        """One click is the answer: the left button is 0 / Reject, the right one 1 / Approve."""
        lit.update(side=right, until=time.time() + 0.25)
        if screen.get("kind") not in (1, 3, 4):  # approval, PIN, other input
            return
        try:
            st, p = dev.call(CMD_DISPLAY_INPUT, struct.pack(">IB", screen["id"], 1 if right else 0))
            if st == 0 and len(p) >= 12:
                apply_snapshot(screen, p)  # the chip's new screen, in the same round trip
            else:
                flash.update(text=p.decode(errors="replace"), until=time.time() + 2)
        except Exception as e:
            status["error"] = f"device: {e}"

    while True:
        for ev in pygame.event.get():
            if ev.type == pygame.QUIT:
                pygame.quit()
                return
            if ev.type == pygame.MOUSEBUTTONDOWN and ev.button == 1:
                if btn_left.collidepoint(ev.pos):
                    press(False)
                elif btn_right.collidepoint(ev.pos):
                    press(True)
            if ev.type == pygame.KEYDOWN:
                if ev.key in (pygame.K_LEFT, pygame.K_a, pygame.K_ESCAPE):
                    press(False)
                elif ev.key in (pygame.K_RIGHT, pygame.K_d):
                    press(True)

        live = screen.get("kind") in (1, 3, 4)
        # device body: a dark enclosure around the panel
        win.fill((14, 15, 18))
        body = pygame.Rect(16, 16, W - 32, H - 70)
        pygame.draw.rect(win, (32, 34, 40), body, border_radius=26)
        pygame.draw.rect(win, (54, 58, 66), body, width=2, border_radius=26)

        # the panel: the chip's framebuffer, pixel for pixel
        pygame.draw.rect(win, (0, 0, 0), (ox - 8, oy - 8, PW + 16, PH + 16), border_radius=6)
        fr = screen.get("frame") if screen.get("connected") else None
        if fr is not None:
            if fr is not cache["frame"]:
                cache.update(frame=fr, surf=pygame.transform.scale(frame_to_surface(pygame, fr), (PW, PH)))
            win.blit(cache["surf"], (ox, oy))
        else:
            t = label.render("no device", True, (110, 114, 124))
            win.blit(t, t.get_rect(center=(W // 2, oy + PH // 2)))

        # the two buttons: left = 0 / Reject (red), right = 1 / Approve (green)
        for rect, right, name, color in ((btn_left, False, "LEFT  0", (255, 74, 74)), (btn_right, True, "RIGHT  1", (46, 224, 122))):
            base = color if live else (70, 74, 82)
            on = time.time() < lit["until"] and lit["side"] == right
            pygame.draw.rect(win, tuple(v // (2 if on else 5) for v in base), rect, border_radius=34)
            pygame.draw.rect(win, base, rect, width=3, border_radius=34)
            t = label.render(name, True, base)
            win.blit(t, t.get_rect(center=rect.center))
        keys = small.render("left = 0 / reject      right = 1 / approve      (click, or Left / Right arrow keys)", True, (110, 114, 124))
        win.blit(keys, keys.get_rect(midtop=(W // 2, btn_left.bottom + 8)))
        dot = (46, 224, 122) if screen.get("connected") else (255, 74, 74)
        pygame.draw.circle(win, dot, (W - 40, 30), 5)

        # footer
        msg = flash["text"] if time.time() < flash["until"] else status.get("error", "")
        foot = "Dev mode: pixels come from the chip, the button from this laptop. Not secure."
        win.blit(small.render(foot, True, (255, 179, 31)), (24, H - 44))
        if msg:
            win.blit(small.render(msg[:90], True, (230, 230, 230)), (24, H - 26))
        elif screen.get("connected") and not screen.get("vdisplay", True):
            win.blit(small.render("firmware built without --vdisplay", True, (255, 74, 74)), (24, H - 26))
        pygame.display.flip()
        clock.tick(30)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--emu", action="store_true", help="use the emulator (127.0.0.1:7610) instead of USB HID")
    ap.add_argument("--emu-port", type=int, default=7610)
    ap.add_argument("--listen", type=int, default=7620, help="bridge port for other tools (raw frames)")
    ap.add_argument("--http", type=int, default=7630, help="HTTP API for the companion app")
    ap.add_argument("--headless", action="store_true",
                    help="no window: only the bridge (for scripted tests; answer through DISPLAY_INPUT)")
    a = ap.parse_args()

    dev = Device(a.emu_port if a.emu else None)
    screen, status = {"connected": False}, {}
    threading.Thread(target=serve_clients, args=(dev, a.listen, status), daemon=True).start()
    threading.Thread(target=poll_screen, args=(dev, screen, status), daemon=True).start()
    threading.Thread(target=serve_http, args=(dev, a.http, screen), daemon=True).start()
    print(f"Qorebit virtual display: device {'emulator :' + str(a.emu_port) if a.emu else 'USB HID'}; "
          f"tools connect to 127.0.0.1:{a.listen}, the companion to http://127.0.0.1:{a.http}")
    if a.headless:
        # Ctrl+C must stop it even if it was started with SIGINT ignored (e.g. from a script)
        signal.signal(signal.SIGINT, signal.default_int_handler)
        signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
        print("headless: no window; screens are still served on DISPLAY_POLL", flush=True)
        while True:
            time.sleep(3600)
    run_window(dev, screen, status, f"Qorebit device ({'emulator' if a.emu else 'Dabao'})")


if __name__ == "__main__":
    main()
