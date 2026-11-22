#!/usr/bin/env python3
"""`make audit`: ask the Dabao's boot1 for its audit report and check it.

The Baochip's boot1 bootloader has an `audit` console command. It hashes boot0 and boot1 as
they sit in the chip (SHA-512), and prints the one-way counters that make up the chip's
lifecycle: developer mode, paranoid mode, PQ signatures required, glitch alarms, revoked boot
keys. Baochip publishes the expected boot0/boot1 hashes for every release ("Baobit Audit
References" in xous-core/bao1x-boot/README.md), built with a reproducible toolchain, so you
can check that the boot code in your chip is the published code.

The board must be sitting in its bootloader: unplug it, hold PROG, plug it back in (it shows
up as the BAOCHIP drive and a serial port). Nothing is written to the chip: `audit` only
reads. Usage: tools/audit.py [--port /dev/ttyACM0] [--raw]
"""
import argparse
import glob
import os
import re
import select
import sys
import termios
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REFERENCES = os.path.join(ROOT, "xous-core", "bao1x-boot", "README.md")
HASHED = ("boot0 code only", "boot1 code only", "boot0 partition", "boot1 partition")


def find_port():
    ports = sorted(glob.glob("/dev/ttyACM*")) + sorted(glob.glob("/dev/cu.usbmodem*"))
    return ports[0] if ports else None


def open_serial(port):
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    attrs = termios.tcgetattr(fd)
    attrs[0] = 0  # iflag
    attrs[1] = 0  # oflag
    attrs[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    attrs[3] = 0  # lflag: raw, no echo
    attrs[4] = attrs[5] = termios.B115200
    termios.tcsetattr(fd, termios.TCSANOW, attrs)
    return fd


def run_command(fd, cmd, quiet=1.5, limit=15.0):
    """Send `cmd`, collect output until the console has been quiet for `quiet` seconds."""
    termios.tcflush(fd, termios.TCIFLUSH)
    os.write(fd, cmd.encode() + b"\r")
    out, start, last = b"", time.time(), time.time()
    while time.time() - start < limit and time.time() - last < quiet:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            try:
                chunk = os.read(fd, 4096)
            except BlockingIOError:
                continue
            if chunk:
                out += chunk
                last = time.time()
    return out.decode(errors="replace").replace("\r", "")


def references():
    """{version: {label: hash}} from the bootloader README's audit references."""
    refs, version = {}, None
    try:
        text = open(REFERENCES).read()
    except OSError:
        return refs
    for line in text.splitlines():
        m = re.match(r"####\s+(v\S+)", line)
        if m:
            version = m.group(1)
            refs[version] = {}
            continue
        m = re.match(r"\s*(boot[01] (?:partition|code only|baobit toolchain)):\s*(.+)$", line)
        if m and version:
            # the README wraps the 128-digit hashes with runs of spaces: join them
            refs[version][m.group(1)] = re.sub(r"\s+", "", m.group(2))
    return refs


def field(report, label):
    m = re.search(rf"^{re.escape(label)}:?\s*(.+)$", report, re.M)
    return m.group(1).strip() if m else None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--port", help="the boot1 serial port (default: the first /dev/ttyACM*)")
    ap.add_argument("--raw", action="store_true", help="also print the whole report")
    a = ap.parse_args()

    port = a.port or find_port()
    if not port:
        sys.exit("no serial port: unplug the Dabao, hold PROG and plug it back in (its bootloader has the console)")
    fd = open_serial(port)
    report = run_command(fd, "audit")
    os.close(fd)
    if "Semver" not in report:
        sys.exit(f"no audit report from {port}. Is the board in its bootloader (hold PROG while plugging in)?\n{report[-800:]}")
    if a.raw:
        print(report)

    ok = True

    def line(good, label, value):
        nonlocal ok
        mark = {True: "\033[32mok  \033[0m", False: "\033[31mFAIL\033[0m", None: "\033[33mnote\033[0m"}[good]
        if good is False:
            ok = False
        print(f"  {mark}  {label:<28} {value}")

    print(f"boot1 audit from {port}")
    line(None, "boot1 version", field(report, "Semver is") or "?")
    line(None, "board", field(report, "Board type reads as") or "?")
    line(None, "serial number", field(report, "Public serial number") or "?")

    # boot code: compare the chip's hashes with Baochip's published references
    refs = references()
    measured = {k: (field(report, k) or "").replace(" ", "") for k in HASHED}
    match = [v for v, r in refs.items() if all(measured[k] and r.get(k) == measured[k] for k in ("boot0 code only", "boot1 code only"))]
    if match:
        line(True, "boot0 + boot1 code", f"match the published Baobit {', '.join(match)} reference")
    elif not refs:
        line(None, "boot0 + boot1 code", f"no references found in {REFERENCES}")
    else:
        b0 = [v for v, r in refs.items() if r.get("boot0 code only") == measured["boot0 code only"]]
        line(bool(b0), "boot0 code", f"matches {', '.join(b0)}" if b0 else "matches no published reference")
        b1 = [v for v, r in refs.items() if r.get("boot1 code only") == measured["boot1 code only"]]
        # boot1 is updateable, and a newer boot1 than the submodule's README is not a failure
        line(True if b1 else None, "boot1 code", f"matches {', '.join(b1)}" if b1 else
             "matches no reference in this xous-core (newer boot1? compare with the current README)")
    for k in ("boot0 code only", "boot1 code only"):
        if measured[k]:
            print(f"        {k}: {measured[k][:32]}...")

    # lifecycle counters
    paranoid = field(report, "Paranoid mode") or "?"
    line(None, "paranoid mode", paranoid + ("  (off: enable with the boot1 `paranoid enable` command)" if paranoid.startswith("0/0") else ""))
    pq = field(report, "PQ required") or "not reported by this boot1"
    line(None, "PQ boot signatures required", pq)
    attacks = field(report, "Possible attack attempts")
    line(attacks == "0" if attacks is not None else None, "possible attack attempts", attacks or "?")
    nxt = field(report, "Next stage")
    if nxt:
        # "key 3/..." is the developer key slot (slot 3)
        dev = re.search(r"key 3/", nxt) is not None
        line(None if dev else True, "loader signed with", "the public developer key (developer mode)" if dev else nxt)
        # newer boot1 releases print "pq Some(tag)" / "pq No PQ sig"; older ones say nothing
        pq = "present" if re.search(r"pq Some", nxt) else "none" if "No PQ sig" in nxt else "not reported by this boot1"
        line(None, "loader PQ signature", pq)
    erase = field(report, "Erase proof")
    if erase:
        line(None, "secret-key area", "erased: developer mode, the chip's own secrets are gone" if erase == "erased" else erase)

    print()
    print("Next: compare the die with the published layout using IRIS (see walkthrough.md,")
    print("'Verify your device'), and `make security` for what the running firmware reports.")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
