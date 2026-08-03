# Privacy Policy

> Last updated: 2026-08-03

---

## Summary

LightSpeed is **privacy-first by design**. The tunnel is unencrypted to remain compatible with anti-cheat systems — game servers always see your real IP address. Telemetry is opt-in only and limited to anonymized aggregate metrics. No personally identifiable information (PII) is ever collected.

---

## What LightSpeed Does NOT Collect

- ❌ IP addresses (yours or game servers')
- ❌ User identities or account information
- ❌ Game account data or player names
- ❌ Packet payloads (game data is encrypted by the game's own protocol)
- ❌ Browsing history or other non-game traffic

---

## What LightSpeed Sees (and Doesn't Store)

During operation, the client and proxy process UDP packet headers:
- Source/destination IP addresses (for routing)
- Source/destination ports (for game detection)
- Packet sizes (for bandwidth accounting)
- Session tokens (for authentication)

This information is held in memory only and is never written to disk beyond debug logs (which you control via the `RUST_LOG` environment variable). Debug logging is off by default.

---

## Opt-In Telemetry (`--telemetry`)

When you explicitly enable telemetry with the `--telemetry` flag, LightSpeed collects:

| Metric | Example | PII? |
|--------|---------|------|
| RTT percentiles | p50: 31ms, p95: 45ms, p99: 52ms | No |
| Jitter | 2.3ms stddev | No |
| FEC recovery rate | 12 packets recovered / 1000 | No |
| Session duration | 45 minutes | No |
| Proxy region | "us-west" | No |
| Game name | "rust" | No |
| LightSpeed version | "0.5.1" | No |

**Explicitly NOT collected:** IP addresses, user identities, game account data, packet payloads.

Telemetry is sent to your own proxy's `/telemetry` endpoint. You control where it goes — there is no central LightSpeed telemetry server.

### How to Enable/Disable

```bash
# Enable for this session
lightspeed --telemetry --start-interceptor --game rust --proxy YOUR_PROXY:4434

# Disable for this session (overrides config)
lightspeed --no-telemetry --start-interceptor --game rust --proxy YOUR_PROXY:4434
```

Telemetry is **off by default**. There is no auto-enrollment.

---

## Proxy Logs

If you run your own proxy, the proxy server writes access logs to stdout (configurable via `RUST_LOG`). These logs contain:
- Client IP addresses (for rate limiting and abuse detection)
- Session start/end times
- Packets/bytes relayed per session

These logs are stored on **your server** — LightSpeed does not have access to them. Configure log rotation and retention according to your own policies.

---

## Data Retention

- **Client:** No data is persisted beyond the current session (in-memory only).
- **Proxy:** Logs are written to stdout. Retention is controlled by your server's logging configuration.
- **Telemetry:** Data is sent to your proxy's endpoint. You control retention.

---

## Third-Party Services

LightSpeed does not integrate with any third-party analytics, advertising, or tracking services. The only network connections are:
1. Your PC → your proxy (UDP tunnel + optional QUIC control plane)
2. Your proxy → game servers (forwarded UDP packets)

---

## Questions?

Open a [GitHub issue](https://github.com/ShibbityShwab/lightspeed/issues) or discussion.
