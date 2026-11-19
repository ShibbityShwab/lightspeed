# Relay selection and the second hop: SOTA survey and a $0 design

> **Status:** research memo + design recommendation. No product code changed.
> Date 2026-11-19. **Owner:** `lead`, from the owner's "spend time researching
> papers and SOTA … but still hit our $0 mark". **Evidence:** three independent
> literature surveys (`research-latency`, `research-relay`, `research-probing`,
> this session); citations inline. **Supersedes:** nothing; it extends
> `docs/server-leg-signal.md` (v1/v2) and `docs/design/relay-server-leg.md`.

## 0. Executive summary

1. **There is no published method that estimates RTT between two arbitrary hosts
   with zero participation at both ends.** Every accurate technique consumes one
   measurement that touches the target or an infrastructure proxy beside it. The
   "just geolocate it" route is a dead end; the choice is *which* measurement to
   take, at $0.
2. **The measurement we are missing is the relay→server leg (L2).** The client
   already measures client→relay (L1), direct client→server (L3d), and the whole
   path through the *current* relay (L3). L2 for *other* relays is the gap.
3. **The strongest $0 signal is a relay-vantage probe of the game server.** This
   is also what commercial systems do when they control both endpoints (WhatsApp's
   joint relay probing, Steam SDR's client+server ping locations); we cannot
   control the game server, so the relay probes it. The previously blocking
   privilege question is solved: unprivileged ICMP (`SOCK_DGRAM` +
   `IPPROTO_ICMP`) needs only `net.ipv4.ping_group_range` covering systemd
   `DynamicUser` GIDs — **no `CAP_NET_RAW` on the daemon**.
4. **Until that lands, do not rank relays on geoIP.** Use free server metadata
   (cloud provider IP-range feeds, reverse-DNS geolocation, operator geofeeds)
   with an explicit "unknown" state, plus the already-shipped triangle floor. A
   high measured RTT cannot refute a far country, so "honest unknown" — not a
   clever guard — is the correct interim label.
5. **Cost stays $0.00 for every recommended component:** the eight relays are
   sponsor-funded; all data sources are free; optional external vantages
   (Globalping free tier, RIPE Atlas credits earned by hosting a software probe)
   are calibration-only and terms-gated. No paid database, no paid measurement
   platform, no trial.

## 1. The problem, restated

`client ──L1── relay ──L2── game server`, plus `client ──L3d── game server`.

- L1 is measured for every relay (keepalive/TCP connect) — ranks the first leg.
- L3d is measured (ICMP burst) and gives a hard bound: `L2 ≥ L3d − L1` (triangle
  inequality), which is the existing `GeometricFloor`.
- L2 is measurable **only for the relay currently carrying the flow** (quiet-flow
  L3 − L1; 2 samples in 17 min on a chatty live game — `docs/server-leg-signal.md`
  F1 confirmed).
- GeoIP is a prior, not a measurement, and it is demonstrably wrong for hosting
  ranges (this session: a G-Core US-Virginia server resolves to JP/Tokyo in both
  DB-IP Lite and DB-IP City Lite; the operator geofeed says US-VA Manassas).

## 2. The core literature result

**No free lunch for arbitrary pairs.** The accurate families all need one side to
participate:

| Family | Representative work | What it needs |
|---|---|---|
| Overlay routing with active probing | RON (SOSP'01) | cooperating overlay nodes |
| Endpoint-pair probing | WhatsApp relay study (ANRW'26) | both endpoints probe relays |
| Server-side ping location | Steam Datagram Relay | game/server integration |
| Passive backbone telemetry | Cloudflare Argo, Akamai SureRoute | owned backbone (paid/closed) |
| Network coordinates | Vivaldi (SIGCOMM'04), PIC, GNP, Phoenix, Treeple | both endpoints participate |
| Constraint geolocation | Octant (NSDI'07), CBG, Spotter | landmarks ping the target |
| DNS-proxy latency | King (IMW'02) | target's DNS identity; ~75 % accuracy |
| Reverse-DNS geolocation | Dan et al. (TOIT'21) | PTR records (~34 % of IPv4) |
| Physical/TIV bounds | — | one measurement from a known point |

Network coordinates cannot embed a non-participating server; triangle inequality
alone has **no server-location signal** when the client leg is already shorter than
the direct ping (the case the just-shipped floor tie-break addresses). The
practical consequence: *the only path to accurate relay choice is to measure L2
from the relays*.

## 3. What the SOTA says about the architecture

- **One hop is enough.** RON's result (one intermediate relay suffices for ~98 %
  of optimal paths) matches a one-relay design; a relay mesh adds routing options,
  not a different architecture.
- **The second hop is the product.** RON, Detour and Lifeguard all report large
  fractions of pairs (30–90 %) with a significantly better alternate path; the
  commercial low-latency systems win exactly by measuring the *server* side
  (WhatsApp: joint endpoint probing; Steam SDR: server-supplied ping locations;
  Argo/SureRoute: owned backbone telemetry). A caller-only choice is measurably
  worse (WhatsApp study: up to ~24–144 ms worse for caller-driven selection).
- **Bounds beat guesses.** With anchors that have a known L2, candidate L2 can be
  bracketed: `L2_i ∈ [max_a |d(i,a) − d(a,s)|, min_a (d(i,a) + d(a,s))]`. Triangle
  inequality violations are real (12–24 % of triples in measurement studies), so
  bounds are pruning/hysteresis heuristics, not a ranking.
- **Passive decomposition is free labels.** `L2(active relay, server) = L3 − L1`
  is already computable per session; accumulated by (relay, server prefix/ASN,
  hosting provider, hour) it trains a prior (~14 % median relative error is the
  Vivaldi-class benchmark) without new packets.
- **Game-query protocols are out.** A2S/GameSpy/UT3/Minecraft query probes are
  game-protocol payloads; they conflict with the transparent-tunnel/no-payload
  policy, are amplification-sensitive, and can be logged/banned. They must not be
  used, even though they would answer RTT directly.

## 4. The $0 signal stack (recommended)

### S0 — Server metadata and honesty (client-only; no fleet gate; days)

Replace "DB-IP country or bust" with a source ladder; every source is free and
the product path runs **offline** (committed artifacts, no runtime third-party
service). Each source is verified against this session's real servers where noted.

1. **Cloud provider IP-range feeds** — AWS `ip-ranges.json` **and**
   `geo-ip-feed.csv` (RFC 8805, Local-Zone-aware); GCP `cloud.json`; Azure service
   tags. When a game server is hosted on one of these, the region is **exact**, not
   a guess. Snapshot the feeds at build time into a compact committed index
   (pinned, hashed) so builds stay hermetic and lookups are offline.
   *Verified:* `3.237.221.191` → AWS us-east-1; `34.38.12.1`/`34.77.14.49` → GCP
   europe-west1; `34.117.231.190` → GCP global. AWS uses the legacy `ip_prefix`
   key — a parser that only reads `ipv4Prefix` silently misses all of AWS.
2. **Hosting-provider prefix index (the honest fix for G-Core).** A curated list
   of game-hosting/CDN ASNs (G-Core, OVH, Hetzner, DigitalOcean, Vultr,
   Linode/Akamai, AWS, GCP, Azure, Oracle, Alibaba, Tencent, Contabo, Scaleway,
   Equinix, Leaseweb, …) resolved to their announced IPv4 prefixes at **build
   time** from the free, key-less RIPE Stat API (`announced-prefixes`), emitted as
   a compact committed index. A match means "hosting network: location unknown"
   (or the exact provider region when the cloud feed also matches) instead of
   asserting a country the data cannot support.
   *Verified:* RIPE Stat `prefix-overview` for `178.107.14.201` → AS199524
   (G-Core), and `announced-prefixes` for AS199524 includes `178.107.14.0/24`.
   **DB-IP ASN Lite alone is not enough:** the free 7 MB ASN table maps
   `178.107.17.0/24` to G-Core but has **no row for `178.107.14.0/24`** — the live
   server. The curated-ASN + RIPE Stat route is what closes that gap.
3. **Reverse-DNS geolocation** (Dan et al., TOIT'21) — median ~17.5 km on ~1.6 M
   IPs, ~34 % IPv4 coverage. Only useful where a PTR exists; the observed G-Core
   addresses have none. Deferred to R1b; live PTR queries are free, but the
   accuracy comes from a model trained on a large dataset, so this stays optional.
4. **Operator geofeeds** (RFC 8805) — authoritative where published; licences are
   the blocker (owner gate), and the hosting index above already removes the false
   country, so this is additive accuracy, not a prerequisite.
5. **DB-IP Lite** stays as the labelled country-level fallback (already shipped,
   CC BY 4.0 attribution in place).
6. **Honest state:** when the sources are absent or conflict, say "location
   unknown" / "hosting provider — location unknown" rather than a country; keep
   the measured direct ping and the triangle verdict as the only path claims.
7. **No new "prior guard":** a high RTT cannot refute a far country (the physical
   bound is an upper bound on distance), so a contradiction test would be
   unsound. The prior's *ordering* is what must stop steering once L2 exists.
8. **Runtime third-party DNS is excluded** (Team Cymru works and is free — it
   resolved the same IP to AS199524 — but a runtime per-match dependency with a
   privacy cost is not free-at-scale; use it only for validation).

### S1 — Relay-vantage L2 probe (the accuracy ceiling; owner-gated fleet step)

Per relay, per server the session already forwards to:

- **Primary: unprivileged ICMP echo.** `SOCK_DGRAM`/`IPPROTO_ICMP`; enable with
  `net.ipv4.ping_group_range = 61184 65519` (systemd `DynamicUser` UID/GID range)
  or the common `0 2147483647`. **No `CAP_NET_RAW` on the daemon** — the prior
  security assessment's only acceptable route. Verify on a canary: `sysctl` +
  ping from the service, compared with an independent `ping`.
- **Fallback 1: TCP handshake RTT** to a profile port (or 443) when ICMP is
  silent — `connect()` + `TCP_INFO`/`SO_TIMESTAMPING`, no privilege.
- **Fallback 2: TTL-limited traceroute lower bound** — the last responding hop's
  RTT is a lower bound on L2 (off by the last hop, ~0.1–2 ms), so "silent" becomes
  "bounded", not unknown.
- **Fallback 3: King-style DNS proxy** — time the relay→authoritative-NS path for
  the server's DNS identity; coarse, used only as a prior.
- **Passive**: `L2 = L3 − L1` for the active relay, already available; the
  request/response counters are in the relay's session already.
- **Reporting:** one `MSG_SERVER_LEG` (server, median, jitter, samples, age) over
  the existing control plane; unknown-type-tolerant; client-side validation:
  plausible range, `L + L2 ≥ direct` (floor), freshness, session binding.
- **Safety gates (carry over from the prior design):** only addresses already
  forwarded to for a live session (N ≥ 20), per-server TTL cache, global
  rate/concurrency cap, private/link-local refusal, `abuse.rs`/`budget.rs` checks,
  per-relay off switch, canary + rollback.

### S2 — Relay mesh and bounds (free, cheap)

Measure all 28 relay↔relay pairs (30–60 s cadence; ~Kbps). Use them to
(a) detect a sick/loaded relay, (b) bracket L2 for candidates that have an
anchored relay, (c) order switches with hysteresis (`LB(i) + margin < current`).
Never switch on a bound alone.

### S3 — Learned prior from passive labels (free)

Accumulate `(relay, server prefix/ASN, hosting provider, hour) → L2` from S1 and
from the active-path decomposition. Train a small additive/GBM model (offline, no
service cost) with free features (provider range, ASN, PeeringDB, coordinates as
a last resort). Use only to break ties / cold-start; always subject to the floor.
This is the Vivaldi-class fallback, not the primary.

### S4 — External free vantages: validation only, **excluded from the product path**

- **Globalping** (free: 250 tests/h unregistered, 500/h registered): useful for
  measuring **our own relays** from many vantage points and spot-checking
  traceroute bounds during validation. **Not** in the product: free tiers are
  rate-limited, volunteer probes probing third-party game servers is distributed
  probing of a third party, and terms are unverified.
- **RIPE Atlas** software probe: free credits (15/connected minute ≈ 21,600/day)
  for independent cross-checks at canary time; commercial-use terms of Atlas data
  need review. Not a product dependency.
- **Free-tier cloud VMs** are already the relays; no new spend.

## 5. Ranking policy

1. Prefer a **measured L2** (S1) for every candidate.
2. Otherwise use **bounds** (triangle/TIV + mesh anchors) to prune; switch only
   when the bound proves a margin, with hysteresis.
3. Otherwise keep the **current** relay (no hopping) and say "server leg not
   measured"; show the direct ping and the triangle verdict.
4. Geo/coordinate priors only as a last-resort ordering, never as a claim, and
   never when a measured signal contradicts their ordering.

## 6. Validation plan (all $0, bounded)

| # | Experiment | Gate |
|---|---|---|
| V1 | Probe coverage: 30–50 public game servers × 8 relays, ICMP + TCP-SYN + traceroute | ≥ ~50 % direct coverage → S1 primary; else S3-first |
| V2 | Compare relay→server RTT against traceroute bound and independent vantages | divergence ≤ 20 % (falsifier F4) |
| V3 | Geo-source scoring against known cloud regions (AWS/GCP/Azure) + the G-Core case | provider feed exact; rDNS coverage/accuracy measured |
| V4 | Relay mesh matrix; bound width on servers with ground truth | kill if p50 bound width > 15 ms |
| V5 | Policy replay: direct vs client-leg vs prior vs S1 oracle | report median/p90 and regret; direct-win rate |
| V6 | Passive label stability over 1–24 h; probe bytes/hour; cache TTL | stability + overhead budget |

## 7. Free-at-scale analysis (the filter)

The owner's rule: **no payment, and nothing that only works because today's
volume is small.** Every component is classified by its cost driver and what
happens when that driver binds.

| Component | Runtime cost driver | Free at scale? | When it binds |
|---|---|---|---|
| Cloud-region index (AWS/GCP/Azure) | committed data; O(log n) lookup | **yes** | — regenerated monthly at build time |
| Hosting-provider prefix index | committed data | **yes** | — curated list grows over time |
| DB-IP country / ASN tables | committed data | **yes** | — |
| Relay-vantage probe (S1) | packets per **distinct server per relay**, not per user | **yes at the margin** | per-server cache + global caps hold the bound; unmeasured candidates fall back to bounds/prior; no hopping, no cost |
| Relay mesh (S2) | 28 pairs × small probes | **yes** | lower cadence under load |
| Passive labels / prior (S3) | offline training, local inference | **yes** | no retrain needed |
| Globalping / RIPE Atlas | free tiers, rate limits, volunteer probes, terms | **no** | **excluded from the product path** — validation only |
| Team Cymru DNS at runtime | third-party DNS per match; privacy exposure | **no** | build-time/validation only; replaced by the committed index |
| Any paid geo DB / measurement API | money | **no** | never |
| Game-query payload probes | policy (no-payload) | **no** | never |

**S1 scale math.** Probes are `O(distinct servers per relay)`, independent of how
many players share a server — one relay can serve a thousand players on ten
servers with ten probes every cache window. The per-server TTL cache and the
global rate/concurrency caps are load-bearing; when a relay hits its cap, new
servers stay unmeasured and the ranking degrades to bounds/prior **without**
hopping. The relay fleet is the product's capacity ceiling and is already
sponsor/free-tier funded; the measurement introduces **no new cost centre and no
new central service**.

**Free-data path verification (2026-11-19, this host, $0):** AWS feeds 2.76 MB +
312 KB, GCP 118 KB, all public and key-less; the AWS `ip_prefix` gotcha above;
G-Core AS199524 confirmed via free DNS and RIPE Stat; DB-IP ASN Lite's
`178.107.14.0/24` gap proven; RIPE Stat `announced-prefixes` covers it. No source
used here charges, requires a key, or needs a runtime call.

## 8. $0 ledger

| Component | Cost |
|---|---|
| 8 sponsor relays | $0 owner spend (existing) |
| Unprivileged ICMP / TCP / traceroute | $0 (no paid API) |
| AWS/GCP/Azure range feeds, rDNS data | $0 (public) |
| DB-IP Lite (fallback) | $0 (CC BY 4.0) |
| Globalping / RIPE Atlas | $0 tiers/credits, calibration only |
| Model training (offline) | $0 (local CPU) |
| **Total** | **$0.00** |

## 9. What not to do

- Game-query payload probes (A2S etc.): violates the no-payload policy.
- `CAP_NET_RAW` on the relay daemon: spoofing capability outweighs the benefit.
- Ranking on geoIP/coordinates as if measured; probing third-party servers from
  volunteer platforms; any paid geo/measurement service; anycast (no allocation).

## 10. Roadmap and owner gates

1. **R1 (now, no gates):** S0a cloud-region index + S0b hosting-provider prefix
   index + honest labels. Three committed artifacts, offline at runtime:
   cloud provider/region → exact region for AWS/GCP/Azure-hosted servers;
   curated hosting-ASN prefixes from the free RIPE Stat API at regeneration time
   → "hosting provider, location unknown" for providers like G-Core, which fixes
   the observed Japan label **without** the geofeed licence; DB-IP stays the
   labelled country fallback. Client + data snapshots + tests + locales.
2. **R2 (owner gate: fleet canary):** S1 probe — `ping_group_range` sysctl, ICMP
   prober, TCP/traceroute fallbacks, protocol message, client validation, canary
   with F3/F4 counters. Owner approval required for the host sysctl/fleet step.
3. **R3:** mesh bounds + passive-label prior; ranking policy hardening.
4. **R4:** geofeed licence decision (G-Core or a general licensed-feed policy)
   for the last data gap; otherwise honest "unknown" stands. Data-licence review
   also covers the RIPE-derived index (no cost, but attribution/terms).

## 11. Open questions / unverified

- ICMP answer rate from the relays to real game servers (V1; the decisive gate).
- Exact effectiveness of `ping_group_range` under the deployed unit (canary).
- G-Core geofeed licence (owner decision).
- rDNS coverage on game-server ranges; provider-feed coverage across the 28 games.
- RIPE Atlas data commercial-use terms (terms-gated).
- Many paper numbers are from PlanetLab-era testbeds; treat as indicative.
