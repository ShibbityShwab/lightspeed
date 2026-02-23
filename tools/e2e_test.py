#!/usr/bin/env python3
"""
LightSpeed E2E Tunnel Test

Tests the full data path:
  Client (BKK) → Proxy (LA/SGP) → Game Server (SGP echo) → Proxy → Client

Encodes LightSpeed tunnel headers matching protocol/src/header.rs format.
"""
import socket
import struct
import time
import sys

# ── Header format (20 bytes) ────────────────────────────────────────
# Byte 0:  (version << 4) | (flags & 0x0F)
# Byte 1:  session_token
# Bytes 2-3:  sequence (big endian u16)
# Bytes 4-7:  timestamp_us (big endian u32)
# Bytes 8-11: orig_src_ip (4 bytes)
# Bytes 12-15: orig_dst_ip (4 bytes)
# Bytes 16-17: orig_src_port (big endian u16)
# Bytes 18-19: orig_dst_port (big endian u16)

PROTOCOL_VERSION = 1
HEADER_SIZE = 20
FLAG_KEEPALIVE = 0x01


def ip_to_bytes(ip_str):
    """Convert dotted IP string to 4 bytes."""
    return socket.inet_aton(ip_str)


def bytes_to_ip(b):
    """Convert 4 bytes to dotted IP string."""
    return socket.inet_ntoa(b)


def encode_header(seq, timestamp_us, src_ip, src_port, dst_ip, dst_port,
                  flags=0, session_token=0):
    """Encode a LightSpeed tunnel header."""
    ver_flags = (PROTOCOL_VERSION << 4) | (flags & 0x0F)
    return struct.pack('!BBHI',
                       ver_flags, session_token, seq, timestamp_us) + \
           ip_to_bytes(src_ip) + ip_to_bytes(dst_ip) + \
           struct.pack('!HH', src_port, dst_port)


def decode_header(data):
    """Decode a LightSpeed tunnel header, return (header_dict, payload)."""
    if len(data) < HEADER_SIZE:
        raise ValueError(f"Too short: {len(data)} < {HEADER_SIZE}")

    ver_flags, token, seq, ts = struct.unpack('!BBHI', data[:8])
    version = ver_flags >> 4
    flags = ver_flags & 0x0F

    src_ip = bytes_to_ip(data[8:12])
    dst_ip = bytes_to_ip(data[12:16])
    src_port, dst_port = struct.unpack('!HH', data[16:20])

    header = {
        'version': version,
        'flags': flags,
        'session_token': token,
        'sequence': seq,
        'timestamp_us': ts,
        'src_ip': src_ip,
        'src_port': src_port,
        'dst_ip': dst_ip,
        'dst_port': dst_port,
    }
    return header, data[HEADER_SIZE:]


def now_us():
    """Current time in microseconds (wrapping u32)."""
    return int(time.time() * 1_000_000) & 0xFFFFFFFF


def test_keepalive(proxy_addr):
    """Test keepalive echo."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(5.0)

    header = encode_header(
        seq=60000,
        timestamp_us=now_us(),
        src_ip='0.0.0.0', src_port=0,
        dst_ip='0.0.0.0', dst_port=0,
        flags=FLAG_KEEPALIVE
    )

    t0 = time.perf_counter()
    sock.sendto(header, proxy_addr)

    try:
        data, addr = sock.recvfrom(2048)
        rtt_ms = (time.perf_counter() - t0) * 1000
        resp_hdr, _ = decode_header(data)
        print(f"  ✅ Keepalive echo: {rtt_ms:.1f}ms (seq={resp_hdr['sequence']}, "
              f"flags=0x{resp_hdr['flags']:02x})")
        return rtt_ms
    except socket.timeout:
        print(f"  ❌ Keepalive: TIMEOUT (5s)")
        return None
    finally:
        sock.close()


def test_data_relay(proxy_addr, game_server_ip, game_server_port, count=5):
    """Test full data relay through the proxy to a game server."""
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(5.0)

    # Get our local IP (for the header)
    local_ip = '127.0.0.1'
    local_port = sock.getsockname()[1] or 12345

    rtts = []
    for i in range(count):
        payload = f"LIGHTSPEED_E2E_TEST_{i}_{time.time():.0f}".encode()
        seq = i

        header = encode_header(
            seq=seq,
            timestamp_us=now_us(),
            src_ip=local_ip, src_port=local_port,
            dst_ip=game_server_ip, dst_port=game_server_port,
        )

        packet = header + payload
        t0 = time.perf_counter()
        sock.sendto(packet, proxy_addr)

        try:
            data, addr = sock.recvfrom(2048)
            rtt_ms = (time.perf_counter() - t0) * 1000
            resp_hdr, resp_payload = decode_header(data)
            rtts.append(rtt_ms)

            payload_match = resp_payload == payload
            print(f"  {'✅' if payload_match else '❌'} Packet {i}: "
                  f"{rtt_ms:.1f}ms, "
                  f"payload={'MATCH' if payload_match else 'MISMATCH'} "
                  f"({len(resp_payload)}B), "
                  f"seq={resp_hdr['sequence']}")

            if not payload_match:
                print(f"      Sent:     {payload[:40]}")
                print(f"      Received: {resp_payload[:40]}")

        except socket.timeout:
            print(f"  ❌ Packet {i}: TIMEOUT")

        time.sleep(0.1)  # Don't flood

    sock.close()
    return rtts


def main():
    print("=" * 60)
    print("LightSpeed E2E Tunnel Test")
    print("=" * 60)

    # Configuration
    proxies = [
        ("Vultr LA",    ("149.28.84.139", 4434)),
        ("Vultr SGP",   ("149.28.144.74", 4434)),
    ]
    game_server_ip = "149.28.144.74"  # SGP echo server
    game_server_port = 9999

    for name, proxy_addr in proxies:
        print(f"\n{'─' * 50}")
        print(f"Testing proxy: {name} ({proxy_addr[0]}:{proxy_addr[1]})")
        print(f"Game server:   {game_server_ip}:{game_server_port}")
        print(f"{'─' * 50}")

        # Test 1: Keepalive
        print(f"\n[1] Keepalive test:")
        ka_rtt = test_keepalive(proxy_addr)

        # Test 2: Data relay
        print(f"\n[2] Data relay test (5 packets):")
        rtts = test_data_relay(proxy_addr, game_server_ip, game_server_port, count=5)

        if rtts:
            avg = sum(rtts) / len(rtts)
            mn = min(rtts)
            mx = max(rtts)
            print(f"\n  📊 Results: avg={avg:.1f}ms, min={mn:.1f}ms, max={mx:.1f}ms")
            print(f"  📊 Packets: {len(rtts)}/5 received")
            if ka_rtt:
                overhead = avg - ka_rtt
                print(f"  📊 Relay overhead vs keepalive: {overhead:+.1f}ms")
        else:
            print(f"\n  ❌ No responses received - relay may not be working")

    print(f"\n{'=' * 60}")
    print("Test complete")
    print(f"{'=' * 60}")


if __name__ == '__main__':
    main()
