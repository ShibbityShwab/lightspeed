# LightSpeed User Guide

> Step-by-step instructions for reducing your ping with LightSpeed.

---

## How LightSpeed Works

Your ISP routes game traffic through paths optimized for cost, not speed. LightSpeed intercepts your game's UDP packets and tunnels them through a **proxy node** — a lightweight relay server you control — that sits in a data center with high-speed backbone connections to game server regions. If that path is faster than your ISP's default route, your ping drops.

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## Prerequisites

- A LightSpeed proxy node running somewhere (see [Deploy Proxy](deploy-proxy.md) — takes 5 minutes)
- The `lightspeed` CLI tool or `lightspeed-gui` (Windows)
- For interceptor mode: root/Administrator privileges

---

## Quick Start (CLI — All Platforms)

### 1. Check your environment

```bash
lightspeed --check
```

This verifies that your OS has the required packet filtering tools (nftables/iptables on Linux, pfctl on macOS, WinDivert on Windows).

### 2. Probe your proxies

```bash
lightspeed --probe-proxies
```

Shows latency to each configured proxy. Pick the one closest to your **game server**, not your location.

### 3. Start the interceptor

```bash
# Linux/macOS (requires root)
sudo lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Windows (requires Administrator)
lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

### 4. Launch your game

Connect to any server normally. LightSpeed auto-detects the game server from outbound packets and begins tunneling within seconds.

### 5. Monitor

The CLI displays live stats:
```
⚡ BOOST ENGAGED — 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## Quick Start (GUI — Windows)

### 1. Download

Grab the latest release from [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Extract all files — keep `WinDivert64.sys` and `WinDivert.dll` next to `lightspeed-gui.exe`.

### 2. Run as Administrator

Right-click `lightspeed-gui.exe` → **Run as administrator**. The interceptor needs kernel-level access (same as VPN software).

### 3. Pick a proxy and game

Select your proxy from the dropdown and choose your game.

### 4. Click **⚡ BOOST MY GAME**

Status changes to "🎯 Finding your game server…"

### 5. Launch your game

Connect to any server. LightSpeed auto-detects it within seconds.

---

## Choosing the Right Proxy

| You're in | Game server in | Best proxy region |
|-----------|---------------|-------------------|
| Australia | US West | US West (Los Angeles) |
| Europe | US East | US East (New York) |
| Southeast Asia | Singapore | Singapore |
| South America | US East | US East (Miami) |
| Anywhere | Same region | Closest to game server |

> **Rule of thumb:** Pick the proxy closest to the **game server**, not closest to you. Your traffic goes PC → proxy → game server, so the proxy-to-game-server leg is what matters most.

---

## Forward Error Correction (FEC)

FEC adds ~25% bandwidth overhead to recover lost packets without retransmission.

**Enable when:**
- You have packet loss (micro-stutters, rubber-banding)
- You're on Wi-Fi with intermittent interference

**Disable when:**
- Your connection is already saturated
- You're on a metered/capped connection
- You have < 0.1% packet loss (no benefit)

```bash
# CLI: enable FEC with default block size (K=4)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec

# Custom block size (K=8 → 12.5% overhead)
lightspeed --start-interceptor --game cs2 --proxy YOUR_PROXY:4434 --fec --fec-k 8
```

---

## Advanced: Manual Server Mode

If auto-detection doesn't work (custom ports, unusual game):

```bash
# Redirect mode: game connects to localhost, LightSpeed forwards to real server
lightspeed --game rust --game-server 123.45.67.89:28015 --proxy YOUR_PROXY:4434
```

Then configure your game to connect to `127.0.0.1:<port>` (the local port LightSpeed prints).

---

## Switching Servers Mid-Session

LightSpeed automatically detects when you disconnect from one server and connect to another. The status briefly shows "🎯 Finding your game server…" and locks onto the new destination. No manual action needed.

---

## System Tray (Windows GUI)

- Click **×** to minimize to tray (doesn't quit)
- Double-click the bolt icon to restore
- Right-click for quick Connect / Disconnect / Quit

---

## See Also

- [CLI Reference](CLI-REFERENCE.md) — every flag explained
- [FAQ](faq.md) — common questions
- [Troubleshooting](troubleshooting.md) — fix issues
- [Deploy Proxy](deploy-proxy.md) — run your own proxy
- [Supported Games](supported-games.md) — game compatibility
