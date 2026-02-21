# ⚡ LightSpeed Tunnel Protocol v1

> WF-001 Step 2b Deliverable — UDP Tunnel Protocol Specification
> Agent: NetEng | Date: 2026-02-21

---

## Overview

The LightSpeed Tunnel Protocol is a lightweight UDP encapsulation protocol designed to route game traffic through proxy nodes with minimal overhead. It is:

- **Unencrypted** — game traffic remains inspectable (anti-cheat friendly)
- **IP-preserving** — original source/destination IPs carried in header
- **Low overhead** — 20 bytes added per packet
- **Sequence-numbered** — supports dedup for multipath routing

---

## Header Format (20 bytes)

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Ver  | Flags |   Reserved    |         Sequence Number       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Timestamp (μs)                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Original Source IP                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Original Dest IP                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Orig Source Port          |     Orig Dest Port            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Field Descriptions

| Field | Bits | Bytes | Description |
|-------|------|-------|-------------|
| **Version** | 4 | 0[7:4] | Protocol version (currently `1`) |
| **Flags** | 4 | 0[3:0] | Packet type flags (see below) |
| **Reserved** | 8 | 1 | Must be `0x00`. Reserved for future use. |
| **Sequence** | 16 | 2-3 | Monotonically increasing packet sequence number |
| **Timestamp** | 32 | 4-7 | Microsecond timestamp for latency measurement |
| **Orig Src IP** | 32 | 8-11 | Original source IPv4 address |
| **Orig Dst IP** | 32 | 12-15 | Original destination IPv4 address |
| **Orig Src Port** | 16 | 16-17 | Original UDP source port |
| **Orig Dst Port** | 16 | 18-19 | Original UDP destination port |

**Total header: 20 bytes.** Payload follows immediately after.

### Flags

| Bit | Name | Description |
|-----|------|-------------|
| 0 | `KEEPALIVE` | Keepalive probe (no payload) |
| 1 | `HANDSHAKE` | Handshake packet |
| 2 | `FIN` | Graceful tunnel close |
| 3 | `FRAGMENT` | Packet is a fragment (reserved for future) |

Multiple flags can be set simultaneously.

---

## Packet Types

### Data Packet
- Flags: `0x00`
- Carries game UDP payload
- Proxy strips header and forwards payload to `Orig Dst IP:Orig Dst Port`

### Keepalive
- Flags: `KEEPALIVE (0x01)`
- No payload
- Sent by client every 5 seconds (configurable)
- Proxy responds with matching keepalive (echoes sequence number)
- Used for latency measurement and liveness detection

### Handshake
- Flags: `HANDSHAKE (0x02)`
- Sent when establishing a new tunnel session
- Proxy records client address and begins accepting data packets
- Payload: JSON-encoded session info (game, client version)

### Fin
- Flags: `FIN (0x04)`
- Graceful tunnel close
- Proxy cleans up session state
- No payload

---

## Handshake Sequence

```
Client                          Proxy
  |                               |
  |--- HANDSHAKE (seq=0) ------->|  Client sends handshake
  |                               |  Proxy records client addr
  |<-- HANDSHAKE ACK (seq=0) ----|  Proxy acknowledges
  |                               |
  |--- DATA (seq=1) ------------>|  Start sending game packets
  |<-- DATA (seq=1) -------------|  Proxy relays responses
  |                               |
  |--- KEEPALIVE (seq=N) ------->|  Periodic keepalive
  |<-- KEEPALIVE (seq=N) --------|  Proxy echoes
  |                               |
  |--- FIN (seq=M) ------------->|  Client closes tunnel
  |<-- FIN ACK (seq=M) ----------|  Proxy confirms
  |                               |
```

### Handshake Payload (JSON)

```json
{
  "version": "0.1.0",
  "game": "fortnite",
  "client_id": "optional-anonymous-id"
}
```

---

## Keepalive Protocol

- **Client sends**: Keepalive every `keepalive_ms` (default: 5000ms)
- **Proxy responds**: Echo with same sequence number
- **RTT measurement**: `RTT = recv_time - header.timestamp`
- **Liveness**: If `max_keepalive_misses` (default: 3) consecutive misses → trigger failover
- **Proxy cleanup**: If no activity for `session_timeout` (default: 300s) → remove session

---

## MTU Considerations

```
Typical Internet MTU:  1500 bytes
IP header:               20 bytes
UDP header:               8 bytes
LightSpeed header:       20 bytes
Available payload:     1452 bytes
Safe payload target:   1400 bytes (margin for IP options, tunnels)
```

- **MAX_PAYLOAD_SIZE**: 1352 bytes (1400 - 20 IP - 8 UDP - 20 LightSpeed)
- Game packets typically 50-500 bytes → well within limits
- Fragmentation: Reserved (flag bit 3) but not needed for typical game traffic

---

## Error Handling

### Error Codes (in Reserved byte, future use)

| Code | Name | Description |
|------|------|-------------|
| 0x00 | OK | No error |
| 0x01 | AUTH_REQUIRED | Client not authenticated |
| 0x02 | RATE_LIMITED | Rate limit exceeded |
| 0x03 | SESSION_EXPIRED | Session timed out |
| 0x04 | PROXY_OVERLOADED | Proxy at capacity |
| 0xFF | INTERNAL_ERROR | Proxy internal error |

### Handling Unknown Versions
- Proxy MUST reject packets with unknown version
- Proxy SHOULD respond with its supported version

---

## Security Considerations

1. **No encryption by design** — game traffic is inspectable
2. **IP preservation** — game servers see real user IP (not proxy IP)
3. **Auth via control plane** — QUIC handshake authenticates clients
4. **Rate limiting** — per-client PPS and BPS limits
5. **Anti-amplification** — proxy tracks inbound/outbound ratio
6. **Anti-reflection** — proxy limits unique destinations per client
7. **No open relay** — only authenticated clients can relay traffic

---

## Wire Format Examples

### Data Packet (Fortnite, 200 byte payload)

```
Offset  Hex                                           ASCII
0000    10 00 00 2A  00 0F 42 40  C0 A8 01 64  68 1A 01 32  ...*..B@...dh..2
0010    30 39 1E 61                                          09.a
0020    [200 bytes of game payload...]

Decoded:
  Version:  1
  Flags:    0x00 (data)
  Reserved: 0x00
  Sequence: 42
  Timestamp: 1,000,000 μs
  Src IP:   192.168.1.100
  Dst IP:   104.26.1.50
  Src Port: 12345
  Dst Port: 7777
```

### Keepalive

```
Offset  Hex
0000    11 00 00 05  00 0F 42 40  00 00 00 00  00 00 00 00
0010    00 00 00 00

Decoded:
  Version:  1
  Flags:    0x01 (KEEPALIVE)
  Sequence: 5
  Timestamp: 1,000,000 μs
  [all address fields zero]
  [no payload]
```

---

## Implementation Reference

The header is implemented in `client/src/tunnel/header.rs`:
- `TunnelHeader::new()` — create data packet header
- `TunnelHeader::keepalive()` — create keepalive header
- `TunnelHeader::encode()` → `Bytes` — serialize to wire format
- `TunnelHeader::decode(&[u8])` → `Result<TunnelHeader>` — parse from wire format

All encode/decode is covered by unit tests (5 tests, all passing).
