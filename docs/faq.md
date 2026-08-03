# Frequently Asked Questions

---

## Basics

### Is LightSpeed really free?

Yes. LightSpeed is MIT-licensed open source. You run your own proxy on a free-tier VPS (see [deployment guide](../infra/README.md)). There are no subscriptions, no usage fees, no paid tiers.

### Will LightSpeed get me banned?

No. LightSpeed uses the same class of network driver (WinDivert/nftables/pfctl) as ExitLag, WTFast, and NoPing. It does not modify game files, memory, or processes. All major anti-cheat systems (EAC, VAC, BattlEye, Riot Vanguard) permit this. Game servers see your real IP address — this is a transparent tunnel, not a VPN or anonymizer.

### Why does the interceptor need root/Administrator?

Kernel-level packet interception requires elevated privileges — the same reason VPNs and firewalls need them. On Linux it uses nftables/iptables. On macOS it uses pfctl. On Windows it uses WinDivert (a signed kernel driver). Without root, you can still use redirect mode (`--game-server`).

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

Your ISP sends packets along whatever path is cheapest for them — often congested or circuitous. LightSpeed sends your packets through a proxy in a major data center with direct backbone connections to game server regions. If that path is shorter or less congested, your ping drops. Typical improvement: 10–40ms.

### My ping went UP. Why?

If the proxy is farther from the game server than your direct path, it adds latency. Try a different proxy region. Rule of thumb: pick the proxy closest to the **game server**, not closest to you.

### Which proxy should I pick?

The proxy closest to the **game server region**. Examples:
- Playing on US West servers → pick a US West proxy
- Playing on Singapore servers from Australia → pick a Singapore proxy
- Playing on EU servers from NA → pick a Frankfurt/London proxy

### How fast is auto-detection?

Usually 1–3 seconds after you connect to a game server. The interceptor watches for 3 packets to the same destination within 1.5 seconds before locking on.

---

## FEC (Reliability Shield)

### What is FEC?

Forward Error Correction. The proxy sends a small amount of redundant data (~25%) alongside your packets. If a packet is lost, it can be reconstructed without retransmission. Much more efficient than ExitLag's packet duplication (which sends every packet 2–3 times, using 200–300% bandwidth).

### When should I enable it?

Enable if you have packet loss (micro-stutters, rubber-banding). Disable if your connection is already saturated, metered, or has negligible loss (< 0.1%).

---

## Running a Proxy

### Do I need to run my own proxy?

Yes. LightSpeed is self-hosted — there's no shared network. You deploy a lightweight proxy (~500KB RAM) on any Linux VPS. Many providers offer free tiers. See the [deployment guide](../infra/README.md).

### How much does a proxy cost?

Zero if you use a free tier. Options include Oracle Cloud Always Free (4 ARM cores, 24GB RAM — permanent), Google Cloud free tier, or AWS free tier. Even a paid $5/mo VPS works — the binary uses ~500KB RAM.

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

Telemetry is **opt-in only** (`--telemetry` flag). When enabled, it collects anonymized aggregate metrics (RTT percentiles, FEC stats). No IP addresses, user identities, or game account data are collected. See [Privacy Policy](privacy.md).

---

## Other

### Can I use LightSpeed with a VPN?

Generally no — both try to intercept network traffic and will conflict. Disable your VPN before using LightSpeed.

### Does LightSpeed work with Cloudflare WARP?

Yes. Use `--warp` to enable WARP for the proxy leg of the connection. WARP can shave 5–10ms off local ISP routing. Combine with a proxy for maximum benefit.

### Where do I report bugs?

[Open an issue on GitHub](https://github.com/ShibbityShwab/lightspeed/issues). Include your OS, game, and log output (run with `RUST_LOG=debug` for verbose logs).
