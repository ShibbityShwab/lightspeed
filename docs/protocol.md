# ⚡ LightSpeed Tunnel Protocol v1/v2

> Last updated: 2026-04-27 — v0.4.0-dev: compact FEC parity format, zero-alloc header encode

---

## Overview

The LightSpeed Tunnel Protocol is a lightweight UDP encapsulation protocol designed to route game traffic through proxy nodes with minimal overhead. It is:

- **Unencrypted** — game traffic remains inspectable (anti-cheat friendly)
- **IP-preserving** — original source/destination IPs carried in header
- **Low overhead** — 20 bytes added per packet (v1), 26 bytes with FEC (v2)
- **Sequence-numbered** — supports dedup for multipath routing
- **FEC-capable** — optional Forward Error Correction for packet loss recovery (v2)

---

## Protocol Versions

| Version | Header Size | Features | Status |
|---------|-------------|----------|--------|
| **v1** | 20 bytes | Plain tunneling, keepalive, handshake, FIN | ✅ Production |
| **v2** | 20 + 6 = 26 bytes | v1 + FEC header extension | ✅ Production |

The version field (4 bits) in byte 0 determines which header format is used.

---

## Header Format — v1 (20 bytes)

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
| **Version** | 4 | 0[7:4] | Protocol version (`1` = plain, `2` = FEC) |
| **Flags** | 4 | 0[3:0] | Packet type flags (see below) |
| **Reserved** | 8 | 1 | Session token byte (used for auth) |
| **Sequence** | 16 | 2-3 | Monotonically increasing packet sequence number |
| **Timestamp** | 32 | 4-7 | Microsecond timestamp for latency measurement |
| **Orig Src IP** | 32 | 8-11 | Original source IPv4 address |
| **Orig Dst IP** | 32 | 12-15 | Original destination IPv4 address |
| **Orig Src Port** | 16 | 16-17 | Original UDP source port |
| **Orig Dst Port** | 16 | 18-19 | Original UDP destination port |

**Total v1 header: 20 bytes.** Payload follows immediately after.

### Flags

| Bit | Name | Description |
|-----|------|-------------|
| 0 | `KEEPALIVE` | Keepalive probe (no payload) |
| 1 | `HANDSHAKE` | Handshake packet |
| 2 | `FIN` | Graceful tunnel close |
| 3 | `FRAGMENT` | Packet is a fragment (reserved for future) |

Multiple flags can be set simultaneously.

---

## Header Format — v2 FEC Extension (6 additional bytes)

When `Version = 2`, the v1 header is followed by a 6-byte FEC extension:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  [--- 20 bytes v1 header as above ---]                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|         FEC Group ID          |  Packet Index |  Group Size   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  FEC Flags    |   Reserved    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### FEC Extension Fields

| Field | Bits | Bytes | Description |
|-------|------|-------|-------------|
| **FEC Group ID** | 16 | 20-21 | Identifies which FEC group this packet belongs to |
| **Packet Index** | 8 | 22 | Index within the group (0..K-1 = data, K = parity) |
| **Group Size** | 8 | 23 | Total data packets in group (K) |
| **FEC Flags** | 8 | 24 | `0x01` = parity packet, `0x00` = data packet |
| **Reserved** | 8 | 25 | Must be `0x00` |

**Total v2 header: 26 bytes.** Payload follows immediately after.

### FEC Algorithm — XOR Parity

The FEC scheme uses simple XOR parity across a group of K data packets:

```
Group of K=4 data packets (variable length: 64, 128, 256, 64 bytes):
  max_len = 256   (max of all payload lengths in the block)

  P1 = [data1...] ‖ 0-pad to max_len
  P2 = [data2...] ‖ 0-pad to max_len
  P3 = [data3...] ‖ 0-pad to max_len
  P4 = [data4...] ‖ 0-pad to max_len

Parity content (max_len bytes):
  XOR  = P1 ⊕ P2 ⊕ P3 ⊕ P4  (byte-wise XOR, only first max_len bytes)

lengths_xor (2 bytes, big-endian):
  LX   = len(P1) ⊕ len(P2) ⊕ len(P3) ⊕ len(P4)  (cumulative XOR of lengths as u16)

Parity packet payload (v0.4.0-dev+):
  [XOR (max_len bytes)][LX (2 bytes BE)]  ← compact format, total = max_len + 2

Recovery (if P2 lost):
  xor_buf  = XOR ⊕ P1 ⊕ P3 ⊕ P4          (XOR in all received, missing region is P2)
  orig_len = LX  ⊕ len(P1) ⊕ len(P3) ⊕ len(P4)   (recover P2 exact length)
  P2       = xor_buf[..orig_len]
```

**Properties**:
- Recovers exactly 1 lost packet per group
- Overhead: 1 extra packet per K data packets (e.g., K=8 → 12.5% overhead)
- Parity computation: ~3μs (negligible latency impact)
- Recovery: ~3ms (vs 400ms+ for TCP-style retransmission)
- Parity packet is `max_payload_len_in_block + 2` bytes — much smaller than old fixed 1400 B
  for typical game traffic (64–512 B), saving ~10× parity bandwidth

### ⚠️ FEC Parity Wire-Format Compatibility

| Release | Parity payload format |
|---------|-----------------------|
| ≤ v0.3.x | Fixed 1400 B XOR buffer (zero-padded to FEC_MAX_PAYLOAD) |
| ≥ v0.4.0-dev | Compact: `[XOR content (max_len B)][lengths_xor (2 B BE)]` |

**Proxy and client must be on the same release for FEC recovery to work.**  
Mixed v0.3.x ↔ v0.4.0-dev deployments: parity packets are silently ignored (no crash,
but no recovery). Perform a coordinated rolling upgrade of all proxy nodes and clients.

### FEC Statistics (`FecStats`)

Both `FecEncoder` and `FecDecoder` track:
- `packets_encoded` / `packets_decoded` — total packets processed
- `parity_generated` / `packets_recovered` — FEC operations performed
- `packets_lost` — packets that could not be recovered (>1 loss per group)

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
- Session token in Reserved byte authenticates the client

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
  |--- HANDSHAKE (seq=0) ------->|  Client sends handshake with session token
  |                               |  Proxy records client addr + token
  |<-- HANDSHAKE ACK (seq=0) ----|  Proxy acknowledges (make_response)
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

---

## Keepalive Protocol

- **Client sends**: Keepalive every `keepalive_ms` (default: 5000ms)
- **Proxy responds**: Echo with same sequence number (via `make_response()`)
- **RTT measurement**: `RTT = recv_time - header.timestamp`
- **Liveness**: If `max_keepalive_misses` (default: 3) consecutive misses → trigger failover
- **Proxy cleanup**: If no activity for `session_timeout` (default: 300s) → remove session

---

## MTU Considerations

### v1 (Plain)
```
Typical Internet MTU:  1500 bytes
IP header:               20 bytes
UDP header:               8 bytes
LightSpeed v1 header:   20 bytes
Available payload:     1452 bytes
Safe payload target:   1400 bytes
```

### v2 (FEC)
```
Typical Internet MTU:  1500 bytes
IP header:               20 bytes
UDP header:               8 bytes
LightSpeed v2 header:   26 bytes
Available payload:     1446 bytes
Safe payload target:   1400 bytes
```

- Game packets typically 50-500 bytes → well within limits for both v1 and v2
- FEC parity packets are padded to the max payload size in the group

---

## Control Plane Protocol (QUIC)

The control plane uses binary-encoded messages over QUIC (feature-gated behind `quic`):

### Message Types

| Type ID | Name | Direction | Description |
|---------|------|-----------|-------------|
| 0x01 | `Ping` | Client → Proxy | Latency probe |
| 0x02 | `Pong` | Proxy → Client | Latency response (echoes timestamp) |
| 0x03 | `Register` | Client → Proxy | Register session (game_id, client_version) |
| 0x04 | `RegisterAck` | Proxy → Client | Confirm registration (session_id, session_token, node_id, region) |
| 0x05 | `Disconnect` | Either | Graceful disconnect (with reason code) |
| 0x06 | `ServerInfo` | Proxy → Client | Proxy metadata (node_id, region, load, version) |

### Wire Format

Messages are length-prefixed with a 2-byte big-endian length field:

```
[length: u16][type: u8][payload...]
```

### Game IDs

| ID | Game |
|----|------|
| 1 | Fortnite |
| 2 | CS2 |
| 3 | Dota 2 |
| 4 | Rust (Facepunch) |
| 5 | Valorant |
| 6 | Apex Legends |

### Disconnect Reason Codes

| Code | Reason |
|------|--------|
| 0 | Normal |
| 1 | Timeout |
| 2 | Auth Failed |
| 3 | Rate Limited |
| 4 | Server Shutdown |

---

## Security Considerations

1. **No encryption by design** — game traffic is inspectable (anti-cheat compatible)
2. **IP preservation** — game servers see real user IP (not proxy IP)
3. **Session tokens** — per-client authentication in header Reserved byte
4. **Rate limiting** — per-client PPS and BPS limits enforced by proxy
5. **Anti-amplification** — proxy tracks inbound/outbound byte ratio
6. **Anti-reflection** — proxy limits unique destinations per client per time window
7. **Destination validation** — proxy blocks private IPs, localhost, multicast, link-local
8. **No open relay** — only authenticated sessions can relay traffic

---

## Wire Format Examples

### v1 Data Packet (Fortnite, 200 byte payload)

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

### v1 Keepalive

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

### v2 FEC Data Packet (3rd packet in group of 8)

```
Offset  Hex
0000    20 00 00 2C  00 0F 42 40  C0 A8 01 64  68 1A 01 32  v2 header (20B)
0010    30 39 1E 61  00 07 02 08  00 00                      + FEC ext (6B)
001A    [game payload...]

Decoded:
  Version:  2 (FEC enabled)
  Flags:    0x00 (data)
  Sequence: 44
  FEC Group ID: 7
  Packet Index: 2 (3rd data packet, zero-indexed)
  Group Size:   8
  FEC Flags:    0x00 (data, not parity)
```

### v2 FEC Parity Packet

```
Offset  Hex
0000    20 00 00 30  00 0F 42 40  00 00 00 00  00 00 00 00  v2 header (20B)
0010    00 00 00 00  00 07 08 08  01 00                      + FEC ext (6B)
001A    [XOR parity payload...]

Decoded:
  Version:  2 (FEC enabled)
  Flags:    0x00
  Sequence: 48
  FEC Group ID: 7
  Packet Index: 8 (= group_size, so this is parity)
  Group Size:   8
  FEC Flags:    0x01 (PARITY)
```

---

## Implementation Reference

### Header (`protocol/src/header.rs`)
- `TunnelHeader::new()` — create data packet header
- `TunnelHeader::keepalive()` — create keepalive header
- `TunnelHeader::with_session_token()` — builder for auth token
- `TunnelHeader::make_response()` — swap src/dst for proxy reply
- `TunnelHeader::encode_to_array()` → `[u8; 20]` — **zero-alloc** stack encode _(v0.4.0-dev+, hot path)_
- `TunnelHeader::encode()` → `Bytes` — serialize to wire format (delegates to `encode_to_array`)
- `TunnelHeader::encode_with_payload(&[u8])` → `Bytes` — header + payload in one allocation
- `TunnelHeader::decode(&[u8])` → `Result<TunnelHeader>` — parse from wire format
- `TunnelHeader::decode_with_payload(&[u8])` → `Result<(TunnelHeader, &[u8])>` — parse header + slice payload

### FEC (`protocol/src/fec.rs`)
- `FecEncoder::new(k_size: u8)` — create encoder with K data packets per block
- `FecEncoder::add_packet(&[u8])` → `Option<Bytes>` — XOR into parity; returns compact parity when block full
- `FecEncoder::flush()` → `Option<(block_id, actual_k, Bytes)>` — force-emit partial block parity
- `FecEncoder::block_id()` → `u16` — current block ID
- `FecEncoder::current_index()` → `u8` — packets accumulated in current block
- `FecHeader::data(block_id, index, k_size)` — create FEC extension header for data packet
- `FecHeader::parity(block_id, k_size)` — create FEC extension header for parity packet
- `FecHeader::encode(&mut BytesMut)` — append 4-byte FEC header to buffer
- `FecHeader::decode(&mut &[u8])` → `Option<FecHeader>` — parse 4-byte FEC header
- `FecDecoder::new()` — create decoder (64-slot ring buffer, 500ms block expiry)
- `FecDecoder::receive_data(&FecHeader, Bytes)` → `Bytes` — track data packet; return payload
- `FecDecoder::receive_parity(&FecHeader, Bytes)` → `Option<(u8, Bytes)>` — store parity; attempt recovery
- `FecDecoder::try_recover_block(block_id)` → `Option<(u8, Bytes)>` — explicit recovery attempt
- `FecDecoder::gc()` — discard blocks older than `max_age_ms`
- `FecDecoder::stats()` → `FecStats` — `active_blocks`, `recovered_count`, `recovery_failures`

### Control (`protocol/src/control.rs`)
- `ControlMessage::read_from(stream)` — async QUIC message read
- `ControlMessage::write_to(stream)` — async QUIC message write
- `ControlMessage::encode()` / `decode()` — binary serialization

All encode/decode covered by unit tests (header: 8 tests, FEC: 10 tests + 6 proptests, control: 6 tests).
