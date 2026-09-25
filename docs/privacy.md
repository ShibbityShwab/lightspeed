# Privacy Policy

> Last updated: 2026-09-25

---

## Summary

LightSpeed is **privacy-first by design**. The tunnel is unencrypted to remain compatible with anti-cheat systems: game servers always see your real IP address. Telemetry is **on by default** and can be turned off at any time; it is limited to anonymized aggregate metrics.

LightSpeed does not collect accounts, identities, game account data, or packet payloads. It does process IP addresses transiently for routing and country derivation, and relay logs can contain client IPs (see [Proxy Logs](#proxy-logs)). The precise split between what is processed, what is aggregated, and what is never collected is documented in the [Data Dictionary](data-dictionary.md).

---

## What LightSpeed Does NOT Collect

- ❌ Stored IP addresses (yours or game servers'). IPs are processed transiently for routing and country derivation, then discarded. Relay logs can contain client IPs; see [Proxy Logs](#proxy-logs). See also [What LightSpeed Sees (and Doesn't Store)](#what-lightspeed-sees-and-doesnt-store).
- ❌ User identities or account information
- ❌ Game account data or player names
- ❌ Packet payloads (game data is encrypted by the game's own protocol)
- ❌ Browsing history or other non-game traffic

---

## What LightSpeed Sees (and Doesn't Store)

During operation, the client and proxy process UDP packet headers:
- Source/destination IP addresses (for routing, and for transient country derivation)
- Source/destination ports (for game detection)
- Packet sizes (for bandwidth accounting)
- Session tokens (for authentication)

This information is held in memory only and is never written to disk beyond debug logs (which you control via the `RUST_LOG` environment variable). Debug logging is off by default. Relay access logs are the exception: they can contain client IPs, as described in [Proxy Logs](#proxy-logs).

---

## Network placement analysis

To decide where relays are most useful, the proxy derives the **country** of an IP address from a locally stored DB-IP Lite database. This happens in memory, transiently, at session creation, for both the client source address and the game-server destination address. The country lookup is the only thing derived from the address; the address itself is not retained.

What is kept is a counter of sessions per `(source_country, destination_country)` pair. These counters are never per-user and never include raw IP addresses. A cell is suppressed until it has at least 3 sessions before it is exported: any `(source_country, destination_country)` cell with fewer than 3 sessions is suppressed and never leaves the proxy.

The counters are exposed on the relay's `/metrics` endpoint as country-code labels, and persisted into the public stats history as **coarsened region-pair counts only** (for example `mena-eu`). The counters contain no raw IP, and no raw IP is exported by the placement pipeline (operational logs are covered in Proxy Logs below). There is no change to what the client sends.

IP geolocation data by DB-IP (https://db-ip.com), licensed under CC BY 4.0.

---

## Anonymous Telemetry (on by default)

Telemetry is **on by default**. It sends anonymous aggregate metrics to the
`/telemetry` endpoint of the relay you are connected to (a community or sponsor
relay, or your own if you self-host). There is no central LightSpeed telemetry
server. A cell is suppressed until it has at least **3 reports**
(`MIN_TELEMETRY_CELL_REPORTS = 3`). This floor counts **reports**, not distinct
people: a single client flushing every 15 minutes can reach it alone, so it is
not a guarantee that 3 different people contributed. It limits how small a cell
can be before it is exported; it does not prove that a cell is backed by a
population of 3.

### Transport and endpoint

- Reports are POSTed over **plaintext HTTP** to `<relay>:8080/telemetry`. The
  body is not encrypted in transit. This is a known limitation and the plan is
  to move telemetry onto the QUIC control plane.
- The `/telemetry` endpoint is **unauthenticated**: the relay accepts a report
  from any source that can reach port 8080. It validates the JSON body shape,
  but it does not verify who sent it.

### What opting in also triggers

Turning telemetry on does more than send a report. While it is enabled, the
client:

- **ICMP-probes the game server** to measure the direct (un-relayed) path.
- **Re-injects a small sample of the game's own packets on the direct path**
  (the "shadow direct" sampler). The packet is the game's own packet, sent
  unchanged, so the client can compare like-for-like direct and relayed RTT.
  No synthetic packet is generated.

Both stop when telemetry is off.

The client measures both the direct RTT to the game server and the tunnelled
round trip, and the relay aggregates the difference. That difference is the
**"RTT saved by LightSpeed"** figure shown on the website. It compares two
different instruments (a direct probe against the game traffic's own round
trip), so it is an estimate, not a measurement of in-game ping. It is a
per-cell aggregate, never a per-client value.

| Metric | Example | PII? |
|--------|---------|------|
| RTT percentiles | p50: 31ms, p95: 45ms, p99: 52ms | No |
| Jitter | 2.3ms (mean of absolute consecutive RTT deltas) | No |
| FEC recovery rate | 12 packets recovered / 1000 | No |
| Direct vs relayed RTT | direct p50: 61ms, relayed p50: 31ms | No |
| RTT saved | 30ms (direct p50 minus relayed p50) | No |
| Game name | "rust" | No |
| LightSpeed version | "1.6.5" | No |

`client_version` is collected but is not aggregated or used by the relay; it is
being coarsened. The full field list, types, and aggregation keys are in the
[Data Dictionary](data-dictionary.md).

**Explicitly NOT collected:** IP addresses, user identities, game account data, packet payloads, game-server IP, session or user IDs, exact coordinates, and raw timestamps.

### How to Turn It Off

```bash
# Disable for this session (overrides the config and the default)
lightspeed --no-telemetry --start-interceptor --game rust --proxy YOUR_PROXY:4434

# Disable permanently in lightspeed.toml
# [general]
# telemetry = false
```

`--telemetry` forces telemetry on for one session; `--no-telemetry` always wins.
The GUI exposes the same switch as the **"Share anonymous latency stats"**
checkbox.

---

## Proxy Logs

The relay that serves your session writes access logs to stdout (configurable via `RUST_LOG`). These are operational logs, not analytics. They can contain:
- Client IP addresses (for rate limiting and abuse detection)
- Session start/end times
- Packets/bytes relayed per session

Client IPs appear in these logs only because the proxy needs them for rate limiting and abuse detection. They are not exported, not used for country derivation, and not joined to the placement counters described above. These logs are stored on the relay that served the session. If you use a community or sponsor relay, that operator holds the logs; if you self-host, they are on your server. LightSpeed does not have access to logs on relays it does not operate. Configure log rotation and retention according to your own policies; retention is limited by the logging configuration of the relay that wrote them.

---

## Data Retention

- **Client:** No data is persisted beyond the current session (in-memory only).
- **Relay:** Logs are written to stdout. Retention is controlled by the relay's logging configuration; if you use a community or sponsor relay, that operator controls it. Placement counters are aggregate only, with a floor of 3 sessions per cell, and carry no raw IPs.
- **Telemetry:** Data is sent to the relay's `/telemetry` endpoint. The relay operator controls retention.

---

## Third-Party Services

LightSpeed does not integrate with any third-party analytics, advertising, or tracking services. The only network connections are:
1. Your PC → the relay (UDP tunnel + optional QUIC control plane, plus plaintext HTTP telemetry to port 8080 when telemetry is on)
2. The relay → game servers (forwarded UDP packets)
3. Your PC → the signed relay registry over HTTPS (discovery only, no identifiers sent)

---

## Questions?

Open a [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues) or discussion.
