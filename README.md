# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/releases)
[![License: Non-Commercial](https://img.shields.io/badge/License-Non--Commercial-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg?style=flat-square)](https://www.rust-lang.org/)
[![Landing Page](https://img.shields.io/badge/web-live-brightgreen?style=flat-square)](https://shibbityshwab.github.io/lightspeed/)

LightSpeed is a zero-cost, open-source global network optimizer for multiplayer games. It reduces latency by tunneling game traffic through optimally-placed proxy nodes that leverage cloud provider backbone networks — delivering measurable ping reduction without any subscription fees.

## How It Works

```
Your PC  ──UDP Tunnel──▶  Proxy Node   ──Direct UDP──▶  Game Server
(LightSpeed Client)      (Vultr Cloud)                  (Epic/Valve)
                          ~500KB RAM
```

1. **Intercept**: LightSpeed captures outbound game UDP packets via local redirect or pcap
2. **Route**: Select the optimal proxy node based on real-time latency (ML-powered or nearest)
3. **Tunnel**: Packets are wrapped in a lightweight 20-byte header and sent through the proxy's superior network path
4. **Protect**: FEC (Forward Error Correction) recovers lost packets in 3ms instead of 400ms+ retransmit
5. **Deliver**: The game server receives your original packet — your real IP is preserved

## Key Features

| Feature | Details |
|---------|---------|
| 🆓 **Free Forever** | Zero infrastructure cost — Vultr free credits, no subscription |
| 🔓 **Transparent** | Unencrypted tunneling — anti-cheat friendly, no IP masking |
| 🛡️ **FEC Protection** | XOR-based Forward Error Correction — recover lost packets without retransmission |
| 🌐 **WARP Boost** | Optional Cloudflare WARP integration for 5-10ms local route optimization |
| 🤖 **AI-Powered** | ML route selection via linfa Random Forest (11 network features) |
| 🎮 **Game Support** | Fortnite, CS2, Dota 2, Rust, Valorant, Apex, OW2, LoL, PUBG (9 games) |
| 🌍 **Global Mesh** | Proxy nodes in US-West (LA) and Asia (Singapore) |
| 🦀 **Rust** | High-performance async runtime with Tokio, ~500KB RAM per proxy |
| 📖 **Open Source** | Full transparency, community-driven development |

## Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| Tunnel overhead | ≤ 5ms | **162μs** ✅ |
| Test pass rate | 100% | **~110 tests (100%)** ✅ |
| Security findings | 0 Critical/High | **0** ✅ |
| Proxy RAM usage | < 10MB | **~500KB** ✅ |
| WARP improvement | — | **5-10ms** ✅ |

### Benchmark Results (Self-Hosted on Vultr vc2-1c-1gb)

| Node Region | Latency (from Bangkok) | Role |
|-------------|----------------------|------|
| US-West (Los Angeles) | 206ms | Primary proxy |
| Asia (Singapore) | 31ms | FEC multipath / SEA relay |

> **With WARP enabled**: 193ms to LA — only 6ms off ExitLag's premium 187ms — at zero cost.

Every user runs **their own proxy node**. Set one up on any VPS in under 5 minutes — see [Running the Proxy Server](#running-the-proxy-server) and [infra/README.md](infra/README.md).

## Supported Games

| Game | CLI Flag | Anti-Cheat | UDP Ports | Auto-detect Process | Status |
|------|----------|-----------|-----------|---------------------|--------|
| Fortnite | `--game fortnite` | EasyAntiCheat | 7000–9000 | `FortniteClient-Win64-Shipping.exe` | ✅ Ready |
| Counter-Strike 2 | `--game cs2` | VAC | 27015–27050 | `cs2.exe` | ✅ Ready |
| Dota 2 | `--game dota2` | VAC | 27015–27050 | `dota2.exe` | ✅ Ready |
| Rust (Facepunch) | `--game rust` | EAC + Facepunch | 28015–28017 | `RustClient.exe` | ✅ Ready |
| Valorant | `--game valorant` | Riot Vanguard | 7000–7500 | `VALORANT-Win64-Shipping.exe` | ✅ Ready |
| Apex Legends | `--game apex` | EasyAntiCheat | 37000–37050 | `r5apex.exe` | ✅ Ready |
| Overwatch 2 | `--game ow2` | Blizzard Warden | 3478–6250 | `Overwatch.exe` | ✅ Ready |
| League of Legends | `--game lol` | Riot Vanguard | 5000–5500 | `League of Legends.exe` | ✅ Ready |
| PUBG: Battlegrounds | `--game pubg` | BattlEye | 7000–17999 | `TslGame.exe` | ✅ Ready |

## Installation

### Download Pre-built Binaries

Download the latest release for your platform from [**GitHub Releases**](https://github.com/ShibbityShwab/lightspeed/releases/latest).

| Platform | Download |
|----------|----------|
| Windows x64 | `lightspeed-vX.Y.Z-windows-x64.zip` |
| Linux x64 | `lightspeed-vX.Y.Z-linux-x64.tar.gz` |
| Linux ARM64 | `lightspeed-vX.Y.Z-linux-arm64.tar.gz` |

### Build from Source

#### Prerequisites

- [Rust](https://rustup.rs/) 1.75+ (stable, 2021 edition)
- C compiler (MSVC on Windows, gcc on Linux)
- Optional: [Npcap](https://npcap.com/) (Windows) or libpcap (Linux/macOS) for packet capture features

#### Build

```bash
# Clone the repository
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed

# Build both client and proxy (release mode)
cargo build --release

# Binaries will be at:
#   target/release/lightspeed        (client)
#   target/release/lightspeed-proxy  (proxy server)
```

## Quick Start

### Running the Client

You need a proxy node to connect to. Either [run your own](#running-the-proxy-server) on any Linux VPS, or join the community to find nodes near you.

```bash
# Redirect mode — recommended for game integration
lightspeed --game fortnite --redirect --proxy YOUR_PROXY_IP:4434

# With FEC enabled (recovers packet loss)
lightspeed --game fortnite --redirect --proxy YOUR_PROXY_IP:4434 --fec

# With WARP optimization (install Cloudflare WARP first)
lightspeed --game cs2 --redirect --proxy YOUR_PROXY_IP:4434 --warp

# Test tunnel connectivity
lightspeed --game fortnite --proxy YOUR_PROXY_IP:4434 --test

# Verbose logging
lightspeed --game dota2 --verbose

# Collect and report session telemetry
lightspeed --game fortnite --proxy YOUR_PROXY_IP:4434 --telemetry
```

### Running the Proxy Server

```bash
# Start the proxy with default settings
lightspeed-proxy

# Custom bind address and ports
lightspeed-proxy --bind 0.0.0.0 --data-port 4434 --control-port 4433

# With config file
lightspeed-proxy --config proxy.toml

# Verbose logging
lightspeed-proxy --verbose
```

### Configuration

LightSpeed uses TOML configuration files. A default config is generated on first run:

```toml
# ~/.lightspeed/config.toml

[general]
game = "fortnite"
log_level = "info"

[proxy]
address = "YOUR_PROXY_IP:4434"   # IP of your own proxy node
control_port = 4433

[tunnel]
keepalive_interval_secs = 5
timeout_secs = 30
fec_enabled = true
fec_group_size = 8

[route]
strategy = "nearest"
```

## Architecture

```
lightspeed/
├── client/          # Rust client — packet capture, tunnel, ML routing
│   └── src/
│       ├── main.rs      # Entry point — arg parsing, mode dispatch
│       ├── cli.rs       # CLI flag definitions (clap)
│       ├── config.rs    # TOML configuration
│       ├── error.rs     # Centralized error types
│       ├── warp.rs      # Cloudflare WARP integration
│       ├── redirect.rs  # UDP redirect proxy (game integration)
│       ├── telemetry.rs # Session telemetry collection + report
│       ├── modes/       # Run-mode handlers (capture, tunnel_test, live_test, proxy_probe)
│       ├── tunnel/      # UDP tunnel engine, header codec, relay
│       ├── quic/        # QUIC control plane (discovery, health)
│       ├── route/       # Route selection, multipath, failover
│       ├── capture/     # Cross-platform packet capture
│       ├── ml/          # ML model: features, training, prediction
│       └── games/       # Per-game configurations (9 games: Fortnite, CS2, Dota 2, Rust, Valorant, Apex, OW2, LoL, PUBG)
├── client-gui/      # Windows system-tray GUI (WinAPI, tray-item)
├── proxy/           # Rust proxy server — UDP relay, auth, metrics
│   └── src/
│       ├── relay.rs         # Session-based packet relay loop
│       ├── auth.rs          # Token authentication
│       ├── rate_limit.rs    # Per-client PPS/BPS rate limiting
│       ├── abuse.rs         # Amplification/reflection detection
│       ├── metrics.rs       # Prometheus metrics
│       ├── health.rs        # HTTP health check endpoint
│       └── control.rs       # QUIC control server
├── protocol/        # Shared protocol crate
│   └── src/
│       ├── header.rs    # 20-byte tunnel header (v1 plain, v2 FEC)
│       ├── control.rs   # Binary control messages
│       ├── fec.rs       # XOR Forward Error Correction
│       └── telemetry.rs # Telemetry event schema + POST /telemetry payload
├── docs/            # Architecture, protocol spec, security audit
├── infra/           # Terraform, Docker, deployment scripts
├── tools/           # E2E test scripts, echo server
└── web/             # Landing page (GitHub Pages)
```

See [`docs/architecture.md`](docs/architecture.md) for the full system design and [`docs/protocol.md`](docs/protocol.md) for the tunnel protocol specification.

## How It's Different

| | ExitLag | WTFast | LightSpeed |
|---|---------|--------|------------|
| **Price** | $6.50/mo | $9.99/mo | **Free** |
| **Encryption** | Yes | Yes | **No** (transparent) |
| **IP Preserved** | No | No | **Yes** |
| **Anti-Cheat Safe** | Sometimes | Sometimes | **By design** |
| **FEC** | No | No | **Yes** (XOR parity) |
| **Open Source** | No | No | **Yes** |
| **AI Routing** | Proprietary | Proprietary | **Open (linfa RF)** |
| **Proxy RAM** | Unknown | Unknown | **~500KB** |

## Development

```bash
# Run all tests
cargo test --workspace

# Run specific test suite
cargo test -p lightspeed-proxy --test integration_e2e

# Run FEC tests
cargo test -p lightspeed-protocol fec

# Check without building
cargo check --workspace

# Format code
cargo fmt --all

# Lint
cargo clippy --workspace
```

## Documentation

- [Architecture Design](docs/architecture.md)
- [Protocol Specification](docs/protocol.md) — v1 (plain) and v2 (FEC)
- [Security Audit](docs/security-audit-mvp.md)
- [Test Report](docs/test-report-mvp.md)
- [Changelog](CHANGELOG.md)
- [Infrastructure](infra/README.md)

## Roadmap

- [x] **v0.1.0** — MVP: UDP tunnel, proxy server, QUIC control, security hardening, 52 tests
- [x] **v0.2.0** — FEC (XOR parity), Cloudflare WARP integration, UDP redirect mode, live Vultr mesh, protocol v2
- [x] **v0.3.0** — Prometheus + Grafana monitoring, 10 alerting rules, CI/CD pipeline, pre-built binaries, load tested at 0.00% packet loss, online ML learning
- [x] **v0.4.0-dev** — 9-game support (OW2, LoL, PUBG added), session telemetry (`--telemetry`), Windows GUI tray app, recvmmsg batched I/O, ~110 tests across 4 crates, security hardening
- [ ] **v1.0.0** — Public stable release: polished UX, installer wizard, community proxy network

## Contributing

LightSpeed is community-driven. Contributions welcome — open an issue or submit a PR.

## License & Commercial Use

This project uses a dual-licensing model to protect its technical discoveries and implementations:

1. **Free for Non-Commercial & Personal Use:** You can use, study, and modify this project for personal or educational purposes, provided you give **explicit credit and attribution** to the original authors.
2. **Paid for Commercial Use:** You are strictly prohibited from using this software, its architecture, or its underlying technologies to generate revenue, offer SaaS products, or integrate into proprietary systems without permission. 

**If you wish to use this technology commercially, you must contact the author to negotiate and purchase a commercial license.**

See [LICENSE](LICENSE) for full details.

---

*Built with 🦀 Rust and ⚡ ambition.*
