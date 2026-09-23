<h1 align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="web/assets/brand/lightspeed-logo-inverse.svg">
    <img alt="LightSpeed - free game network optimizer" src="web/assets/brand/lightspeed-logo.svg" width="340">
  </picture>
</h1>

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square&color=blue)](https://github.com/ShibbityShwab/lightspeed/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/ShibbityShwab/lightspeed/ci.yml?branch=master&style=flat-square)](https://github.com/ShibbityShwab/lightspeed/actions)
[![License](https://img.shields.io/badge/license-NonCommercial-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust%201.88+-orange.svg?style=flat-square)](https://rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-200%2B%20passing-brightgreen.svg?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/actions)

LightSpeed is a **zero-cost global network optimizer** for multiplayer games. It routes your game traffic through an optimized proxy tunnel, bypassing slow ISP paths to reduce and stabilize your ping - no subscriptions, no usage fees, no infrastructure bills.

> **How?** Your ISP routes game packets through congested paths chosen for cost, not speed. LightSpeed tunnels them through a proxy node with high-speed backbone connections to game server regions. The result is lower, more stable latency - typically 10-40ms improvement depending on your location and the game server.

### 🌐 Community Relay Network

LightSpeed now runs a **community relay network**: eight sponsor-funded relays in Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo, Mumbai, Madrid, and Sydney. Clients discover them automatically through a signed registry (`https://shibbityshwab.github.io/lightspeed/registry.json`) with the operator's key compiled in, so there is nothing to configure. The GUI picks the fastest relay for you on first run, and self-hosting your own proxy is still fully supported. See the [Community Relay Network guide](docs/community-network.md) for details.

---

## 🚀 Quick Start

```bash
# Download the latest release
# https://github.com/ShibbityShwab/lightspeed/releases/latest

# Option 1: Interactive demo (no proxy needed)
./lightspeed --demo

# Option 2: Jump straight in (auto-selects the fastest community relay)
./lightspeed --start-interceptor --game rust

# Option 3: Probe the community relays first
./lightspeed --probe-proxies

# Or point at your own proxy
./lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

📖 **[Full User Guide →](docs/user-guide.md)** | **[CLI Reference →](docs/CLI-REFERENCE.md)**

---

## 🎮 Supported Games

| Game | CLI Flag | Anti-Cheat | Auto-Detect |
|------|----------|------------|-------------|
| Rust | `--game rust` | EAC | ✅ |
| Counter-Strike 2 | `--game cs2` | VAC | ✅ |
| CS:GO Legacy | `--game csgo` | VAC | ✅ |
| Fortnite | `--game fortnite` | EAC + BattlEye | ✅ |
| Dota 2 | `--game dota2` | VAC | ✅ |
| Apex Legends | `--game apex` | EAC | ✅ |
| Valorant | `--game valorant` | Riot Vanguard | ✅ |
| Overwatch 2 | `--game ow2` | Blizzard Warden | ✅ |
| League of Legends | `--game lol` | Riot Vanguard | ✅ |
| PUBG: Battlegrounds | `--game pubg` | BattlEye | ✅ |
| MapleStory | `--game maplestory` | BlackCipher (NGS) | ✅ |
| Genshin Impact | `--game genshin` | None | ✅ |
| Rocket League | `--game rocketleague` | EAC | ✅ |
| World of Tanks | `--game wot` | None | ✅ |
| Dead by Daylight | `--game deadbydaylight` | EAC | ✅ |
| Bodycam | `--game bodycam` | None | ✅ |
| Roblox | `--game roblox` | Byfron (Hyperion) | ✅ |
| Project Zomboid | `--game zomboid` | None | ✅ |

📖 **[Full Game List →](docs/supported-games.md)**

---

## ✨ Features

### Smart Routing
- **Automatic proxy selection** - probes all configured proxies and picks the fastest; the GUI auto-selects the fastest relay on first run with an "Auto (fastest)" checkbox
- **Community relay auto-discovery** - finds the eight community relays via the signed registry with zero config, or point at your own with `--registry <url>`
- **RTT saved** - measures the client's direct (ICMP) game-server round trip against the tunnelled round trip so you can see the estimated saving, with the caveat that the two are different instruments
- **ML-based route prediction** - 11-feature Random Forest model learns from your connection patterns
- **Multipath FEC** - XOR-based Forward Error Correction with ~25% bandwidth overhead (vs. ExitLag's 200%)
- **TCP tunnel fallback** - client↔proxy leg over TCP (`--tcp`) for networks that block UDP

### Packet Interception
- **Kernel-level MITM** - nftables/iptables (Linux), pfctl (macOS), WinDivert (Windows)
- **Per-process targeting** - auto-detects your game process and its UDP connections
- **IP-transparent** - game servers always see your real IP (not a VPN)

### Operations
- **Zero-cost self-hosting** - deploy your own proxy on any Linux VPS (~500KB RAM)
- **Prometheus + Grafana** - built-in monitoring stack
- **Cross-platform** - Windows, Linux, macOS (Intel + Apple Silicon)

---

## 📦 Installation

### Which file do I download?

LightSpeed ships three packages. **You only need one**:

| You want to… | Download | Notes |
|--------------|----------|-------|
| **Play on Windows** (recommended) | `lightspeed-gui-...-windows-msvc.msi` (or `.zip`) | Everything included - GUI + engine + WinDivert driver. No separate client needed. |
| **Play on Linux** | `lightspeed-gui-...-linux-gnu.tar.xz` (or `lightspeed-client`) | GUI + engine, or the CLI for power users. |
| **Play on macOS** | `lightspeed-client-...` | CLI client. The GUI compiles for macOS but is **untested** on real hardware. |
| **Host a proxy node** | `lightspeed-proxy-...` | Only if you're running a relay server on a VPS. |

> **Why is there both a "client" and a "gui"?** The GUI (`lightspeed-gui`) is a standalone app that already contains the client engine. Grab it for the easiest experience. The CLI (`lightspeed-client`) is for headless/power users and for macOS, where the GUI is untested. You never need to install both.

### Package Managers

| Platform | Command |
|----------|---------|
| **Windows (Scoop)** | `scoop bucket add ShibbityShwab https://github.com/ShibbityShwab/scoop-bucket && scoop install lightspeed` |
| **Windows (Chocolatey)** | `choco install lightspeed` (submitted, pending moderation) |
| **Windows (winget)** | `winget install ShibbityShwab.LightSpeed` (submitted, awaiting Microsoft review) |
| **macOS and Linux** | `brew tap ShibbityShwab/lightspeed https://github.com/ShibbityShwab/lightspeed && brew install ShibbityShwab/lightspeed/lightspeed` |
| **Arch Linux** | `yay -S lightspeed-bin` (AUR, prepared but not yet published) |

### One-line Install (Linux and macOS)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://github.com/ShibbityShwab/lightspeed/releases/latest/download/lightspeed-client-installer.sh | sh
```

### Pre-built Binaries
Download from **[Releases](https://github.com/ShibbityShwab/lightspeed/releases)** - Windows, Linux, macOS. The [website](https://shibbityshwab.github.io/lightspeed/#download) offers one-click downloads that pick the right artifact for your OS and CPU.

### Build from Source
```bash
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed
cargo build --release

# Run the client
./target/release/lightspeed --demo

# Or run the proxy
./target/release/lightspeed-proxy --config proxy/proxy.toml
```

**Requirements:** Rust 1.88+ (1.95+ for the GUI), `libpcap-dev` (Linux), Npcap SDK (Windows).

---

## 🐳 Self-Host a Proxy

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/shibbityshwab/lightspeed-proxy:latest
docker run -d -p 4434:4434/udp -p 8080:8080 lightspeed-proxy

# Or build from source
docker build -f infra/docker/Dockerfile -t lightspeed-proxy .
docker run -d -p 4434:4434/udp -p 8080:8080 lightspeed-proxy
```

📖 **[Full Deployment Guide →](infra/README.md)**

---

## 🧪 Development

```bash
# Build
cargo build --release --workspace --exclude lightspeed-gui

# Test
cargo test --workspace --exclude lightspeed-gui

# Lint
cargo clippy --workspace --all-targets --exclude lightspeed-gui

# Security audit
cargo audit
```

### Project Structure
```
lightspeed/
├── client/          # CLI client (packet capture, routing, interceptor)
├── client-gui/      # Windows GUI tray app (egui)
├── proxy/           # UDP relay server (proxy mesh node)
├── protocol/        # Shared tunnel protocol (header, FEC, control)
├── infra/           # Docker, monitoring, deploy scripts
├── docs/            # Documentation
└── web/             # GitHub Pages landing page
```

---

## 📚 Documentation

| Document | Description |
|----------|-------------|
| [User Guide](docs/user-guide.md) | Step-by-step setup and usage |
| [CLI Reference](docs/CLI-REFERENCE.md) | All commands and flags |
| [FAQ](docs/faq.md) | Common questions |
| [Troubleshooting](docs/troubleshooting.md) | Fix common issues |
| [Deploy Proxy](docs/deploy-proxy.md) | Self-hosting guide |
| [Community Relay Network](docs/community-network.md) | Run + publish a relay for others |
| [Architecture](docs/architecture.md) | System design |
| [Protocol](docs/protocol.md) | Wire protocol spec |
| [Supported Games](docs/supported-games.md) | Game profiles |
| [Privacy](docs/privacy.md) | Telemetry policy |
| [Glossary](docs/glossary.md) | Terminology |

---

## 🔒 Security

- **Token-based authentication** for all data-plane sessions
- **Rate limiting** per client (packets/sec, bytes/sec)
- **Destination validation** - blocks RFC 1918, localhost, multicast
- **Anti-amplification** - inbound/outbound byte ratio tracking
- **Unencrypted by design** - game traffic remains inspectable (anti-cheat compatible)

📖 **[Security Audit →](docs/security-audit-mvp.md)**

---

## 🔏 Privacy

Since 1.6.5, LightSpeed shares **anonymous aggregate latency statistics on by default** so the community can see real ping improvements. Only percentiles (p50/p95/p99), jitter, FEC counters, and the direct-vs-tunnelled round-trip difference (the "RTT saved" metric) are sent to **your own relay**. No IP addresses, tokens, identifiers, or packet contents are ever collected, and the relay suppresses any cell with fewer than 3 reports (k=3).

Turn it off any time with `--no-telemetry`, `telemetry = false` in `lightspeed.toml`, or the **"Share anonymous latency stats"** checkbox in the GUI.

📖 **[Privacy Policy →](docs/privacy.md)**

---

## 🗺️ Roadmap

- [x] **v0.1.0** - MVP: UDP tunnel, proxy server, QUIC control, security hardening
- [x] **v0.2.0** - FEC (XOR parity), WARP integration, redirect mode, live proxy mesh
- [x] **v0.3.0** - Prometheus + Grafana, CI/CD pipeline, pre-built binaries
- [x] **v0.4.0** - 9-game support, session telemetry, Windows GUI, recvmmsg batched I/O
- [x] **v0.5.0** - Linux interceptor CLI, cross-platform GUI, Docker, MockInterceptor
- [x] **v1.0.0** - Public stable release: installer wizard + self-hosted proxy model
- [x] **v1.6.5** - Eight-relay community network, on-by-default anonymous telemetry with opt-out, and the "RTT saved" metric

---

## 🤝 Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) and the [issue tracker](https://github.com/ShibbityShwab/lightspeed/issues).

---

## 📄 License

Free for non-commercial use. Commercial use requires a paid license. See [LICENSE](LICENSE).

---

<p align="center">
  <sub>⚡ Built with Rust. Self-hosted on any VPS. Zero cost. Forever.</sub>
</p>
