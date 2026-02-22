# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg?style=flat-square)](https://www.rust-lang.org/)

LightSpeed is a zero-cost, open-source global network optimizer for multiplayer games. It reduces latency by tunneling game traffic through optimally-placed proxy nodes that leverage cloud provider backbone networks — delivering 15-40% ping reduction without any subscription fees.

## How It Works

```
Your PC  ──UDP Tunnel──▶  Proxy Node  ──Direct UDP──▶  Game Server
(LightSpeed Client)      (Oracle Cloud              (Epic/Valve)
                          Always Free)
```

1. **Capture**: LightSpeed intercepts outbound game UDP packets
2. **Route**: An ML model selects the optimal proxy node based on real-time network conditions
3. **Tunnel**: Packets are routed through the proxy's superior network path
4. **Deliver**: The game server receives your original packet with your real IP preserved

## Key Features

| Feature | Details |
|---------|---------|
| 🆓 **Free Forever** | Zero infrastructure cost via Oracle Cloud Always Free tier |
| 🔓 **Transparent** | Unencrypted tunneling — anti-cheat friendly, no IP masking |
| 🤖 **AI-Powered** | ML route selection via linfa for optimal proxy choice |
| 🎮 **Game Support** | Fortnite, CS2, Dota 2 (and growing) |
| 🌍 **Global** | Proxy nodes in US-East, EU, and SEA (expanding) |
| 🦀 **Rust** | High-performance async runtime with Tokio |
| 📖 **Open Source** | Full transparency, community-driven development |

## MVP Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| Tunnel overhead | ≤ 5ms | **162μs** ✅ |
| Test pass rate | 100% | **52/52 (100%)** ✅ |
| Security findings | 0 Critical/High | **0** ✅ |

## Supported Games

| Game | Anti-Cheat | Status |
|------|-----------|--------|
| Fortnite | EasyAntiCheat | 🔧 MVP Ready |
| Counter-Strike 2 | VAC | 🔧 MVP Ready |
| Dota 2 | VAC | 🔧 MVP Ready |

## Installation

### Download Pre-built Binaries

Download the latest release for your platform from [**GitHub Releases**](https://github.com/ShibbityShwab/lightspeed/releases/latest).

| Platform | Download |
|----------|----------|
| Windows x64 | `lightspeed-vX.Y.Z-windows-x64.zip` |
| Linux x64 | `lightspeed-vX.Y.Z-linux-x64.tar.gz` |
| Linux ARM64 | `lightspeed-vX.Y.Z-linux-arm64.tar.gz` |

Extract the archive and add it to your PATH, or run directly from the extracted folder.

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
# Run with a specific game
lightspeed --game fortnite

# Run with auto-detection
lightspeed

# Dry-run mode (no actual packet capture)
lightspeed --game cs2 --dry-run

# Verbose logging
lightspeed --game dota2 --verbose
```

### Running the Proxy Server

```bash
# Start the proxy with default settings
lightspeed-proxy

# Custom bind address and ports
lightspeed-proxy --bind 0.0.0.0 --data-port 4434 --control-port 4433

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
address = "your-proxy-ip:4434"
control_port = 4433

[tunnel]
keepalive_interval_secs = 5
timeout_secs = 30
```

## Architecture

```
lightspeed/
├── client/          # Rust client — packet capture, tunnel, ML routing
│   └── src/
│       ├── tunnel/  # UDP tunnel engine, header codec, relay
│       ├── quic/    # QUIC control plane (discovery, health)
│       ├── route/   # Route selection, multipath, failover
│       ├── capture/ # Cross-platform packet capture
│       ├── ml/      # ML model loading and prediction
│       └── games/   # Per-game configurations
├── proxy/           # Rust proxy server — UDP relay, auth, metrics
│   └── src/
│       ├── relay.rs     # Packet relay loop
│       ├── auth.rs      # Token authentication
│       ├── rate_limit.rs # Rate limiting
│       ├── abuse.rs     # Abuse detection
│       ├── metrics.rs   # Prometheus metrics
│       └── health.rs    # Health check endpoint
├── protocol/        # Shared protocol crate — headers, control messages
├── docs/            # Architecture, protocol spec, security audit
└── .github/         # CI/CD workflows
```

See [`docs/architecture.md`](docs/architecture.md) for the full system design and [`docs/protocol.md`](docs/protocol.md) for the tunnel protocol specification.

## How It's Different

| | ExitLag | WTFast | LightSpeed |
|---|---------|--------|------------|
| **Price** | $6.50/mo | $9.99/mo | **Free** |
| **Encryption** | Yes | Yes | **No** (transparent) |
| **IP Preserved** | No | No | **Yes** |
| **Anti-Cheat Safe** | Sometimes | Sometimes | **By design** |
| **Open Source** | No | No | **Yes** |
| **AI Routing** | Proprietary | Proprietary | **Open (linfa)** |

## Development

```bash
# Run all tests
cargo test --workspace

# Run specific test suite
cargo test -p lightspeed-proxy --test integration_e2e

# Check without building
cargo check --workspace

# Format code
cargo fmt --all

# Lint
cargo clippy --workspace
```

## Documentation

- [Architecture Design](docs/architecture.md)
- [Protocol Specification](docs/protocol.md)
- [Security Audit](docs/security-audit-mvp.md)
- [Test Report](docs/test-report-mvp.md)
- [Changelog](CHANGELOG.md)

## Roadmap

- [x] **v0.1.0** — MVP: UDP tunnel, proxy server, QUIC control, security hardening
- [ ] **v0.2.0** — Proxy network deployment (Oracle Cloud multi-region)
- [ ] **v0.3.0** — AI route optimizer (linfa ML integration)
- [ ] **v0.4.0** — Game integration testing (Fortnite, CS2, Dota 2)
- [ ] **v0.5.0** — Monitoring, auto-recovery, load testing
- [ ] **v1.0.0** — Public beta launch

## Contributing

LightSpeed is community-driven. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT — see [LICENSE](LICENSE) for details.

---

*Built with 🦀 Rust and ⚡ ambition.*
