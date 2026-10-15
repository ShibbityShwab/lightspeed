# LightSpeed User Guide

> Step-by-step instructions for reducing your ping with LightSpeed.

---

## How LightSpeed Works

Your ISP routes game traffic through paths optimized for cost, not speed. LightSpeed intercepts your game's UDP packets and tunnels them through a **relay** - a lightweight server in a data center with high-speed backbone connections to game server regions. By default you use the **community relay network** (eight sponsor-funded relays, discovered automatically through a signed registry, no setup needed). You can also self-host your own proxy. If that path is faster than your ISP's default route, your ping drops.

```
Your PC ──→ ISP (slow path) ──→ Game Server        ❌ High ping
Your PC ──→ LightSpeed Proxy (fast backbone) ──→ Game Server   ✅ Low ping
```

---

## Prerequisites

- The `lightspeed` CLI tool or `lightspeed-gui` (Windows). No proxy setup is needed: the client discovers the community relays automatically.
- For interceptor mode: root/Administrator privileges
- Optional: your own proxy node if you prefer self-hosting (see [Deploy Proxy](deploy-proxy.md))

---

## Which app do I need?

| You're on | Download | Why |
|-----------|----------|-----|
| **Windows** | `lightspeed-gui` (MSI or ZIP) | The GUI is a standalone app - it already includes the client engine + WinDivert driver. You do **not** need the CLI. |
| **Linux** | `lightspeed-gui` (or `lightspeed-client`) | The GUI works on Linux (the system tray is a stub); the CLI is for power users. |
| **macOS** | `lightspeed-client` | No tested GUI yet. The GUI compiles for macOS but is **untested** on real hardware. |
| **Hosting a proxy** | `lightspeed-proxy` | Only if you're running a relay node on a VPS. |

> **You only ever need one package.** If you're a Windows player, grab `lightspeed-gui` and ignore the rest. The `lightspeed-client` is for Linux power users and macOS players; `lightspeed-proxy` is for self-hosters.

---

## Quick Start (CLI - All Platforms)

### 1. Check your environment

```bash
lightspeed --check
```

This verifies that your OS has the required packet filtering tools (nftables/iptables on Linux, pfctl on macOS, WinDivert on Windows).

### 2. Probe your relays

```bash
lightspeed --probe-proxies
```

Shows latency to each discovered relay. The client auto-selects the fastest on first run; you can override by picking the one closest to your **game server**, not your location.

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
⚡ OPTIMIZING - 123.45.67.89:28015
Packets Sent: 142 | Packets Returned: 139 | Packets Delivered: 139
```

---

## Quick Start (GUI - Windows)

### 1. Download

Grab the latest release from [Releases](https://github.com/ShibbityShwab/lightspeed/releases). Extract all files - keep `WinDivert64.sys` and `WinDivert.dll` next to `lightspeed-gui.exe`.

### 2. Run as Administrator

Right-click `lightspeed-gui.exe` → **Run as administrator**. The interceptor needs kernel-level access (same as VPN software).

### 3. Pick a relay and game

The GUI discovers the community relays and auto-selects the fastest on first run. You can override the relay from the dropdown, then choose your game.

### 4. Click **⚡ OPTIMIZE MY ROUTE**

Status changes to "🎯 Finding your game server…"

### 5. Launch your game

Connect to any server. LightSpeed auto-detects it within seconds.

---

### macOS GUI (untested)

The GUI compiles for macOS but is **untested** on real hardware. The release
ships a bare `tar.xz` (cargo-dist 0.32 has no `.app`/`.dmg` support), so to
produce a proper bundle, run on a Mac:

```bash
cargo build --release -p lightspeed-gui
./tools/package-macos.sh 1.6.5
```

This creates `LightSpeed.app` and `LightSpeed-1.6.5.dmg`. The app is ad-hoc
signed, so the first launch needs right-click → Open (or
`xattr -dr com.apple.quarantine LightSpeed.app`).

---

## Add your own game

If your game is not in the list you do not have to wait for a release: define it yourself.
Custom games and saved servers live in `profiles.toml`, next to `config.toml`:

- **Windows:** `%APPDATA%\light-speed\profiles.toml`
- **macOS:** `~/Library/Application Support/light-speed/profiles.toml`
- **Linux:** `$XDG_CONFIG_HOME/light-speed/profiles.toml` (usually `~/.config/light-speed/`)

A `[[games]]` entry takes `name` (required), an optional `key` (slugged from the name when you
omit it), optional `process_names` (this is what makes auto-detect work), an optional
`udp_ports` range such as `27015-27050`, an optional `dynamic_server` flag for games that rotate
through new servers, and optional `notes`. Copy-paste the full sample from the
[CLI reference](CLI-REFERENCE.md).

In the app, the **Game** dropdown gains **My games** and **New custom game…**, which writes the
same file. From the CLI, `lightspeed --game my-game` works as soon as the file exists,
`--list-games` shows your entries under `Your profiles`, `--profiles <path>` points at a
reproducible file, and `--no-user-profiles` ignores yours for one run.

Rules worth knowing: an entry whose `key`, alias or `process_names` a built-in game already owns
is refused on save and warned about on load, because built-in games always win. `tcp_ports` is
rejected - interception is UDP-only today. Saved server presets for the Manual / Custom mode go
in a `[[servers]]` entry with a `name` and an `address`.

## Choosing the Right Relay

| You're in | Game server in | Best relay region |
|-----------|---------------|-------------------|
| Australia | US West | US West (Los Angeles) |
| Europe | US East | US East (New Jersey) |
| Southeast Asia | Singapore | Singapore |
| South Asia | India | Mumbai |
| East Asia | Japan | Tokyo |
| South America | US East | US East (New Jersey) |
| Anywhere | Same region | Closest to game server |

> **Rule of thumb:** Pick the relay closest to the **game server**, not closest to you. Your traffic goes PC → relay → game server, so the relay-to-game-server leg is what matters most.

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

- [CLI Reference](CLI-REFERENCE.md) - every flag explained
- [FAQ](faq.md) - common questions
- [Troubleshooting](troubleshooting.md) - fix issues
- [Deploy Proxy](deploy-proxy.md) - run your own proxy
- [Supported Games](supported-games.md) - game compatibility
