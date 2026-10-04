# TCP Game Support: Minecraft Java Edition over the LightSpeed UDP Tunnel

Design decision record. This document is the complete, implementable specification
for adding TCP game traffic (first target: Minecraft Java Edition) to LightSpeed's
UDP-only tunnel. Every file, struct, field, filter, and byte below is named as it
exists today in this repository; workers implementing from this document alone
should never need to make a protocol-level decision.

Scope of the change: the tunnel gains one new packet type (protocol version 5)
that carries TCP payloads inside the existing UDP datagram path, TCP is
terminated at both ends of the tunnel (client-side terminator, proxy-side
connector), and the Minecraft Java profile is registered as a TCP game. The
existing UDP path (versions 3 and 4) is not touched.

---

## Mechanism

The chosen mechanism is **TCP termination at both ends with an explicit
sequenced-reliable datagram channel in between** — a splice, not a transparent
TCP-in-TCP tunnel:

- **Client side (terminator):** the WinDivert intercept path answers the game's
  TCP handshake itself, spoofing SYN-ACK/ACK/FIN segments from the real server's
  IP. The game's TCP stack talks to a user-mode connection state machine; the
  game's stream payloads are pulled out of the intercepted segments and sent
  through the existing UDP tunnel.
- **Proxy side (connector):** the relay opens its own `TcpStream` to the real
  game server and splices bytes: tunnel payload → server socket, server bytes →
  tunnel packets back to the client. The real server never sees the game's TCP
  segments or its handshake; it sees an ordinary connection from the proxy.
- **Middle (the tunnel):** the client-to-proxy UDP path stays exactly what it is
  today — `TunnelHeader` (24 bytes) + optional FEC + payload, protected by the
  existing FEC/adaptive loss recovery in `client/src/tunnel/relay.rs`. On top of
  it we add a per-connection **sequenced reliable delivery layer** carried in a
  new 12-byte extension header (`TcpExtHeader`, protocol version 5). FEC stays
  underneath it and treats the `TcpExtHeader` as part of the protected payload
  (the FEC layer encodes/decodes opaque bytes; it never parses the TCP
  extension), so a lost tunnel datagram is recovered by FEC when possible, and
  anything FEC cannot recover is retransmitted by the reliability layer.

**Why TCP-in-TCP is forbidden here.** Carrying the game's real TCP segments
inside the tunnel unmodified is off the table for two independent reasons:

1. **The tunnel may itself ride TCP.** `client/src/tunnel/transport.rs`
   (`TunnelTransport::connect_tcp`) already supports a TCP fallback transport
   for UDP-restricted networks. Raw TCP-in-TCP nests two congestion controllers
   and two retransmission loops. When the inner TCP retransmits after outer-TCP
   loss, the outer flow sees duplicate data as reordering and inflates its RTO;
   when the outer TCP retransmits, the inner TCP sees sudden delay spikes and
   its own timer fires. The loops amplify each other into a retransmission
   storm, exactly the failure mode game tunnels cannot afford.
2. **The tunnel is lossy by design** (`client/src/tunnel/relay.rs` does FEC
   recovery, no ordering, no retransmit). TCP segments forwarded raw through it
   would arrive reordered and with holes; the game's stack would collapse the
   congestion window on every tunnel loss. Unmodified forwarding is a
   correctness failure, not just a performance one.

Termination removes the inner TCP entirely, so no TCP-in-TCP interaction can
exist. The rule for workers: **`TcpExtHeader` packets are forbidden on
`TunnelTransport::Tcp` legs.** The client must refuse to enable the TCP game
path when the transport fallback is active.

**Sequenced reliable delivery — sequence reuse vs extension.** The tunnel
already has a `u16 sequence` in `TunnelHeader` (protocol/src/header.rs). It is a
per-datagram counter shared by all traffic on the leg and consumed by FEC block
bookkeeping, keepalive echo, and the breaker. It is not per-connection and it
does not count bytes. Reusing it for TCP ordering would corrupt FEC block
semantics and cannot be fixed without breaking versions 3/4. Therefore:

- `TunnelHeader.sequence` is **unchanged and reused as-is** (datagram
  sequencing for the leg).
- The reliability layer gets its own **`u32 tcp_seq` and `u32 tcp_ack`** in the
  new `TcpExtHeader`, one number space **per connection per direction**,
  counting stream bytes (SYN consumes sequence 1; the first data byte is 2).
  This is the RFC 793 convention and makes the header self-describing for
  proxy-side demux.

**Retransmit / ACK design (per connection, per direction).**

- Sender keeps up to 64 unacknowledged segments in a retransmit buffer keyed by
  `tcp_seq`, each with a send time. Cumulative ACK (`tcp_ack`) slides the
  window; fast retransmit fires on 3 duplicate ACKs; otherwise an RTO starting
  at 200 ms with exponential backoff and a 15 s cap drives retransmission.
  RTO resets on new ACK progress. Bounded in-flight count (64) also caps memory
  per connection (64 × ~1.4 KB ≤ ~92 KB).
- Receiver keeps a reorder window of up to 64 out-of-order segments keyed by
  `tcp_seq`, emits a cumulative ACK immediately (no delayed-ACK timer; the
  channel is a single logical pipe, not a congested link), and drops segments
  older than the window so the sender retransmits them.
- Segments and ACKs on one direction may ride together (piggyback) in one
  datagram: one `TcpExtHeader` carries `tcp_seq` for the payload it moves plus
  `tcp_ack` for the opposite direction. An ACK-only packet carries a zero-length
  payload.

**Per-connection TCP state machine (terminator and connector share it).**

States: `SynRcvd`, `Established`, `FinWait1`, `FinWait2`, `Closing`, `Closed`.
Transitions driven by `TcpExtFlags` on the tunnel side and by real TCP segments
on the game/server side:

- **SYN (game → terminator):** terminator answers the game with a spoofed
  SYN-ACK. The SYN-ACK's ISN is a **SYN cookie**: deterministic per 5-tuple
  (`HMAC-SHA256(key_epoch, src_ip, src_port, dst_ip, dst_port)` truncated to 32
  bits). The key epoch rotates every 5 minutes and the terminator keeps the
  previous epoch for a 2-minute overlap, so a game ACK arriving across a
  rotation still validates. The proxy does not need this ISN at all — the
  proxy-side connector runs the OS TCP stack, which picks its own ISN toward
  the real server. The cookie exists so the terminator can validate the game’s
  ACK with zero half-open state on the client. A tunnel SYN packet (zero payload,
  `TcpExtFlags::SYN`) is sent to the proxy to create the connection there.
  The game's ACK of the SYN-ACK completes the handshake; its sequence number is
  validated against the cookie by the terminator.
- **Data:** terminator extracts the segment payload, buffers/orders it via the
  reliability layer, and sends tunnel v5 packets. Connector writes ordered
  payloads to its `TcpStream`. Reverse direction mirrors this.
- **ACK:** game ACKs are consumed locally by the terminator (they acknowledge
  the spoofed stack); tunnel `tcp_ack` is the transport-level acknowledgement.
- **FIN (game → terminator):** terminator ACKs the game's FIN, drains, sends a
  tunnel FIN (zero payload, `TcpExtFlags::FIN`), moves to `FinWait1`; tunnel
  FIN-ACK and the peer's FIN advance it to `Closed`. The proxy forwards FIN to
  the real server via `TcpStream::shutdown` and mirrors the sequence back.
- **RST:** a game RST or a server RST is propagated as a zero-payload tunnel
  packet with `TcpExtFlags::RST`; the peer tears the connection down
  immediately (both directions, no FIN dance). A tunnel failure or relay
  rejection synthesizes a local RST to the game so its socket does not hang in
  `SYN_SENT`/`ESTABLISHED`.
- **Idle:** a connection with no traffic for 300 s is torn down with RST on
  both ends (matches the proxy's existing UDP session-timeout posture).

**Connection tracking tables, keyed by 5-tuple.**

- Client: `HashMap<Tcp5Tuple, TcpConn>` in the WinDivert redirect task.
  `Tcp5Tuple = (src_ip, src_port, dst_ip, dst_port)` — the game's ephemeral
  port disambiguates concurrent connections (Minecraft opens one, but the
  design must not assume it).
- Proxy: `HashMap<Tcp5Tuple, TcpConnState>` in `proxy/src/relay.rs`, keyed by
  the same four fields **plus nothing else needed** because the tunnel is one
  UDP socket per client (today's `client_addr` is implicit in the receive
  path). The client's UDP source port is not part of the connection key — the
  tunnel is per-client anyway — but the 5-tuple is carried verbatim in
  `TunnelHeader` and is what gets forwarded to the real server, which is what
  the server-side socket must reflect.

Anti-cheat posture follows `wat/rules.md` [TRANSPARENCY_STUB]: tunnel payloads
are **never modified**. The terminator rewrites only TCP headers (seq/ack/flags/
window) at the local termination point, never the game payload bytes; the
connector writes payload bytes verbatim into its server socket.

---

## Wire changes

All changes are in `protocol/src/` (crate `lightspeed-protocol`).

### protocol/src/header.rs

- Add `pub const PROTOCOL_VERSION_TCP: u8 = 5;` next to
  `PROTOCOL_VERSION` (= 3) and `PROTOCOL_VERSION_FEC` (= 4). The version nibble
  is 4 bits, so 5 is free. Meaning of version 5: **FEC header follows the
  tunnel header, then `TcpExtHeader`, then payload.** v5 always carries both
  extensions (TCP without FEC is out of scope — the lossy leg needs FEC).
- `TunnelHeader::decode` currently rejects everything but 3 and 4 with
  `DecodeError::UnsupportedVersion`; extend the accepted set to include 5.
  `has_fec()` returns true for both 4 and 5 (v5 packets still start with the
  4-byte `FecHeader`). Add `pub fn has_tcp(&self) -> bool { self.version ==
  PROTOCOL_VERSION_TCP }`.
- No changes to `HEADER_SIZE` (24), field order, `flags` nibble, or
  `encode_to_array`. v3/v4 wire bytes are byte-identical after this change.

### protocol/src/tcp_ext.rs (new file)

```rust
pub const TCP_EXT_HEADER_SIZE: usize = 12;

/// Flags byte inside TcpExtHeader.
pub mod tcp_flags {
    pub const SYN: u8 = 0x01;
    pub const ACK: u8 = 0x02;
    pub const FIN: u8 = 0x04;
    pub const RST: u8 = 0x08;
    pub const PSH: u8 = 0x10;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpExtHeader {
    pub tcp_seq: u32,   // per-connection, per-direction stream sequence
    pub tcp_ack: u32,   // cumulative ACK for the opposite direction
    pub flags: u8,      // tcp_flags::*
    pub window: u8,     // reserved for flow control (0 in v1)
    pub reserved: [u8; 2],
}

pub struct Tcp5Tuple {
    pub src_ip: Ipv4Addr, pub src_port: u16,
    pub dst_ip: Ipv4Addr, pub dst_port: u16,
}
```

Wire layout of a v5 data packet (offsets relative to packet start):

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 24 | `TunnelHeader` (byte 0 high nibble = 5) |
| 24 | 4 | `FecHeader` (block_id u16, index u8, k_size u8) |
| 28 | 4 | `TcpExtHeader.tcp_seq` (big-endian) |
| 32 | 4 | `TcpExtHeader.tcp_ack` (big-endian) |
| 36 | 1 | `TcpExtHeader.flags` |
| 37 | 1 | `TcpExtHeader.window` |
| 38 | 2 | `TcpExtHeader.reserved` (zero) |
| 40 | n | game stream payload |

API: `TcpExtHeader::encode(&mut BytesMut)` / `decode(&mut &[u8])`,
`build_tcp_packet(header, fec, tcp_ext, payload) -> BytesMut`,
`decode_tcp_payload(payload_slice) -> Option<(TcpExtHeader, &[u8])>`,
`derive_syn_cookie(key: &[u8; 32], tuple: &Tcp5Tuple) -> u32`. The reliability
layer (sender window, reorder window, RTO, fast retransmit) lives here as
`TcpReliable` (encode/decode-side halves) so client and proxy share one
implementation. Export everything from `protocol/src/lib.rs`.

### Backward compatibility (hard guarantees)

- **Old relays, old clients:** v5 is rejected by the old
  `TunnelHeader::decode` version check (`UnsupportedVersion { version: 5 }`) and
  the packet is dropped by the existing malformed-packet path. Old clients
  never emit v5. Old relays are therefore completely unaffected — they keep
  serving v3/v4, keepalives, and FEC exactly as today.
- **New client vs old relay:** the client must not emit v5 unless the relay
  advertised TCP capability. Add a one-byte capabilities bitmap to the QUIC
  control-plane registration response (protocol/src/control.rs — extend the
  existing registration/ack message with `caps: u8`, bit 0 = TCP tunnel
  support; the old relay's encoder simply omits the field, which decodes as
  zero, and zero means "no TCP"). Until the relay advertises it, the TCP path
  is inert and the UDP path is used unchanged.
- **New relay vs old client:** old clients send v3/v4 only; the relay's new
  code branches on `header.has_tcp()` and keeps the old branch byte-for-byte.
- **Session token rotation:** the existing `RotateRequest`/`RotateAck` window
  (both tokens valid during rotation) applies to v5 unchanged — the token is in
  the shared `TunnelHeader` and is validated per packet by the same
  `auth::validate(principal, data_port, token, expiry)` call.

---

## Capture changes

All in `client/src/capture/windivert_redirect.rs` (plus one call site in
`client/src/engine.rs` and one in `client/src/modes/redirect_windivert.rs`).

1. **Filter (line ~276, the `out_filter` construction).** Today every branch
   starts with `udp and outbound ...`. Change the proto token to
   `config.Transport` and emit, for a TCP profile:
   - manual mode:
     `tcp and outbound and ip.DstAddr == <server_ip> and tcp.DstPort == <port>`
   - auto-detect, single port: `tcp and outbound and tcp.DstPort == <port>`
   - auto-detect, range:
     `tcp and outbound and tcp.DstPort >= <lo> and tcp.DstPort <= <hi>`
   UDP profiles keep today's strings verbatim. The inject handle stays filter
   `"false"` (injection only) — unchanged.
2. **`WinDivertConfig`** gains `pub transport: TransportProto` where
   `enum TransportProto { Udp, Tcp }` (default `Udp` for every existing
   call site in `engine.rs`/`redirect_windivert.rs`, so no existing behavior
   shifts).
3. **New parsers/builders next to `parse_ipv4_udp`/`build_ipv4_udp`:**
   - `parse_ipv4_tcp(raw) -> Option<(SocketAddrV4, SocketAddrV4, u8 /*flags*/, &[u8] /*payload*/)>`
     — same IHL validation as the UDP parser (reuse its checks, replacing the
     `raw[9] != 17` protocol test with `!= 6`), then read the TCP data-offset
     nibble (`raw[ihl + 12] >> 4`, must be ≥ 5), flags at `raw[ihl + 13]`,
     and payload at `raw[ihl + data_offset*4 ..]`.
   - `build_ipv4_tcp(src, dst, seq, ack, flags, payload) -> Vec<u8>` — IPv4
     header with protocol byte 6, 20-byte TCP header (data offset 5, window
     64240), `seq`/`ack`/`flags` set by the caller. TCP checksum: zero it and
     let WinDivert recompute on injection — the exact convention the existing
     `build_ipv4_udp` path already relies on (zeroed checksum, inject flags 0,
     `WinDivertHelperCalcChecksums`). If a receive-path tool ever needs it,
     compute it over the IPv4 pseudo-header in userspace; never inject a
     wrong-checksum TCP segment (the OS stack silently drops it and the game
     hangs in handshake).
4. **Intercept thread:** after `parse_ipv4_udp`, add the TCP arm: parse
   `parse_ipv4_tcp`, build the `Tcp5Tuple`, and send
   `(Tcp5Tuple, flags, payload)` to the tunnel task instead of the UDP
   triple. **Do not re-inject TCP segments unchanged** — the terminator owns
   them. All non-TCP/UDP traffic keeps the existing re-inject fallback.
5. **Tunnel task (the `tokio::select!` loop):** add the terminator.
   - Outbound arm: for TCP tuples, run the state machine described in
     Mechanism. On game SYN → send tunnel v5 SYN packet, answer the game with
     `build_ipv4_tcp(spoof_src=server, dst=game, seq=cookie_isn, ack=game_seq+1,
     flags=SYN|ACK)`. On data → hand the payload to the shared `TcpReliable`
     encoder, wrap as v5, send via the existing `tunnel_socket.send_to(proxy)`.
     FEC: reuse the existing `FecEncoder` — the terminator calls the same
     `build_fec_data_packet` path with `TcpExtHeader` inserted after
     `FecHeader`; parity packets carry the tunnel header but **no** TcpExt.
   - Inbound arm: `TunnelHeader::decode_with_payload`; when `header.has_tcp()`,
     run `decode_tcp_payload`, feed the `TcpReliable` receiver, then inject
     ordered payloads into the game via
     `build_ipv4_tcp(server, game, server_seq, ack, ACK|PSH, payload)` through
     the existing inject channel (the channel carries raw IP frames; TCP frames
     flow through it unchanged).
   - The inject-channel drop path ("Inject channel full") and stats counters
     are unchanged in shape; add `tcp_syn_answered`, `tcp_data_injected`,
     `tcp_retransmits` counters to `WinDivertStats`.
6. **`client/src/tunnel/relay.rs`:** no behavioral change to the UDP path. The
   one rule to enforce here: refuse to start a TCP-profile game when
   `TunnelTransport::Tcp` is the active transport (TCP-in-TCP ban).
7. **Windows Firewall helper** (`add_windivert_firewall_rule`): for TCP
   profiles also add an inbound allow rule
   `protocol=TCP remoteip=<server_ip> localport=<game_local_port>` so Windows
   Firewall does not drop the spoofed SYN-ACK/data segments injected toward
   the game.

---

## Relay changes

All in `proxy/src/relay.rs` (crate `lightspeed-proxy`).

1. **Inbound packet path** (`process_inbound_packet`): the existing
   keepalive → auth → FIN → abuse checks run **before** any protocol branch and
   stay untouched (they operate on the shared `TunnelHeader`, so v5 inherits
   auth, rate limiting, and destination validation for free). After the FEC
   parse, branch on `header.has_tcp()`:
   - v3/v4 → today's UDP session path, byte-for-byte.
   - v5 → FEC-decode exactly like v3/v4 first (the FEC layer treats the
     `TcpExtHeader` as opaque protected bytes; recover what the block can),
     then `decode_tcp_payload` to get `TcpExtHeader`; look up
     `(src_ip, src_port, dst_ip, dst_port)` in the new **TCP connection
     table** `HashMap<Tcp5Tuple, TcpConnState>` on `RelayEngine`. The TCP
     connection carries its own per-connection `FecDecoder` so a block's
     parity packets and its data packets converge on the same connection.
2. **Per-5-tuple `TcpConnState`:** `{ upstream: tokio::net::TcpStream,
   buffer: BytesMut, client_seq/ack state (TcpReliable), established: bool,
   last_activity: Instant }`. On `TcpExtFlags::SYN` with no entry: create the
   state, spawn a task that dials `dst_ip:dst_port` with a 5 s connect timeout;
   payloads arriving before the dial completes are buffered in order and
   written once connected; a failed dial sends a v5 RST to the client. On data:
   write the ordered payload to `upstream`; on `FIN`: `shutdown(Write)`; on
   `RST`: drop the state and abort the upstream.
3. **Server → client direction:** the per-connection task reads from
   `upstream`, runs bytes through the shared `TcpReliable` encoder (its own
   `tcp_seq` space), wraps v5 responses
   (`TunnelHeader::make_response` + FEC encode with the per-connection
   `FecEncoder`, same K as the client's leg), and sends on the client's data
   socket via the existing `ClientSender::Udp`. The tunnel ACK field
   acknowledges client bytes; the client's terminator turns arriving ordered
   bytes into injected TCP segments.
4. **Demux table lifecycle:** the table shares the engine's session-timeout
   sweep (300 s idle → RST both sides + drop entry), `max_sessions` budget, and
   egress-budget accounting (each connection counts like one session). The
   existing `sessions: HashMap<SocketAddrV4, Arc<ClientSession>>` **is not
   repurposed** — it is keyed by client address and bound to the UDP data
   plane; giving it TCP keys would corrupt the UDP session sweep.
5. **Budget/auth/abuse:** `auth::validate` is already per-packet
   (principal, data_port, token, expiry) and needs no signature change. The
   abuse detector's private-destination and allowlist checks run before the
   v5 branch, so TCP connections get the same destination policy as UDP.
6. **Old-proxy compatibility:** old relays decode v5 as `UnsupportedVersion`
   and drop it on the existing malformed path — nothing else on the proxy sees
   v5 packets, which is why the capability advertisement (Wire changes) is
   mandatory before the client enables the path.

---

## Minecraft Java profile

File: `client/src/games/minecraft.rs` (Bedrock config stays as-is).

1. **New `MinecraftJavaConfig`** implementing `GameConfig` (trait in
   `client/src/games/mod.rs`):
   - `name() -> "Minecraft Java Edition"`
   - `process_names() -> &["javaw.exe", "java.exe", "Minecraft.exe"]` —
     **process match caveat:** `javaw.exe`/`java.exe` are shared by every Java
     application, so auto-detection must not stop at the name. When the
     process scanner learns the image name, it must additionally verify the
     command line contains `net.minecraft.client.main.Main` (the launcher's
     `--mainClass` argument) before locking in this profile; otherwise
     fall through to Bedrock/other games. The scanner today
     (`client/src/interceptor/process_scanner.rs`) only maps process names to
     UDP sockets (`netstat -anou`); extend it with a TCP table
     (`netstat -anot`) and add `TransportProtocol::Tcp` to the
     `Route` enum so the interceptor can lock onto the game's 25565 socket.
   - Default port **25565**: the profile's TCP port range is
     `(25565, 25565)`.
2. **Trait changes in `client/src/games/mod.rs`:** add
   `fn tcp_ports(&self) -> (u16, u16) { (0, 0) }` (default: no TCP), and make
   `build_capture_filter()` emit `tcp port`/`tcp portrange` BPF when
   `tcp_ports()` is non-zero (Bedrock and every other UDP game keep `udp` BPF
   because their `tcp_ports()` default is `(0,0)`). Add a
   `transport()` accessor (`TransportProto::Tcp` for the Java profile) that
   `WinDivertConfig` consumes. `redirect_port()` returns 25565 for Java.
3. **Registration:** add `"minecraft-java" | "java"` to the `detect_game`
   match and append `MinecraftJavaConfig` to `all_games()` in
   `client/src/games/mod.rs`. `auto_detect()` gains the command-line
   verification step above.
4. **`redirect_instructions()`** for Java: connect to the server through
   LightSpeed with `--game minecraft-java --game-server <SERVER_IP>:25565`;
   servers on custom ports pass that port. Update the Bedrock profile's
   doc comment and issue-#137 note to say Java is now supported via the TCP
   path instead of "out of scope".
5. **Not in scope:** Bedrock stays UDP 19132/19133 with zero changes.

---

## Test spec

The protocol-level tests that **fail against the old code** (i.e., they are
the proof the change landed):

### 1. `tcp_v5_wire_layout_roundtrip` — `protocol/src/tcp_ext.rs`

This is the primary test. It pins the exact 42-byte v5 fixture below, decodes
it, and asserts every field and a byte-identical re-encode:

```
src = 192.168.1.100:12345, dst = 104.26.1.50:25565
tunnel header: version=5, flags=0, token=0xDEADBEEF, seq=42,
               timestamp_us=1_000_000
fec header:    block_id=0x0000, index=0, k_size=4
tcp ext:       tcp_seq=1, tcp_ack=2, flags=SYN, window=0
payload:       "MC"

5F 00 DE AD BE EF 00 2A 00 0F 42 40 C0 A8 01 64 68 1A
01 32 30 39 63 E1 00 00 00 00 00 04 00 00 00 01 00
00 00 02 01 00 00 00 4D 43
```

Breakdown (byte offsets): `[0]=0x5F` (version 5, flags 0), `[1..5]` token (`DE AD BE EF`),
`[5..7]=0x002A` tunnel seq 42, `[7..11]=0x000F4240` timestamp 1,000,000,
`[11..15]=C0 A8 01 64` src IP, `[15..19]=68 1A 01 32` dst IP,
`[19..21]=0x3039` src port 12345, `[21..23]=0x63E1` dst port 25565,
`[23]=0x00` reserved, `[24..28]=00 00 00 00 00 04` FEC header (block 0,
index 0, k 4 — note FecHeader::encode writes block_id u16, index u8, k_size
u8), `[28..32]=0x00000001` tcp_seq, `[32..36]=0x00000002` tcp_ack,
`[36]=0x01` SYN, `[37]=0x00` window, `[38..40]=00 00` reserved,
`[40..42]=4D 43` payload.

- **Old-code behavior it falsifies:** `TunnelHeader::decode` accepts only
  versions 3 and 4; on this fixture it returns
  `Err(UnsupportedVersion { version: 5, expected: 3 })`, so
  `decode_with_payload` errors and the test fails at the first assert. The
  test therefore cannot pass until the version-5 decode acceptance in
  `protocol/src/header.rs` lands, and the byte-pinned re-encode cannot pass
  until `TcpExtHeader` exists with exactly this layout. Any drift in
  `TcpExtHeader` field order or size breaks the fixture comparison.
- **Companion negative test** `unsupported_version_pins_old_accepted_set`: with
  the new code in place, assert that a decoder gated to the old accepted-set
  (the version check compared directly against `PROTOCOL_VERSION` /
  `PROTOCOL_VERSION_FEC`) still yields `UnsupportedVersion { version: 5,
  expected: 3 }` for this fixture. This pins what an old relay does when it
  receives the new wire format — reject and drop — which is the
  backward-compatibility contract.

### 2. `syn_cookie_is_deterministic_and_keyed` — `protocol/src/tcp_ext.rs`

`derive_syn_cookie` on the same `Tcp5Tuple` and key epoch returns the same
u32 twice; a different key epoch or a different tuple port returns a
different value. Old code fails to compile (no `tcp_ext` module), and the
test pins the client-local ISN contract that lets the terminator validate the
game's handshake ACK with zero half-open state.

### 3. `parse_ipv4_tcp_reads_data_offset` — `client/src/capture/windivert_redirect.rs`

Existing test module (next to the UDP parser tests). Fixture: minimal IPv4
header (IHL 5, protocol 6) + 20-byte TCP header with data offset 5, SYN set,
seq `0x01020304`, 3-byte payload. Asserts flags, seq, and payload slice. Old
code: `parse_ipv4_udp` rejects protocol 6 (there is a test today asserting
exactly that), and `parse_ipv4_tcp` does not exist — the test cannot compile
or pass until the capture changes land.

Note for workers: the wire-layout test above is the only test that pins prose
about the protocol; every other assertion is on machine-consumed bytes.

---

## File map

**protocol/** (crate `lightspeed-protocol`)
- `protocol/src/header.rs` — add `PROTOCOL_VERSION_TCP = 5`; accept v5 in
  `decode`; add `has_tcp()`; extend the `has_fec()` comment.
- `protocol/src/tcp_ext.rs` — **new**: `TcpExtHeader` (12 B), `Tcp5Tuple`,
  `tcp_flags`, build/decode helpers, `derive_syn_cookie`, `TcpReliable`
  (retransmit/ACK windows).
- `protocol/src/lib.rs` — export the new module's public items.
- `protocol/src/control.rs` — add the 1-byte capability bitmap to the
  registration ack (`caps`, bit 0 = TCP tunnel).

**client/** (crate `lightspeed-client`)
- `client/src/games/minecraft.rs` — add `MinecraftJavaConfig` (port 25565,
  javaw/java process names with command-line caveat); update Bedrock docs.
- `client/src/games/mod.rs` — add `tcp_ports()`/`transport()` defaults to
  `GameConfig`; TCP-aware `build_capture_filter()`; register
  `minecraft-java` in `detect_game` and `all_games()`; command-line
  verification in `auto_detect()`.
- `client/src/capture/windivert_redirect.rs` — proto-token in `out_filter`
  (TCP grammar), `parse_ipv4_tcp`/`build_ipv4_tcp`, TCP arm in intercept
  thread, terminator in the tunnel task, TCP stats counters, inbound TCP
  firewall rule.
- `client/src/engine.rs` — pass `transport` into `WinDivertConfig`
  (default UDP; Java profile passes Tcp).
- `client/src/modes/redirect_windivert.rs` — thread the new config field
  through (compile-only change).
- `client/src/tunnel/relay.rs` — gate: refuse TCP game profiles while
  `TunnelTransport::Tcp` is the active transport (TCP-in-TCP ban).
- `client/src/interceptor/process_scanner.rs` — `TransportProtocol::Tcp`
  variant and a TCP socket table (`netstat -anot`) for the Java profile.

**proxy/** (crate `lightspeed-proxy`)
- `proxy/src/relay.rs` — v5 branch in `process_inbound_packet` after
  auth/abuse; `TcpConnState`; per-5-tuple `HashMap<Tcp5Tuple, TcpConnState>`
  on `RelayEngine`; connection tasks (dial/buffer/splice/teardown); idle
  sweep for the TCP table; budget accounting per connection.
- `proxy/src/auth.rs` — no code change required (per-packet validation is
  version-agnostic); confirm with a test that a v5 packet passes
  `validate` with the same token as a v3 packet.

**docs/**
- `docs/tcp-tunnel-design.md` — this document.

No changes to `client-gui/`.

---

## Risks

**Ordering in live play (head-of-line blocking).** The reliability layer is
strictly ordered per connection; one lost and unrecovered datagram stalls the
stream until the RTO (≥ 200 ms) fires. FEC mitigates this (a single hole in a
K=4 block is usually recovered in ~1 RTT), but on paths with burst loss the
game sees stall spikes the UDP path never had. Mitigations, in order of
preference: keep FEC enabled on the leg (mandatory — this is why v5 always
carries FEC), fast retransmit on 3 dup ACKs, and cap RTO backoff at 15 s so a
dead path fails over to RST instead of hanging the game socket. Accept that
Minecraft chunk loads will show occasional 200–400 ms stalls on lossy links;
measure with the in-game F3 debug screen before shipping.

**NAT/ALG interactions.** The game's TCP SYN is answered locally by the
terminator and never leaves the machine; the router sees an unanswered SYN
and may (a) drop its NAT mapping for the 5-tuple (harmless — the real
traffic rides the UDP tunnel, not that mapping), (b) treat the spoofed
SYN-ACK injected outbound as an abnormal packet on the LAN (home routers do
not act on this; carrier-grade NATs ignore non-established outbound ACKs),
or (c) run an ALG that rewrites or drops TCP options — irrelevant here since
the handshake never crosses the router. The real interaction risk is
**Windows Firewall/WFP on the client**: the spoofed inbound SYN-ACK from the
server IP must be allowed for the game process (the capture-section firewall
rule covers this). Documented rather than mitigated further: an enterprise
middlebox that blocks TCP to a game port will still block the real
connection the game would have made — that is a pre-existing condition, not
a regression.

**Anti-cheat posture.** Payloads are forwarded byte-for-byte; the design
never inspects or rewrites game bytes, and the terminator rewrites only
local TCP headers. Server-side visibility matches the UDP tunnel exactly:
the game server sees the proxy's IP as the peer — the UDP relay already
forwards from proxy sockets, so the TCP connector changes nothing about
what servers observe. Minecraft servers using IP-based bans (MCBans-style)
or rate limits see shared proxy egress IPs on both paths today. Anti-cheat
compatibility is unchanged (no process memory access; WinDivert
interception already carries the same posture as the UDP path).

**Cost and capacity — what could make this not worth shipping.** TCP costs
more than UDP on the proxy: one `TcpStream`, one retransmit buffer (≤ ~92 KB),
and one task per connection versus one UDP socket per session. A busy
Minecraft server proxy scales at connections, not packets. If per-connection
state exceeds the Always Free tier budget ([COST_STUB] — infrastructure must
stay $0.00), the feature is not shippable at scale; gate it behind the
capability advertisement and `max_sessions` so a non-advertising or
budget-capped relay degrades to "TCP profile unavailable" rather than paying
money. Per-connection memory is hard-bounded by the 64-segment window either
way. Also weigh it: Java Edition traffic is already TCP, which self-heals
loss; the win LightSpeed offers is path selection and loss reduction, not
ordering. If a pilot on lossy consumer paths shows the reliability layer's
stalls erase the path win, stop and revisit (e.g., selective-ACK blocks
instead of a strict stream) before enabling the profile by default.
