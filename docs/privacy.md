# Privacy Policy

> Last updated: 2026-09-19

---

## Summary

LightSpeed is **privacy-first by design**. The tunnel is unencrypted to remain compatible with anti-cheat systems: game servers always see your real IP address. Telemetry is **on by default** and can be turned off at any time; it is limited to anonymized aggregate metrics. No personally identifiable information (PII) is ever collected or stored.

---

## What LightSpeed Does NOT Collect

- ❌ Stored IP addresses (yours or game servers'). IPs are processed transiently for routing and country derivation, then discarded. See [What LightSpeed Sees (and Doesn't Store)](#what-lightspeed-sees-and-doesnt-store).
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

This information is held in memory only and is never written to disk beyond debug logs (which you control via the `RUST_LOG` environment variable). Debug logging is off by default.

---

## Network placement analysis

To decide where relays are most useful, the proxy derives the **country** of an IP address from a locally stored DB-IP Lite database. This happens in memory, transiently, at session creation, for both the client source address and the game-server destination address. The country lookup is the only thing derived from the address; the address itself is not retained.

What is kept is a counter of sessions per `(source_country, destination_country)` pair. These counters are never per-user and never include raw IP addresses. A k-anonymity floor of 3 is applied before any cell is exported: any `(source_country, destination_country)` cell with fewer than 3 sessions is suppressed and never leaves the proxy.

The counters are exposed on the relay's `/metrics` endpoint as country-code labels, and persisted into the public stats history as **coarsened region-pair counts only** (for example `mena-eu`). The counters contain no raw IP, and no raw IP is exported by the placement pipeline (operational logs are covered in Proxy Logs below). There is no change to what the client sends.

IP geolocation data by DB-IP (https://db-ip.com), licensed under CC BY 4.0.

---

## Anonymous Telemetry (on by default)

Telemetry is **on by default**. It sends anonymous aggregate metrics to **your
own proxy's** `/telemetry` endpoint. There is no central LightSpeed telemetry
server. The relay suppresses any cell with fewer than **3 reports** (k-anonymity
floor of 3), so no individual client's data is ever exported.

| Metric | Example | PII? |
|--------|---------|------|
| RTT percentiles | p50: 31ms, p95: 45ms, p99: 52ms | No |
| Jitter | 2.3ms stddev | No |
| FEC recovery rate | 12 packets recovered / 1000 | No |
| Session duration | 45 minutes | No |
| Proxy region | "us-west" | No |
| Game name | "rust" | No |
| LightSpeed version | "1.4.2" | No |

**Explicitly NOT collected:** IP addresses, user identities, game account data, packet payloads.

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

If you run your own proxy, the proxy server writes access logs to stdout (configurable via `RUST_LOG`). These are operational logs, not analytics. They contain:
- Client IP addresses (for rate limiting and abuse detection)
- Session start/end times
- Packets/bytes relayed per session

Client IPs appear in these logs only because the proxy needs them for rate limiting and abuse detection. They are not exported, not used for country derivation, and not joined to the placement counters described above. These logs are stored on **your server**. LightSpeed does not have access to them. Configure log rotation and retention according to your own policies; retention is limited by your logging configuration.

---

## Data Retention

- **Client:** No data is persisted beyond the current session (in-memory only).
- **Proxy:** Logs are written to stdout. Retention is controlled by your server's logging configuration. Placement counters are aggregate only, with a k>=3 floor, and carry no raw IPs.
- **Telemetry:** Data is sent to your proxy's endpoint. You control retention.

---

## Third-Party Services

LightSpeed does not integrate with any third-party analytics, advertising, or tracking services. The only network connections are:
1. Your PC → your proxy (UDP tunnel + optional QUIC control plane)
2. Your proxy → game servers (forwarded UDP packets)

---

## Questions?

Open a [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues) or discussion.
