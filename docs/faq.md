# Frequently Asked Questions

---

## Basics

### Is LightSpeed really free?

Yes. LightSpeed is free for personal, non-commercial use under the LightSpeed Software License. Commercial use requires a paid license - see [LICENSE](../LICENSE). You run your own proxy on a small VPS (see [deployment guide](../infra/README.md)). There are no subscriptions, no usage fees, no paid tiers.

### Will LightSpeed get me banned?

No. LightSpeed uses the same class of network driver (WinDivert/nftables/pfctl) as ExitLag, WTFast, and NoPing. It does not modify game files, memory, or processes. All major anti-cheat systems (EAC, VAC, BattlEye, Riot Vanguard) permit this. Game servers see your real IP address - this is a transparent tunnel, not a VPN or anonymizer.

### Why does the interceptor need root/Administrator?

Kernel-level packet interception requires elevated privileges - the same reason VPNs and firewalls need them. On Linux it uses nftables/iptables. On macOS it uses pfctl. On Windows it uses WinDivert (a signed kernel driver). Without root, you can still use redirect mode (`--game-server`).

### What platforms are supported?

| Platform | Interceptor | Redirect Mode | GUI |
|----------|-------------|---------------|-----|
| Windows 10/11 | ✅ WinDivert | ✅ | ✅ egui |
| Linux | ✅ nftables/iptables | ✅ | ❌ CLI only |
| macOS | ✅ pfctl | ✅ | ❌ CLI only |
| Linux ARM64 | ✅ | ✅ | ❌ |

---

## How It Works

### How does LightSpeed actually reduce ping?

LightSpeed does **not** make your traffic go faster - packets can't outrun the speed of light. What it does is *proactively route* your traffic onto the fastest available path, avoiding congestion and needlessly long detours.

Your ISP sends packets along whatever path is cheapest for *them* - often congested or circuitous. LightSpeed sends your packets through a proxy in a major data center with direct backbone connections to game server regions. If that path is shorter or less congested than your ISP's default, your ping drops and stabilizes. Typical improvement: 10-40ms.

### My ping went UP. Why?

Two common reasons:

1. **Wrong proxy location** - if the proxy is farther from the game server than your direct path, the extra hop adds latency. This is the most common cause. Rule of thumb: pick the proxy closest to the **game server**, not closest to you.
2. **Poorly-connected proxy** - not all data centers are equal. A proxy only helps if that data center sits close to a major internet backbone or peering exchange. A cheap VPS in the "right" city but on a congested or residential upstream can be slower than your direct route.

LightSpeed can only optimize the route it's given. If you point it at a badly-placed proxy, your ping will go up - that's expected behavior, not a bug.

### Which proxy should I pick?

The proxy closest to the **game server region**. Examples:
- Playing on US West servers → pick a US West proxy
- Playing on Singapore servers from Australia → pick a Singapore proxy
- Playing on EU servers from NA → pick a Frankfurt/London proxy

### A note on routing reality (BGP)

Real internet routing is governed by **BGP** (Border Gateway Protocol) - the contracts and policies ISPs and transit providers use to hand off traffic. Your packets don't travel in a straight line; they follow whatever path the BGP tables and peering agreements decide, and providers routinely prioritize or deprioritize certain routes for cost or policy reasons.

What that means for you:

- A proxy only helps if it sits on a *better* BGP path than your home connection's default - typically a data center near a major backbone or peering point.
- "Closer on the map" doesn't always mean "faster on the wire."
- Route-optimization tools (including LightSpeed) estimate and re-route, but the physical path is ultimately dictated by the networks in between - which neither you nor LightSpeed control.

### How fast is auto-detection?

Usually 1-3 seconds after you connect to a game server. The interceptor watches for 3 packets to the same destination within 1.5 seconds before locking on.

### How does LightSpeed decide where to add relays?

The proxy derives the **country** of an IP address from a locally stored DB-IP Lite database. This happens in memory, transiently, at session creation, for both the client source address and the game-server destination address. It then counts sessions per `(source_country, destination_country)` pair, so the network can see which region pairs are underserved. A cell is suppressed until it has at least 3 sessions before it is exported, and the public stats carry only coarsened region-pair counts (for example `mena-eu`). The counters contain no raw IP, and no raw IP is exported by the placement pipeline.

---

## FEC (Reliability Shield)

### What is FEC?

Forward Error Correction. The proxy sends a small amount of redundant data (~25%) alongside your packets. If a packet is lost, it can be reconstructed without retransmission. Much more efficient than ExitLag's packet duplication (which sends every packet 2-3 times, using 200-300% bandwidth).

### When should I enable it?

Enable if you have packet loss (micro-stutters, rubber-banding). Disable if your connection is already saturated, metered, or has negligible loss (< 0.1%).

---

## Running a Proxy

### How do I get a proxy node?

You don't have to do anything. LightSpeed ships with the community relay network as the default: eight sponsor-funded relays (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, Sydney) that the client discovers automatically through a signed registry. The registry URL and the operator's public key are compiled into the client, so there is no setup and no config file needed.

If you want to use a different registry, override it with `--registry <url>` or a `[registry]` block in `lightspeed.toml`. See the [Community Relay Network guide](community-network.md).

### Do I need to run my own proxy?

No. The community network is the default and works out of the box. Self-hosting is still fully supported if you want your own dedicated relay: deploy a lightweight proxy (~500KB RAM) on any Linux VPS. See the [deployment guide](../infra/README.md).

### How much does a proxy cost?

Nothing if you use the community network. If you self-host, a small VPS runs a few dollars a month, and the proxy binary uses ~500KB RAM, so even the smallest instance is plenty. LightSpeed itself has no fees.

### Can I share my proxy with friends?

Yes. The proxy supports multiple concurrent sessions with per-client rate limiting and authentication. Configure tokens in `proxy.toml`.

---

## Troubleshooting

### "No game traffic seen"

- Make sure your game is actually connected to a server (not just the main menu)
- Verify you selected the correct game (`--game` flag)
- Try `--scan-processes` to list running game processes
- If your server uses a non-standard port, use manual server mode (`--game-server`)

### "Interceptor not available"

- Linux: make sure you're running as root and nftables/iptables is installed
- macOS: pfctl is built-in but requires root
- Windows: ensure `WinDivert64.sys` and `WinDivert.dll` are next to the `.exe`

### Packets sent but not delivered

Your packets reach the proxy but responses aren't reaching your game. Usually a firewall issue. LightSpeed tries to add firewall rules automatically. If that fails, add an inbound UDP rule for `lightspeed` or `lightspeed-gui.exe` manually.

---

## Privacy

### Does LightSpeed read my game traffic?

LightSpeed sees UDP packet headers (source/destination IP, port, size) to route them. Game content (player positions, chat, etc.) is encrypted by the game's own protocol and is not decrypted or logged. See the full [Privacy Policy](privacy.md).

### Is there telemetry?

Telemetry is **on by default** since v1.6.5. It sends anonymized aggregate metrics (RTT percentiles, jitter, FEC stats, and the direct/relayed/saved latency numbers) to the `/telemetry` endpoint of the relay you are connected to (a community or sponsor relay, or your own if you self-host). No IP addresses, tokens, identifiers, or game account data are collected. A cell is suppressed until it has at least 3 reports; this floor counts reports, not distinct people, so it is not a guarantee that 3 different people contributed. Reports are POSTed over plaintext HTTP to port 8080, and the endpoint is unauthenticated. Turn it off any time with `--no-telemetry`, `telemetry = false` under `[general]` in `lightspeed.toml`, or the GUI's **"Share anonymous latency stats"** checkbox. See [Privacy Policy](privacy.md) and the [Data Dictionary](data-dictionary.md).

### Does the proxy store my IP address?

No. The proxy processes your source IP and the game-server destination IP transiently, in memory, to route packets and to derive a country for placement analysis. The address itself is not stored. What is kept is an aggregate session counter per `(source_country, destination_country)` pair, with a floor of 3 sessions per cell so small cells are suppressed, and the public stats carry only coarsened region-pair counts. The placement counters contain no raw IP, and no raw IP is exported by the placement pipeline; the client telemetry contract is unchanged. Note that relay access logs can contain client IPs; see [What does the proxy log?](#what-does-the-proxy-log).

### What does the proxy log?

Operational logs written to stdout (configurable via `RUST_LOG`) can include client IPs, session start/end times, and bytes relayed. Client IPs appear there only for rate limiting and abuse detection. Those logs live on the relay that served the session (a community or sponsor relay, or your own if you self-host), are not exported, and are not joined to the placement counters. Retention is controlled by that relay's logging configuration. See [Privacy Policy](privacy.md).

---

## Other

### Can I use LightSpeed with a VPN?

Generally no - both try to intercept network traffic and will conflict. Disable your VPN before using LightSpeed.

### Does LightSpeed work with Cloudflare WARP?

Yes. Use `--warp` to enable WARP for the proxy leg of the connection. WARP can shave 5-10ms off local ISP routing. Combine with a proxy for maximum benefit.

### Where do I report bugs?

[Open an issue on GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Include your OS, game, and log output (run with `RUST_LOG=debug` for verbose logs).
