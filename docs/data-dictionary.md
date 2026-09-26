# Data Dictionary

> Last updated: 2026-09-25

This document lists every field LightSpeed's telemetry wire report can carry, its
type, granularity, purpose, aggregation key, suppression rule, retention, and
transport. It also lists what is explicitly **not** collected. It is the
authoritative companion to the [Privacy Policy](privacy.md).

Source of truth: `protocol/src/telemetry.rs` (`TelemetryReport`,
`PathObservation`) and `proxy/src/metrics.rs` (`TelemetryAggregator`,
`RouteTelemetryAggregator`).

---

## Transport

- Reports are POSTed by the client over **plaintext HTTP** to
  `<relay>:8080/telemetry` (`client/src/telemetry.rs`).
- The `/telemetry` endpoint is **unauthenticated** (`proxy/src/health.rs`). The
  relay validates the JSON body shape but does not verify the sender.
- The body is not encrypted in transit. Moving telemetry onto the QUIC control
  plane is a known planned change.

---

## Report fields

Every field below is part of a single `TelemetryReport`. All latency values are
rounded to one decimal place in milliseconds.

| Field | Type | Granularity | Purpose | Aggregation key | Suppression | Retention | Transport |
|-------|------|-------------|---------|-----------------|-------------|-----------|-----------|
| `game_id` | `u8` | Per game profile | Bucket metrics per game type | Part of `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `client_country` | `String` (2-char ISO 3166-1 alpha-2, or `""`) | Per client, from OS locale | Coarse region context; **never derived from IP** | Part of `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `p50_ms` | `f32` | Per client report | Median RTT to the relay | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `p95_ms` | `f32` | Per client report | 95th-percentile RTT | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `p99_ms` | `f32` | Per client report | 99th-percentile RTT | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `jitter_ms` | `f32` | Per client report | Mean of absolute consecutive RTT deltas (a jitter proxy, **not** a standard deviation) | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `sample_count` | `u32` | Per client report | Number of RTT samples the report is based on | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `fec_recoveries` | `u32` | Per client report | Packets recovered by FEC during the session segment | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `fec_losses` | `u32` | Per client report | FEC blocks where recovery was not possible | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `direct_p50_ms` | `Option<f32>` | Per client report | Median direct (un-relayed) RTT to the game server, from an ICMP probe. Omitted when absent | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `relayed_p50_ms` | `Option<f32>` | Per client report | Median tunnelled round trip through the relay. Omitted when absent | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `direct_app_p50_ms` | `Option<f32>` | Per client report | Median direct application RTT, measured by timing a sample of the game's own packets on the un-relayed path. Omitted when absent | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `saved_app_pairs` | `u32` (defaults to 1; max 100,000) | Per client report | Count of paired direct-application and relayed samples behind `direct_app_p50_ms` / `relayed_p50_ms`. Aggregate and non-identifying, like `sample_count`. Weights the saved-app sample counter, signed sum, and negative counter so a report covering several pairs counts as several samples. Legacy reports default to 1 | `(game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `client_version` | `String` (SemVer, max 32 chars) | Per client | Compatibility tracking only. **Not aggregated or used by the relay; being coarsened** | None (not aggregated) | n/a | Not retained as an aggregate | Plaintext HTTP POST |
| `route_legs` | `Vec<PathObservation>` (max 8) | Per relay leg | Opt-in multipath quality reporting. Empty vector is valid | `(normalized relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |

### `route_legs` sub-fields (`PathObservation`)

| Field | Type | Granularity | Purpose | Aggregation key | Suppression | Retention | Transport |
|-------|------|-------------|---------|-----------------|-------------|-----------|-----------|
| `relay` | `String` (1-64 chars, registry id like `relay-fra`) | Per relay leg | Stable registry node identifier; never a raw address | Part of `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `rtt_p50_ms` | `f32` | Per relay leg | Median RTT on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `rtt_p95_ms` | `f32` | Per relay leg | 95th-percentile RTT on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `rtt_p99_ms` | `f32` | Per relay leg | 99th-percentile RTT on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `jitter_ms` | `f32` | Per relay leg | Mean of absolute consecutive RTT deltas on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `samples` | `u32` | Per relay leg | Number of RTT samples this leg is based on | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `lost` | `u32` | Per relay leg | Packets observed lost on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `recovered` | `u32` | Per relay leg | Packets recovered by FEC on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |
| `dedup_saved` | `u32` | Per relay leg | Duplicate packets suppressed by the dedup window on this leg | `(relay, game_id, country)` | Cell withheld until >= 3 distinct source IPs | Relay memory; exported to `/metrics` | Plaintext HTTP POST |

---

## Suppression rule

Two floors apply, depending on whether the aggregator has a source address to
key on.

**Client telemetry cells** (the flat `(game_id, country)` report cell, and the
per-`(relay, game_id, country)` route-leg cell) floor on **distinct source IPs**:
`MIN_TELEMETRY_CELL_SOURCE_IPS = 3`, enforced in `proxy/src/metrics.rs:1358`
(flat cell) and `proxy/src/metrics.rs:1516` (route-leg cell). A cell is withheld
from `/metrics` until it has been seen from at least 3 distinct source addresses.
This is the real k-anonymity floor for client-submitted reports: a single client
can inflate a report count by flushing repeatedly, but it cannot manufacture
distinct source addresses. The relay keeps a bounded set of at most 3 addresses
per cell (`MAX_TELEMETRY_CELL_SOURCE_IPS`), uses it only to make the export
decision, and never exports or logs the addresses. The set stops growing exactly
at the floor, which bounds memory even when a client rotates addresses.

**The session-geo aggregator**, which has no source address to key on, floors on
**observations**: `MIN_TELEMETRY_CELL_REPORTS = 3`, enforced in
`proxy/src/metrics.rs:1621`. A cell is withheld from `/metrics` until it holds at
least 3 observations. This floor counts observations, not distinct people: it
limits how small a cell can be before export, but it does not prove that a cell
is backed by a population of 3.

Both floors match the published k = 3 promise. The `source_ips` set is not a wire
field and is never serialized; it exists only inside the relay's in-memory cell.

---

## Published aggregates derived from the report

The metrics collector (`infra/scripts/collect-metrics.sh`) folds the relays'
Prometheus output into the bounded `web/network-history.json` history. For the
saved-app families it keeps two views of the same already-exported data:

- the collapsed per-relay totals, as before; and
- `per_relay.<id>.sources.<region>`, the same signed saving summed by the
  client's coarse world region. The two-letter `client_country` label is mapped
  through `infra/geo/regions.json` before anything is retained, so a country is
  never published, only a region. A country with no catalog entry is counted in
  `sources_unmapped`, never invented into a region.

This adds no collection and no wire field: it only re-aggregates series the
relay already emitted, so a cell below the k floor is invisible to the collector
and cannot be recovered. The region keys come from the trusted local catalog, so
the retained map is bounded by the catalog's region count. The recommender
(`infra/scripts/recommend-regions.sh`) reports the same breakdown per relay and
source region but withholds any cell below the k=3 report floor, counts the
withheld cells, and labels each reported row with its sample count.

Note on scale: the recommender's per-source `saved_app_samples` counts are
summed from the relay's `saved_app_ms_count`, which is weighted by
`saved_app_pairs`. A single report can therefore contribute more than one sample
to that count. This does **not** change the k-anonymity floor: the relay still
withholds a cell until it has seen 3 distinct source IPs, and `saved_app_pairs`
cannot manufacture a distinct source. What the pair weighting does change is the
scale of the recommender's advisory `min_measured_samples` gate, which is
calibrated to 20 pair-weighted samples so that a lone inflated report (a client
reporting `saved_app_pairs` up to 8) stays below the floor and cannot suppress a
legitimate ADD on its own. The gate is a quality floor on the pair-weighted
sample count, not a k-anonymity guarantee; the k floor is the distinct-source
rule above. See the recommender's `source_quality` block and
`min_measured_samples`.

---

## Explicitly NOT collected

The wire report contains none of the following. The protocol test
`test_no_pii_fields_in_json` asserts that `ip`, `user`, `session`, and `host`
never appear in the serialized JSON.

- **Game-server IP** (or port)
- **Session or user ID**
- **Packet payloads** (game data is encrypted by the game's own protocol)
- **Exact coordinates**
- **Raw timestamps** (only aggregated latency statistics are sent)
- **IP address** (the relay sees the client source IP at transport, but it is
  not part of the report)
- **Hostname, username, or device fingerprint**
- **User / account identity**
- **Game account data or player names**

Note: relay access logs are separate from the telemetry report and can contain
client IPs. See [Proxy Logs](privacy.md#proxy-logs).

---

## What opting in also triggers

While telemetry is enabled, the client ICMP-probes the game server and
re-injects a small sample of the game's own packets on the direct path (the
"shadow direct" sampler). Both stop when telemetry is off. See
`client/src/latency.rs`.
