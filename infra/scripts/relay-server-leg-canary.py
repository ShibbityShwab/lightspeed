#!/usr/bin/env python3
"""Relay-side Step-0 canary for the relay-measured server leg.

Runs ON a relay host (as root, via ssh or a CI runner). It proves the one
blocking unknown of ``docs/design/relay-server-leg.md``: can the relay send
ICMP without ``CAP_NET_RAW`` on the daemon? The answer depends on
``net.ipv4.ping_group_range`` covering the GIDs systemd ``DynamicUser`` hands
out (61184-65519 per systemd.exec(5)).

Modes:
  --check    read-only: show the sysctl and try an unprivileged ICMP echo.
  --apply    write the persistent drop-in, apply it, verify, and test.
  --revert   restore the value saved before ``--apply`` and remove the drop-in.

Safety: one public IPv4 target per run (default 1.1.1.1), three echoes, one
second timeout; private/loopback/link-local targets are refused; no port
scanning, no payload. The script prints ``CANARY ...`` lines for the driver to
parse and exits non-zero when the unprivileged echo does not work.

Usage:
  python3 relay-server-leg-canary.py --check
  python3 relay-server-leg-canary.py --apply
  python3 relay-server-leg-canary.py --revert
"""

from __future__ import annotations

import argparse
import ipaddress
import json
import os
import socket
import struct
import subprocess
import sys
import time

SYSCTL_KEY = "net.ipv4.ping_group_range"
DROP_IN = "/etc/sysctl.d/99-lightspeed-ping.conf"
BACKUP = "/var/tmp/lightspeed-ping-group-range.before"
GID_LOW, GID_HIGH = 61184, 65519

# The child runs as root but drops to the DynamicUser GID range before it opens
# the ICMP socket, so a success is the same permission path the service will
# take. It never regains privileges.
CHILD = r"""
import json, os, socket, struct, sys, time

target, gid, count = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])

def checksum(data):
    if len(data) % 2:
        data += b"\x00"
    total = sum(struct.unpack("!%dH" % (len(data) // 2), data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return ~total & 0xFFFF

os.setgroups([])
os.setgid(gid)
os.setuid(gid)

try:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_ICMP)
except PermissionError as exc:
    print(json.dumps({"ok": False, "error": "socket: %s" % exc}))
    sys.exit(0)

sock.settimeout(1.0)
ident = os.getpid() & 0xFFFF
rtts = []
for seq in range(count):
    payload = struct.pack("!d", time.monotonic()) + b"lightspeed-canary"
    header = struct.pack("!BBHHH", 8, 0, 0, ident, seq)
    packet = struct.pack("!BBHHH", 8, 0, checksum(header + payload), ident, seq) + payload
    sent = time.monotonic()
    try:
        sock.sendto(packet, (target, 0))
        data, _ = sock.recvfrom(1024)
        # IPv4 header length is the low nibble of byte 0; ICMP starts after it.
        offset = (data[0] & 0x0F) * 4
        kind = data[offset]
        if kind == 0:
            rtts.append((time.monotonic() - sent) * 1000.0)
    except (socket.timeout, OSError):
        pass  # a timeout is a data point, not a crash
print(json.dumps({"ok": bool(rtts), "rtts_ms": rtts, "gid": gid, "target": target}))
"""


def read_sysctl() -> str:
    out = subprocess.run(
        ["sysctl", "-n", SYSCTL_KEY], capture_output=True, text=True, check=False
    )
    return out.stdout.strip()


def write_sysctl(value: str) -> None:
    subprocess.run(["sysctl", "-w", f"{SYSCTL_KEY}={value}"], check=True)


def unprivileged_ping(target: str) -> dict:
    proc = subprocess.run(
        [sys.executable, "-c", CHILD, target, str(GID_LOW), "3"],
        capture_output=True,
        text=True,
        check=False,
        timeout=30,
    )
    for line in proc.stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                return json.loads(line)
            except json.JSONDecodeError:
                continue
    return {"ok": False, "error": (proc.stderr or proc.stdout or "no output").strip()[:300]}


def _checksum(data: bytes) -> int:
    if len(data) % 2:
        data += b"\x00"
    total = sum(struct.unpack("!%dH" % (len(data) // 2), data))
    while total >> 16:
        total = (total & 0xFFFF) + (total >> 16)
    return ~total & 0xFFFF


def privileged_ping(target: str) -> dict:
    sock = None
    try:
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM, socket.IPPROTO_ICMP)
        sock.settimeout(1.0)
        ident = os.getpid() & 0xFFFF
        rtts = []
        for seq in range(3):
            payload = b"lightspeed-canary"
            head = struct.pack("!BBHHH", 8, 0, 0, ident, seq)
            packet = (
                struct.pack("!BBHHH", 8, 0, _checksum(head + payload), ident, seq) + payload
            )
            sent = time.monotonic()
            sock.sendto(packet, (target, 0))
            data, _ = sock.recvfrom(1024)
            offset = (data[0] & 0x0F) * 4
            if data[offset] == 0:
                rtts.append((time.monotonic() - sent) * 1000.0)
        return {"ok": bool(rtts), "rtts_ms": rtts}
    except OSError as exc:
        return {"ok": False, "error": str(exc)}
    finally:
        if sock is not None:
            sock.close()


def refuse_target(raw: str) -> str:
    try:
        addr = ipaddress.IPv4Address(raw)
    except ValueError:
        sys.exit(f"CANARY error=bad-target target={raw}")
    if not addr.is_global:
        sys.exit(f"CANARY error=non-public-target target={raw}")
    return str(addr)


def main() -> int:
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--apply", action="store_true")
    group.add_argument("--revert", action="store_true")
    parser.add_argument("--target", default="1.1.1.1")
    args = parser.parse_args()
    target = refuse_target(args.target)

    if not (args.check or args.revert) and os.geteuid() != 0:
        sys.exit("CANARY error=root-required")

    before = read_sysctl()
    print(f"CANARY mode={'apply' if args.apply else 'revert' if args.revert else 'check'} "
          f"sysctl_before='{before}' want='{GID_LOW} {GID_HIGH}' target={target}")

    if args.revert:
        if os.path.exists(BACKUP):
            with open(BACKUP, encoding="utf-8") as handle:
                saved = handle.read().strip() or "1 0"
            write_sysctl(saved)
            print(f"CANARY restored='{saved}'")
        else:
            print("CANARY restore=skipped (no backup; nothing was applied by this script)")
        if os.path.exists(DROP_IN):
            os.remove(DROP_IN)
            print(f"CANARY removed={DROP_IN}")
        print(f"CANARY sysctl_after='{read_sysctl()}' verdict=REVERTED")
        return 0

    if args.apply:
        os.makedirs(os.path.dirname(DROP_IN), exist_ok=True)
        if not os.path.exists(BACKUP):
            with open(BACKUP, "w", encoding="utf-8") as handle:
                handle.write(before or "1 0")
        with open(DROP_IN, "w", encoding="utf-8") as handle:
            handle.write(f"# LightSpeed relay server-leg canary (revert: --revert)\n"
                         f"{SYSCTL_KEY} = {GID_LOW} {GID_HIGH}\n")
        subprocess.run(["sysctl", "-p", DROP_IN], check=True)
        after = read_sysctl()
        print(f"CANARY drop_in={DROP_IN} sysctl_after='{after}'")

    # The unprivileged echo is the verdict in every mode; report it before the
    # privileged comparison so a failure there cannot hide the result.
    unpriv = unprivileged_ping(target)
    print(f"CANARY unprivileged ok={unpriv.get('ok')} rtts_ms={unpriv.get('rtts_ms', [])} "
          f"error={unpriv.get('error', '')}")
    try:
        priv = privileged_ping(target)
        print(f"CANARY privileged   ok={priv.get('ok')} rtts_ms={priv.get('rtts_ms', [])} "
              f"error={priv.get('error', '')}")
    except Exception as exc:  # never let the comparison hide the verdict
        print(f"CANARY privileged   error={type(exc).__name__}: {exc}")
    rtts = unpriv.get("rtts_ms") or []
    median = sorted(rtts)[len(rtts) // 2] if rtts else None
    verdict = "PASS" if unpriv.get("ok") else "FAIL"
    print(f"CANARY verdict={verdict} unprivileged_median_ms={median}")
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
