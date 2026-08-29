# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square&color=blue)](https://github.com/ShibbityShwab/lightspeed/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/ShibbityShwab/lightspeed/ci.yml?branch=master&style=flat-square)](https://github.com/ShibbityShwab/lightspeed/actions)
[![License](https://img.shields.io/badge/license-NonCommercial-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/built%20with-Rust%201.85+-orange.svg?style=flat-square)](https://rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-200%2B%20passing-brightgreen.svg?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/actions)

LightSpeed is a **zero-cost global network optimizer** for multiplayer games. It routes your game traffic through an optimized proxy tunnel, bypassing slow ISP paths to reduce and stabilize your ping — no subscriptions, no usage fees, no infrastructure bills.

> **How?** Your ISP routes game packets through congested paths chosen for cost, not speed. LightSpeed tunnels them through a proxy node with high-speed backbone connections to game server regions. The result is lower, more stable latency — typically 10–40ms improvement depending on your location and the game server.

---

## 🚀 Quick Start

```bash
# Download the latest release
# https://github.com/ShibbityShwab/lightspeed/releases/latest

# Option 1: Interactive demo (no proxy needed)
./lightspeed --demo

# Option 2: Jump straight in
./lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434

# Option 3: Probe your proxy first
./lightspeed --probe-proxies
```

📖 **[Full User Guide →](docs/user-guide.md)** | **[CLI Reference →](docs/CLI-REFERENCE.md)**

---

## 🎮 Supported Games

| Game | CLI Flag | Anti-Cheat | Auto-Detect |
|------|----------|------------|-------------|
| Rust | `--game rust` | EAC | ✅ |
| Counter-Strike 2 | `--game cs2` | VAC | ✅ |
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

📖 **[Full Game List →](docs/supported-games.md)**

---

## ✨ Features

### Smart Routing
- **Automatic proxy selection** — probes all configured proxies and picks the fastest
- **ML-based route prediction** — 11-feature Random Forest model learns from your connection patterns
- **Multipath FEC** — XOR-based Forward Error Correction with ~25% bandwidth overhead (vs. ExitLag's 200%)
- **TCP tunnel fallback** — client↔proxy leg over TCP (`--tcp`) for networks that block UDP

### Packet Interception
- **Kernel-level MITM** — nftables/iptables (Linux), pfctl (macOS), WinDivert (Windows)
- **Per-process targeting** — auto-detects your game process and its UDP connections
- **IP-transparent** — game servers always see your real IP (not a VPN)

### Operations
- **Zero-cost self-hosting** — deploy your own proxy on any Linux VPS (~500KB RAM)
- **Prometheus + Grafana** — built-in monitoring stack
- **Cross-platform** — Windows, Linux, macOS (Intel + Apple Silicon)

---

## 📦 Installation

### Which file do I download?

LightSpeed ships three packages. **You only need one**:

| You want to… | Download | Notes |
|--------------|----------|-------|
| **Play on Windows** (recommended) | `lightspeed-gui-...-windows-msvc.msi` (or `.zip`) | Everything included — GUI + engine + WinDivert driver. No separate client needed. |
| **Play on Linux / macOS** | `lightspeed-client-...` | CLI client (no GUI on these platforms yet). |
| **Host a proxy node** | `lightspeed-proxy-...` | Only if you're running a relay server on a VPS. |

> **Why is there both a "client" and a "gui"?** The GUI (`lightspeed-gui`) is a standalone app that already contains the client engine — Windows players should download the GUI only. The CLI (`lightspeed-client`) is for headless/power users and for Linux/macOS, where the GUI isn't built yet. You never need to install both.

### Pre-built Binaries
Download from **[Releases](https://github.com/ShibbityShwab/lightspeed/releases)** — Windows, Linux, macOS.

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
| [Architecture](docs/architecture.md) | System design |
| [Protocol](docs/protocol.md) | Wire protocol spec |
| [Supported Games](docs/supported-games.md) | Game profiles |
| [Privacy](docs/privacy.md) | Telemetry policy |
| [Glossary](docs/glossary.md) | Terminology |

---

## 🔒 Security

- **Token-based authentication** for all data-plane sessions
- **Rate limiting** per client (packets/sec, bytes/sec)
- **Destination validation** — blocks RFC 1918, localhost, multicast
- **Anti-amplification** — inbound/outbound byte ratio tracking
- **Unencrypted by design** — game traffic remains inspectable (anti-cheat compatible)

📖 **[Security Audit →](docs/security-audit-mvp.md)**

---

## 🗺️ Roadmap

- [x] **v0.1.0** — MVP: UDP tunnel, proxy server, QUIC control, security hardening
- [x] **v0.2.0** — FEC (XOR parity), WARP integration, redirect mode, live proxy mesh
- [x] **v0.3.0** — Prometheus + Grafana, CI/CD pipeline, pre-built binaries
- [x] **v0.4.0** — 9-game support, session telemetry, Windows GUI, recvmmsg batched I/O
- [x] **v0.5.0** — Linux interceptor CLI, cross-platform GUI, Docker, MockInterceptor
- [x] **v1.0.0** — Public stable release: installer wizard + self-hosted proxy model

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
