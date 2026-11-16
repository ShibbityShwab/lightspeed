# A real server-leg signal for asymmetric game traffic

> **Status:** design + measurement proposal (board `task-12`). **No product code
> changed.** Date 2026-11-16, HEAD `46c1a61` (other teammates were editing
> `client-gui/src/app.rs`, `docs/design/interface.md` and the interceptor crates
> while this was written — function names are cited instead of line numbers for
> those files).
>
> **Evidence:** `wat/evidence/2026-11-16/server-leg-signal/` (index in
> `README.md` there). **Supersedes:** nothing. **Owner:** `net-eng`.

## 1. The question

The destination-aware relay chooser currently logs, on a real ROB RUN session:

```
Destination-aware relay choice for 91.199.82.166: 207.246.106.36:4434
(RegionPrior, total 498 ms) - 0 of 8 candidates raised to the direct-ping floor
```

`RegionPrior` means **no candidate had a measured relay→server figure**, so the
choice was made from geography plus the client→relay leg. The owner's remaining
complaint is that the chosen relay "can be sub-optimal". This document settles
what signal can replace the prior, what it measures, how many samples it takes,
and what to say when a server never answers through a relay.

The relay is the only lever LightSpeed has over the second half of the path.
Getting its choice wrong costs the whole product's claim.

## 2. Definitions — name the leg, name the window

| Leg | Definition | Measured today by | Valid? |
|---|---|---|---|
| **L1** | client→relay round trip (one RTT) | tunnel keepalive echo (`latency.rs::record_client_relay_rtt`, 1/5 s); QUIC control `Ping`/`Pong`; TCP connect | yes |
| **L1h** | client→relay **HTTP `GET /health` wall time** | `client-gui/src/discovery.rs::probe_latency` (`RELAY_PROBES = 5`) — **what the chooser's `client_leg_us` is** | one-directional *measurement*, but ~2× L1 |
| **L2** | relay→game-server round trip | **nothing, client-side.** `lightspeed_relay_latency_*` is *not* this (see §4.2) | **no client measurement exists** |
| **L3** | whole relayed path: client→relay→server→relay→client | `latency.rs::LatencyTracker::record_inbound` (biased, see §4.1); `relay_app_ms` in the GUI log | **no — currently invalid** |
| **L3d** | direct client→server round trip | ICMP echo burst, 5 probes, median, ≥60 s apart (`latency.rs::run_direct_burst`) | yes (correlated round trip) |
| **Lag** | relay-observed "time from the latest client forward to the next server reply" | relay `/metrics` `lightspeed_relay_latency_*` | **not an RTT** (one-sided, §4.2) |

Rules this document applies to every table:

1. A number is only comparable to another number of the **same leg and same
   estimator**. L1h is not L1; a lag is not an RTT; a prior is not a measurement.
2. Every table carries **n**, the **window**, and the **reducer** (median, mean,
   quantile, share).
3. A ranking key must be a **whole-path total** or a quantity measured in the
   same units on every candidate; a per-candidate bound is not an estimate.
4. No game payload is modified, duplicated or decoded to obtain any of this
   ([`TRANSPARENCY_STUB`](../wat/rules.md)); no game-server IP is published
   ([`PRIVACY_STUB`](../wat/rules.md)).

## 3. What the chooser actually does today (from the code)

Path: `client-gui/src/app.rs::choose_relay_for_destination` →
`client/src/route/relay_choice.rs::rank_at_with_direct`.

- **Client leg**: for every candidate, `client_leg_us` and `client_jitter_us`
  come from `discovery.rs::measure_spreads_blocking` — five `GET /health` wall
  times, median and `second_slowest − fastest`. That is ~**two round trips**
  (TCP handshake + request/response) plus relay processing (§4.3).
- **Destination leg**: `relay_choice::dest_leg_for_at` first asks the estimator
  (`route/destination.rs::RelayDestinationEstimator::estimate`). At this HEAD
  **no production code ever feeds the estimator's tunnelled leg**:
  `destination.rs::note_inbound` says *“Nothing is recorded”* and returns, and
  `git grep 'observe_tunnelled' -- client/src` shows only the definition and
  tests (evidence `code-paths.txt`; the feed was disabled in commit `7f34412`,
  *“drop the whole-loop figure the traffic cannot support”*). So
  `DestLegSource::Measured` is **unreachable for every game**, not just for
  asymmetric ones, and the prior is the only source.
- **Prior**: exact relay position (`RELAY_POSITIONS`) → the **country
  representative point** of the server from the offline DB-IP table
  (`client-gui/src/geo.rs`, country level) → `round_trip_us = km × 20 µs,
  floor 10 ms`. Prior candidates are charged `PRIOR_UNDERTAINTY_US = 25 ms`.
  Every US address resolves to one point (37.09, −95.71).
- **Ranking key (at the time of these measurements)**: `dest_leg_us / 50_000`
  bucket first, then `client_leg + dest_leg + penalty + 2 × jitter`, then
  address. **Changed in WF-065 task-17:** the engine now compares the whole-path
  total (`total_leg_us`), jitter only as a tie-break, address last; the bucket and
  the `2 × jitter` term are gone (see §4.3 item 2).
- **Floor**: when the server's direct ICMP ping exists at choice time,
  `apply_floor` raises a destination leg below `direct − client_leg`.
- `client/src/route/server_location.rs` contains a *precise* coordinate resolver
  (operator endpoint template), but it is not wired anywhere (`git grep
  ServerLocator`: definition + tests only).

## 4. Measurements

Every number below comes from a command listed in §9, and the raw output is in
the evidence directory.

### 4.1 Live ROB RUN session — the estimators that are supposed to feed L2/L3

Source: `wat/evidence/2026-11-16/relay-hopping/qa-live/live-session-final.log`
(task-10's live run). Window **2026-11-16T08:44:55.106Z → 08:49:24.542Z
(264 s)**, game server `91.199.82.166:36704`, relay LAX `207.246.106.36:4434`.
Analysis: `scripts/analyze_live_trace.py` → `live-trace-analysis.out`.

| Quantity | Value | n | Window / reducer |
|---|---|---|---|
| Relay choice | LAX, `RegionPrior`, total **498 ms**, **0 of 8** floored | 2 log lines (startup re-init) | 08:45:00Z |
| L3d direct ICMP burst medians | 283.47, 291.20, 284.42, 284.38, 284.81 ms | 5 bursts of 5 echoes (**25 probes issued**; a burst publishes only with ≥3 replies) | 264 s; median of burst medians |
| L1 keepalive (`relay_ms`) | 204.72 – 217.41 ms | **26** logged, one per 10 s (the first frame has no relay yet) | 264 s |
| `relay_app_ms` (attempted L3) | 7.33 – 34.30 ms | 25 logged; tracker window capped at 256 | 264 s; median |
| `shadow_app_ms` (attempted L3d) | **0.4248 ms**, one value | **1** sample (shadow sampler runs 1/5 s ⇒ ~52 expected) | 264 s |
| `like_for_like` | `None` on every line | 27/27 | 264 s |
| relay-history, LAX for that server | 179/0 → 9,816/5,147 forwards/replies; **+9,637 / +5,147 = 53.41 %** return share; **36.5 forwards/s** | 264 s of session delta | counters |

**Physical checks — both existing estimators fail:**

- A tunnelled client→relay→server→relay→client round trip can never be shorter
  than the client→relay leg. `relay_app_ms < relay_ms` on **25 of 25** pairs;
  ratio min **0.035**, median **0.129**, max **0.166**. The estimator is timing
  the gap to the *most recent* outbound (`latency.rs::LatencyTracker::note_outbound`
  overwrites `pending` on every send), so on a ~36 sends/s flow it measures
  roughly one inter-send interval (median ratio × mean relay leg ≈ 27 ms ≈ 1/36.5 s).
  **It is not a round trip and cannot be the server leg.**
- `shadow_app_ms` 0.4248 ms is **0.15 %** of the same session's direct ICMP p50.
  `latency.rs::SHADOW_MIN_FRACTION_OF_ICMP = 0.5` is supposed to reject exactly
  this sample, but the check only runs when ICMP history already exists; the one
  sample was recorded before the first burst. Sample availability was also
  effectively zero (1 in 264 s instead of ~52).
- **The triangle-inequality lower bound for L2 in that session:**
  `direct − L1 = 283.47 − 204.72 ≥ 78.75 ms`. Any proposed server-leg estimate
  for LAX → that server must not report less; the relay-wide public average for
  LAX at the next scrape was **24.01 ms**, i.e. 3.3× below the bound, which is
  the cleanest illustration of why §4.2's number is not this server's leg.

### 4.2 Option A — the relay's own `lightspeed_relay_latency_*`

Definition (`proxy/src/metrics.rs:611-627`, `proxy/src/relay.rs:2206-2216`):
the relay stores the time of the latest forward on a session
(`pending_forward_us`) and, when the **next** datagram from the game server is
read, samples `now − marker` and **clears the marker**. Its own HELP text says
*“NOT client RTT”*. Labels exposed: `region`, `node_id` — **no destination
label**. The family is relay-wide and cumulative.

Scrapes: 8/8 community relays answered `GET /metrics` (two scrapes 50–52 s
apart, from this host, 2026-11-16 08:59Z, `/health` 200 for all 8).
Analysis: `scripts/analyze_relay_metrics.py` → `relay-metrics-analysis.out`.

**A1/A2 — lifetime and 52 s window**

| relay | n lifetime | mean ms | p10 | p50 | p90 | Δn / 52 s | window mean |
|---|---|---|---|---|---|---|---|
| relay-lax-1 | 82,676 | 24.01 | 2.26 | 18.53 | 48.66 | +23 (0.44/s) | 52.25 ms |
| relay-ewr-1 | 348,394 | 15.05 | 1.38 | 8.24 | 34.11 | 0 | – |
| relay-sgp-1 | 2,069,999 | 5.23 | 1.02 | 3.74 | 13.33 | +17 (0.33/s) | 4.04 ms |
| relay-fra | 1,458,208 | 5.88 | 0.78 | 4.08 | 11.35 | 0 | – |
| relay-nrt | 69,338 | 18.79 | 1.56 | 8.43 | 59.86 | 0 | – |
| relay-bom-1 | 5,458,276 | 4.65 | 0.70 | 4.00 | 9.47 | 0 | – |
| relay-mad-1 | 1,269,146 | 4.19 | 0.52 | 3.61 | 8.96 | 0 | – |
| relay-syd-1 | 281 | 33.73 | 4.73 | 21.62 | 81.80 | 0 | – |

**A3 — the sampling artefact is visible in the distribution.** Samples faster
than a public-internet game-server round trip can physically be:

| relay | ≤1 ms | ≤5 ms | ≤10 ms | n |
|---|---|---|---|---|
| relay-lax-1 | 4.8 % | 21.4 % | 33.9 % | 82,676 |
| relay-ewr-1 | 7.4 % | 35.0 % | 58.1 % | 348,394 |
| relay-sgp-1 | 9.7 % | 68.6 % | 87.9 % | 2,069,999 |
| relay-fra | 12.4 % | 61.3 % | 89.3 % | 1,458,208 |
| relay-nrt | 6.8 % | 29.5 % | 59.4 % | 69,338 |
| relay-bom-1 | 13.8 % | 62.1 % | 93.3 % | 5,458,276 |
| relay-mad-1 | 16.3 % | 67.9 % | 95.8 % | 1,269,146 |
| relay-syd-1 | 0.7 % | 10.7 % | 18.9 % | 281 |

4.8–16.3 % of samples across the fleet are ≤ 1 ms (≤ 100 µs per hop). That is
not a relay→server RTT; it is the marker being cleared by a datagram that was
already in flight. The lag is a **one-sided, send-cadence-flavoured quantity**:
it is bounded above by the true RTT for the causing packet but is biased down by
the client's send rate.

**A4 — it is not stable over time either.** `stats` branch
`web/network-history.json`: **315 snapshots, 57.8 days, 239,075,849 samples**,
fleet mean 4.86 ms. Per-relay interval mean of the same family:

| relay | intervals with samples | min | p10 | median | p90 | max |
|---|---|---|---|---|---|---|
| relay-lax-1 | 263 | 0.29 | 6.57 | 23.13 | 90.58 | 237.58 ms |
| relay-ewr-1 | 157 | 2.81 | 4.41 | 15.52 | 91.94 | 238.70 ms |
| relay-sgp-1 | 197 | 0.94 | 3.53 | 5.24 | 80.39 | 879.37 ms |
| relay-fra | 198 | 0.86 | 3.71 | 6.41 | 21.98 | 775.15 ms |
| relay-nrt | 198 | 1.27 | 5.99 | 15.76 | 146.31 | 264.78 ms |
| relay-bom-1 | 168 | 1.37 | 3.62 | 3.97 | 25.51 | 329.48 ms |
| relay-mad-1 | 211 | 3.04 | 3.51 | 3.93 | 14.86 | 235.18 ms |
| relay-syd-1 | 107 | 0.60 | 3.82 | 17.12 | 207.06 | 424.45 ms |

A p90/median ratio of **3.4–15.3×** per relay, tracking traffic mix and load,
not distance. And a 50 s scrape observed **n = 0** on 6 of 8 relays: there is no
guarantee a usable aggregate exists at the moment a choice must be made.

**A5/A6 — nothing else public is a server leg.** The only other latency family
the fleet exports, `lightspeed_telemetry_route_*`, is *client-reported per-route
leg* RTT from `PathObservation { relay, rtt_p50_ms, … }`
(`client/src/telemetry/paths.rs`), fed by
`session.rs::multipath_record_response → record_relay_response` — i.e. **the
client→relay leg again**, aggregated per `(relay, game, client country)` with a
k=3 floor. Lifetime mean reported p50 is 55.8–168.5 ms per relay (n = 257–8,215
reports per relay). It contains no destination information at all.

**Verdict on Option A:** `lightspeed_relay_latency_*` **cannot be a server-leg
estimate for a candidate relay.** It is (i) not a paired RTT, (ii) relay-wide
with no destination attribution, (iii) unstable by more than an order of
magnitude over time, and (iv) sometimes absent in the window. Keep it as a
*relay health / overload* signal (a lag p90 that jumps is a real problem signal);
do not put it in the ranking.

### 4.3 The client-leg input is two round trips, and it decides the choice

Measured from this host, 2026-11-16 ~08:59Z, n = 5 probes per relay each:

| relay | HTTP leg (chooser input) | TCP-connect leg (1 RTT) | ratio |
|---|---|---|---|
| relay-lax-1 | 439.94 ms | 220.17 ms | 2.00 |
| relay-ewr-1 | 622.04 ms | 305.84 ms | 2.03 |
| relay-sgp-1 | 92.07 ms | 47.83 ms | 1.92 |
| relay-fra | 404.92 ms | 203.63 ms | 1.99 |
| relay-nrt | 240.50 ms | 122.73 ms | 1.96 |
| relay-bom-1 | 202.20 ms | 103.25 ms | 1.96 |
| relay-mad-1 | 407.81 ms | 202.46 ms | 2.01 |
| relay-syd-1 | 272.71 ms | 141.12 ms | 1.93 |

ratio min 1.92, median **1.99**, max 2.03. The chooser adds a 2-RTT client leg
to a 1-RTT destination estimate, so **every relay is charged twice for its own
distance** relative to the server side. Two consequences measured with the same
arithmetic (`scripts/rank_harness.py`, `scripts/client_leg_bias.py`):

1. **The direct-ping floor is inert.** With the leg the GUI used (HTTP), the
   floor `direct − client_leg` is negative for all 8 relays (LAX −156.5 ms);
   with the same session's one-RTT keepalive (204.72 ms), LAX's floor is
   **+78.75 ms** and binds. The live log's "0 of 8" is reproduced exactly by the
   harness with HTTP legs (winner LAX, total **506.2 ms** vs the live log's
   498 ms, same relay, same source).
2. **The sort policy can rank a relay its own estimate calls slower than
   direct.** Counterfactual arithmetic with one-RTT legs and the live direct ping
   (283.47 ms): EWR's client leg is 305.84 ms, so by the triangle inequality EWR
   *cannot* beat direct for this server; its estimated total is 368.9 ms. Yet the
   `dest_leg/50 ms` bucket puts EWR first (prior 38.0 ms → bucket 0) and LAX
   second (floored to 63.3 ms → bucket 1, total 283.5 ms ≈ direct). The bucket
   overrides the whole-path total. This is a policy defect to fix together with
   the units, and it is why a server-leg signal must enter a **whole-path**
   comparison rather than a destination-only bucket.

Both counterfactual tables are arithmetic on measured inputs, not a live run;
the concurrent tree edits mean the GUI call sites may move.

**Status (WF-065 task-17, 2026-11-16): fixed.** The engine's `sort_scored` now
compares the whole-path total with jitter as a tie-break only, the
`dest_leg/50 ms` bucket and the `2 × jitter` term are deleted, and the GUI's
interim duplicate comparator is removed - the counterfactual above is pinned by
`route::relay_choice::tests::the_whole_path_total_outranks_the_destination_bucket`.
No other scoring semantics remain. Consequence to keep in mind when reading §4.4:
with only a country-centroid prior the ranking can no longer separate the US
coasts (LAX 37.1 ms vs EWR 41.3 ms prior, 4.2 ms apart, while the true difference
for an Ashburn server is 63 ms), so the client leg decides. That is the honest
outcome of removing the bucket, and it is the strongest argument for the
measured L2 signals in §5.

### 4.4 Option C — how wrong is the geometric prior?

`scripts/rank_harness.py` mirrors the code's own formula (README gives the
constants). Prior error = prior − great-circle round trip from the same relay
position to a *real* city, which is a direct measure of what the country point
collapses. The DB-IP point for every US address is (37.09, −95.71):

| relay | prior to US point | true to Ashburn | error | true to Los Angeles | error |
|---|---|---|---|---|---|
| relay-lax-1 | 41.2 ms | 73.0 ms | **−31.8 ms** | 10.0 ms | **+31.2 ms** |
| relay-ewr-1 | 38.0 ms | 10.0 ms | **+28.0 ms** | 78.4 ms | **−40.4 ms** |
| relay-sgp-1 | 305.8 ms | 310.4 ms | −4.6 ms | 282.4 ms | +23.4 ms |
| relay-fra | 156.5 ms | 131.0 ms | +25.6 ms | 186.1 ms | −29.5 ms |
| relay-nrt | 202.2 ms | 217.4 ms | −15.2 ms | 176.3 ms | +25.9 ms |
| relay-bom-1 | 273.1 ms | 257.1 ms | +16.0 ms | 279.8 ms | −6.7 ms |
| relay-mad-1 | 151.9 ms | 122.3 ms | +29.6 ms | 187.3 ms | −35.4 ms |
| relay-syd-1 | 281.5 ms | 313.5 ms | −32.0 ms | 241.5 ms | +40.0 ms |

For a US-East server the prior's **relative** error between LAX and EWR is
**59.8 ms**, while the prior itself separates them by **3.2 ms** — a 19×
compression, and the two candidates land in the same 50 ms bucket. The sign is
systematic, not random: a point in the middle of the country always
over-estimates the near-coast relay and under-estimates the far-coast one. For a
US server the server side therefore **cannot change the ranking**, and the choice
falls to `client_leg + 2 × jitter` — exactly the client-leg race the chooser
exists to replace.

Across a grid of eight US cities the per-relay error range is −43.7 … +48.3 ms.
The prior is still a useful **fallback and a stabiliser** (it is the only
evidence for a region the client has never used, and it keeps the order
deterministic), but it must be labelled as an estimate, never as a measurement.

`client/src/route/server_location.rs` already carries a precise-coordinate
resolver; wiring it would shrink these errors (and nothing else here uses it).
That is a *prior refinement*, not a measurement — it still cannot tell a US-East
server from a US-West one sitting behind the same anycast/CDN address.

## 5. Options evaluated against the acceptance criteria

| Option | Leg it actually measures | Estimator | n / window measured | Verdict |
|---|---|---|---|---|
| **A. relay `/metrics` lag** | none of L1/L2/L3 — one-sided lag, relay-wide | mean/histogram of a one-sided gap | 82,676–5,458,276 lifetime; 0–23 per 52 s; 315 snapshots / 57.8 d | **reject as server leg**; keep as relay health signal |
| **B. shadow vs relayed app RTT as-is** | attempted L3/L3d, both biased | latest-send pairing; single shadow sample | 25 pairs live (100 % below L1), 1 shadow sample | **reject as implemented**; salvage via D1 |
| **C. geometric prior fallback** | none (distance model) | haversine to country point, ±25 ms uncertainty inside the whole-path total | 1 server, 8 relays; error −11.7…+48.3 ms | **keep as fallback**; surface uncertainty; never present as measurement |
| **D1. quiet-flow tunnelled RTT** | L3 (then L2 by subtraction) | attributed quiet-flow samples, median | unknown until instrumented (falsifier) | **recommended v1** |
| **D2. relay-vantage destination probe** | L2 directly (relay→server ICMP RTT) | per-relay ICMP echo burst, per requesting client | unknown until built (falsifier) | **recommended v2** for candidate ranking |

### 5.1 D1 — quiet-flow tunnelled RTT (client-only, ships first)

**Metric.** Per `(relay, server)`:
`L3 = t(first inbound after a quiet gap) − t(last outbound)`, where quiet means
no inbound for that server for `Q` (proposed 1000 ms), so the next datagram is
unambiguously the answer to the last outbound. Reduce with a **median** over a
64-sample ring; publish only with `n ≥ 8` and a newest sample ≤ 120 s old;
uncertainty = half the inter-quartile range. The derived server leg is the
existing definition `L2 = median(L3) − median(L1 keepalive)`
(`route/destination.rs`), which makes `DestLegSource::Measured` reachable for
the first time since `7f34412`.

**Validity rules (each one is a test):** accepted only if the relay is unchanged
between send and reply; `L1_p50 ≤ L3 ≤ 5 s`; the inbound arrived after the
arming send. Anything else is counted and dropped, never clamped.

**Why it is the right first step.** Zero added packets (it times the game's own
traffic), no wire change, no relay deployment, `$0.00`. It turns three currently
invalid numbers into honest ones: `relay_app_ms` / `like_for_like` (the
do-no-harm bypass gate), the destination estimator, and the GUI's relayed
application RTT. It measures only the **current** relay — it cannot rank
candidates, and it must not be presented as if it could.

**Sample-rate reality.** At 36.5 forwards/s a 1000 ms quiet gap is an 18× gap,
so active play yields few samples; menus, loading screens, lobby and
request/response-style games yield most of them. The instrumented run is the
falsifier: **if a 10-minute match produces < 8 accepted samples for the locked
server, v1 cannot publish and the estimator stays at the prior.**

**On a spiky link** the median resists an outlier, but the ring keeps the last
64 samples, so a burst of bad samples ages out only after 64 new ones; publish
the IQR and n next to any figure the UI shows.

**Telemetry scale change (consequence of implementing v1).** `client/src/telemetry.rs`
builds `TelemetryReport.relayed_p50_ms` from `latency::peek_report_values()`,
which now returns the **validated quiet-flow whole-path p50**
(client -> relay -> server -> relay -> client) instead of the invalid busy-flow
pairing (reply minus the most recent send, which is roughly one inter-send
interval: 7.33-34.30 ms on the task-10 session, median ratio 0.129 of the relay
leg). Consequences: fleet `saved_ms` (direct ICMP p50 - relayed p50) and
`saved_app` (direct-app p50 - relayed p50) shift scale, because the old relayed
side was an inter-send gap rather than a round trip; reports will carry
`relayed_p50_ms` much less often - the validated sample rate *is* falsifier F1 -
so `lightspeed_telemetry_saved_*` sample counts will drop. Dashboard baselines
must not be read as a regression, and historical stats-branch points are on the
old scale and are not directly comparable with post-change points. The wire
field is unchanged (`Option<f32>`, same valid range). Separately, `--diagnose`
keeps its own exact-payload diagnostic window, separate from the validated
game-traffic window, so the CLI mode is not silently broken by the new validity
rules.

### 5.2 D2 — relay-vantage destination probe (the candidate-ranking fix)

**Metric.** `L2(relay, server)` measured **from the relay**: when a client asks,
the relay sends `N = 3` ICMP echo requests (issued is fixed at 3 to bound cost)
to the destination, and reports `{rtt_p50_us, samples, answered}` to **that
client only**. No game payload is touched, no traffic is duplicated to the
server, no per-destination series is exported.

**Why it is the only candidate-ranking fix.** The client can only measure a
relay it sends traffic through; measuring all candidates by carrying game traffic
would duplicate the player's datagrams to the same server through eight paths
(unacceptable: the server sees 8 copies from one player, and it is payload
duplication). A probe from each relay's own vantage measures exactly the leg the
ranking needs, for every candidate.

**Existing infrastructure it reuses.** `ControlMessage::Register` already carries
the destination IP (`protocol/src/control.rs`, `destination: Option<Ipv4Addr>`)
and `RegisterAck` already returns `dest_region`; the ack fields use
`#[serde(default)]` trailing-field compatibility (“pre-upgrade relays omit this
trailing field”), and the `caps` bitmap exists for capability signalling. A new
additive `ControlMessage::PathProbe { destination }` → `PathProbeReply
{ rtt_us, samples }` pair, or an additive `server_leg_rtt_us: Option<u32>` on
`RegisterAck`, follows the pattern already used for `telemetry_quic` and
`dest_region`; old peers treat an unknown message type as ignorable.

**Security constraints (must be in the review, not added later).** The probe is
a client-directed, rate-limited remote-ICMP primitive from eight relays:
authenticated control connection only (mTLS), destination must be public unicast
(reuse the proxy's existing RFC1918/loopback/link-local destination blocking),
`≤ 1 probe / (client, relay, destination) / 60 s` and a per-client hourly cap,
no reply data in `/metrics`, no relay logs of destination IPs. Note the relay
**already forwards arbitrary public UDP** for an authenticated client, so this is
strictly narrower than the product's existing capability. `sec-ops` reviews it
before implementation; `infra-dev` owns the fleet canary and rollback.

**Cost.** 3 ICMP echoes per `(client, relay, server)` per cache window, ~8
relays per server change: tens of packets per match. Owner spend stays `$0.00`
(no new resource, no paid service).

**Ranking use.** Total = `L1 (one RTT) + L2(relay, server)`, jitter from the
same samples; a candidate with no probe result stays on the prior with its
uncertainty. Combine with the triangle bound as a *validity* check (the probe
must not report below `direct − L1` for the same server), never as a substitute.

**v2 status (2026-11-16, `infra-dev` feasibility, NO-GO today; conditional GO
path).** Three corrections to the design above, from
[`relay-server-leg-probe.md`](relay-server-leg-probe.md) and its evidence:

- **`Register.destination` is not usable as the destination source.** All four
  production callers pass `None`, so `RegisterAck.dest_region` is `None` in
  practice. The relay learns the destination from the data-plane tunnel header
  and stores it in `ClientSession.game_server`; it already logs
  `New client session created client=... game_server=<ip:port>` at INFO after
  destination validation, which is the real sample source (876 session events in
  the last 41 h fleet-wide).
- **Privilege blocker.** Relay units run `DynamicUser=true` +
  `NoNewPrivileges=true` with no ambient capabilities, so they cannot send raw
  ICMP as deployed. Unprivileged ICMP (host `net.ipv4.ping_group_range`) is the
  only acceptable route; granting the daemon `CAP_NET_RAW` is a NO-GO.
- **Gates fixed in advance:** G1 privilege/egress per relay; G2 >= 30 distinct
  destinations across >= 3 relays and >= 2 games; G3 the decisive F3 ICMP answer
  rate >= 50%; F4 v2 vs v1 within 20%; G5 `sec-ops` review plus the owner fleet
  gate. The operator runbook and rollback are in
  [`relay-server-leg-probe.md`](relay-server-leg-probe.md); the owner must approve
  the fleet step and any reversible host sysctl change. Cost stays `$0.00`.

**Per-game signal (2026-11-16, `net-eng`).** The two negative `fortnite/US` cells
are partly an estimator artifact (the old relayed figure was a one-sided lower
bound, not an RTT) but their **sign survives** an artifact-free ICMP bound
(`saved_icmp` negative on both relays, n = 6 and 4); `arcraiders/RU` cannot be
settled without an ICMP anchor. Two data defects matter for any future per-game
work: the published `client_country` is the OS locale rather than the client IP,
and the `stats` branch has no per-game dimension (the collector aggregates by
locale country only). Board tasks 18/19 own the estimator/publication gate and
the label/dimension fixes; re-measure with the validated quiet-flow L3 after
v1 and a real Fortnite/Arc Raiders session.

### 5.3 What to do when a server never answers through a relay

Asymmetric traffic is normal, not a fault (task-9 already stopped condemning a
relay for it). The honest behaviour:

1. **Keep the relay.** No hopping, no re-rank, no timer; the prior + client leg
   chose it and the next server change re-evaluates.
2. **Say “server leg: not measured”** on the route surface, with the prior's
   value and its ± uncertainty beside it — never a bare number that looks
   measured. (State-contract change: interface-designer task.)
3. **Surface the provable part**: when `L1 ≥ L3d` (one-RTT client leg vs the
   direct ICMP ping), that relay **cannot** beat direct — a true statement from
   the triangle inequality, worth showing as “this relay cannot be faster than
   direct for this server”.
4. **Never write a relay-history verdict** from silence, and never let the
   watchdog use reply-share as a latency signal.
5. **Do not retry faster.** Re-probe at the next server change, and (v2) when a
   relay reports a probe result.

## 6. Recommendation

**v1 — build now, client-only, no wire change, no fleet action.**
1. Implement D1 in `client/src/latency.rs` + `route/destination.rs`, with the
   validity rules above, counters (`accepted`, `rejected: behind_floor`,
   `rejected: stale_relay`, `rejected: timeout`) and one trace line per accepted
   sample. Keep the current `relayed` window for telemetry **only** if it is
   relabelled; it must stop being called a round trip.
2. Fix the units: the chooser must rank on a **one-RTT** client leg (QUIC
   `Ping`/`Pong` or TCP connect — both exist), not the two-RTT HTTP wall time.
3. Compare candidates on the **whole-path total**, with the destination leg as a
   validity/bucket signal rather than an overriding key, and only claim
   “better than direct” when the arithmetic supports it.
4. Surface the measurement state honestly (measured n/window/uncertainty, or
   “no server-leg measurement”) and the triangle-inequality verdict when it
   holds.
5. Keep RegionPrior as the fallback with `PRIOR_UNDERTAINTY_US`; add a
   `no server-leg measurement` reason code to the choice log line.

**v2 — after v1 and the fleet feasibility gate, owner-gated.**
Relay-vantage destination probe (D2): sec-ops review → `proxy/` + `protocol/` +
client measurement round → canary on **one** relay (`n ≥ 200` probes, ICMP
answer rate and added control messages recorded) → fleet. Any relay that
predates the message simply omits the field; the client falls back to the prior.

**Rejected outright.** Ranking on Option A; ranking on Option B as implemented;
presenting the geometric prior as a measurement; any scheme that duplicates
game datagrams through several relays; any scheme that publishes per-destination
series.

## 7. What would falsify this recommendation

| # | Falsifier | Probe that detects it |
|---|---|---|
| F1 | D1's quiet-gap rate is below 8 accepted samples per (relay, server) per 10-minute match on real games | the new counters + a 10-minute live run |
| F2 | D1's samples violate the physical bound (`L3 < L1`) or disagree with D2 by > 20 % on the same pair | both signals side by side on one relay+server |
| F3 | ICMP from the relays is not answered by game servers in most sessions (rate < 50 %) | D2 canary counters per relay |
| F4 | Relay→server ICMP RTT is a poor proxy for the UDP path (differs > 20 % from D1's L2 or from the total-path change measured by 1.10.x app-level saving) | canary comparison, per relay |
| F5 | Fixing the client-leg units does not change the chosen relay on the owner's reported cases (i.e. the server leg was never the problem) | replay the live selection with one-RTT legs for a week of traces |
| F6 | The prior's ordering agrees with a real L2 measurement on > 90 % of games | the first week of D2 samples vs the prior |

The cheapest falsifier is F1/F2: v1 ships with instrumentation and no ranking
change, so a live run settles them without any fleet work.

## 8. Follow-up tasks (filed on the board)

- **task-13 `rust-dev`**: v1 quiet-flow tunnelled RTT signal + validity rules +
  tests and mutation check (client crate; the destination estimator must become
  reachable). Write scopes: `client/src/latency.rs`,
  `client/src/route/destination.rs`, `client/src/telemetry.rs`, evidence.
- **task-14 `interface-designer`**: honest server-leg state on the route surface,
  one-RTT client leg in the chooser, whole-path ranking key; screenshots + state
  contract. Write scopes: `client-gui/src/app.rs`, `client-gui/src/discovery.rs`,
  `docs/design/interface.md`, evidence.
- **task-15 `infra-dev`**: v2 feasibility and rollout — measure ICMP answer rate
  from the relays to real game servers, sec-ops review input, canary/rollback,
  GO/NO-GO; no production config change in the task. Write scopes:
  `docs/relay-server-leg-probe.md`, evidence.

## 9. Reproduce every table

All commands are PowerShell 5.1 from the repo root; scripts live in
`wat/evidence/2026-11-16/server-leg-signal/scripts/`.

```powershell
# 4.2 relay /metrics: two scrapes + analysis (writes ../relay-metrics/)
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/analyze_relay_metrics.py
# 4.3 client legs: HTTP (chooser input) and one-RTT
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/measure_client_legs.py
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/measure_client_rtt.py
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/client_leg_bias.py
# 4.4 ranking reproduction and prior error
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/rank_harness.py
# 4.4 server positions, through the same DB-IP table the GUI uses
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/geo_lookup.py 91.199.82.166 207.246.106.36
# 4.1 live session extraction and bound checks
py -3 wat/evidence/2026-11-16/server-leg-signal/scripts/analyze_live_trace.py
# stats history (9.3 MB, public repo data on the stats branch)
gh api "repos/ShibbityShwab/lightspeed/contents/web/network-history.json?ref=stats" `
  -H "Accept: application/vnd.github.raw" | Set-Content -Encoding UTF8 `
  wat/evidence/2026-11-16/server-leg-signal/stats-history/network-history.json
# relay scrapes were taken with:
#   GET http://<relay>:8080/health  and  GET http://<relay>:8080/metrics
#   (registry: web/registry.json; two rounds 50-52 s apart)
```

## 10. Unverified / limits of this document

- **No product code changed, no live signal built.** v1 and v2 are designs;
  their sample rates are unknown until instrumented (that is F1/F3).
- The live-session figures are from task-10's capture; this document re-derived
  them from the log, it did not re-run the session.
- The client legs in §4.3 were measured at ~08:59Z on this Bangkok host (n = 5
  per relay), **not** in the 08:44–08:49Z live window; they reproduce the live
  choice's total within 8 ms and the same winner, but they are a different
  window.
- §4.4 and the counterfactual table in §4.3 are arithmetic on measured inputs,
  not a live run; the "true" relay→server values there are great-circle model
  values to a named city, not measurements.
- The relay `/metrics` values are relay-wide aggregates over all sessions; the
  52 s window n is small (0–23) and the lifetime counters cannot be attributed
  to one client.
- ICMP answer rate from relay vantage points to game servers is unknown from
  this host (no fleet access) — F3.
- The stats-branch history is a snapshot copy; its 57.8 days include periods
  with known collector gaps (relay-fra report).
- Concurrent team edits: `client-gui/src/app.rs` and
  `docs/design/interface.md` were modified by another teammate while this was
  written; line numbers there will drift.
