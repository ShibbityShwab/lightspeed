# Contributing to LightSpeed ⚡

Thanks for your interest in making LightSpeed better! This is an open-source project and contributions of all kinds are welcome — code, bug reports, game requests, proxy hosting, and documentation.

## Table of Contents

- [Quick Start](#quick-start)
- [Ways to Contribute](#ways-to-contribute)
- [Development Setup](#development-setup)
- [Proxy Hosting](#proxy-hosting)
- [Submitting Changes](#submitting-changes)
- [Community Guidelines](#community-guidelines)

---

## Quick Start

```bash
# Clone the repo
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed

# Build everything
cargo build --release

# Run tests
cargo test --workspace

# Run the proxy locally (for development)
cargo run --bin lightspeed-proxy -- --config proxy/proxy.toml.default
```

**Prerequisites:** Rust stable (1.75+), libpcap-dev (Linux), Npcap (Windows)

---

## Ways to Contribute

### 🐛 Bug Reports
Open a [GitHub Issue](https://github.com/ShibbityShwab/lightspeed/issues) with:
- Your OS and version
- Which game you were playing
- Your proxy node region (e.g. US-West, Asia-SE)
- What happened vs. what you expected
- Logs if available (run with `RUST_LOG=debug`)

### 🎮 Game Support Requests
Open an issue tagged `game-request` with:
- Game name
- UDP port range the game uses
- Your region and current ping

### 💬 Community Discussion
Use [GitHub Discussions](https://github.com/ShibbityShwab/lightspeed/discussions) for:
- General questions
- Feature ideas
- Benchmark sharing
- "Introduce yourself" posts

### 🌐 Run Your Own Proxy Node
LightSpeed is **self-hosted** — there is no shared network. You run your own proxy on a VPS near the game servers you play on. This is the core model.

1. Get a Linux VPS ($5-6/mo on Vultr, or free with Oracle Cloud Always Free)
2. Follow the setup guide in [`infra/README.md`](infra/README.md)
3. Use `infra/scripts/setup-new-node.sh` for automated setup
4. Requires: Linux VPS, UDP ports 4433/4434 open, 512MB RAM minimum

> **Managed cloud nodes** (where we host for you) are planned for a future release. For now, hosting your own is the way to go.

### 💻 Code Contributions
See [Development Setup](#development-setup) below.

---

## Development Setup

### Project Structure

```
lightspeed/
├── client/         # Rust client (packet capture + routing)
│   └── src/
│       ├── capture/    # pcap backends (Linux/macOS/Windows)
│       ├── games/      # Game-specific profiles (Fortnite, CS2, Dota 2, Rust)
│       ├── ml/         # ML route selection (linfa)
│       ├── route/      # Route selector + failover
│       └── tunnel/     # UDP tunnel engine
├── proxy/          # Rust proxy server
│   └── src/
│       ├── relay.rs    # Core packet relay
│       ├── auth.rs     # Client auth
│       ├── metrics.rs  # Prometheus metrics
│       └── health.rs   # /health + /metrics HTTP endpoints
├── protocol/       # Shared tunnel protocol (header, FEC)
├── infra/          # Infrastructure (Terraform, Docker, scripts)
│   └── monitoring/ # Prometheus + Grafana stack
└── web/            # Landing page (GitHub Pages)
```

### Adding Game Support

1. Create `client/src/games/yourgame.rs` — see `cs2.rs` as a reference
2. Add the game to `client/src/games/mod.rs`
3. Test with a local proxy: `cargo run --bin lightspeed-proxy`
4. Submit a PR with benchmark results

### Running the Test Suite

```bash
# Unit + integration tests
cargo test --workspace

# E2E test against your proxy node (requires YOUR_PROXY_IP to be running)
node tools/e2e_test.js

# Load test against your own node
python tools/load_test.py YOUR_PROXY_IP --duration 60
```

### Building for Linux (from Windows)

```bash
# In WSL or with cross-compilation toolchain:
rustup target add x86_64-unknown-linux-gnu
cargo build --release --bin lightspeed-proxy --target x86_64-unknown-linux-gnu
```

---

## Proxy Hosting

Self-hosting a proxy is how LightSpeed works. See [`infra/README.md`](infra/README.md) for the full guide.

### Requirements
- Linux VPS (Ubuntu 22.04+ recommended)
- 512MB RAM minimum (binary uses ~500KB in practice)
- UDP ports 4433 and 4434 open in firewall
- TCP port 8080 open for health/metrics (internal)
- Stable uptime (ideally 99%+)

### Setup (Automated)
```bash
# Download the setup script and review it first
wget https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/infra/scripts/setup-new-node.sh
# Review the script, then run:
bash setup-new-node.sh YOUR_VPS_IP your-node-id your-region
```

### Supported Cloud Providers (Free Tier Available)
| Provider | Instance | Free Period | Notes |
|----------|----------|------------|-------|
| Vultr | Cloud Compute | With credits | $300 credit for new accounts |
| Oracle Cloud | E2.1.Micro | Forever | 2 instances per region |
| Fly.io | shared-cpu-1x | Limited | 3 free VMs |

---

## Submitting Changes

1. Fork the repo and create a branch: `git checkout -b fix/my-feature`
2. Make your changes with tests
3. Run `cargo test --workspace` — all tests must pass
4. Run `cargo clippy --workspace` — no warnings
5. Submit a PR with a clear description

### PR Checklist
- [ ] Tests pass (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy --workspace`)
- [ ] Docs updated if needed
- [ ] No new infrastructure costs introduced

---

## Community Guidelines

- **Be respectful.** We're all here to improve gaming for everyone.
- **No discrimination.** Region, rank, or skill level — everyone's welcome.
- **No commercial spam.** Don't promote paid alternatives in our community.
- **Keep it constructive.** Bug reports and criticism are welcome; complaining without context isn't.

---

## License

LightSpeed uses a custom dual-license. Personal and open-source use is free. See [LICENSE](LICENSE) for details.

**tl;dr:** Free for gamers. If you want to use this commercially, contact us.
