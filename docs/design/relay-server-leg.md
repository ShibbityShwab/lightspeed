# Scope: relay-measured server leg

Status: proposed, not started.
Owner: whoever picks it up; the blocking unknown is listed first because it can
kill the whole design.

## The problem this solves

The window shows `Relay RTT` (client to relay) and `Server ping` (direct client
to game server, ICMP). Neither is the relayed path, and the relayed path cannot
be recovered on the client:

- The client sees two streams (sent, received) with no per-packet correlation, so
  any pairing is a guess. Three attempts produced 45 ms, 3101 ms and 0.157 ms
  against a relay leg of 220-240 ms; each was impossible, each was withdrawn.
- On a mobile uplink (hotspot over WiFi) the client-to-relay leg carries the
  link's jitter, so comparing it with the direct ping measures the hotspot rather
  than the relay.

The relay does not have either problem for the piece that matters: it is one
endpoint of the relay-to-server path, and it can measure that path with ICMP,
which carries its own sequence and therefore attributes exactly.

## Step 0: the blocking unknown (do this before anything else)

**Can a relay send ICMP?**

The relay unit runs under systemd `DynamicUser`, which has no `CAP_NET_RAW`, and
a raw ICMP socket needs it. Two ways forward:

1. Add `AmbientCapabilities=CAP_NET_RAW` (and a matching
   `CapabilityBoundingSet`) to the unit written by `infra/scripts/relay-install.sh`.
   Reliable, but it is a unit change on every relay, not just a binary swap.
2. Unprivileged ICMP (`SOCK_DGRAM`, `IPPROTO_ICMP`) against
   `net.ipv4.ping_group_range`. No capability needed, but the range must cover
   every DynamicUser GID, which systemd allocates dynamically.

Spike: on one canary relay, with the capability added, ping a known-responsive
game server and confirm echo replies arrive and the reading matches an
independent `ping`. If neither option works on the fleet, the design has to
change and everything below is void.

## Design

1. Relay measures `S` (relay-to-server RTT) with ICMP echo, per game server that
   a live session is actually forwarding to.
2. Relay reports `S` to the client over the existing control plane.
3. Client holds `L` (client-to-relay, keepalive echo, already measured) and `S`
   together, and can finally show a path where both terms are measured:
   `expected path = L + S`, against `D` (direct, already measured).
4. Ranking may then use `S` as the destination leg instead of a region prior.

### Which servers may be probed

A relay probing addresses a client names is a scanning and amplification vector.
The gates, all mandatory:

- Only an address the relay has already forwarded at least N packets to for a
  live session (N = 20), never an address supplied out of band.
- Results cached per server address with a TTL (30 s), so a hundred sessions to
  the same server cost one probe, not a hundred.
- A global cap per relay: probes per second and concurrent probes, whether or not
  the cache would allow more.
- Never probe a private, loopback, link-local or reserved address.

### Cadence and shape

- One burst of 3-5 echoes every 10 s per active server, median recorded.
- Keep the last 16 medians per server, which yields `S` jitter the same way the
  client's direct series does.
- Report on change or at most once every 5 s per session.

## Protocol

New proxy-to-client message type, e.g. `MSG_SERVER_LEG`, framing identical to the
existing length-prefixed messages:

| field | type | notes |
|---|---|---|
| server | IPv4 (4) | the address this measurement is for |
| median_us | u32 | median of the burst, microseconds |
| jitter_us | u32 | mean absolute delta across the window |
| samples | u8 | how many bursts the window holds |
| age_ms | u16 | staleness at send time |

No version bump. Compatibility precedent is already in the tree: `RotateRequest`
documents that "a relay that predates this message decodes the type byte as
`ControlDecodeError::UnknownMessageType` and ignores the frame". A client that
predates `MSG_SERVER_LEG` ignores it, so no capability gate is required for
safety; a `caps` bit (`protocol/src/control.rs`, `caps::{TCP_TUNNEL}`) is still
worth adding so a relay can skip sending to clients that cannot use it.

## Relay work

- `proxy/src/ping.rs`: ICMP echo prober. `libc` is already a dependency; no new
   crate needed. Model it on `client/src/latency.rs::SystemIcmpProber`, including
  the "plausible reply" rules, because that shape is already tested.
- Per-server cache + rate limiter, keyed by `SocketAddrV4`, shared across
  sessions.
- Hook: after the relay has forwarded N packets to `server` for a session,
  register interest; the prober reports to interested sessions only.
- Unit and integration tests with a scripted prober, exactly as
  `client/src/latency.rs` tests `run_direct_burst`.

## Client work

- Control-plane handling for `MSG_SERVER_LEG`, stored per `(relay, server)` with
  a timestamp; stale after 60 s and then not displayed.
- Card rows when present: `Relay→server N ms` and `Expected path ≈ L + S`.
  `Server ping` (direct) stays, because it is the figure the player compares with
  the game's own display.
- Ranking: a new `DestLegSource::RelayReported`, preferred over the region prior
  and still subject to the existing geometric floor (`direct - client_leg`),
  which stays as an invariant regardless.
- Locales for the two new labels in all nine files.

## Rollout

- Relays self-update through `infra/scripts/relay-updater.sh` and the deploy
  workflow, so the binary reaches the fleet without a manual step.
- The unit change from Step 0 does not ride the binary. `relay-install.sh` writes
  the unit, so the rollout needs it re-run or an equivalent unit update, and the
  canary gate is the place to prove it.
- Relays restart to pick up a new binary, which drops sessions. That already
  happens on release; this feature does not make it worse, but it is a reason not
  to ship it in the same release as anything else risky.

## Testing

- Protocol: encode/decode round trip for the new message; unknown-type tolerance
  from an old-style reader.
- Relay: no probe without forwarded traffic; cache prevents a second probe inside
  the TTL; global cap holds under many sessions; private ranges refused; jitter
  maths over a scripted series.
- Client: state per `(relay, server)`; staleness hides the row; the floor still
  wins when the report is below `direct - client_leg`.
- Live: canary relay against the person's real server, comparing `S` with an
  independent `ping` run from that same relay.

## Risks

| risk | mitigation |
|---|---|
| ICMP blocked by the game server | report `unavailable`; the row hides rather than showing a guess |
| ICMP not permitted on the fleet | Step 0 decides before any code is written |
| Relay used as a scanner or amplifier | the gates above, with tests, are part of the deliverable |
| Extra egress per relay | cached per server, capped globally, 3-5 packets per 10 s |
| `S` excludes the relay's own queuing and CPU | say so in the docs; `L + S` is a floor on the real path, not an exact figure |
| Client compares `S` jitter with the direct ping's jitter | those are not the same measurement: the direct ping includes the player's link, `S` does not. Compare `expected path` with `direct`, never jitter to jitter |

## Effort

- Step 0 spike: small, but blocked on access to a relay.
- Protocol message and tests: small.
- Relay prober, cache, gates, tests: the bulk of it.
- Client handling, UI, ranking, locales: moderate.
- Unit/rollout work: small but fleet-wide, and the least reversible part.

## Open questions

1. Is `CAP_NET_RAW` acceptable on the fleet, or is unprivileged ICMP preferred?
2. Do the relays' hosts and upstreams allow outbound ICMP to arbitrary
   destinations? Some cloud networks filter it.
3. Are game operators likely to treat ICMP from relay IPs as hostile?
4. Is a 10 s cadence right, or should it follow session activity more closely?
