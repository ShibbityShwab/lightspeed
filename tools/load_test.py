#!/usr/bin/env python3
"""
LightSpeed — Proxy Load Test

Stress tests proxy nodes to find capacity limits within free-tier constraints.
Sends concurrent tunnel traffic (keepalive + data) and measures:
  - Max concurrent sessions
  - Throughput (packets/sec, bytes/sec)
  - Latency under load (p50, p95, p99)
  - Packet loss rate
  - Memory/CPU impact (via /health endpoint)

Usage:
  python load_test.py <proxy_ip> [options]

Examples:
  python load_test.py 149.28.84.139                  # Quick test (10 clients, 30s)
  python load_test.py 149.28.84.139 -c 50 -d 60      # 50 clients, 60 seconds
  python load_test.py 149.28.84.139 -c 100 --ramp 10  # Ramp from 1→100 over 10s
  python load_test.py 149.28.84.139 --all-nodes        # Test all mesh nodes

Requires: Python 3.8+
"""

import argparse
import json
import math
import socket
import statistics
import struct
import sys
import threading
import time
import urllib.request
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field

# ── LightSpeed Protocol Constants ────────────────────────────────
PROTOCOL_VERSION = 1
HEADER_SIZE = 20
FLAG_KEEPALIVE = 0x01
DATA_PORT = 4434

# Well-known game server IPs (public, safe to target in headers — traffic
# goes to proxy only since these are just header fields for routing)
ECHO_TARGETS = [
    ("104.26.1.50", 7777),    # Cloudflare IP (won't respond but proxy will relay)
    ("104.26.2.50", 7778),
    ("104.26.3.50", 7779),
]

# Built-in mesh nodes
MESH_NODES = {
    "proxy-lax":  "149.28.84.139",
    "relay-sgp":  "149.28.144.74",
}


def encode_header(seq, timestamp_us, src_ip, src_port, dst_ip, dst_port,
                  flags=0, session_token=0):
    """Encode a LightSpeed tunnel header (20 bytes)."""
    version_flags = (PROTOCOL_VERSION << 4) | (flags & 0x0F)
    return struct.pack(
        "!BBHIIIhh",
        version_flags,
        session_token & 0xFF,
        seq & 0xFFFF,
        timestamp_us & 0xFFFFFFFF,
        struct.unpack("!I", socket.inet_aton(src_ip))[0],
        struct.unpack("!I", socket.inet_aton(dst_ip))[0],
        src_port,
        dst_port,
    )


def decode_header(data):
    """Decode a LightSpeed tunnel header. Returns (seq, timestamp_us, flags)."""
    if len(data) < HEADER_SIZE:
        return None
    vf, token, seq, ts, _, _, _, _ = struct.unpack("!BBHIIIhh", data[:HEADER_SIZE])
    flags = vf & 0x0F
    return seq, ts, flags


def now_us():
    return int(time.time() * 1_000_000) & 0xFFFFFFFF


@dataclass
class ClientStats:
    """Per-client statistics."""
    sent: int = 0
    received: int = 0
    latencies_us: list = field(default_factory=list)
    errors: int = 0
    start_time: float = 0.0
    end_time: float = 0.0


@dataclass
class LoadTestResult:
    """Aggregate load test results."""
    target: str
    duration_secs: float
    num_clients: int
    total_sent: int
    total_received: int
    total_errors: int
    loss_pct: float
    throughput_pps: float
    throughput_bps: float
    latency_p50_ms: float
    latency_p95_ms: float
    latency_p99_ms: float
    latency_avg_ms: float
    latency_min_ms: float
    latency_max_ms: float
    health_before: dict
    health_after: dict


def fetch_health(ip, timeout=5):
    """Fetch /health JSON from a proxy node."""
    try:
        url = f"http://{ip}:8080/health"
        req = urllib.request.Request(url, method="GET")
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.loads(resp.read().decode())
    except Exception as e:
        return {"error": str(e)}


def run_client(client_id, proxy_ip, duration_secs, pps, stats: ClientStats):
    """
    Simulate a single client sending keepalive packets at the given PPS rate
    and measuring response latency.
    """
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(2.0)

    local_port = 10000 + client_id
    src_ip = f"10.0.{client_id // 256}.{client_id % 256}"
    dst_ip, dst_port = ECHO_TARGETS[client_id % len(ECHO_TARGETS)]

    stats.start_time = time.time()
    seq = 0
    interval = 1.0 / pps if pps > 0 else 0.1
    end_time = stats.start_time + duration_secs

    while time.time() < end_time:
        try:
            send_ts = now_us()
            header = encode_header(
                seq=seq,
                timestamp_us=send_ts,
                src_ip=src_ip,
                src_port=local_port,
                dst_ip=dst_ip,
                dst_port=dst_port,
                flags=FLAG_KEEPALIVE,
                session_token=client_id % 256,
            )
            sock.sendto(header, (proxy_ip, DATA_PORT))
            stats.sent += 1

            # Try to receive keepalive response
            try:
                data, _ = sock.recvfrom(2048)
                recv_ts = now_us()
                parsed = decode_header(data)
                if parsed:
                    _, orig_ts, flags = parsed
                    if flags & FLAG_KEEPALIVE:
                        # Calculate RTT
                        rtt_us = (recv_ts - send_ts) & 0xFFFFFFFF
                        if rtt_us < 10_000_000:  # sanity: < 10s
                            stats.latencies_us.append(rtt_us)
                        stats.received += 1
            except socket.timeout:
                pass  # Expected under load

            seq = (seq + 1) & 0xFFFF
            time.sleep(interval)

        except Exception:
            stats.errors += 1

    stats.end_time = time.time()
    sock.close()


def run_load_test(proxy_ip, num_clients, duration_secs, pps_per_client, ramp_secs):
    """Run the full load test against a single proxy node."""
    print(f"\n⚡ LightSpeed Load Test")
    print(f"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    print(f"  Target:     {proxy_ip}:{DATA_PORT}")
    print(f"  Clients:    {num_clients}")
    print(f"  Duration:   {duration_secs}s")
    print(f"  PPS/client: {pps_per_client}")
    print(f"  Ramp:       {ramp_secs}s")
    print(f"━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")

    # Pre-test health check
    print("\n📊 Pre-test health check...")
    health_before = fetch_health(proxy_ip)
    if "error" not in health_before:
        print(f"   Status: {health_before.get('status', '?')}")
        print(f"   Connections: {health_before.get('active_connections', '?')}")
        print(f"   Uptime: {health_before.get('uptime_secs', '?')}s")
        print(f"   Packets relayed: {health_before.get('packets_relayed', '?')}")
    else:
        print(f"   ⚠️  Could not reach health endpoint: {health_before['error']}")

    # Prepare client stats
    client_stats = [ClientStats() for _ in range(num_clients)]

    # Launch clients with optional ramp-up
    print(f"\n🚀 Launching {num_clients} clients...")
    start_time = time.time()

    with ThreadPoolExecutor(max_workers=min(num_clients, 200)) as pool:
        futures = []
        for i in range(num_clients):
            # Stagger start times for ramp-up
            if ramp_secs > 0 and num_clients > 1:
                delay = (i / (num_clients - 1)) * ramp_secs
            else:
                delay = 0

            def launch(cid=i, d=delay):
                if d > 0:
                    time.sleep(d)
                run_client(cid, proxy_ip, duration_secs, pps_per_client, client_stats[cid])

            futures.append(pool.submit(launch))

        # Progress updates
        while not all(f.done() for f in futures):
            elapsed = time.time() - start_time
            active = sum(1 for s in client_stats if s.start_time > 0 and s.end_time == 0.0)
            total_sent = sum(s.sent for s in client_stats)
            total_recv = sum(s.received for s in client_stats)
            print(f"\r   ⏱  {elapsed:.0f}s  active={active}  sent={total_sent}  recv={total_recv}", end="", flush=True)
            time.sleep(1)

        # Wait for all to complete
        for f in futures:
            f.result()

    total_duration = time.time() - start_time
    print(f"\n\n✅ Test completed in {total_duration:.1f}s")

    # Post-test health check
    print("\n📊 Post-test health check...")
    health_after = fetch_health(proxy_ip)
    if "error" not in health_after:
        print(f"   Status: {health_after.get('status', '?')}")
        print(f"   Connections: {health_after.get('active_connections', '?')}")
        print(f"   Packets relayed: {health_after.get('packets_relayed', '?')}")
    else:
        print(f"   ⚠️  {health_after['error']}")

    # Aggregate results
    total_sent = sum(s.sent for s in client_stats)
    total_received = sum(s.received for s in client_stats)
    total_errors = sum(s.errors for s in client_stats)
    all_latencies = []
    for s in client_stats:
        all_latencies.extend(s.latencies_us)

    loss_pct = ((total_sent - total_received) / max(total_sent, 1)) * 100
    throughput_pps = total_sent / max(total_duration, 0.001)
    throughput_bps = throughput_pps * HEADER_SIZE * 8  # bits/sec (header only)

    if all_latencies:
        all_latencies.sort()
        lat_p50 = all_latencies[int(len(all_latencies) * 0.50)] / 1000.0
        lat_p95 = all_latencies[int(len(all_latencies) * 0.95)] / 1000.0
        lat_p99 = all_latencies[min(int(len(all_latencies) * 0.99), len(all_latencies) - 1)] / 1000.0
        lat_avg = statistics.mean(all_latencies) / 1000.0
        lat_min = min(all_latencies) / 1000.0
        lat_max = max(all_latencies) / 1000.0
    else:
        lat_p50 = lat_p95 = lat_p99 = lat_avg = lat_min = lat_max = 0.0

    result = LoadTestResult(
        target=proxy_ip,
        duration_secs=total_duration,
        num_clients=num_clients,
        total_sent=total_sent,
        total_received=total_received,
        total_errors=total_errors,
        loss_pct=loss_pct,
        throughput_pps=throughput_pps,
        throughput_bps=throughput_bps,
        latency_p50_ms=lat_p50,
        latency_p95_ms=lat_p95,
        latency_p99_ms=lat_p99,
        latency_avg_ms=lat_avg,
        latency_min_ms=lat_min,
        latency_max_ms=lat_max,
        health_before=health_before,
        health_after=health_after,
    )

    # Print report
    print_report(result)
    return result


def print_report(r: LoadTestResult):
    """Print a formatted load test report."""
    print(f"\n{'='*60}")
    print(f"  ⚡ LOAD TEST REPORT — {r.target}")
    print(f"{'='*60}")
    print(f"")
    print(f"  Duration:        {r.duration_secs:.1f}s")
    print(f"  Clients:         {r.num_clients}")
    print(f"  Packets sent:    {r.total_sent:,}")
    print(f"  Packets recv:    {r.total_received:,}")
    print(f"  Errors:          {r.total_errors:,}")
    print(f"  Packet loss:     {r.loss_pct:.2f}%")
    print(f"  Throughput:      {r.throughput_pps:.0f} pps | {r.throughput_bps/1000:.1f} kbps")
    print(f"")
    print(f"  Latency (ms):")
    print(f"    avg:  {r.latency_avg_ms:.2f}")
    print(f"    p50:  {r.latency_p50_ms:.2f}")
    print(f"    p95:  {r.latency_p95_ms:.2f}")
    print(f"    p99:  {r.latency_p99_ms:.2f}")
    print(f"    min:  {r.latency_min_ms:.2f}")
    print(f"    max:  {r.latency_max_ms:.2f}")
    print(f"")

    # Capacity assessment
    if r.loss_pct < 1.0:
        verdict = "✅ HEALTHY — no significant packet loss"
    elif r.loss_pct < 5.0:
        verdict = "⚠️  MODERATE — some packet loss under load"
    else:
        verdict = "❌ DEGRADED — high packet loss, capacity limit reached"

    print(f"  Verdict: {verdict}")

    # Memory delta from health checks
    if "error" not in r.health_before and "error" not in r.health_after:
        pkts_before = r.health_before.get("packets_relayed", 0)
        pkts_after = r.health_after.get("packets_relayed", 0)
        print(f"\n  Proxy-side packets delta: {pkts_after - pkts_before:,}")

    print(f"{'='*60}\n")


def export_json(results, filepath):
    """Export results to JSON file."""
    data = []
    for r in results:
        data.append({
            "target": r.target,
            "duration_secs": r.duration_secs,
            "num_clients": r.num_clients,
            "total_sent": r.total_sent,
            "total_received": r.total_received,
            "loss_pct": round(r.loss_pct, 3),
            "throughput_pps": round(r.throughput_pps, 1),
            "latency_p50_ms": round(r.latency_p50_ms, 2),
            "latency_p95_ms": round(r.latency_p95_ms, 2),
            "latency_p99_ms": round(r.latency_p99_ms, 2),
            "latency_avg_ms": round(r.latency_avg_ms, 2),
        })
    with open(filepath, "w") as f:
        json.dump(data, f, indent=2)
    print(f"📁 Results exported to {filepath}")


def main():
    parser = argparse.ArgumentParser(
        description="LightSpeed Proxy Load Test",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python load_test.py 149.28.84.139                    # Quick 10-client test
  python load_test.py 149.28.84.139 -c 50 -d 60       # 50 clients, 60s
  python load_test.py 149.28.84.139 -c 100 --ramp 10  # Ramp 1→100 over 10s
  python load_test.py --all-nodes -c 25 -d 30          # All mesh nodes
        """,
    )
    parser.add_argument("target", nargs="?", help="Proxy IP address")
    parser.add_argument("-c", "--clients", type=int, default=10, help="Number of concurrent clients (default: 10)")
    parser.add_argument("-d", "--duration", type=int, default=30, help="Test duration in seconds (default: 30)")
    parser.add_argument("-p", "--pps", type=int, default=10, help="Packets per second per client (default: 10)")
    parser.add_argument("--ramp", type=int, default=0, help="Ramp-up time in seconds (default: 0)")
    parser.add_argument("--all-nodes", action="store_true", help="Test all mesh nodes sequentially")
    parser.add_argument("--output", "-o", type=str, help="Export results to JSON file")
    args = parser.parse_args()

    if not args.target and not args.all_nodes:
        parser.print_help()
        sys.exit(1)

    results = []

    if args.all_nodes:
        print(f"🌐 Testing all {len(MESH_NODES)} mesh nodes...")
        for name, ip in MESH_NODES.items():
            print(f"\n{'─'*60}")
            print(f"  Node: {name} ({ip})")
            print(f"{'─'*60}")
            result = run_load_test(ip, args.clients, args.duration, args.pps, args.ramp)
            results.append(result)
    else:
        result = run_load_test(args.target, args.clients, args.duration, args.pps, args.ramp)
        results.append(result)

    # Summary for multi-node
    if len(results) > 1:
        print(f"\n{'='*60}")
        print(f"  📊 MESH SUMMARY")
        print(f"{'='*60}")
        for r in results:
            status = "✅" if r.loss_pct < 1.0 else "⚠️" if r.loss_pct < 5.0 else "❌"
            print(f"  {status} {r.target:20s}  loss={r.loss_pct:.1f}%  p50={r.latency_p50_ms:.1f}ms  p95={r.latency_p95_ms:.1f}ms  pps={r.throughput_pps:.0f}")
        print(f"{'='*60}\n")

    # Export
    if args.output:
        export_json(results, args.output)


if __name__ == "__main__":
    main()
