# Relay-vantage destination probe (v2): feasibility, verdict, and the fleet runbook

> **Status:** feasibility + GO/NO-GO deliverable (board `task-15`, owner
> `infra-dev`). **Nothing was deployed, no relay config or service was touched,
> no SSH session or fleet command was run from this task.** The decisive ICMP
> measurement needs the relay hosts and is specified here as an operator runbook.
> Date 2026-11-16, HEAD `afcde5a` (task-12's source doc was written at `46c1a61`).
>
> **Evidence:** `wat/evidence/2026-11-16/server-leg-probe/` (index in `README.md`
> there). **Source proposal:** `docs/server-leg-signal.md` §5.2, §6 v2, §7
> F3/F4 (task-12). **Prior design:** `docs/design/relay-server-leg.md`.

## 0. Verdict at the top

**NO-GO today for starting the v2 relay-vantage probe implementation.** This is
not a verdict that v2 is impossible; it is a verdict that the two facts that
decide it are still unmeasured and one of them (F4) cannot be measured until v1
lands, plus the relay privilege path is unresolved. The measurements are
specified, bounded, read-only, and can be run by the owner in about 30 minutes
plus a day of normal play. If they pass, v2 becomes implementable with a canary
and rollback that already exist in the fleet's release machinery.

| Gate | Question | State today | Evidence |
|---|---|---|---|
| G1 | Can a relay host send ICMP at all (privilege + egress)? | **UNMEASURED** (host facts) | §2.3, §5.1; `icmp-policy.out` |
| G2 | Is there a real destination sample of the required size? | **DATA SOURCE FOUND, sample not extracted**: relay journald logs every new session's `game_server=`; ~876 session events fleet-wide in the last ~41 h | §2.2, §3.2; `code-paths.out` |
| G3 | F3: do game servers answer ICMP from relay IPs at rate ≥ 50 %? | **UNMEASURED** (the decisive result; fleet-only) | §2.4 (indicative warning), §4 phase D, §5.4 |
| G4 | F4: does ICMP RTT track the tunnelled UDP path within 20 %? | **NOT EVALUABLE**: v1 quiet-flow samples do not exist yet (task-13 in progress) | §5.3 |
| G5 | Security review + owner gate for the fleet step | Not started; constraints enumerated in §6 | `docs/server-leg-signal.md` §5.2 |

**What upgrades NO-GO to GO:** G1 passes with *unprivileged* ICMP (no
`CAP_NET_RAW` on the daemon), G2's sample gate passes, G3 reports a fleet answer
rate ≥ 50 %, v1 has landed so F4 can be measured, sec-ops signs the constraint
list, and the owner approves the one-relay canary. **What ends v2 permanently:**
G1 fails with `CAP_NET_RAW` as the only route (the design doc's own verdict is
"not acceptable"), or G3 < 50 %, or G4 > 20 % divergence.

**Two findings changed the plan from the task description:**

1. **The destination is not available from the control plane in production.**
   `ControlMessage::Register` has carried an optional `destination` since the
   region feature, but *no production client call site ever supplies it* (all
   four production callers use `connect(addr, game)`, which passes `None`), so
   `RegisterAck.dest_region` is `None` in practice. A v2 implementation must not
   assume that field is populated.
2. **The relay already learns and logs the destination on the data plane.** Every
   new game session is created from the tunnel header's original destination
   *after* `is_public_ipv4` validation, and the relay logs it at INFO:
   `New client session created client=<player ip:port> game_server=<server ip:port>`.
   That is the real destination sample source — existing logs, no capture, no
   code change — and it is also where a v2 probe should read from (the session
   struct already stores `game_server` in memory).

## 1. The question this document settles

From `docs/server-leg-signal.md` §5.2: the client can only measure the relay it
sends traffic through, so ranking the other candidates on the server leg needs a
measurement *from each relay's vantage*. Carrying the player's game datagrams
through all candidates duplicates the player's packets to the game server and is
rejected (payload duplication). The proposed v2 signal is: relay sends `N = 3`
fixed-format ICMP echoes to the session's game server, returns
`{rtt_p50_us, samples, answered}` to the requesting client only, cached and
rate-limited.

This task (task-15) must answer the two falsifiers:

- **F3** — "ICMP from the relays is not answered by game servers in most sessions
  (rate < 50 %)" → if true, v2 is dead, stay after v1.
- **F4** — "relay→server ICMP RTT is a poor proxy for the UDP path (differs > 20 %
  from v1's L2 or from the total-path change)" → if true, the probe must not
  steer the ranking.

and it must produce the security constraints, the rollout/rollback plan, the
exact fleet runbook, and an explicit verdict. It must not change production.

## 2. What can be verified from the workstation (read-only, done)

### 2.1 Fleet exposure and what `/metrics` already publishes

Command: `powershell -NoProfile -ExecutionPolicy Bypass -File
wat\evidence\2026-11-16\server-leg-probe\scripts\probe-relays.ps1`
(public `GET /health` + `GET /metrics` on the documented port 8080 for the 8
registry nodes; no other port touched).

Window **2026-11-16T09:21:59Z** (host local 16:21:59+07), one scrape round:

| relay | region | /health | /metrics | version | uptime h | metric lines | IPv4 literals in /metrics |
|---|---|---|---|---|---|---|---|
| relay-lax-1 | us-west | 200 | 200 | 1.10.3 | 41.1 | 290 | 0 |
| relay-ewr-1 | us-east | 200 | 200 | 1.10.3 | 41.3 | 357 | 0 |
| relay-sgp-1 | ap-southeast | 200 | 200 | 1.10.3 | 41.0 | 349 | 0 |
| relay-fra | eu-central | 200 | 200 | 1.10.3 | 41.0 | 710 | 0 |
| relay-nrt | ap-northeast | 200 | 200 | 1.10.3 | 41.0 | 301 | 0 |
| relay-bom-1 | ap-south | 200 | 200 | 1.10.3 | 41.0 | 432 | 0 |
| relay-mad-1 | eu-south | 200 | 200 | 1.10.3 | 41.0 | 309 | 0 |
| relay-syd-1 | ap-southeast-2 | 200 | 200 | 1.10.3 | 41.1 | 199 | 0 |

(`relay-fra`'s line count includes the destination-country counter series; all
eight bodies are in `relay-scrape/`. `metric lines` is the raw text-line count,
HELP families are 65 per relay — see `relay-probe.out`.)

Relevant absences, all verified across all eight bodies:

- **No destination IP in any `/metrics` body** — zero IPv4 literals in the
  entire fleet's metrics (`ipv4_in_metrics = 0` for all 8).
- **No ICMP counter** — the only case-insensitive `icmp` match per relay is the
  HELP text of `lightspeed_telemetry_direct_ms_sum`, which is *client-reported*
  direct ICMP, not a relay probe.
- The only destination data the fleet publishes is **country pairs**:
  `lightspeed_geo_sessions_total{src="US",dst="DE"}` etc. (3–33 series per
  relay; `proxy/src/metrics.rs:1664-1706`). The country is resolved from
  `ClientSession.game_server` (`proxy/src/relay.rs:721-751`), i.e. from the data
  plane, not from the control plane and not from the client register field.
- The collector that writes the public stats branch stores no destination either:
  its counter list is latency/volume only and its geo map is country pairs
  (`infra/scripts/collect-metrics.sh:54-58,108-120`).

`/health` does expose the deployment facts an operator needs for a canary:
`version` (1.10.3 fleet-wide), `update.current_version/active_release`,
`handoff.supported=true`, uptime, session/packet counters — but no destination.

### 2.2 Where a real destination sample comes from (the data-source finding)

Reading the relay code paths (`code-paths.out` has the full `git grep`
transcript):

| Fact | Location |
|---|---|
| Control `Register` carries `destination: Option<Ipv4Addr>` (trailing field) | `protocol/src/control.rs:175-190` |
| Ack carries `dest_region: Option<String>` (trailing) | `protocol/src/control.rs:193-219, 457-486` |
| **Production never passes a destination** — all callers use `connect(addr, game)` → `None` | `client/src/quic/mod.rs:180-224`; callers `quic/discovery.rs:141`, `quic/health.rs:132`, `modes/live_test.rs:253`, `modes/control_test.rs:30` |
| Relay uses the register destination only to compute the ack region, then drops it | `proxy/src/control.rs:539-615` (no destination on the control session struct) |
| Data plane: destination comes from the tunnel header, is **validated public unicast** before use | `proxy/src/relay.rs:1461-1504`, `proxy/src/abuse.rs:141-194,292-356` |
| Session stores `game_server: SocketAddrV4` in relay memory, keyed by client address | `proxy/src/relay.rs:398-451, 785-877` |
| **Relay logs it at INFO** — default log filter is `info` | `proxy/src/relay.rs:831-839`; `proxy/src/main.rs:282-287` |
| Relay logs the game id at INFO on registration | `proxy/src/control.rs:590-593` |
| Tunnel header preserves original src/dst IP + ports (plaintext, by design) | `protocol/src/header.rs:132-154, 228-248` (`orig_dst_ip` at payload bytes 15-18) |

Expected journald lines (shapes taken from those statements; synthetic fixture
in `fixtures/`, parser test in `parser-test.out`):

```
2026-11-16T08:44:55Z  INFO lightspeed_proxy::control: Registered client <client ip:port> → session 42 token=... (game=27, data_port=0)
2026-11-16T08:44:56Z  INFO lightspeed_proxy::relay: New client session created client=<client ip:port> game_server=<server ip:port> outbound_port=... fec=false
```

So the sample source named in the task ("read from each relay's existing
logs/metrics") **exists and is already on every relay's journal**, at the default
log level, with no code or config change. Two caveats:

- **Game attribution is an inference.** The session line has the client IP and
  the destination but not the game; the register line has the client IP and the
  game but not the destination. Both are joined on client IP + nearest preceding
  register (bounded to 6 h, see `scripts/parse-sessions.ps1`), and rows without a
  match are reported as `game=unknown` rather than guessed. This is adequate for
  a per-game feasibility table; a canary implementation has the real `game` in
  the session itself.
- **Journal retention is a host fact.** `uptime` is ~41 h and the session
  counters show ~876 lifetime session events across the fleet in that uptime
  (`sessions_created`: lax 65, ewr 152, sgp 77, fra 325, nrt 70, bom 117, mad 61,
  syd 9). If the journal is volatile, the sample window is "since last boot";
  the preflight reports the real oldest/newest line. A 6-way relay extraction of
  a day of ordinary play is expected to clear n = 30 distinct destinations; that
  expectation is a calculation from session counts, **not** a measured distinct
  count — the extraction is the check (`scripts/parse-sessions.ps1` prints the
  sample gate explicitly).

### 2.3 ICMP policy in the repo and on the unit

Full transcript: `icmp-policy.out`.

| Surface | Fact | Location |
|---|---|---|
| Relay unit | `DynamicUser=true`, `NoNewPrivileges=true`, no `AmbientCapabilities`/`CapabilityBoundingSet` → **no raw ICMP socket for the daemon as deployed** | `infra/scripts/setup-new-node.sh:180-207` |
| Unprivileged ICMP | possible only if host `net.ipv4.ping_group_range` covers the service's dynamic GID — a **host** fact, unknown here | runbook phase A |
| Prior decision | ship only with unprivileged ICMP; do **not** give the network-facing daemon `CAP_NET_RAW` (it also permits spoofed sources); if capability is the only route, v2 is not acceptable | `docs/design/relay-server-leg.md:24-41,159-223` |
| Decision log | "needs a relay ICMP-privilege decision" — still open | `wat/state/decisions/2026-09.md:12` |
| Relay egress | ufw rules add only inbound allows; default outgoing is allow and replies match ESTABLISHED,RELATED; no cloud egress filter configured by the repo | `infra/scripts/setup-new-node.sh:209-217` |
| Provider (external) | Vultr firewall filters inbound only; outbound is the instance's own responsibility | <https://docs.vultr.com/support/products/network/does-vultr-firewall-filter-outgoing-traffic-from-my-instance> |
| Ubuntu caveat (external) | iputils dropped its setcap in 2024 assuming unprivileged ping sockets work, but Ubuntu historically ships the kernel default `0 1` (root-only) unless `ping_group_range` is set | <https://lists.ubuntu.com/archives/foundations-bugs/2024-November/528057.html> |
| Client precedent (does not transfer) | the desktop client already uses unprivileged DGRAM ICMP on Linux/macOS and raw on Windows because it runs elevated for WinDivert | `client/src/latency.rs:121-139` |
| Relay code today | the proxy has no ICMP sender and no raw-socket dependency; the only `icmp` string in the proxy is a HELP text | `git grep -i icmp -- proxy/` |

**Reading:** the blocking unknown is unchanged from the design doc and is a
privilege question, not a protocol question. The probe itself is low risk
(1:1 echo, no spoofing possible on an unprivileged socket, and the relay already
forwards arbitrary public UDP for authenticated clients); the privilege required
to send it is the risk. If unprivileged ICMP cannot be enabled on the hosts, the
answer is NO-GO, not `CAP_NET_RAW`.

### 2.4 An indicative warning about F3 from the deployed fleet

Client telemetry reports carry `direct_p50_ms: Option<f32>` — the median ICMP
RTT to the game server, which the client sets only when its 5-echo burst got
≥ 3 replies and the median is fresh and for the same server as the relayed window
(`protocol/src/telemetry.rs:103-120`; `client/src/latency.rs:307-315,728-736`).
Across the fleet's **lifetime counters** (same scrape as §2.1; window = ~41 h
uptime; reducer = sum of `_count` over (game,country) cells):

| game | telemetry reports | reports with a usable direct ICMP median | share |
|---|---|---|---|
| fortnite | 795 | 47 | 5.9 % |
| robrun | 67 | 2 | 3.0 % |
| roblox | 473 | 0 | 0.0 % |
| valorant | 659 | 0 | 0.0 % |
| cs2 | 97 | 0 | 0.0 % |
| pubg | 62 | 0 | 0.0 % |
| others / unknown | 623 | 0 | 0.0 % |
| **fleet** | **2,776** | **49** | **1.8 %** |

Per relay the extremes are `relay-bom-1 fortnite 36/43 (83.7 %)` and everything
else at ~0 (`icmp-coverage.out` has the per-(relay,game) table).

**How to read this, honestly:** it is a *negative prior* for F3, not evidence
against it. It says that from the deployed client base, ICMP to real game servers
usually does not produce a usable median — the same environment from which the
relay would probe, minus the datacenter-to-datacenter path. It is confounded: a
report omits the field when the last burst failed *or* when the relayed window's
server changed in the last 300 s; the `unknown` bucket cannot carry one by
construction; the counts are lifetime and small. The one case we can see working
is the ROB RUN server in the task-10 live session, which answered ICMP from the
player's vantage (5 published bursts of 5 probes, ≥ 3/5 replies required per
burst, medians 283.47–291.20 ms — task-12 §4.1). So game servers *can* answer
ICMP; whether they do so **from relay IPs, at ≥ 50 %** is exactly what phase C
measures.

## 3. The measurement design (what the fleet run must produce)

### 3.1 Definition of the number

For a relay `R` and a game-server IP `S` observed in that relay's sessions:

- issue **one burst of 5 ICMP echo requests** from `R` to `S` (interval 0.2 s,
  1 s per-reply timeout, 12 s overall deadline — bounded, no retry);
- `answered(R,S) = 1` if at least one echo reply arrived, else 0;
- `answer_rate(R) = mean over S of answered(R,S)`; fleet answer rate is the mean
  over all `(R,S)` measurements (or per-relay, both reported);
- `rtt` statistics are min/avg/max from the replies; the analysis uses the avg as
  the burst RTT and reports a **median across destinations** per relay/game.
- The RTT unit is **µs-equivalent ms from the relay's kernel clock**, one-sided
  ICMP, not the game's UDP path (that is F4's question, §5.3).

Every table in the report carries **n** (bursts, distinct destinations), the
**window** (start/end UTC of the run and the journal window the sample came
from), and the **reducer** (share for answer rate, median for RTT).

### 3.2 Minimum sample (the task's gate, made concrete)

- **≥ 30 distinct game-server IPs**, from logs only;
- observed across **≥ 3 relays** (default cross-probe: every destination is
  probed **from each of 3+ relays**, so a 30-destination sample yields ≥ 90
  bursts — the minimum for a per-relay comparison, and the shape the ranking
  would actually use: a candidate relay measured against a destination it has
  not carried traffic to yet);
- **≥ 2 games** attributed (per-game table; `unknown` rows do not satisfy this);
- one run per destination per relay (no repeated bursts inside 60 s);
- total traffic: ≤ 30 destinations × 8 relays × 5 echoes = 1,200 echo packets
  worst case; default 3 relays = 450. Nothing is written on the relay.

If the extraction yields fewer than the gate, the run reports the gate as NOT MET
and stops — widening `--since`, adding relays, or collecting another day of play
are the only permitted ways to grow it (no invented destinations, no port scans,
no prefix sweeps).

### 3.3 What is deliberately *not* used

- **No `tcpdump` / packet capture.** The journal already has the destination;
  capture would add a package and a privacy surface for no extra information.
  (Fallback, documented but not part of the run: if a host's journal is
  unavailable, the tunnel header carries `session_token` at payload bytes 1-4 and
  `orig_dst_ip` at bytes 15-18 (`protocol/src/header.rs:228-248`), and the
  session token is already printed by the register log line — that is the only
  zero-code alternative, and it is strictly worse.)
- **No control-plane inspection.** The QUIC control connection is mTLS; the
  register destination is not observable on the wire, and production never sends
  it (§2.2).
- **No port scans, no prefix sweeps, no invented destinations.** Every probed
  address is a game server the relay already forwarded an authenticated session
  to, so the relay's own `abuse.rs` validation already ran on it.
- **No relay service, config, or package change.** The collector payload is
  `sh relay-probe.sh …` over SSH; the only side effects are ICMP echo requests.

## 4. Operator runbook (what a human with fleet access must run)

Everything below is driven from the owner's Windows workstation over the owner's
fleet SSH key. **`DEPLOY_SSH_KEY` (the CI secret) is off-limits to a subagent and
was not used.** The run is read-only; owner approval is required for the fleet
step itself (production hosts), and the script prints every remote command first.

### Phase A — preflight (≈ 5 minutes, read-only)

Purpose: answer G1 and establish journal retention. Required output per relay:
service posture, `ping_group_range`, the two control pings, journal window.

```powershell
# repo root, PowerShell 5.1 (per-process bypass: this host blocks .ps1 execution)
powershell -NoProfile -ExecutionPolicy Bypass -File `
  wat\evidence\2026-11-16\server-leg-probe\scripts\run-operator-probe.ps1 `
  -Phase preflight -Relays relay-lax-1,relay-ewr-1,relay-fra
```

Manual equivalent for one relay:

```powershell
Get-Content -Raw wat\evidence\2026-11-16\server-leg-probe\scripts\relay-probe.sh |
  ssh -i $HOME\.ssh\lightspeed_deploy root@<relay-ip> "sh -s -- preflight"
```

What to write down from each `preflight-<node>.txt`:

| Field | Pass | Meaning if it fails |
|---|---|---|
| `ping_bin` | an absolute path | no `ping`; the run cannot proceed without an operator package decision |
| `root_ping_rc` | `0` | the relay host itself cannot reach the internet with ICMP → **G1 FAIL**, stop |
| `dynamicuser_ping_rc` | `0` | the *service posture* (DynamicUser + NoNewPrivileges) cannot send ICMP → see privilege branch below |
| `ping_group_range` | reported; if a sysctl change is needed, record the current value for rollback | — |
| `unit_DynamicUser` / `unit_NoNewPrivileges` | `true` / `yes` (expected) | unexpected posture: report it before touching anything |
| `journal_oldest` / `journal_session_lines` | non-empty / > 0 | journal unavailable or rotated → widen or wait for play; do not install anything |

**Privilege branch (decision, not a guess):**
1. If `dynamicuser_ping_rc = 0`, unprivileged ICMP works as deployed → G1 PASS
   with **no unit change at all**.
2. If it fails and `ping_group_range` excludes the dynamic GID range, the owner
   may *consider* a host sysctl change
   (`net.ipv4.ping_group_range` covering systemd's dynamic range) as a canary
   step — a host-level change, reversible by restoring the recorded value. It is
   not made in this task and needs the owner gate.
3. If the only workable route is `AmbientCapabilities=CAP_NET_RAW` on the relay
   daemon, **G1 FAIL → NO-GO permanently** (prior design verdict; do not do it).

### Phase B — destination sample (≈ 10 minutes, read-only)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  wat\evidence\2026-11-16\server-leg-probe\scripts\run-operator-probe.ps1 `
  -Phase sessions -Relays relay-lax-1,relay-ewr-1,relay-fra -SessionsDays 7
```

This saves `journal-<node>.txt` per relay (raw journald lines) and runs
`parse-sessions.ps1`, which writes:

- `sample.csv`, `ip-map.local.txt` — **real server IPs, local only, never copy
  into the repo**;
- `sample-anon.csv`, `destinations.txt`, `parse-summary.out` — IDs and counts;
  `parse-summary.out` is the repo-safe artifact;
- a `SAMPLE GATE` line: PASS only with ≥ 30 destinations, ≥ 3 relays, ≥ 2 games.
  NOT MET means stop and widen the sample, not proceed with a smaller n.

Notes for the operator: the sample window is stated in `parse-summary.out`; the
game labels are inferred (client-IP + nearest-preceding-register join, §2.2) and
labelled `game_source=ip_time_join`; `unknown` rows are still valid ICMP targets
(they are real destinations) but do not count toward the ≥ 2 games gate. If
attribution coverage is poor, the owner playing two supported games through the
default relay set during the collection window fixes it — that is the only
manual input the measurement needs.

### Phase C — ICMP measurement (≈ 3 minutes, one burst per pair)

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  wat\evidence\2026-11-16\server-leg-probe\scripts\run-operator-probe.ps1 `
  -Phase ping -Relays relay-lax-1,relay-ewr-1,relay-fra
```

For each destination IP it runs, on each relay:

```text
ping -n -q -c 5 -i 0.2 -W 1 -w 12 <server-ip>
```

and writes `ping-<relay>.txt` plus, after `parse-ping.ps1`,
`ping-results.csv` (local only) and `ping-summary.out` (repo-safe). The payload
refuses non-public/loopback/link-local/multicast targets as defence in depth and
probes each address exactly once. Do not loop or re-run it repeatedly; one run is
the measurement (a re-run is only for a genuinely failed SSH session, and the
report must say so).

### Phase D — read the gate and report

`ping-summary.out` contains, with n and window: per-relay answer rate, per-game
answer rate, per-destination × relay RTT matrix (anonymised IDs), the fleet
verdict, and the worst relay. Report template (final numbers go in the task
report and `wat/evidence/<date>/server-leg-probe/`):

| relay | destinations | bursts | answered | answer rate | RTT median ms | worst-affected game |
|---|---|---|---|---|---|---|
| … | n | n | n | % | ms | game (rate) |

and the verdict lines:

- **G3 PASS** if fleet answer rate ≥ 50 % (report per relay/game; any relay below
  ~25 % is not a canary candidate);
- **G3 FAIL** if < 50 % → stop after v1, v2 stays unbuilt (record the exact
  numbers and the `ping-summary.out` path);
- **F4 pending** until §5.3 can run.

### Phase E — cleanup / rollback of the run

The run has no state to roll back: no service restart, no config write, no
package install, no file written on the relay, no container/volume change. The
only artifacts are ICMP packets and the operator's local `sample.csv` /
`ip-map.local.txt` / `ping-results.csv`, which stay outside the repo and can be
deleted after the anonymised tables are copied. If phase A's privilege branch
step 2 was used, restore the recorded `net.ipv4.ping_group_range` value and
re-run `sysctl` (the preflight record is the rollback reference); that change is
host-level and does not touch the relay process.

## 5. Falsifiers and the exact gates

### 5.1 G1 — privilege + egress

Already specified in phase A. Outcome options and their meaning are fixed in
advance: unprivileged path works → continue; sysctl path with owner approval →
continue on the canary host only; `CAP_NET_RAW`-only → **NO-GO**.

### 5.2 G2 — sample

Phase B's `SAMPLE GATE`. n, window, per-relay/per-game counts reported. NOT MET
is a legitimate stop, not a defect: it means the fleet seen in that window did
not produce enough distinct real destinations.

### 5.3 F4 — ICMP RTT vs the UDP path (needs v1)

F4 cannot be evaluated now: the v1 quiet-flow L2 signal does not exist on master
(task-13 is in progress at the time of writing). When v1 lands, on the same
`(relay, server)` pairs and window:

1. take the relay probe's median ICMP RTT for the pair from phase C's data
   (re-run on the canary relay against the servers v1 measured, or reuse the
   phase C values if the pairs match);
2. compare with v1's L2 (derived `median(L3) − median(L1 keepalive)`;
   `docs/server-leg-signal.md` §5.1);
3. **fail** if the relative difference exceeds 20 % on the population (per-pair
   spread reported), or if the probe's RTT cannot be reconciled with the
   independent app-level saving change measured in the same window;
4. also apply the triangle-inequality validity rule (§5.2: probe must not report
   below `direct − L1`).

If F4 fails, the probe may still be reported as a **labelled estimate** on the
route surface, but it must not steer the ranking; that is a product decision,
not an infra one.

### 5.4 Decision table

| G1 | G2 | G3 (F3) | F4 | Verdict |
|---|---|---|---|---|
| FAIL (CAP_NET_RAW only) | — | — | — | **NO-GO forever** (design verdict) |
| PASS unprivileged | NOT MET | — | — | **NO-GO now**; widen sample, rerun phases B-C |
| PASS unprivileged | PASS | < 50 % | — | **NO-GO**: stop after v1 (F3 falsified) |
| PASS unprivileged | PASS | ≥ 50 % | not yet measurable | **CONDITIONAL GO**: finish v1, then measure F4 |
| PASS unprivileged | PASS | ≥ 50 % | ≤ 20 % disagreement | **GO** to implementation (canary, owner-gated) |
| PASS unprivileged | PASS | ≥ 50 % | > 20 % disagreement | **NO-GO as a ranking key**; estimate-only at most |

## 6. Security review input (the constraints v2 must carry)

This is the `sec-ops` input named in the task; the implementation must satisfy
all of it, and the review happens before code, not after.

1. **Authentication and scope.** Probes are accepted only on the mTLS control
   connection and only for the destination of that session; the reply goes only
   to the requesting client. No unauthenticated probe endpoint, no cross-session
   result, no relay-to-relay relay.
2. **Destination validation.** Reuse `proxy/src/abuse.rs::is_public_ipv4`
   (RFC 1918, loopback, link-local, multicast, broadcast, CGNAT, documentation
   ranges) **and** the configured `destination_allowlist`; additionally require
   that the address has actually carried ≥ 20 forwarded packets for that live
   session (never an out-of-band address). Revalidate at probe time, not only at
   session creation.
3. **Rate and cost limits.** ≤ 1 probe per `(client, relay, destination)` per
   60 s; a per-client hourly cap (proposed 20 destination bursts); a per-relay
   global qps + concurrency cap; per-destination cache with 30 s TTL; burst size
   fixed at 3 (the implementation can use 3 where the manual run uses 5);
   no retry storm. Probes must be accounted against the egress budget or
   explicitly exempted with a documented reason — not silently excluded.
4. **Data handling.** No destination in `/metrics`; no new destination logging.
   Note the pre-existing surface: the relay already logs `game_server=` for every
   new session at INFO (`proxy/src/relay.rs:831-839`) — v2 must not widen or
   duplicate it, and the canary must not add per-destination series. The client
   must not persist per-destination probe series beyond what the estimator ring
   already keeps (`PATH_PROBE_WINDOW = 16`).
5. **Client trust.** Accept a report only from the bound control session; reject
   zero/absurd values; require `L1 + S` not below the direct measurement floor
   (triangle inequality); treat `unanswered` as "not measured" and fall back to
   the labelled prior — never rank on silence.
6. **Abuse posture.** ICMP echo is 1:1 and an unprivileged socket cannot spoof the
   source, so there is no amplification/reflection primitive; the relay already
   forwards arbitrary public UDP for authenticated clients, so the probe is
   strictly narrower. But relay IPs become recognisable as ICMP sources: keep a
   per-relay config switch to disable probing, and alert on unusual probe volume.
7. **Existing finding to fix separately (not this task):** the relay's INFO
   register line currently prints the data-plane `session_token`
   (`proxy/src/control.rs:590-593`; log line shape in
   `wat/evidence/2026-11-16/server-leg-probe/fixtures/`). That is a pre-existing
   credential-in-log smell and it is load-bearing only for the *fallback* capture
   path this runbook does not use. It should go to `rust-dev`/`sec-ops` as a Low
   finding; v2 must not add any token to any log.

## 7. Rollout and rollback plan (only if GO)

No production change happens in this task; this is the plan the GO would use.

**Protocol (additive, compatibility precedent already in tree).** Add one
control message pair `PathProbe { destination: Ipv4Addr }` →
`PathProbeReply { rtt_p50_us: u32, samples: u8, answered: u8 }`, or an additive
trailing `server_leg_rtt_us: Option<u32>` on `RegisterAck`. The codec already
supports unknown trailing fields: `RegisterAck` decodes optional trailing
telemetry/region/caps exactly this way (`protocol/src/control.rs:457-486`), and a
relay that predates a new message type ignores the frame
(`ControlDecodeError::UnknownMessageType`, the same precedent `RotateRequest`
documents). Add a `caps` bit (`protocol/src/control.rs:145-153`) so old relays
never receive a message type they do not know, and the client falls back to the
prior when the bit is absent. The client treats missing/unknown as "unmeasured".

**Relay work.** A new `proxy/src/ping.rs` prober modelled on
`client/src/latency.rs::SystemIcmpProber` (unprivileged DGRAM on Linux, plausible
reply rules, reply-source must equal target, no sub-1-ms readings), with a
per-session cache keyed by destination and the gates in §6. No new crate
(`libc` already a dependency per the design doc). Hook at the existing session
insert (`proxy/src/relay.rs:1550-1587`) or after N forwarded packets.

**Canary gates (one relay, e.g. `relay-fra` for volume or `relay-ewr-1`):**

- ≥ 200 probe bursts issued; `probes_issued/answered/rate_limited/cache_hit`
  counters recorded (relay-local, not per-destination in `/metrics`);
- measured answer rate compared with the phase C manual result for the same
  relay; **zero** abuse/rate-limit events attributable to probing;
- `/health` healthy and version correct after the deploy; control-plane
  registration still succeeds (the existing deploy gate); relay CPU/egress
  change within noise;
- client still registers and falls back to region prior for relays without the
  caps bit (verify with a mixed fleet: one upgraded, others not);
- sessions dropped only by the one restart (or zero with the existing in-place
  handoff, which `/health` reports as `handoff.supported=true`).

**Rollback.**

- **Binary:** the fleet already has the mechanism — `relay-install.sh` keeps
  `/opt/lightspeed/releases/<version>` and atomically repoints
  `/opt/lightspeed/current` with a health gate, and `deploy.sh revert_node`
  repoints the previous release and restarts (`infra/scripts/deploy.sh:318-347`;
  `infra/scripts/relay-install.sh:1-59,259-269,318-368`). The client is
  unaffected because it already treats the message as optional.
- **Unit/sysctl (only if the unprivileged path needed the host sysctl):** restore
  the recorded `net.ipv4.ping_group_range` value; no unit file changed in the
  preferred path. If a drop-in were ever used, remove it and
  `systemctl daemon-reload`; the process restart is the same one a deploy does.
- **Blast radius of the canary:** one relay; probes are ≤ 3 echoes per cached
  `(client, destination)` per 30 s against addresses the session already
  forwards to; the fleet's release path already tolerates a relay restart. No
  data plane behavior change when the probe is disabled or unsupported.

## 8. Cost impact: **$0.00**

- No resource is created, resized, or billed: the 8 relays are sponsor-funded
  community relays, the measurement runs on them, and v2 adds no server,
  endpoint, database, or CI service.
- Probe egress is tens of ICMP packets per match (3 echoes per
  `(client, relay, server)` per cache window; ~8 relays per server change), far
  below any included transfer allowance and far below the relays' existing game
  traffic. The manual run is ≤ 1,200 echo packets total.
- The runbook uses only tools already present or already deployed
  (`ping`, `journalctl`, `systemctl`, `ssh`), so there is no new paid service,
  no usage-based billing, and no trial. Logging is already on. No image/API
  calls. `[COST_STUB]` holds.
- Implementation (if GO) is a relay binary change plus an unprivileged ICMP
  socket; no new infrastructure. The only cost-like consideration is egress
  accounting (§6.3), which the existing `budget.rs` already meters.

## 9. What this document does not establish (named missing data)

| # | Missing data | Who can obtain it | Where it goes |
|---|---|---|---|
| M1 | privilege/egress per relay (`ping_group_range`, `root_ping_rc`, `dynamicuser_ping_rc`) | owner/operator over SSH (phase A) | `preflight-<node>.txt`, then `icmp-policy.out` update |
| M2 | journald retention + the real destination sample (n, window, games) | owner/operator (phase B) | `parse-summary.out`, `sample-anon.csv` |
| M3 | **F3 answer rate** (the decisive number) | owner/operator (phase C) | `ping-summary.out` |
| M4 | F4 baseline: v1 quiet-flow L2 | `rust-dev` task-13 (in progress) then a live run | v1 evidence + canary comparison |
| M5 | sec-ops review of §6, owner approval of the fleet step and any sysctl change | sec-ops / owner | review note + decision log |

The read-only parts above are reproducible today from this workstation; the
fleet parts are deliberately not attempted here (no SSH secret use by a
subagent, no relay host access, owner gate for the fleet step). A smaller n, a
journal without history, or an ICMP-blocking upstream is a **result**, not an
error: it converts the verdict to NO-GO with numbers.

## 10. Evidence index

`wat/evidence/2026-11-16/server-leg-probe/`

| File | What it is |
|---|---|
| `README.md` | index, commands, what is verified vs missing |
| `relay-scrape/` | 8 × raw `/health` + `/metrics` captured 2026-11-16T09:21:59Z, `relay-probe.out` summary |
| `icmp-coverage.out` | per-relay/per-game client-reported ICMP coverage (§2.4) |
| `code-paths.out` | `git grep` transcript for every code claim in §2.2, §2.3 |
| `icmp-policy.out` | privilege/egress/security-policy analysis (§2.3, §6) |
| `scripts/probe-relays.ps1` | the read-only fleet scrape that produced `relay-scrape/` |
| `scripts/relay-probe.sh` | Linux-side read-only collector (preflight / sessions / ping) — not executed on this host |
| `scripts/run-operator-probe.ps1` | Windows operator runner for phases A-C (`-DryRun` previews every ssh command) |
| `scripts/parse-sessions.ps1` | journald → destination sample + game join + sample gate |
| `scripts/parse-ping.ps1` | ping output → per-relay/per-game F3 table + verdict |
| `fixtures/`, `parser-test.out`, `ping-parser-test.out`, `operator-dryrun.out` | synthetic-fixture tests of the parsers and the runner's command construction (clearly synthetic; TEST-NET addresses) |
| `report.md` | task completion report (what changed, commands, limits) |

## 11. References

- `docs/server-leg-signal.md` §5.2 (D2 proposal), §6 v2 (recommended sequence),
  §7 F3/F4 (falsifiers), §9 (reproduce commands).
- `docs/design/relay-server-leg.md` (prior design: Step 0 blocking unknown,
  security assessment, open questions).
- `wat/state/decisions/2026-09.md:12` (the still-open relay ICMP-privilege
  decision).
- `infra/scripts/setup-new-node.sh` (unit + firewall), `infra/scripts/deploy.sh`
  and `infra/scripts/relay-install.sh` (canary/rollback), `infra/README.md`.
- External: Ubuntu iputils/ping_group_range bug
  <https://lists.ubuntu.com/archives/foundations-bugs/2024-November/528057.html>;
  Vultr firewall egress behaviour
  <https://docs.vultr.com/support/products/network/does-vultr-firewall-filter-outgoing-traffic-from-my-instance>.
