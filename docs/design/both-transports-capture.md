# Both-transports capture: UDP and TCP in one interceptor session

Design plan for the ARC Raiders transport split.
Status: **proposed, awaiting owner review.** Base revision `4aa3221` (after 1.8.4). Date 2026-10-21.

## 1. The problem

ARC Raiders uses two transports at once:

- **UDP 7600-7777** for gameplay and the match server the player joins.
- **TCP 443** for its control plane, established to a backend such as
  `34.160.204.112:443`.

The 2026-10-19 live run proved the consequence. The Frankfurt relay forwarded the
game's UDP cleanly (1,290 packets, 70,131 bytes, `drops_relay_send_errors: 0`) and
its egress counter, which covers both directions, equaled the client-to-server
bytes exactly. **Zero bytes came back.** The game's TCP 443 was never captured,
so it left from the player's own address while the UDP left from Frankfurt. The
server saw one session from two source addresses and answered neither.

A profile-only change does not fix this. On Windows, a non-zero `tcp_ports`
makes the capture filter TCP-only, which drops the game's UDP. The fix is a
capture path that carries both transports in one session, from one relay and one
client source address.

## 2. What exists today (verified in tree)

- `client/src/games/mod.rs`: `GameConfig::tcp_ports()` exists (line 133) and
  defaults to `(0, 0)`. `TransportProto` is a single-transport enum (UDP or TCP).
- `client/src/games/arcraiders.rs`: declares UDP `7600-7777` (line 101) and
  inherits `tcp_ports() = (0, 0)` and `transport() = Udp`.
- `client/src/interceptor/traits.rs`: `InterceptorConfig.transport` is a single
  `TransportProto` (line 158). The GUI passes it to the interceptor.
- `client/src/interceptor/windows.rs`: the GUI's WinDivert interceptor builds a
  **UDP-only** filter (`build_capture_filter`, line 230: `"udp and outbound"`),
  parses with `parse_ipv4_udp` (line 972) and has no TCP terminator.
- `client/src/capture/windivert_redirect.rs`: the legacy redirect engine has the
  complete TCP path: `InterceptEvent::{Udp,Tcp}` (line 445), `TcpTerminator`
  (line 518), a `tcp and outbound` filter branch (line 889), the TCP FEC
  encoder/decoder and the retransmit/idle timers. It has no live caller at HEAD;
  it is the reference implementation.
- `protocol/src/header.rs`: `PROTOCOL_VERSION_TCP = 5` (line 51) and the
  `TcpExtHeader` wire format. The relay's TCP path already exists (the Minecraft
  Java profile uses it; `proxy/tests/integration_tcp.rs`).
- `proxy/src/auth.rs`: a data-plane token may be bound to the client's data-plane
  **source port** (line 113, `authorize(principal, data_port, ...)`). Two tunnel
  sockets would be two ports and two sessions, so both transports must share one
  tunnel socket.
- `client/src/profiles/schema.rs`: user profiles reject `tcp_ports` at load and
  save (line 1009). Built-in profiles are not affected.

## 3. Design

One WinDivert capture session, one tunnel socket, one session token, two
transport paths.

### 3.1 Config

Add a second transport to the capture config without overloading the profile's
primary transport:

```rust
// InterceptorConfig and WinDivertConfig
pub transport: TransportProto,          // primary; drives the server lock (UDP here)
pub capture_tcp_ports: Option<(u16, u16)>, // secondary; None for every existing game
```

`GameConfig::capture_tcp_ports()` feeds `capture_tcp_ports` when it is non-zero.
ARC Raiders returns `(443, 443)`. Its `tcp_ports()` stays `(0, 0)` so the portable
pcap filter is still UDP-only, and its `transport()` stays `Udp`, so every
existing call site keeps its meaning and the server lock keeps following the
game's UDP server.

### 3.2 Filter

One intercept handle, one inject handle, a disjunction:

```text
(udp and outbound and udp.DstPort >= 7600 and udp.DstPort <= 7777)
or
(tcp and outbound and tcp.DstPort == 443)
```

The existing `build_capture_filter` gains the TCP clause only when
`capture_tcp_ports` is `Some`. The shadow-direct filter stays on the primary
transport.

### 3.3 Dispatch

Parse by the IP header's protocol byte, not by configuration. The intercept
thread already models both cases as `InterceptEvent::{Udp,Tcp}` in the legacy
engine; move that enum and the TCP terminator into a shared module
(`client/src/capture/tcp.rs`) and use it from both engines.

- UDP: the existing path, unchanged.
- TCP: the extracted `TcpTerminator`, with its own FEC encoder/decoder, SYN
  cookie, retransmit tick and idle eviction.

The tunnel socket and token are owned by the one loop, so both transports send
from the same source port and carry the same registration. The relay's optional
port binding therefore sees one principal and one port.

### 3.4 Server lock

Feed `ServerTracker` only from the primary transport. The game's TCP backend is
a secondary flow with its own five-tuple; it must not become `detected_server`,
or the relay choice would follow `34.160.204.112:443` instead of the match
server.

Track the secondary destination separately for the relay allowlist check and
for diagnostics.

### 3.5 Relay allowlist

The production relays set `destination_allowlist` (`proxy/src/config.rs:194`,
CIDR strings). The game server's IP is already allowed for UDP, so the same IP
on TCP 443 should pass a CIDR-only list. Confirm this live before shipping:

1. On a relay with the production allowlist, send a TCP segment to the game
   backend through the tunnel.
2. Require `packets_relayed` and `egress_bytes` to grow and
   `drops_abuse_blocked` to stay flat.

If the allowlist is port-aware, extend it for the profile's declared TCP ports
and add a proxy test.

## 4. Work breakdown

1. **Extract the TCP terminator** from `windivert_redirect.rs` into
   `client/src/capture/tcp.rs` (types, state machine, FEC wiring, retransmit and
   idle logic). The legacy engine keeps its behavior; its tests move with it.
2. **Extend the configs**: `InterceptorConfig.capture_tcp_ports`,
   `WinDivertConfig.capture_tcp_ports`, and the builder that resolves a game.
3. **Extend the GUI filter and loop**: `build_capture_filter` disjunction, parse
   by protocol, dispatch TCP to the terminator, share the tunnel socket and
   token.
4. **Keep the server lock on UDP**: split `detected_server` from secondary
   destinations; log the TCP backend at info once per connection.
5. **ARC Raiders profile**: `capture_tcp_ports() -> (443, 443)`; `tcp_ports()`
   stays `(0, 0)` so the portable pcap filter is unchanged.
6. **Docs**: game profile, CLI reference if a flag is added, and this design's
   status line.

## 5. Tests

- `build_capture_filter` with and without `capture_tcp_ports`: the UDP-only
  string is byte-identical to today for every existing profile; the both string
  contains both clauses.
- Protocol dispatch on a raw IPv4 header: UDP goes to the UDP path, TCP to the
  terminator, ICMP and fragments are re-injected.
- The extracted terminator passes every existing Minecraft test unchanged.
- A mock session with a UDP flow and a TCP flow through one tunnel socket and
  one token: both reach the relay, both return, the token is accepted once.
- Server lock: a TCP backend seen before the UDP server must not become
  `detected_server`.
- Live: ARC Raiders in the lobby (TCP 443 established) and in a raid (UDP),
  with the relay's `packets_relayed` and `egress_bytes` growing for both and the
  game's TCP source address equal to the relay's.

## 6. Risks and open questions

- **Two engines, one terminator.** Extracting the terminator is the largest
  mechanical risk. Do it as a pure move first, with no behavior change, and keep
  the legacy engine green.
- **TCP timers in the UDP loop.** The terminator's 50 ms retransmit tick already
  lives in a `tokio::select!`; the UDP path's latency must be measured before
  and after (p99 intercepted-to-sent).
- **TLS and anti-cheat.** The terminator is transparent at the TCP layer, so TLS
  passes through unchanged. Anti-cheat that inspects the source address will
  see one address only after this lands, which is the goal.
- **Allowlist coverage.** The game's TCP backend may rotate to addresses outside
  the current CIDRs. Add the observed ranges only after a live check; do not
  widen the allowlist speculatively.
- **Custom profiles.** `tcp_ports` stays rejected in user profiles until the
  schema can validate a second range and the process-first path can carry it.
  This plan covers the built-in ARC Raiders profile only.

## 7. Rollout

Ship behind the built-in profile: only ARC Raiders sets `capture_tcp_ports`, so
every other game keeps the current UDP-only filter byte for byte. Version as
1.8.5 if the change is profile-local, or 1.9.0 if the shared terminator move
changes public behavior. The live gate is one ARC Raiders join through
Frankfurt with both transports visible in the relay counters and the raid
loading.
