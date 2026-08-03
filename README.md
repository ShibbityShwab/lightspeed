# ⚡ LightSpeed

**Reduce your ping. Free. Forever.**

[![Release](https://img.shields.io/github/v/release/ShibbityShwab/lightspeed?style=flat-square)](https://github.com/ShibbityShwab/lightspeed/releases)
[![CI](https://github.com/ShibbityShwab/lightspeed/actions/workflows/ci.yml/badge.svg)](https://github.com/ShibbityShwab/lightspeed/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/ShibbityShwab/lightspeed?style=flat-square)](LICENSE)

LightSpeed is a **zero-cost global network optimizer** for multiplayer games. It reduces and stabilizes your ping by routing game traffic through an intelligent proxy network — no subscriptions, no usage fees, no infrastructure bills.

> 💡 **Why?** Multiplayer game servers are often thousands of kilometers away. Your ISP routes traffic through slow, congested paths. LightSpeed lets you bypass those routes, sending your packets through an optimized proxy tunnel that's faster and more stable.

---

## 🚀 Quick Start

```bash
# Download the latest release
# https://github.com/ShibbityShwab/lightspeed/releases/latest

# Run the interactive demo
./lightspeed --demo

# Or jump straight in:
./lightspeed --start-interceptor --game rust --proxy YOUR_PROXY_IP:4434
```

See [**CLI Reference**](docs/CLI-REFERENCE.md) for all commands.

---

## 🎮 Supported Games (9 total)

| Game | CLI Flag | Auto-Detect Process | Anti-Cheat |
|------|----------|---------------------|------------|
| Rust | `--game rust` | `RustClient.exe` | EAC (EasyAntiCheat) |
| Counter-Strike 2 | `--game cs2` | `cs2.exe` | VAC |
| Fortnite | `--game fortnite` | `FortniteClient-Win64-Shipping.exe` | EAC + BattlEye |
| Dota 2 | `--game dota2` | `dota2.exe` | VAC |
| Apex Legends | `--game apex` | `r5apex.exe` | EAC |
| Valorant | `--game valorant` | `VALORANT-Win64-Shipping.exe` | Riot Vanguard |
| Overwatch 2 | `--game ow2` | `Overwatch.exe` | Blizzard Warden |
| League of Legends | `--game lol` | `League of Legends.exe` | Riot Vanguard |
| PUBG: Battlegrounds | `--game pubg` | `TslGame.exe` | BattlEye |

---

## 📋 Features

### Client (`lightspeed`)
- **UDP Tunnel Engine** — async packet relay with Tokio, keepalive, stats
- **FEC (Forward Error Correction)** — XOR-based parity for packet loss recovery
- **Route Selection** — nearest-proxy, multipath, ML-based prediction with heuristic fallback
- **ML Route Prediction** — 11-feature Random Forest model via linfa, online learning
- **Game Profiles** — 9 built-in configurations with auto-detection
- **Cross-Platform** — Windows x64, Linux x64, Linux ARM64, macOS (Intel + Apple Silicon)
- **TrafficInterceptor** — platform-native MITM (nftables/iptables on Linux, pfctl on macOS, WinDivert on Windows)

### Proxy Server (`lightspeed-proxy`)
- **UDP Relay** — high-performance session-based packet relay
- **Session Management** — token-based auth with automatic timeout
- **Rate Limiting** — per-IP and per-session (PPS/BPS)
- **Abuse Detection** — destination validation, amplification prevention, private IP blocking
- **Monitoring** — Prometheus metrics, HTTP health check
- **Docker** — multi-stage Dockerfile + docker-compose for easy deployment

### Protocol (`lightspeed-protocol`)
- **20-byte binary header** — version, flags, sequence, timestamp, original IPs/ports
- **Binary control messages** — Ping/Pong, Register/RegisterAck, Disconnect, ServerInfo
- **Unencrypted by design** — game traffic remains inspectable (anti-cheat compatible)

---

## 📦 Installation

### Pre-built Binaries

Download from [Releases](https://github.com/ShibbityShwab/lightspeed/releases).

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

**Requirements:** Rust 1.75+, `libpcap-dev` (Linux), Npcap SDK (Windows).

---

## 🐳 Docker

```bash
docker compose up -d

# Or build and run manually:
docker build -t lightspeed-proxy .
docker run -p 4434:4434/udp -p 8080:8080 lightspeed-proxy
```

---

## 🌍 Infrastructure

LightSpeed runs on **Always Free** tier cloud resources ($0.00/month).

| Node | Region | Provider |
|------|--------|----------|
| proxy-us | US-West (Los Angeles) | Any Free Tier VPS |
| proxy-sgp | Asia (Singapore) | Any Free Tier VPS |

See [docs/deploy-proxy.md](docs/deploy-proxy.md) for deployment guide.

---

## 📊 Monitoring

- `:8080/health` — HTTP health check
- `:8080/metrics` — Prometheus metrics endpoint
- `:8080/telemetry` — Opt-in anonymous latency reporting

---

## 📚 Documentation

| Document | Description |
|----------|-------------|
| [CLI Reference](docs/CLI-REFERENCE.md) | All CLI commands and flags |
| [Architecture](docs/architecture.md) | System design and data flow |
| [Protocol](docs/protocol.md) | Wire protocol specification |
| [User Guide](docs/user-guide.md) | Step-by-step setup |
| [FAQ](docs/faq.md) | Common questions |
| [Troubleshooting](docs/troubleshooting.md) | Common issues and solutions |
| [Supported Games](docs/supported-games.md) | Game-specific configuration |
| [Glossary](docs/glossary.md) | Terminology reference |
| [Privacy](docs/privacy.md) | Telemetry privacy policy |
| [Deploy Proxy](docs/deploy-proxy.md) | Self-hosted deployment guide |
| [Security Audit](docs/security-audit-mvp.md) | MVP security review |

---

## 🧪 Testing

```bash
# Run all tests
cargo test --workspace --exclude lightspeed-gui

# Run with formatting and linting (what CI does)
cargo fmt --all --check
cargo clippy --workspace --all-targets --exclude lightspeed-gui
cargo build --release --workspace --exclude lightspeed-gui
cargo test --workspace --exclude lightspeed-gui
```

---

## 🔒 Security

- **Token-based authentication** for all data-plane sessions
- **Rate limiting** per client (packets/sec, bytes/sec)
- **Destination validation** — blocks RFC 1918, localhost, multicast, link-local
- **Anti-amplification** — inbound/outbound byte ratio tracking
- **QUIC control plane** with mTLS for registration and token distribution
- **No secrets in source** — configuration via environment variables

See [docs/security-audit-mvp.md](docs/security-audit-mvp.md) for full audit report.

---

## 🤝 Contributing

We welcome contributions! See the [WAT system](wat/) for our autonomous development workflow.

### CI Pipeline

All PRs must pass:
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --exclude lightspeed-gui`
- `cargo test --workspace --exclude lightspeed-gui`
- `cargo audit` (security vulnerabilities)

### Project Structure

```
lightspeed/
├── client/          # `lightspeed` CLI client
├── client-gui/      # `lightspeed-gui` Windows tray app
├── proxy/           # `lightspeed-proxy` UDP relay server
├── protocol/        # `lightspeed-protocol` shared types
├── docs/            # Documentation
├── web/             # GitHub Pages landing page
├── wat/             # WAT autonomy engine
├── infra/           # Infrastructure (Terraform, Docker)
└── tools/           # Development utilities
```

---

## 📈 Roadmap

- [x] **v0.1.0** — MVP: UDP tunnel, proxy server, QUIC control, security hardening, 52 tests
- [x] **v0.2.0** — FEC (XOR parity), Cloudflare WARP integration, UDP redirect mode, live Vultr mesh, protocol v2
- [x] **v0.3.0** — Prometheus + Grafana monitoring, 10 alerting rules, CI/CD pipeline, pre-built binaries, load tested at 0.00% packet loss, online ML learning
- [x] **v0.4.0** — 9-game support (OW2, LoL, PUBG added), session telemetry (`--telemetry`), Windows GUI tray app, recvmmsg batched I/O, zero-alloc FEC hot path (-57% encode time), 153 tests across 4 crates, CI coverage job
- [x] **v0.5.0** — Linux interceptor CLI (--watch, --benchmark, --smoke-test, --status, --demo, --check), cross-platform GUI refactor (Platform trait), Docker deployment, MockInterceptor, 200 tests, 0 clippy warnings
- [x] **v0.5.1** — Cross-platform CI fixes (macOS, Windows), security audit passing, cargo-deny compliance, benchmark regression fixed
- [ ] **v1.0.0** — Public stable release: polished UX, installer wizard, community proxy network

## Contributing

We welcome contributions! See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT OR Apache-2.0 — see [LICENSE](LICENSE).

---

<p align="center">
  <sub>⚡ Built with Rust. Self-hosted on any VPS. Zero cost. Forever.</sub>
</p>