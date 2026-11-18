# Fleet ALPN — owner decision memo

**Read-only measurement. No fleet change was made.** No relay config was edited,
no deploy was dispatched, no process was restarted, and no public post was made
for this document. Every figure below comes from public `GET /health` and
`GET /metrics` responses on the eight community relays.

**Decision (2026-11-18): Option A — keep `control_alpn = "required"` on all
eight relays.** The owner accepted the team recommendation. No relay was
changed, no deploy was dispatched and no process was restarted for this
decision. The mercy path (Option B on `relay-syd-1`) is not taken. Revisit only
if the rejected-client evidence changes materially or a relay shows a
data-plane fault; the M1 instrumentation follows up to make the trend
measurable.

---

## 1. Current state

- **8/8 relays reachable and healthy.** All eight returned HTTP 200 from both
  endpoints, `status=healthy`, `version=1.11.0`, `control_alpn=required`.
- **One process generation.** Uptimes are 38.36–38.63 h, i.e. the eight processes
  started between `2026-11-16T14:49:27Z` and `2026-11-16T15:05:18Z`. A relay fixes
  its ALPN mode at startup, so the counters below are a `required`-only window
  and a bounded one: the current process lifetime.
- **Almost everything that reaches the control ports is refused.**
  Fleet-wide, of 677,561 control handshakes in that window: **3,973 accepted
  (0.586 %)**, **673,495 rejected with TLS alert 120 `no_application_protocol`
  (99.400 %)**, 93 `rejected_other`, and `no_alpn = 0` on every relay. That is
  **169.5 rejections per accepted handshake**.
- **What is actually served.** Those 3,973 accepted handshakes produced 373
  sessions and 6,723,354 relayed packets over the window: **103 accepted
  handshakes/h, 9.7 new sessions/h and ~174.7 k relayed packets/h** fleet-wide.
  Each created session costs ~10.7 accepted handshakes (93.9 sessions per 1,000
  accepted handshakes).
- **The drain has not stopped.** In a paired sample taken across
  `2026-11-18T05:26:57Z`–`05:32:39Z` (per-relay delta window 334.2–334.3 s, i.e.
  5 min 34 s; n = 728 handshakes) the fleet added **6 accepted and 722 alert-120
  rejections (99.18 %)**, with
  **0 new sessions and 0 relayed packets on all eight relays**. The rejection
  rate during that window was ~7,776/h, below the 38.5 h mean of ~17,498/h — this
  sample is from the quiet part of the UTC day and is not an estimate of the
  mean.
- **No relay is faulted.** The red `Health Anomaly Monitor` runs are measuring
  the stranded population (auth rejects with ~0 sessions on `relay-syd-1`, and
  `relay-fra` carrying 52.8 % of relayed packets), not a data-plane failure. All
  eight relays are healthy and forwarding.
- **`relay-syd-1` is the extreme case:** 42 accepted of 32,969 handshakes
  (0.127 % accept share), 6 sessions and 17,903 packets in 38.57 h.

## 2. Evidence table

**Window:** one snapshot per relay at `2026-11-18T05:26:57Z–05:27:04Z` UTC
(`/health` then `/metrics`), cumulative counters over each relay's process
lifetime (38.36–38.63 h; a `required`-only window by construction).

| relay | region | uptime h | handshakes n | accepted | rejected alert-120 | rejected_other | reject share | rejects : accepts | sessions | sessions / 1,000 accepted | packets relayed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| relay-lax-1 | us-west | 38.38 | 30,955 | 213 | 30,738 | 4 | 99.299 % | 144.3 : 1 | 23 | 108.0 | 497,807 |
| relay-ewr-1 | us-east | 38.56 | 150,938 | 526 | 150,411 | 1 | 99.651 % | 286.0 : 1 | 64 | 121.7 | 577,509 |
| relay-sgp-1 | ap-southeast | 38.36 | 50,411 | 137 | 50,273 | 1 | 99.726 % | 367.0 : 1 | 14 | 102.2 | 440,533 |
| relay-fra | eu-central | 38.63 | 292,711 | 2,500 | 290,125 | 86 | 99.117 % | 116.1 : 1 | 190 | 76.0 | 3,551,788 |
| relay-nrt | ap-northeast | 38.56 | 43,305 | 111 | 43,193 | 1 | 99.741 % | 389.1 : 1 | 13 | 117.1 | 65,172 |
| relay-bom-1 | ap-south | 38.37 | 51,880 | 196 | 51,684 | 0 | 99.622 % | 263.7 : 1 | 29 | 148.0 | 1,325,918 |
| relay-mad-1 | eu-south | 38.48 | 24,392 | 248 | 24,144 | 0 | 98.983 % | 97.4 : 1 | 34 | 137.1 | 246,724 |
| relay-syd-1 | ap-southeast-2 | 38.57 | 32,969 | 42 | 32,927 | 0 | 99.873 % | 784.0 : 1 | 6 | 142.9 | 17,903 |
| **fleet** | — | **38.49** | **677,561** | **3,973** | **673,495** | **93** | **99.400 %** | **169.5 : 1** | **373** | **93.9** | **6,723,354** |

`accepted = negotiated_v1 + no_alpn`; `no_alpn = 0` on all eight relays, so under
`required` every accepted handshake offered `lightspeed-control/1`. "Accepted" is
a handshake count, not a count of unique clients. `no_alpn` means "handshake
completed, no ALPN selected" and `negotiated_v1` means one was
(`proxy/src/control.rs:594-601`), so the split is a property of the handshake, not
of the session.

**Paired sample** (taken `2026-11-18T05:26:57Z`–`05:32:39Z`; per-relay delta
window 334.2–334.3 s = 5 min 34 s; values are counter deltas):

| relay | accepted | rejected alert-120 | window reject share | new sessions | relayed packets | rejects/h in window |
|---|---:|---:|---:|---:|---:|---:|
| relay-lax-1 | 0 | 89 | 100.00 % | 0 | 0 | 958.5 |
| relay-ewr-1 | 0 | 176 | 100.00 % | 0 | 0 | 1,895.3 |
| relay-sgp-1 | 0 | 87 | 100.00 % | 0 | 0 | 936.9 |
| relay-fra | 5 | 91 | 94.79 % | 0 | 0 | 979.9 |
| relay-nrt | 0 | 93 | 100.00 % | 0 | 0 | 1,001.4 |
| relay-bom-1 | 1 | 8 | 88.89 % | 0 | 0 | 86.2 |
| relay-mad-1 | 0 | 89 | 100.00 % | 0 | 0 | 958.7 |
| relay-syd-1 | 0 | 89 | 100.00 % | 0 | 0 | 958.8 |
| **fleet** | **6** | **722** | **99.18 %** | **0** | **0** | **7,775.9** |

**Source and method.** Relay addresses were resolved from the signed registry
published at `https://shibbityshwab.github.io/lightspeed/registry.json`
(8 nodes, 0 revoked; document sha256
`431019508bff33cf0a79c968d0371f7d7ae7b2abf16fb228b71aac72e43557e9`). The
Ed25519 signature was **not re-verified** in this task; the two relay addresses
that prior evidence records independently — `relay-syd-1` 45.76.114.93 and
`relay-fra` 95.179.164.138 — match this document
(`wat/evidence/2026-11-18/project-audit/fleet-alpn-confirmation.md`). Fetch
scripts, raw `/health` and `/metrics` bodies (three rounds), derived CSVs and JSON
are in `wat/evidence/2026-11-18/team/net-eng/` (see §7). `packets_relayed`,
`sessions_created` and `uptime` read from `/health` were cross-checked against
`/metrics` for all eight relays with zero mismatches.

## 3. What the numbers can and cannot say

**Can say (verified):**

- The rejection share, accept share, sessions/packets and per-relay split above,
  with n and window stated.
- The rejection counter is still increasing at both the fleet and per-relay
  level; it is not a one-off spike.
- The relay fleet is healthy while doing this; the rejections are a policy
  outcome (`required` refusing peers that do not offer the ALPN identifier), not
  a relay fault.
- No relay has zero lifetime accepted sessions: every relay has served at least
  6 sessions (`relay-syd-1`, the minimum). In the 5 min 34 s sample, however, all
  eight relays created **zero** new sessions and relayed **zero** packets.

**Cannot say (unverified — do not treat as fact):**

- **How much of the 99.4 % is stranded clients.** The alert-120 counter counts
  every peer that arrives without the ALPN identifier: a pre-1.11.0 client, an
  internet port scanner, a middlebox or a misconfigured tool are
  indistinguishable in the metric. Public endpoints expose no source addresses.
  The 99.4 % is therefore an **upper bound on the stranded-client share, not a
  measurement of it**. Attribution needs the relay's own rejection log line
  (`reason="no_application_protocol"`, `remote=<ip:port>`) — operator-only, not
  available from here.
- **How many unique stranded users exist, or whether they still retry.** The
  counters have no client identity. Six of the eight relays each added 87–93
  rejections in the 5.6 min sample (`relay-ewr-1` 176, `relay-bom-1` 8), which is
  consistent with automated retry/probe traffic but does not prove it.
- **A historical trend.** The `stats` branch is reachable and its history was
  read, but it does **not** carry the control-handshake counters — see below.

**Stats branch attempt (result: trend unavailable).** The `stats` branch is
reachable
(`https://raw.githubusercontent.com/ShibbityShwab/lightspeed/stats/web/network-history.json`
and the contents API both worked). Its `web/network-history.json` is a
9,832,384-char, version-1 document with **333 snapshots** spanning
`2026-09-19T12:20:34Z` → `2026-11-18T02:01:10Z` (median gap 15,800 s ≈ 4.4 h;
min 15 s, max 87,598 s). Each snapshot records per relay: `version`,
`active_sessions`, and the data-plane counter family (`packets_relayed`,
`packets_dropped`, `drops_auth_rejected`, `sessions_created`, FEC/route/latency
sums, `reset` flags). **It contains no `control_handshakes` / ALPN / `control_alpn`
field at all** (verified by schema scan and by a full-file search for
`handshake`, `alpn`, `no_application` — 0 matches). Consequences:

- `M1 = rejected_no_application_protocol` cannot be trended from the stats
  branch; the only ALPN-rejection evidence is the point-in-time
  `/metrics` sampling in this memo (two independent samples).
- The history does show every relay moving `1.10.3 → 1.11.0` at
  `2026-11-16T17:49:46Z` (first snapshot to observe 1.11.0), and it records
  `sessions_created` deltas through the flip. That is not a substitute for M1:
  accepted sessions are confounded by traffic level and client mix, and the
  history does not record `control_alpn`, so the off→required flip itself is not
  observable in it.

## 4. Why this is an owner decision and not an operator one

The plan reserves the choice (`docs/quic-alpn-rollout.md` §11): flipping a relay
to `required` or back to `off` is a fleet operation with a known blast radius
(that relay's pre-1.11.0 clients). The project already has an owner decision on
record — the 2026-11-16 traffic-impact authorization and the 2026-11-17
"update requirement accepted" decision, announced publicly in discussion #177
(1.11.0+ is required on the community relays). What is still open is the
**stranded-client mercy question**: the fleet is forcing pre-1.11.0 users to
update or self-host, and the rejection counters show that population is large in
count but unmeasured in identity. Re-opening the policy (Options B or C) needs a
new owner yes/no; keeping it (Option A) is the status quo.

## 5. Options

All three are configuration-only, use the existing fail-closed flip tool, and
keep owner spend at $0.00 (`[COST_STUB]` unaffected). None is executed by this
memo.

### Option A — keep `required` on all eight relays (no change)

- **Impact.** Status quo continues: updated clients are served (3,973 accepted
  handshakes → 373 sessions → 6.72 M packets over 38.5 h), and peers without the
  ALPN identifier keep being refused at ~1.75 × 10⁴ rejections/h. No relay is
  restarted; nothing changes on the wire for 1.11.0+ clients.
- **Risk.** Pre-1.11.0 clients cannot recover on any community relay; they must
  update or self-host. The rejection churn and the red anomaly runs continue,
  and the only public signal remains the client's alert-120 error. If a material
  share of those rejects is real users (unverified), the project is refusing
  them.
- **Rollback.** Not applicable — this is the current state. Any later move to B
  or C uses the same owner-gated flip path.
- **Falsifier.** Falsified if the stranded peers are users rather than scanners:
  a relay-log source-IP sample over one window showing repeated, client-like
  retry cadence from distinct addresses that scanning does not explain would mean
  `required` is refusing real users and the policy must change. The trigger is
  observable only from the relay's own rejection log — public counters cannot
  produce it.

### Option B — mercy `off` on selected relay(s), starting with `relay-syd-1`

- **Impact.** One relay stops advertising the ALPN identifier: pre-1.11.0
  clients can register there again, and current clients keep working (they fall
  back once per process — already the shipped 1.11.0 behaviour against an `off`
  relay). `relay-syd-1` is the suggested candidate because it has the lowest
  accept share (0.127 %), the fewest sessions (6 in 38.57 h) and the strongest
  rejection signature (32,927 alert-120 vs 42 accepted); its served load is small
  (17,903 packets, ~464 packets/h). It remains one relay of eight, so the other
  seven keep the strict policy.
- **Risk.** The "all eight relays `required`" invariant in
  `docs/community-network.md` becomes false and needs a docs correction; the
  alert-120 counter on that relay stops discriminating (accepted handshakes
  become `no_alpn`), so that relay's ALPN meter is lost. It also spends one relay
  on an unverified hypothesis: if the peers were scanners, `off` simply accepts
  more unauthenticated control-plane handshakes (they still cannot relay: auth
  stays enforced) and buys nothing.
- **Rollback.** `infra/scripts/relay-control-alpn.sh --node relay-syd-1 --mode required`
  (or the `Deploy Proxy` dispatch form). The tool backs up the config, runs the
  deployed binary's `--check`, restarts, health-gates, and auto-restores on any
  failure. Same one-command path as the original flip; no client change.
- **Falsifier.** Falsified if the relay's `sessions_created_total` and
  `packets_relayed_total` do not rise after the flip relative to a comparable
  window on a relay left `required` (paired `/metrics` samples before/after, same
  clock hour): then no stranded users came back to that relay, the mercy relay
  bought nothing, and it should be re-flipped to `required`.

### Option C — revert all eight relays to `off`

- **Impact.** Every relay accepts peers that do not offer the identifier: any
  pre-1.11.0 client can register fleet-wide again, and current clients keep
  working with one fallback handshake per process. Rejections become acceptances
  and M1 drops toward zero fleet-wide — but that measures the policy change, not
  the stranded population.
- **Risk.** Undoes the strict-ALPN property achieved by the 2026-11-16 rollout,
  requires eight restarts (one per relay, sequential, health-gated), and
  contradicts the public requirement in discussion #177 — re-opening it needs a
  new owner-approved Announcements post (`[COMMS_STUB]`). It also widens the
  control plane to unauthenticated handshake load from scanners/middleboxes,
  which `required` currently refuses, and loses the fleet-wide discrimination
  the `required` counters provide. It does not restore any capability that
  1.11.0+ clients lack.
- **Rollback.** The same tool in reverse, one relay at a time:
  `--node <relay> --mode required`, each health-gated with its own backup.
- **Falsifier.** Falsified if, after all relays are `off`, accepted handshakes
  rise without a corresponding rise in `sessions_created`/`packets_relayed`, or
  if `drops_abuse_blocked`/`rejected_other` rise: the newly accepted peers are
  not users, and the fleet gave up the strict policy for no user benefit. Paired
  `/metrics` samples plus a source-IP read of the accepted population are the
  probe.

## 6. Recommendation

**Keep `required` on all eight relays (Option A) for now.** The measured data
does not support a fleet reversion: the relays are healthy and serving, the
rejection churn coincided with no data-plane activity at all (0 sessions, 0
packets fleet-wide in the paired sample), and the stranded-client share is
unverified — Option C would trade a shipped, owner-approved and publicly
announced policy for an unmeasured benefit.

**If the owner wants to act for stranded users, Option B on `relay-syd-1` is the
only defensible fleet change**, because it is one relay, one command to undo, and
its falsifier is directly observable (does that relay's session/packet rate rise
or not?). Option C is not supported by any measurement in this memo.

**Before spending a relay, get the two cheap instruments**, both outside this
memo's write scope and neither requiring a fleet config change:

1. **Make M1 trendable.** Extend the CI-side collector
   (`infra/scripts/collect-metrics.sh` + the history schema) to record the four
   `lightspeed_control_handshakes_total{outcome=...}` series per relay, so the
   `stats` branch — which currently records no ALPN field — can show whether the
   rejection population is draining. This is a POSIX/CI change, not a desktop
   one (owner: `infra-dev`).
2. **Attribute M1.** Read one relay's rejection log lines
   (`reason="no_application_protocol"`, `remote=<ip:port>`) over a window and
   count distinct source addresses/ASNs against retry cadence. This separates
   stranded clients from scanners and is the one input this memo could not
   obtain. Operator-only; no config change.

If the owner chooses Option B, the follow-up owner gate is the docs/announcement
correction for the "all eight required" statement; if the owner chooses Option C,
the gate is the new public announcement that goes with it. Neither follow-up is
started here.

## 7. Evidence index

All under `wat/evidence/2026-11-18/team/net-eng/` (gitignored):

| File | What it is |
|---|---|
| `registry.json` | Raw signed registry document used for relay addresses (sha256 in §2) |
| `fetch-fleet.ps1` | Read-only fetch: `/health` + `/metrics` for all eight relays, three rounds |
| `raw/round1/`, `raw/round2/`, `raw/round3/` | Raw per-relay `*.health.json` and `*.metrics.txt` bodies |
| `round{1,2,3}-summary.csv`, `round{1,2,3}-meta.json` | Fetch timestamps, HTTP codes, health fields, registry hashes |
| `parse-fleet.ps1`, `round{1,2,3}-table.{csv,txt}` | Parsed per-relay handshake split/sessions/packets |
| `final-lifetime-metrics.csv` | Per-relay derived lifetime metrics (reject share, ratios, rates) |
| `fleet-lifetime-summary.json` | Fleet totals and shares used in §1–§2 |
| `compute-deltas.ps1`, `delta-round1-round3.{csv,txt}`, `delta-round1-round3-fleet.json` | Paired-sample deltas and window rates |
| `stats-network-history.json` | Raw `stats` branch history (333 snapshots) analysed in §3 |
| `stats-contents.json`, `analyze-stats-history.ps1`, `stats-history-analysis.txt` | Stats-branch reachability, schema scan (no ALPN fields) and version transitions |

No fleet configuration was read, written or restarted to produce this memo.
