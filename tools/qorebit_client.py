#!/usr/bin/env python3
"""Command-line client for the Qorebit device, through the bridge (tools/qorebit_display.py, 127.0.0.1:7620).

    tools/.venv/bin/python tools/qorebit_client.py info | security | account [N] | pq-public [N] | version | bio-test

`--port` picks another bridge (the emulator test uses 7621); `--hid` talks to the Dabao directly over USB
(only when no bridge is running).
"""
import argparse, socket, struct, sys

MSG = 0x51
STATUS = {0: "ok", 1: "bad request", 2: "unknown command", 4: "signing failed", 5: "storage failed", 6: "no hardware",
          9: "rejected", 10: "no approval UI", 11: "refused by the device", 13: "no PIN", 14: "PIN locked"}


class Bridge:
    def __init__(self, port):
        self.s = socket.create_connection(("127.0.0.1", port))

    def call(self, cmd, payload=b""):
        body = bytes([MSG, cmd]) + payload
        self.s.sendall(struct.pack(">I", len(body)) + body)
        n = struct.unpack(">I", self._read(4))[0]
        r = self._read(n)
        return r[1], r[2:]

    def _read(self, n):
        out = b""
        while len(out) < n:
            c = self.s.recv(n - len(out))
            if not c:
                raise ConnectionError("the bridge closed the connection")
            out += c
        return out


class Hid:
    def __init__(self):
        try:
            import hidraw as hid
        except ImportError:
            import hid
        self.d = hid.device()
        self.d.open(0x1D50, 0x6197)

    def call(self, cmd, payload=b""):
        body = bytes([MSG, cmd]) + payload
        frame = struct.pack(">I", len(body)) + body
        for off in range(0, len(frame), 64):
            self.d.write(b"\0" + frame[off:off + 64].ljust(64, b"\0"))
        buf = bytes(self.d.read(64, 5000))
        n = struct.unpack(">I", buf[:4])[0]
        while len(buf) < 4 + n:
            buf += bytes(self.d.read(64, 2000))
        r = buf[4:4 + n]
        return r[1], r[2:]


def run(dev, cmd, payload=b""):
    st, p = dev.call(cmd, payload)
    while st in (7, 8):  # PENDING / BUSY: the job is running (maybe waiting for the user on the device)
        import time
        time.sleep(0.3)
        st, p = dev.call(0x20)
    if st != 0:
        sys.exit(f"{STATUS.get(st, st)}: {p.decode(errors='replace')}")
    return p


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("command", choices=["info", "security", "account", "pq-public", "version", "bio-test"])
    ap.add_argument("n", nargs="?", type=int, default=0, help="account number, from 0")
    ap.add_argument("--port", type=int, default=7620)
    ap.add_argument("--hid", action="store_true")
    a = ap.parse_args()
    dev = Hid() if a.hid else Bridge(a.port)
    if a.command == "version":
        print(run(dev, 0x01).decode())
    elif a.command == "info":
        print(run(dev, 0x05).decode())
    elif a.command == "bio-test":
        print(run(dev, 0x12).decode())
    elif a.command == "account":
        p = run(dev, 0x02, struct.pack(">I", a.n))
        print(f"account {a.n + 1}\n  address   {p[53:].decode()}\n  pubkey    {p[20:53].hex()}")
    elif a.command == "pq-public":
        p = run(dev, 0x03, struct.pack(">I", a.n))
        print(f"ML-DSA-87 public key of account {a.n + 1} ({len(p)} bytes):\n{p.hex()}")
    else:
        p = run(dev, 0x09)
        attacks, = struct.unpack(">I", p[5:9])
        seed = {0: "plain keystore slots", 1: "wrapped by the chip's key", 2: "host file (emulator)"}.get(p[9], "?")
        print(f"chip: {'Baochip-1x' if p[1] else 'emulator'}\ndeveloper mode: {bool(p[2])}\nparanoid mode: {bool(p[3])}\n"
              f"possible attacks: {attacks}\nseed storage: {seed}\nPIN: {'set, ' + str(p[11]) + ' tries left' if p[10] else 'not set'}\n"
              f"passphrase wallet: {bool(p[12])}\nfirmware: {p[13:].decode()}")


if __name__ == "__main__":
    main()
