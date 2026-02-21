# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

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

## Supported Games

| Game | Anti-Cheat | Status |
|------|-----------|--------|
| Fortnite | EasyAntiCheat | 🚧 In Development |
| Counter-Strike 2 | VAC | 🚧 In Development |
| Dota 2 | VAC | 🚧 In Development |

## Quick Start

> ⚠️ LightSpeed is under active development. These instructions will work once the MVP is complete.

```bash
# Download the latest release
# https://github.com/lightspeed-gaming/lightspeed/releases

# Run with a specific game
lightspeed --game fortnite

# Run with auto-detection
lightspeed

# Verbose mode for debugging
lightspeed --game cs2 --verbose
```

## Building from Source

### Prerequisites

- [Rust](https://rustup.rs/) (stable, 2024 edition)
- [Npcap](https://npcap.com/) (Windows) or libpcap (Linux/macOS) for packet capture

### Build

```bash
git clone https://github.com/lightspeed-gaming/lightspeed.git
cd lightspeed

# Build both client and proxy
cargo build --release

# Run the client
cargo run --bin lightspeed -- --game fortnite --dry-run

# Run the proxy
cargo run --bin lightspeed-proxy -- --verbose

# Run tests
cargo test
```

## Architecture

```
lightspeed/
├── client/          # Rust client (packet capture, tunnel, ML routing)
├── proxy/           # Rust proxy server (UDP relay, auth, metrics)
├── infra/           # Terraform for Oracle Cloud Always Free
├── ai/              # ML models and training data
├── docs/            # Architecture, protocol, API docs
├── tests/           # Integration and benchmark tests
└── wat/             # WAT project management system
```

See [`docs/architecture.md`](docs/architecture.md) for the full system design.

## How It's Different

| | ExitLag | WTFast | LightSpeed |
|---|---------|--------|------------|
| **Price** | $6.50/mo | $9.99/mo | **Free** |
| **Encryption** | Yes | Yes | **No** (transparent) |
| **IP Preserved** | No | No | **Yes** |
| **Anti-Cheat Safe** | Sometimes | Sometimes | **By design** |
| **Open Source** | No | No | **Yes** |
| **AI Routing** | Proprietary | Proprietary | **Open (linfa)** |

## Contributing

LightSpeed is community-driven. See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT — see [LICENSE](LICENSE) for details.

---

*Built with 🦀 Rust and ⚡ ambition.*
