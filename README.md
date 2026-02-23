# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
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
| 🎮 **Game Support** | Fortnite, CS2, Dota 2 (and growing) |
| 🌍 **Global Mesh** | Proxy nodes in US-West (LA) and Asia (Singapore) |
| 🦀 **Rust** | High-performance async runtime with Tokio, ~500KB RAM per proxy |
| 📖 **Open Source** | Full transparency, community-driven development |

## Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| Tunnel overhead | ≤ 5ms | **162μs** ✅ |
| Test pass rate | 100% | **52/52 + 8 FEC (100%)** ✅ |
| Security findings | 0 Critical/High | **0** ✅ |
| Proxy RAM usage | < 10MB | **~500KB** ✅ |
| WARP improvement | — | **5-10ms** ✅ |

### Live Infrastructure

| Node | Region | Latency (BKK) | Role |
|------|--------|----------------|------|
| **proxy-lax** | US-West (Los Angeles) | 206ms | Primary proxy |
| **relay-sgp** | Asia (Singapore) | 31ms | FEC multipath / SEA relay |

> **With WARP**: 193ms to LA — only 6ms from ExitLag's premium 187ms!

## Supported Games

| Game | Anti-Cheat | UDP Ports | Status |
|------|-----------|-----------|--------|
| Fortnite | EasyAntiCheat | 7000-9000 | ✅ Redirect mode ready |
| Counter-Strike 2 | VAC | 27015-27050 | ✅ Redirect mode ready |
| Dota 2 | VAC | 27015-27050 | ✅ Redirect mode ready |

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

```bash
# Redirect mode — recommended for game integration
lightspeed --game fortnite --redirect --proxy 149.28.84.139:4434

# With FEC enabled (recovers packet loss)
lightspeed --game fortnite --redirect --proxy 149.28.84.139:4434 --fec

# With WARP optimization (install Cloudflare WARP first)
lightspeed --game cs2 --redirect --proxy 149.28.84.139:4434 --warp

# Test tunnel connectivity
lightspeed --game fortnite --proxy 149.28.84.139:4434 --test

# Verbose logging
lightspeed --game dota2 --verbose
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
address = "149.28.84.139:4434"
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
│       ├── main.rs      # CLI, test modes, WARP/FEC/redirect orchestration
│       ├── config.rs    # TOML configuration
│       ├── error.rs     # Centralized error types
│       ├── warp.rs      # Cloudflare WARP integration
│       ├── redirect.rs  # UDP redirect proxy (game integration)
│       ├── tunnel/      # UDP tunnel engine, header codec, relay
│       ├── quic/        # QUIC control plane (discovery, health)
│       ├── route/       # Route selection, multipath, failover
│       ├── capture/     # Cross-platform packet capture
│       ├── ml/          # ML model: features, training, prediction
│       └── games/       # Per-game configurations (Fortnite, CS2, Dota 2)
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
│       └── fec.rs       # XOR Forward Error Correction
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
- [x] **v0.1.x** — Infrastructure: 3-node Vultr mesh, native binary deployment (~500KB RAM)
- [x] **v0.1.x** — FEC: XOR-based forward error correction, protocol v2
- [x] **v0.1.x** — WARP: Cloudflare integration for 5-10ms local route optimization
- [x] **v0.1.x** — Game integration: UDP redirect mode, per-game profiles
- [x] **v0.1.x** — ML: Synthetic training data, feature extraction, Random Forest model
- [x] **v0.1.x** — CI/CD: GitHub Actions (Rust CI, Docker GHCR, Pages)
- [ ] **v0.2.0** — FEC wired into client relay, game capture pipeline complete
- [ ] **v0.3.0** — Online ML learning from live traffic, auto route switching
- [ ] **v0.4.0** — Monitoring dashboard, auto-recovery, load testing
- [ ] **v1.0.0** — Public beta launch

## Contributing

LightSpeed is community-driven. Contributions welcome — open an issue or submit a PR.

## License

MIT — see [LICENSE](LICENSE) for details.

---

*Built with 🦀 Rust and ⚡ ambition.*
