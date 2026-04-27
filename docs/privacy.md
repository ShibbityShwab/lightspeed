# LightSpeed Privacy Policy (Telemetry)

_Last updated: 2026-04-27_

## Summary

LightSpeed collects **no data by default**.

The optional `--telemetry` flag enables anonymous, aggregated network-quality
reporting.  This feature is **opt-in only** — it is never enabled without your
explicit choice.

---

## What is collected (opt-in only)

When you pass `--telemetry`, the client periodically (every 15 minutes) sends
a small JSON report to the LightSpeed proxy you are already connected to.  No
third-party servers are ever contacted.

| Field | Description | Example |
|---|---|---|
| `game_id` | Numeric game identifier (0 = unknown/keepalive) | `1` |
| `client_country` | Locale string derived from OS regional settings | `"en-US"` |
| `p50_ms` | Median round-trip time (ms) over the flush window | `42.3` |
| `p95_ms` | 95th-percentile RTT (ms) | `87.1` |
| `p99_ms` | 99th-percentile RTT (ms) | `124.5` |
| `jitter_ms` | Mean absolute deviation of RTT samples (ms) | `8.2` |
| `sample_count` | Number of RTT samples in this window (max 1024) | `180` |
| `fec_recoveries` | Packets recovered by forward error correction | `3` |
| `fec_losses` | Unrecoverable packet losses detected | `0` |
| `client_version` | Application version string | `"0.4.0"` |

All latency values are **aggregates** — the raw per-packet timestamps are
discarded immediately after percentile computation and never leave your device.

---

## What is NOT collected

The following information is **never** included in any telemetry report:

- IP address (source or destination)
- User ID, account ID, or any persistent identifier
- Session token or authentication credential
- Raw packet contents or game payload data
- Player name, gamertag, or any in-game identifier
- Hardware fingerprint, MAC address, or device serial number
- Location beyond coarse OS locale string (e.g. `"en-US"`, `"ja-JP"`)
- Timestamps with sub-minute resolution
- Any data about other players or game servers

A compile-time regression test (`test_no_pii_fields_in_json` in
`protocol/src/telemetry.rs`) guards against accidental addition of PII fields
to the serialised report.

---

## Where data goes

Telemetry reports are sent via **HTTP POST to port 8080 on the proxy node you
are already tunnelling through** (e.g. `149.28.84.139:8080/telemetry`).

- The proxy node stores **only an in-memory counter** (`telemetry_reports_total`)
  that is exposed in its Prometheus metrics endpoint.
- Individual report contents are **not persisted** to disk or any database.
- Data is **lost on proxy restart** — this is intentional.
- The proxy is operated on Vultr Always Free / $5 VPS tier.  No data is shared
  with Vultr or any other third party.

---

## How to enable or disable

```
# Enable telemetry
lightspeed --telemetry

# Explicitly disable (also the default when neither flag is passed)
lightspeed --no-telemetry
```

`--no-telemetry` always wins if both flags are passed simultaneously.

When `--telemetry` is first used, a disclosure banner is printed to stdout
before any data collection begins.

---

## Open source

The full telemetry implementation is auditable in the repository:

- `protocol/src/telemetry.rs` — data structure + PII regression test
- `client/src/telemetry.rs` — collection, aggregation, HTTP flush
- `proxy/src/health.rs` — server-side receiver
- `proxy/src/metrics.rs` — in-memory counter only

This privacy document is tracked in version control alongside the code and
updated with every change to the telemetry data schema.

---

## Contact

Open an issue at <https://github.com/ShibbityShwab/lightspeed/issues> for any
privacy questions or concerns.
