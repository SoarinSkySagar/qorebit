#!/usr/bin/env python3
"""`make harden WHAT=paranoid|require-pq`: turn on a Baochip hardening switch. IRREVERSIBLE.

Both switches are pairs of one-way counters that boot1 advances from its console; nothing can
turn them off again, on this chip, ever.

  paranoid     PARANOID_MODE: the glitch / tamper detectors run on a hair trigger. Alarms are
               counted in POSSIBLE_ATTACKS, and past a limit the chip WIPES ITSELF. Noisy power
               or rough handling can raise alarms too.
  require-pq   REQUIRE_PQ: every boot stage must carry a valid SLH-DSA (SPHINCS+) signature as
               well as ed25519, so the boot chain is post-quantum like the wallet's signatures.
               Firmware without a PQ signature no longer boots. Refused here unless boot1 and
               the installed loader already carry PQ signatures (xous-core signs Qorebit's loader
               and kernel with an SLH-DSA key by default).

What this can't do on a Dabao: give Qorebit its own signing key or lock out developer mode.
boot0 trusts only Baochip's keys and the public developer key, and Qorebit images are signed with
the developer key, so the chip stays in developer mode and boot1 refuses `lockdown`. See
walkthrough.md, "Verify your device".

The board must be in its bootloader (hold PROG while plugging in). You type a confirmation
phrase before anything is sent. Usage: tools/harden.py paranoid|require-pq [--port ...]
"""
import argparse
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from audit import field, find_port, open_serial, run_command  # noqa: E402

SWITCHES = {
    "paranoid": ("paranoid enable", "Paranoid mode", "make the glitch detectors aggressive (alarms can wipe the chip)"),
    "require-pq": ("require-pq confirm", "PQ required", "refuse every boot stage without an SLH-DSA signature"),
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("what", choices=sorted(SWITCHES))
    ap.add_argument("--port")
    a = ap.parse_args()
    command, label, effect = SWITCHES[a.what]

    port = a.port or find_port()
    if not port:
        sys.exit("no serial port: unplug the Dabao, hold PROG and plug it back in")
    fd = open_serial(port)
    report = run_command(fd, "audit")
    if "Semver" not in report:
        sys.exit(f"no audit report from {port}: is the board in its bootloader?")
    state = field(report, label) or "?"
    print(f"{label} now: {state}")
    if state not in ("0/0", "?"):
        print("already on: nothing to do")
        return

    if a.what == "require-pq":
        # only an explicit "pq Some(...)" counts: older boot1 releases don't report PQ at all
        missing = [stage for stage in ("Boot1", "Next stage") if not re.search(r"pq Some", field(report, stage) or "")]
        if missing:
            sys.exit(f"refused: boot1 doesn't report a PQ signature for {', '.join(missing)}, so the chip might stop booting")
        if field(report, "PQ required") is None:
            sys.exit("refused: this boot1 doesn't report the PQ-required counters (too old for require-pq)")

    serial = field(report, "Public serial number") or "this chip"
    phrase = f"{a.what} {serial}"
    print(f"\nThis will {effect} on {serial}. It CANNOT be undone.")
    print(f"Type exactly '{phrase}' to go ahead, anything else to stop:")
    if input("> ").strip() != phrase:
        print("stopped: nothing was changed")
        return

    out = run_command(fd, command)
    print(out.strip())
    after = field(run_command(fd, "audit"), label) or "?"
    os.close(fd)
    print(f"{label} now: {after}")
    sys.exit(0 if re.match(r"[1-9]", after) else 1)


if __name__ == "__main__":
    main()
