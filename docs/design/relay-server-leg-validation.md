# Server-leg validation and TCP backpressure: panel decision (2026-11-19)

> Status: decision memo from a three-stream panel (`architect`, `net-eng`,
> `rust-dev-4`), plus the lead's synthesis. Read-only panel: no code changed.
> Applies to `docs/design/relay-server-leg.md` (R2, implemented) and the
> reachable TCP backpressure introduced by the TCP sender-lifetime fix.

## 1. The F4 result as run is invalid

The fleet canary compared relay-lax ICMP **53.5 ms** with a hard-coded
`F4_ESTIMATE_MS = 73` in the temporary measurement script. That constant has no
derivation in the repo (it numerically equals the LAX→Ashburn great-circle figure
in `docs/server-leg-signal.md §4.4`), it came from a different window, the link
was measurably degraded that day (direct 320→410 ms), and the target may answer
from a front-end. A cross-window difference between two proxies cannot condemn
the relay leg.

**F3 re-scored under shipping rules** (`BURST_ECHOES=3`, `MIN_ECHO_REPLIES=2`,
`MIN_PLAUSIBLE_US=1_000`): the sub-ms anycast reply is implausible, so the same
rows are **6/14 = 42.9 % per relay**, not 50 %. F3 therefore also has not passed
the production gate; it merely proves destination-side filtering (the same seven
answer on both relays) and that unprivileged ICMP works.

## 2. Decision: pre-registered matched-epoch experiment (E1)

Adopt the panel's recommended design; no ranking change until it passes.

- **Population:** >= 60 distinct destinations already forwarded to (journal
  `game_server=`, >= 20 packets) across >= 3 relays (`relay-lax-1`,
  `relay-syd-1`, plus `relay-fra`/`relay-ewr-1`) and >= 2 games.
- **Probes:** production semantics (3 echoes, >= 2 plausible replies >= 1 ms,
  source == target, 30 s cache, no repeat < 60 s), each destination from >= 2
  relays.
- **F4 pairs (the decisive test):** >= 30 matched epochs on one relay+server:
  >= 3 ICMP bursts and >= 5 client keepalive `L1` samples within +/-30 s of an
  accepted quiet-flow `L3` sample. `Delta = (L3 - L1_window) - S_icmp` per pair.
- **Statistics:** per-pair relative difference; median, MAD, 95 % bootstrap CI,
  share in band `max(20 %, 3 ms)`; no optional stopping.
- **Provenance tiers:** tier A = last responsive hop inside the destination's
  hosting ASN (RIPE Stat); tier B = anycast/cloud-global; tier C = unknown.
  Tier B destinations never count toward F3 and are never ranked; two
  independent flags are required to exclude a destination, and exclusions are
  published.
- **Gates:** F4 PASS iff `|median Delta| <= 20 %` (or 3 ms) and >= 80 % of pairs
  in band; FAIL iff the CI excludes the band; INCONCLUSIVE otherwise (n < 8 or
  > 50 % tier B). F3 PASS iff tier-A answer rate per relay >= 50 % with n >= 30
  (Wilson CI).
- **Decision rule:** ICMP `S` may steer ranking only if F4 PASS **and** tier-A
  F3 >= 50 % per relay. F4 PASS + F3 < 50 %: display-only per destination.
  Both fail: no `S` steering - triangle bounds + labelled prior + "server leg
  not measured"; TCP SYN becomes primary only if tier-A ICMP F3 < 50 %, tier-A
  SYN F3 >= 50 %, and cross-method agreement >= 80 % in band.

**Safety/privacy:** targets only in the repo secret (deleted after the run);
aggregate/redacted evidence only (no game-server IPs, no per-destination
series); production gates unchanged (only forwarded destinations, private-range
refusal, 30 s cache, caps, egress metered); sysctl only on canary relays and
reversible.

## 3. Decision: TCP backpressure fix = option E

Adopt **non-blocking per-connection handoff with a pre-reliability drop valve
and a per-connection stall watchdog**. Rejected: `try_send` at the current seam
(after `on_segment` the cumulative ACK has advanced, so dropping is silent byte
corruption), and wire-level credit control (a client/protocol change).

Minimal change:
1. `process_tcp_packet` (relay.rs ~2004-2033): gate **before** `on_segment` on a
   per-connection slot/byte budget; on exhaustion `record_drop(TcpQueueFull)` and
   return without touching the receiver (a pre-ACK drop is indistinguishable from
   path loss and is recovered by the client's RTO).
2. Enqueue once as a batch `TcpCmd::Data(Vec<Bytes>)` with `try_send` (single
   producer, a free slot is guaranteed by the gate).
3. FIN: `try_send`; if full, tear down **only that connection** (RST) - a FIN
   cannot be ordered behind 64 unsent chunks.
4. `run_tcp_connection`: decrement the budget after `write_all`, poll `cancel`
   inside the write (`select!`), and reset the connection if no progress for
   ~60 s.
5. Tests: stalled upstream does not block other sessions (per-call < 50 ms,
   `TcpQueueFull` recorded, healthy connection still forwards); a pre-ACK drop
   is retransmit-recoverable and delivers bytes exactly once; FIN under
   congestion tears down only that connection; mutations reproduce the original
   hang and the corruption gap; existing TCP tests unchanged with zero drops.

## 4. Open questions

- Do UDP game ports answer TCP SYN (fallback viability)?
- Does ordinary play yield >= 30 matched F4 pairs on one relay+server, or must
  the experiment pool relays/servers?
- Should the 20 % agreement gate carry a 3 ms floor for same-region relays?
- Provenance of the front-end/anycast classifier (two flags) - evaluate on the
  first 60 destinations.

## 5. Revision after the live E1 window (2026-11-19)

The live window produced **n = 0 pairs**: the client published exactly one
accepted quiet-flow sample in 20 minutes (at session start, 181 s before the
first relay burst), so quiet-flow cannot serve as the F4 peer reference - the
sample scarcity (F1) is the binding limit, not the relay measurement.

One paired observation from that session is still decisive for the design: the
relay ICMP value sat **above the triangle floor** (direct 289 ms - L1 229 ms =
60 ms; S_icmp median 64.3 ms), while the client's own L3 - L1 estimate was
50.5 ms - **below the floor**, which is physically impossible. The client-side
v1 estimate is therefore the noisy/possibly-stale term; relay ICMP is consistent
with the bound. F4 must not be judged against L3 - L1 alone.

Revised F4 reference: use the **relay's own passive app-RTT sample** - the time
from forwarding a packet to receiving the game server's response, which the
relay already tracks (pending_forward_us) without new traffic - reported
alongside S (median over a window, with sample count and age). Then:

- peer = passive relay->server app RTT (frequent: one per response pair);
- floor = direct - L1 (client measurements, frequently available);
- per-epoch checks: S_icmp >= floor - 3 ms (physical consistency);
  |S_icmp - peer| / peer <= 20 % (agreement); flag peer < floor as an
  invalid passive sample rather than as relay error;
- gates: >= 30 matched epochs; PASS iff >= 90 % pass the floor check and
  >= 80 % are within the 20 % agreement band; FAIL iff the median agreement
  error is outside the band with a bootstrap CI excluding it; INCONCLUSIVE
  otherwise.

The quiet-flow sample remains a sparse cross-check only. Implementation follows:
add the passive sample to the relay->client server-leg report (additive message,
unknown-type tolerant), store it client-side next to S, and log one
QUIET_FLOW_SAMPLE-style line per passive report for the E1 harvest.
