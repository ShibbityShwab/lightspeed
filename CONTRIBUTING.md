# <picture><source media="(prefers-color-scheme: dark)" srcset="web/assets/brand/lightspeed-mark-inverse.svg"><img src="web/assets/brand/lightspeed-mark.svg" width="26" height="26" align="absmiddle" alt=""></picture> Contributing to LightSpeed

Thanks for your interest in making LightSpeed better! This is an open-source project and contributions of all kinds are welcome - code, bug reports, game requests, proxy hosting, and documentation.

## Table of Contents

- [Quick Start](#quick-start)
- [Ways to Contribute](#ways-to-contribute)
- [Development Setup](#development-setup)
- [Proxy Hosting](#proxy-hosting)
- [Submitting Changes](#submitting-changes)
- [Release Notes](#release-notes)
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

**Prerequisites:** Rust 1.88+ (1.95+ for the GUI), libpcap-dev (Linux), Npcap (Windows)

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
LightSpeed ships a **community relay network** of eight sponsor-funded relays that clients discover automatically, and self-hosting your own proxy is still fully supported. You can run your own proxy on a VPS near the game servers you play on, or publish it to the signed registry for others.

1. Get a Linux VPS (any provider with a free tier - see infra/README.md)
2. Follow the setup guide in [`infra/README.md`](infra/README.md)
3. Use `infra/scripts/setup-new-node.sh` for automated setup
4. Requires: Linux VPS, UDP ports 4433/4434 open, 512MB RAM minimum

> **Managed cloud nodes** (where we host for you) are not offered. The community relays are sponsor-funded, and hosting your own remains the way to run a private node.

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
│       ├── games/      # Game-specific profiles (19 games: Rust, CS2, Fortnite, ...)
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

1. Create `client/src/games/yourgame.rs` - see `cs2.rs` as a reference
2. Add the game to `client/src/games/mod.rs`
3. Test with a local proxy: `cargo run --bin lightspeed-proxy`
4. Submit a PR with benchmark results

### Running the Test Suite

```bash
# Unit + integration tests
cargo test --workspace

# E2E test against your proxy node (requires YOUR_PROXY_IP to be running)
python3 tools/e2e_test.py

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

LightSpeed clients use the community relay network by default, and self-hosting a proxy is fully supported. See [`infra/README.md`](infra/README.md) for the full guide.

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
Any provider with an Always Free tier works. The community relays run on sponsor-funded Vultr instances, and the setup script targets Ubuntu 22.04+.

---

## Submitting Changes

1. Fork the repo and create a branch: `git checkout -b fix/my-feature`
2. Make your changes with tests
3. Run `cargo test --workspace` - all tests must pass
4. Run `cargo clippy --workspace` - no warnings
5. Submit a PR with a clear description

### PR Checklist
- [ ] Tests pass (`cargo test --workspace`)
- [ ] No clippy warnings (`cargo clippy --workspace`)
- [ ] Docs updated if needed
- [ ] No new infrastructure costs introduced

---

## Release Notes

Every release entry in [`CHANGELOG.md`](CHANGELOG.md) becomes that release's
GitHub body, verbatim. At release time `infra/scripts/release-notes.sh`
extracts the matching `## [<version>]` section and replaces cargo-dist's
generated boilerplate with it, so keep each entry self-contained and written
for someone deciding whether to upgrade.

Use these subsections, in this order, and omit any that are empty:

- `### Added` - new features, flags, games, and telemetry.
- `### Changed` - behavior changes and migrations users must know about.
- `### Fixed` - bugs and regressions.
- `### Security` - hardening and vulnerability fixes.
- `### Dependencies` - notable dependency bumps.
- `### Community` - credit the humans and reporters behind the release:
  code contributors, issue reporters, and dependency-bump bots. One line
  each, naming the handle, what they did, and the PR or issue number.
  `infra/scripts/release-contributors.sh <from-ref> <to-ref>` lists the
  non-bot authors in a range, which is the starting point for this section.

Do **not** write release-engineering prose. No install instructions, no
checksum tables, and no supply-chain verification steps such as "Verifying
GitHub Artifact Attestations". GitHub already lists every artifact on the
release page, and cargo-dist's generated block is discarded in favor of the
changelog entry.

---

## Community Guidelines

- **Be respectful.** We're all here to improve gaming for everyone.
- **No discrimination.** Region, rank, or skill level - everyone's welcome.
- **No commercial spam.** Don't promote paid alternatives in our community.
- **Keep it constructive.** Bug reports and criticism are welcome; complaining without context isn't.

---

## License

LightSpeed is free for non-commercial use. Commercial use requires a paid license. By contributing, you agree to license your contribution under the same terms. See [LICENSE](LICENSE) for full details.

**tl;dr:** Free for gamers. If you want to use this commercially, contact us.
