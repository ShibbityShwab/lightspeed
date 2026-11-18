# Control-handshake trend (M1) in the published history

> **Status: instrumented 2026-11-18.** The CI collector and the anomaly monitor
> now record and evaluate the relay control-plane handshake split, so the
> pre-1.11.0 (TLS alert 120) rejection population can be trended per relay from
> the `stats` branch instead of inferred from point samples. This is an
> observability change only: no relay config, deploy, restart or client change
> is involved, and owner spend stays $0.00.

## What M1 is

The relay's QUIC control plane exports four monotone counters, one per handshake
outcome (`proxy/src/metrics.rs`), all four series present from process start
even at zero:

```
lightspeed_control_handshakes_total{region="...",node_id="...",outcome="negotiated_v1"}
lightspeed_control_handshakes_total{...,outcome="no_alpn"}
lightspeed_control_handshakes_total{...,outcome="rejected_no_application_protocol"}
lightspeed_control_handshakes_total{...,outcome="rejected_other"}
```

- **accepted handshakes** = `negotiated_v1` + `no_alpn`. Under
  `control_alpn = "required"` (the fleet's current mode) a completed handshake
  must have offered `lightspeed-control/1`, so `no_alpn` is expected to be 0 and
  `accepted ≈ negotiated_v1`.
- **M1** = `rejected_no_application_protocol` — the peer was refused with TLS
  alert 120. Under `required` this is the pre-1.11.0 client signal (it also
  counts scanners/middleboxes that do not offer the identifier).
- `rejected_other` is every non-alert-120 rejection (timeout, transport error,
  ...) and completes the partition: the four outcomes sum to all control
  handshakes the process observed.

The counters carry **no client identity**. M1 is an upper bound on the stranded
client share, not a measurement of it: attributing rejects to real users needs
the relay's own rejection log lines (`reason="no_application_protocol"`,
`remote=<ip:port>`), which are operator-only and not part of the public
snapshot.

## Snapshot schema

`infra/scripts/collect-metrics.sh` scrapes `/metrics` per relay and writes the
bounded (360-snapshot) history document that `pages.yml` publishes on the
`stats` branch as `web/network-history.json`. Since 2026-11-18 each
`per_relay.<id>` row carries four new reset-safe counters, alongside the
existing data-plane counters:

| Path | Meaning |
|---|---|
| `cumulative.control_handshakes_rejected_no_application_protocol` | raw current `/metrics` value (M1) |
| `delta.control_handshakes_rejected_no_application_protocol` | reset-safe window delta (a rolled-back counter books its new lifetime and flags `reset`) |
| `lifetime.control_handshakes_rejected_no_application_protocol` | accumulator that survives relay restarts |
| same three views for `control_handshakes_negotiated_v1`, `..._no_alpn`, `..._rejected_other` | the rest of the split |
| `interval.*` / `totals.*` | fleet-level sum of the per-relay deltas / lifetimes, same shape as every other counter |

`auth_rejections_total` (session-auth failures) and `sessions_created_total`
were already recorded as
`drops_auth_rejected` / `sessions_created`; the M1 fields are additive, so a
consumer that only reads the old keys keeps working.

**Compatibility rules** (also pinned by the hermetic suites):

- A snapshot written before the field existed has **no** `control_handshakes_*`
  keys. Treat an absent field as unknown, not as a measured zero (the
  `m1_reject_drain` detector skips such snapshots).
- A relay running a pre-1.11.0 build exports no handshake family, but this is
  not an absent field: the collector records an explicit 0 for it, so the field
  is present with a zero split in every snapshot the current collector writes.
  A zero split is not an anomaly; it has nothing to trend until that relay's
  first 1.11.0 scrape. The first snapshot that sees a counter the previous
  snapshot did not carry books that process's current lifetime as the interval
  (the same first-seen-counter rule a new relay gets), so exactly one snapshot
  shows a seeding jump, not a real one-window spike.
- A relay restart rolls the counters back; the collector books the new
  lifetime and sets `reset` / `reset_metrics` exactly like the data-plane
  counters, and the anomaly detector skips windows that contain a restart.

## Reading the trend

The published document is a plain JSON file; `t` is epoch seconds per snapshot.
Latest per-relay M1 and its last window delta:

```sh
curl -fsSL https://raw.githubusercontent.com/ShibbityShwab/lightspeed/stats/web/network-history.json \
  | jq -r '.snapshots[-1].per_relay | to_entries[]
           | [.key,
              .value.lifetime.control_handshakes_rejected_no_application_protocol,
              .value.delta.control_handshakes_rejected_no_application_protocol,
              .value.lifetime.control_handshakes_negotiated_v1] | @tsv'
```

Accepted share and rejects-per-accepted-handshake for the newest snapshot:

```sh
jq -r '.snapshots[-1].per_relay | to_entries[]
       | .value.lifetime as $l
       | ($l.control_handshakes_negotiated_v1 + $l.control_handshakes_no_alpn) as $acc
       | [$key, .value.lifetime.control_handshakes_rejected_no_application_protocol, $acc] | @tsv'
```

A relay's M1 history (one row per snapshot, oldest first):

```sh
jq -r '.snapshots[] | [.t, .per_relay["relay-syd-1"].lifetime.control_handshakes_rejected_no_application_protocol]
       | @tsv' web/network-history.json
```

Rates need the snapshot gaps (`t` differences), not the row count: GitHub
throttles the Pages cron, so consecutive snapshots are typically ~3.5–6 h apart.

## Anomaly detector: `m1_reject_drain`

`infra/scripts/health-anomaly.sh` gained one detector. Because the fleet is
deliberately `required` (owner decision recorded in
[fleet-alpn-options-2026-11-18](fleet-alpn-options-2026-11-18.md)), the steady
state is a large rejection stream; alerting on it every run would be the
"monitor stops being read" failure the script's own comments warn about. So the
detector is a **rate-change** alarm, bounded like the existing
traffic-distribution detectors:

- fires when the window's alert-120 reject rate is at least **2x** the relay's
  own pre-window baseline rate, **and**
- the rate is at least **100 rejects/hour** (above the fleet's quietest real
  relay at the 2026-11-18 sample), **and**
- accepted handshakes stay flat: at most **10/hour** over the window, **and**
- the window is sustained (`--window`, default 3 snapshots, never 1), the relay
  is reachable for the whole window, and neither the window nor the baseline
  contains a relay restart, **and**
- the collector's handshake fields are present at the baseline start, the
  window start and the window end. A snapshot older than the schema (no field
  at all) is skipped, never read as zero. A pre-1.11.0 relay is different: its
  collector row carries an explicit 0 split, which is present and simply cannot
  accelerate, so it stays silent.

All three M1 rates are **true per-hour rates**: the window delta and its
denominator span the same snapshot interval, the last pre-window snapshot
(`wstart`) to the newest snapshot in the window — W intervals at `--window W`,
floored at one hour. (The shared `window_hours` variable the older count-based
detectors use spans only the in-window snapshots, W−1 intervals; the M1 rates
do not use it.) So the 100/h floor and the 2× factor are the effective
thresholds at any window size, and the emitted `rejects_per_hour` /
`accepted_per_hour` are true hourly rates.

It is severity `warning`, type `m1_reject_drain`, and it is in
`NON_FATAL_TYPES`: it is printed and posted to Discord but does not fail the
scheduled run (a policy outcome, not a relay fault). `LIGHTSPEED_ANOMALY_STRICT=1`
restores fail-on-any for operators who want the monitor red on it.

One caveat on the first runs after the collector upgrade: the first snapshot
that observes the new counters books each counter's current lifetime as its
interval (the standard first-seen-counter seed), so while that snapshot is still
inside the detector's baseline the baseline rate is inflated. That only
suppresses a would-be alert for a few runs; it cannot fabricate one, and the
flat-accepted gate independently rejects the seeding snapshot.

The detector file also carries the contract guards: the collector must read the
`lightspeed_control_handshakes_total` family with the four outcome labels the
proxy exports, lifetime-accumulate the alert-120 field, and the detector must
read the field the collector writes; a metric-name drift fails the self-test.

## Verification is CI-only on a Windows workstation

The collector and the anomaly monitor are POSIX (`bash` + `curl` + `jq`), and
this project's development host is Windows without bash or jq: **these scripts
are not executed on the workstation and no local run is claimed.** Verification
paths:

- `Infra Script Tests` (`.github/workflows/ci.yml`) runs
  `infra/scripts/test_collect_metrics.sh` and
  `infra/scripts/test_health_anomaly.sh` on ubuntu-latest. The suites cover the
  new fields, reset handling, the old-schema snapshot, and every detector guard
  above.
- `Health Anomaly Monitor` (`.github/workflows/health-anomaly.yml`) runs
  `test_health_anomaly.sh` before the live detector on every scheduled and
  dispatched run, so the new detector and its guards execute against the real
  history on the next run after the change is pushed.

## Files

| Path | Role |
|---|---|
| `infra/scripts/collect-metrics.sh` | parses the four series into the snapshot schema |
| `infra/scripts/health-anomaly.sh` | `m1_reject_drain` detector + thresholds |
| `infra/scripts/test_collect_metrics.sh` | collector fields/reset/old-snapshot/producer contract |
| `infra/scripts/test_health_anomaly.sh` | detector fire/silence guards + old-snapshot skip + contract |
| `docs/data-dictionary.md` | consumer-side schema statement |
