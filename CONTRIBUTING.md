# <picture><source media="(prefers-color-scheme: dark)" srcset="web/assets/brand/lightspeed-mark-inverse.svg"><img src="web/assets/brand/lightspeed-mark.svg" width="26" height="26" align="absmiddle" alt=""></picture> Contributing to LightSpeed

Thanks for your interest in making LightSpeed better! This is an open-source
project and contributions of all kinds are welcome - code, bug reports, game
requests, proxy hosting, and documentation.

## Table of Contents

- [Quick Start](#quick-start)
- [Ways to Contribute](#ways-to-contribute)
- [Development Setup](#development-setup)
- [Windows Development](#windows-development)
- [Proxy Hosting](#proxy-hosting)
- [Submitting Changes](#submitting-changes)
- [Working With An AI Agent](#working-with-an-ai-agent)
- [Release Notes](#release-notes)
- [Community Guidelines](#community-guidelines)

---

## Quick Start

Pick the tab for your platform. The Rust toolchain is pinned by
`rust-toolchain.toml` (1.100.0); `rustup` installs it on first build.

**Windows (PowerShell)**

```powershell
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed

cargo build --release
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui

# The GUI is Windows-only and excluded above:
cargo build -p lightspeed-gui
cargo test -p lightspeed-gui

# Run the proxy locally
cargo run --bin lightspeed-proxy -- --config proxy/proxy.toml.default
```

**Linux / macOS (bash)**

```bash
git clone https://github.com/ShibbityShwab/lightspeed.git
cd lightspeed

cargo build --release
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui

# Run the proxy locally
cargo run --bin lightspeed-proxy -- --config proxy/proxy.toml.default
```

**Prerequisites:** Rust 1.100+ (via rustup), a C compiler for the `quic` feature,
libpcap-dev on Linux, Npcap + WinDivert on Windows. Windows GUI builds need the
MSVC build tools.

---

## Ways to Contribute

### 🐛 Bug Reports
Open a [GitHub Issue](https://github.com/ShibbityShwab/lightspeed/issues) with:
- Your OS and version
- Which game you were playing
- Your proxy node region (e.g. US-West, Asia-SE)
- What happened vs. what you expected
- Logs if available (`RUST_LOG=debug`)

### 🎮 Game Support Requests
Open an issue tagged `game-request` with:
- Game name
- UDP port range the game uses
- Your region and current ping

### 💬 Community Discussion
Use [GitHub Discussions](https://github.com/ShibbityShwab/lightspeed/discussions)
for general questions, feature ideas, benchmark sharing and introductions.

### 🌐 Run Your Own Proxy Node
LightSpeed ships a **community relay network** of eight sponsor-funded relays
that clients discover automatically, and self-hosting your own proxy is still
fully supported. Run your own proxy on a VPS near the game servers you play on,
or publish it to the signed registry for others.

1. Get a Linux VPS (any provider with a free tier - see `infra/README.md`)
2. Follow the setup guide in [`infra/README.md`](infra/README.md)
3. Use `infra/scripts/setup-new-node.sh` on the VPS
4. Requires: Linux VPS, UDP ports 4433/4434 open, 512MB RAM minimum

> **Managed cloud nodes** (where we host for you) are not offered. The community
> relays are sponsor-funded, and hosting your own remains the way to run a
> private node.

---

## Development Setup

### Project Structure

```
lightspeed/
├── client/         # Rust client (packet capture + routing)
│   └── src/
│       ├── capture/    # pcap backends (Linux/macOS/Windows) + WinDivert
│       ├── games/      # Game-specific profiles (24 games)
│       ├── ml/         # ML route selection (linfa)
│       ├── route/      # Route selector + failover
│       └── tunnel/     # UDP tunnel engine
├── client-gui/     # Windows egui tray app
├── proxy/          # Rust proxy server
│   └── src/
│       ├── relay.rs    # Core packet relay
│       ├── auth.rs     # Client auth
│       ├── metrics.rs  # Prometheus metrics
│       └── health.rs   # /health + /metrics HTTP endpoints
├── protocol/       # Shared tunnel protocol (header, FEC)
├── infra/          # Infrastructure (Terraform, Docker, Linux relay scripts)
│   └── monitoring/ # Prometheus + Grafana stack
├── web/            # Landing page (GitHub Pages, generated locales)
├── .dsh/skills/    # Agent procedures (DeepSeek Harness project skills)
├── wat/            # Agent state, personas, rules
└── docs/           # User docs, design docs, protocol
```

### Adding Game Support

1. Create `client/src/games/yourgame.rs` - see `cs2.rs` as a reference
2. Add the game to `client/src/games/mod.rs`
3. Test with a local proxy: `cargo run --bin lightspeed-proxy`
4. Submit a PR with benchmark results

### Running the Test Suite

```powershell
# Unit + integration tests (Windows; GUI is separate)
cargo test --workspace --all --exclude lightspeed-gui
cargo test -p lightspeed-gui
cargo test -p lightspeed-client --features windivert-redirect
```

End-to-end and load harnesses live in `tools/` and need a real proxy node plus a
real CPython 3.8+ interpreter (see [Windows Development](#windows-development)):

```powershell
py -3 tools/e2e_test.py            # against YOUR_PROXY_IP
py -3 tools/load_test.py YOUR_PROXY_IP --duration 60
```

### Quality Gates

The same commands CI runs, on one line each:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets
cargo test -p lightspeed-gui
cargo deny check   # separate install: cargo install cargo-deny
cargo audit        # separate install: cargo install cargo-audit
```

`tools/Check-LightSpeed.ps1` runs the pre-push set for you (`-SkipGui`,
`-SkipAudit`, `-SkipBuild` for a faster loop). If `cargo-audit`/`cargo-deny` are
not installed it reports those two as skipped rather than failing the run; the
CI security workflow still runs them on every push.

---

## Windows Development

The project is developed on Windows; these are the host facts that matter, not
workarounds.

- **Shell: Windows PowerShell 5.1.** `pwsh` is not required and not assumed. No
  Git Bash or WSL is needed for any documented task.
- **Line endings:** `.gitattributes` pins LF for text and CRLF for `.cmd`/`.bat`.
  A fresh checkout is consistent; an older worktree may still be CRLF until it
  is rechecked out.
- **Python is optional and separate.** The localization pipeline and `tools/*.py`
  need a real CPython 3.8+. The Microsoft Store aliases `python`/`python3` are
  stubs that exit 9009 and do not count. Install once with:

  ```powershell
  winget install Python.Python.3.12
  ```

  then use `py -3 ...` (or `scripts/Localization.ps1`, which finds it).
- **Linux-only scripts** (`infra/scripts/*.sh`) run on relay hosts and in CI.
  They are not part of the Windows dev loop.
- **GUI builds:** a running instance locks its own exe and the single-instance
  mutex is shared across builds, so check `Get-Process lightspeed-gui` before
  rebuilding. `scripts/dev-gui.ps1` and `scripts/install-gui.ps1` handle the
  elevation and copy-verification rules.

Localization, on any platform:

```powershell
.\scripts\Localization.ps1 -Check     # verify only
.\scripts\Localization.ps1            # regenerate + translate + verify
```

### Building for Linux (from Windows, optional)

Cross-compilation is not needed for normal work - CI builds Linux. If you want
to build locally, use a Linux container or WSL; note that the repository's
supported workflow does not require either.

---

## Proxy Hosting

LightSpeed clients use the community relay network by default, and self-hosting
a proxy is fully supported. See [`infra/README.md`](infra/README.md) for the full
guide.

### Requirements
- Linux VPS (Ubuntu 22.04+ recommended)
- 512MB RAM minimum (binary uses ~500KB in practice)
- UDP ports 4433 and 4434 open in firewall
- TCP port 8080 open for health/metrics (internal)
- Stable uptime (ideally 99%+)

### Setup (Automated)
```bash
# On the VPS, not on Windows:
wget https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/infra/scripts/setup-new-node.sh
# Review the script, then run:
bash setup-new-node.sh YOUR_VPS_IP your-node-id your-region
```

### Supported Cloud Providers (Free Tier Available)
Any provider with an Always Free tier works. The community relays run on
sponsor-funded Vultr instances, and the setup script targets Ubuntu 22.04+.

---

## Submitting Changes

1. Fork the repo and create a branch: `git checkout -b fix/my-feature`
2. Make your changes with tests
3. Run the [quality gates](#quality-gates) - all must pass
4. Submit a PR with a clear description
5. Accept the [Contributor License Agreement](CLA.md) in the PR (required;
   it is the first section of the pull-request template)

### PR Checklist
- [ ] I have read and accepted [CLA.md](CLA.md) - it assigns the copyright in
      this contribution to the Owner and is required before a merge
- [ ] Tests pass (`cargo test --workspace --all --exclude lightspeed-gui`, plus
      `-p lightspeed-gui` on Windows)
- [ ] No clippy warnings (`-D warnings` in CI)
- [ ] Docs updated if behaviour or a command changed
- [ ] No new infrastructure costs introduced (`$0.00` owner spend)

---

## Working With An AI Agent

This repository carries its own agent system: procedures as DeepSeek Harness
project skills under `.dsh/skills/`, a persona roster under `wat/personas/`,
rules in `wat/rules.md`, and durable state under `wat/state/`.

- Humans: start at [`wat/README.md`](wat/README.md).
- Agents: `AGENTS.md` is loaded automatically; the skills
  `lightspeed-wat`, `lightspeed-team`, `lightspeed-windows` and
  `lightspeed-verify` carry the procedures.
- Multi-agent work uses Agent Teams; the contract is
  [`wat/team-mode.md`](wat/team-mode.md).

An agent's change is only "verified" when the evidence path is recorded in
`wat/state/current-phase.md`.

**Authorship.** Commits name only the human who wrote them. Never add an AI
tool, model, or agent as an author or co-author (`Co-authored-by`/
`Co-Authored-By` trailers, tool session links), and never accept the CLA on
behalf of a tool. A contribution is merged only after its human author accepts
[CLA.md](CLA.md).

---

## Release Notes

Every release entry in [`CHANGELOG.md`](CHANGELOG.md) becomes that release's
GitHub body, verbatim. At release time `infra/scripts/release-notes.sh`
extracts the matching `## [<version>]` section and replaces cargo-dist's
generated boilerplate with it, so keep each entry self-contained and written for
someone deciding whether to upgrade.

Use these subsections, in this order, and omit any that are empty:

- `### Added` - new features, flags, games, and telemetry.
- `### Changed` - behavior changes and migrations users must know about.
- `### Fixed` - bugs and regressions.
- `### Security` - hardening and vulnerability fixes.
- `### Dependencies` - notable dependency bumps.
- `### Community` - credit the humans and reporters behind the release: code
  contributors, issue reporters, and dependency-bump bots. One line each, naming
  the handle, what they did, and the PR or issue number.
  `infra/scripts/release-contributors.sh <from-ref> <to-ref>` lists the non-bot
  authors in a range, which is the starting point for this section.

Do **not** write release-engineering prose. No install instructions, no
checksum tables, and no supply-chain verification steps such as "Verifying
GitHub Artifact Attestations". GitHub already lists every artifact on the
release page, and cargo-dist's generated block is discarded in favor of the
changelog entry.

### Announcements

Any release that changes client/relay compatibility or a user-visible
requirement must also ship a **GitHub Discussions -> Announcements** post,
published with (or before) the release. It names the affected versions, what
the user must do, where to update and any rollback path, and links the release
notes. Issue/discussion replies link to it instead of re-explaining. This is
the `[COMMS_STUB]` rule in [`wat/rules.md`](wat/rules.md); routine release
notices are covered by the owner's standing grant, anything beyond shipped
state needs an explicit owner decision first. Example:
[discussion #177](https://github.com/ShibbityShwab/lightspeed/discussions/177).

---

## Community Guidelines

- **Be respectful.** We're all here to improve gaming for everyone.
- **No discrimination.** Region, rank, or skill level - everyone's welcome.
- **No commercial spam.** Don't promote paid alternatives in our community.
- **Keep it constructive.** Bug reports and criticism are welcome; complaining
  without context isn't.

---

## License

LightSpeed is owned solely by ShibbityShwab and is free for non-commercial use;
commercial use requires a paid license. Contributions are accepted only under
the [Contributor License Agreement](CLA.md), which assigns the copyright in your
contribution to the Owner - you do not retain rights in a contribution. See
[LICENSE](LICENSE), [CLA.md](CLA.md) and
[docs/licensing.md](docs/licensing.md) for the full terms.

**tl;dr:** Free for gamers. If you want to use this commercially, contact us.
Contributors assign their work to the owner when they sign the CLA.
