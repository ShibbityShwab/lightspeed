# Changelog

All notable changes to LightSpeed will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added (2026-04-27)
- **Rust (Facepunch) game profile** — `RustConfig` in `client/src/games/rust.rs`
  - Auto-detects `RustClient.exe`, port range 28015–28017, EAC + Facepunch Anti-Hack
  - CLI: `--game rust` (also accepts `--game rustgame`)
  - No Steam Datagram Relay — direct UDP, ideal for LightSpeed proxying
  - Added `game_id::RUST = 4` to `protocol/src/control.rs`

### Fixed (2026-04-27)
- **`capture/injector.rs`**: `sendpacket` API break on `pcap` v2.4.0 — `&raw_packet` → `raw_packet` (type now requires `Borrow<[u8]>` not `&Borrow<[u8]>`)
- **`capture/injector.rs`**: Spurious `unused_mut` warning on local `udp` vec
- **`ml/predict.rs`**: `predict_route` (ml feature path) now gracefully falls back to weighted heuristic when `model_bytes` is empty (first run before model is trained), instead of returning `Err`

### Maintenance (2026-04-27)
- **Fixed** unused-variable warning `sent` → `_sent` in `client/src/main.rs:786`
- **Updated** `proxy/src/main.rs` doc-comment: OCI → Vultr
- **Updated** `docs/security-audit-mvp.md` threat model: OCI → Vultr infrastructure
- **Added** `infra/terraform/LEGACY-OCI.md` — explains OCI decommission context
- **Updated** `infra/README.md` — `infra/fly/` and `infra/docker/` labeled as not-pursued/legacy
- **Added** `infra/terraform/versions.tf` — LEGACY header with Vultr redirect
- **Added** `.geminirules`, `.antigravityrules`, `.agents/workflows/wat-loop.md` to repo (AI tool config, same pattern as `.clinerules`)
- **Pruned** stale git remote tracking refs (`origin/main`, `origin/redesign-2026` — belonged to a previous site, no longer exist on GitHub)

### Next Up
- US-East / EU-West mesh expansion
- Discord community server
- v1.0.0 public stable release

---

## [0.3.0] — 2026-03-20

### 🚀 Production-Validated Release — Monitoring, Load Testing & CI/CD

First fully load-tested, monitoring-equipped release. Both proxy nodes validated at **0.00% packet loss** under sustained load. Pre-built binaries now ship with every release via automated CI/CD.

### Added

#### Online Learning
- **Online learning wired into main.rs** — keepalive probe RTTs now feed into `OnlineLearner`
  during both keepalive mode and capture mode, with automatic model retraining and
  cross-session persistence to `~/.lightspeed/measurements.json`

#### Monitoring & Observability (WF-005)
- **Enhanced Prometheus metrics** — 20+ metrics including latency histograms (11 buckets),
  FEC recovery counters, auth/abuse/rate-limit security metrics, session lifecycle tracking,
  build info, and uptime gauges. All exported with `region` + `node_id` labels.
- **Route-aware health server** — `/health` returns JSON, `/metrics` returns Prometheus
  exposition format. Proper HTTP routing with 404 for unknown paths.
- **Prometheus config** — scrape targets for both Vultr nodes (proxy-lax + relay-sgp),
  10s scrape interval, 30d retention.
- **Alerting rules** — 10 alert rules across 5 groups: node health (down/restart),
  latency (warning at 100ms, critical at 500ms), capacity (connections, drops, no traffic),
  security (auth rejections, abuse, rate limits), and FEC health.
- **Pre-built Grafana dashboard** — 6 sections (Overview, Traffic, Latency, FEC, Security,
  Sessions) with 20 panels including stat, timeseries, and histogram visualizations.
  Auto-provisioned on startup.
- **Docker Compose monitoring stack** — one-command `docker compose up -d` deploys
  Prometheus + Grafana with persistent volumes, health checks, and auto-provisioning.
- **Enhanced mesh-health.sh** — built-in node list, `--metrics` flag for Prometheus
  output, `--json` flag for machine-readable output, FEC recovery display.
- **Load testing tool** (`tools/load_test.py`) — multi-client concurrent UDP stress
  test with ramp-up, per-node and `--all-nodes` modes, latency percentiles (p50/p95/p99),
  packet loss measurement, pre/post health checks, and JSON export.
- **Vultr deploy script** (`infra/scripts/deploy-vultr.sh`) — cross-compile, SCP upload,
  rolling restart via systemd with pre/post health verification.

#### CI/CD & Release Infrastructure
- **GitHub Actions CI pipeline** — test → fmt → clippy → cross-compile (Windows x64, Linux x64, Linux ARM64) → auto-release on tag push
- **Pre-built binaries** — all three platform binaries attached to every GitHub Release automatically
- **Issue templates** — Bug Report, Game Request, Feature Request
- **CONTRIBUTING.md** — full dev setup guide, game support guide, proxy hosting guide
- **GitHub Discussions** — community Q&A and announcements enabled
- **12 repo topics** — rust, gaming, network-optimizer, ping-reducer, multiplayer, proxy, fortnite, cs2, dota2, open-source, udp, latency

### Load Test Results

| Node | Region | Packets Sent | Packets Recv | Loss | p50 | p95 | Throughput |
|------|--------|-------------|-------------|------|-----|-----|-----------|
| proxy-lax | US-West (LA) | 9,131 | 9,131 | **0.00%** | 214ms | 282ms | 129 pps |
| relay-sgp | Singapore | 22,742 | 22,742 | **0.00%** | 31ms | 35ms | 320 pps |

Estimated capacity: **500–1,000+ concurrent clients per node**. Free tier headroom: >99.9%.

### Fixed
- 20 Clippy lints across workspace
- 2 failing warp integration tests

---

## [0.2.0] — 2026-02-23

### 🚀 Beta Release — Live Infrastructure Verified

Major infrastructure deployment, real-world latency analysis, competitive feature development, and **live integration test passing on 2-node Vultr mesh**.

### Added

#### Forward Error Correction (FEC)
- **XOR-based FEC codec** — `protocol/src/fec.rs` with `FecEncoder`/`FecDecoder`
- Group K data packets → 1 parity packet for zero-retransmit loss recovery
- `FecHeader` extension (protocol v2): group_id, packet_index, group_size, parity flag
- `FecStats` tracking: encoded/decoded/recovered/lost counters
- 8 unit tests: encode/decode, single-loss recovery, multi-group, stats, edge cases
- Integrated FEC into live tunnel pipeline (`client/src/main.rs`)

#### Cloudflare WARP Integration
- **`client/src/warp.rs`** — `WarpManager` for automatic WARP detection and control
- Detects WARP CLI installation and connection state
- Auto-connect/disconnect with state restoration on shutdown (`Drop` impl)
- IP routing analysis: checks if traffic routes through WARP's excluded ranges
- Provides status, tunnel stats, and connection info
- **5-10ms latency improvement confirmed** (203ms → 193ms from Bangkok)
- Bypasses ISP HGC Singapore detour via Cloudflare NTT backbone

#### Game Integration — UDP Redirect Mode
- **`client/src/redirect.rs`** — `UdpRedirect` local UDP proxy
- Binds local port, intercepts game traffic, wraps in tunnel headers
- Forwards through proxy node, relays responses back to game client
- Supports per-game port configuration with automatic session management
- CLI: `--redirect` flag for redirect mode vs capture mode

#### Landing Page
- **`web/`** — GitHub Pages landing page with live benchmark data
- Real E2E performance results, FEC explanation, WARP integration info
- Deployed at https://shibbityshwab.github.io/lightspeed/

### Changed

#### Infrastructure — Vultr-Only Pivot
- **Dropped OCI San Jose** — switched to Vultr-only infrastructure
- 3-node mesh deployed: Vultr LA (primary) + Vultr SGP (relay) + OCI SJ (decommissioned)
- Native binary deployment: **~500KB RAM** per node (350x less than Docker)
- systemd service with sandboxing (DynamicUser, ProtectSystem, NoNewPrivileges)

#### Protocol
- **Protocol v2** — added FEC header extension (6 bytes: group_id, index, size, flags)
- Version field now supports v1 (plain) and v2 (FEC-enabled)
- `TunnelHeader::with_session_token()` builder pattern added
- `make_response()` method for proxy-side header swapping

### Infrastructure Deployed

| Node | IP | Region | Latency (BKK) | RAM | Status |
|------|----|--------|----------------|-----|--------|
| proxy-lax | [redacted] | us-west-lax | 206ms | 504KB | ✅ Active |
| relay-sgp | [redacted] | asia-sgp | 31ms | 496KB | ✅ Active |

### Research & Analysis

- **Relay strategy analysis**: SGP relay does NOT reduce latency (31ms + 178ms = 209ms > 206ms direct)
- **Pacific crossing bottleneck**: ~172ms submarine cable physics, not routing
- **ISP path analysis**: True Internet → SBN/AWN → HGC Singapore (29ms detour identified)
- **WARP analysis**: CF BKK PoP → NTT backbone bypasses HGC detour, 5-10ms net improvement
- **ExitLag gap**: 6ms remaining (193ms vs 187ms) — premium BGP transit peering

### Testing

- **Live integration test passing** (2026-02-23):
  - proxy-lax: 204.8ms, 10/10 keepalives, 0.3ms jitter ✅
  - relay-sgp: 34.0ms, 10/10 keepalives, 0.3ms jitter ✅
- E2E tunnel relay verified across all proxy nodes
- FEC module: 8 tests passing
- WARP IP routing logic: unit tested
- UDP redirect mode: tested with game traffic simulation

---

## [0.1.0] — 2026-02-22

### 🎉 Initial MVP Release

The first release of LightSpeed — a zero-cost, open-source global network optimizer for multiplayer games.

### Added

#### Client (`lightspeed`)
- **UDP Tunnel Engine** — async packet relay with Tokio, keepalive, stats, and timeout handling
- **Tunnel Header Protocol** — efficient 20-byte binary header with encode/decode, session tokens, sequence numbers
- **QUIC Control Plane** — proxy discovery, health checks, and control messaging via quinn
- **Game Profiles** — built-in configurations for Fortnite, CS2, and Dota 2 (port ranges, server IPs, anti-cheat info)
- **Route Selection Framework** — nearest-proxy selector, multipath config, failover logic
- **ML Route Prediction** — feature extraction (11 features), synthetic training data, Random Forest model via linfa, heuristic fallback
- **Packet Capture Abstraction** — cross-platform capture trait with platform-specific backends (Windows/Linux/macOS)
- **Configuration System** — TOML-based config with CLI overrides via clap

#### Proxy Server (`lightspeed-proxy`)
- **UDP Relay Loop** — high-performance session-based packet relay with concurrent client support
- **Session Management** — token-based sessions with automatic timeout and cleanup
- **Rate Limiting** — per-IP and per-session rate limiting with configurable thresholds
- **Abuse Detection** — destination validation, amplification prevention, private IP blocking
- **Authentication** — lightweight token-based client authentication
- **Metrics** — Prometheus-compatible metrics endpoint (connections, packets, bytes, latency)
- **Health Endpoint** — HTTP health check for monitoring and load balancing
- **QUIC Control Server** — control plane for client discovery and health probing

#### Protocol (`lightspeed-protocol`)
- **Binary Header Format** — 20-byte tunnel header: version, flags, sequence, timestamp, original IPs and ports
- **Control Messages** — Binary-encoded control protocol (Ping, Pong, Register, RegisterAck, Disconnect, ServerInfo)
- **Shared Types** — common types used by both client and proxy

#### Documentation
- Full architecture design (`docs/architecture.md`)
- Protocol specification (`docs/protocol.md`)
- Security audit report (`docs/security-audit-mvp.md`)
- Integration test report (`docs/test-report-mvp.md`)

#### Testing
- 52 tests total, 100% pass rate
- End-to-end tunnel lifecycle tests
- Concurrent client relay tests
- Security integration tests (spoofed tokens, rate limiting, abuse detection)
- Performance benchmarks (162μs tunnel overhead)

#### Security
- Token-based session authentication
- Per-IP and per-session rate limiting
- Destination validation (blocks private IPs, localhost, multicast)
- Amplification attack prevention
- No Critical or High findings in security audit

### Technical Details

- **Language**: Rust (2021 edition)
- **Async Runtime**: Tokio
- **Tunnel Protocol**: Custom 20-byte UDP header, unencrypted for transparency
- **Control Plane**: QUIC via quinn (feature-gated)
- **Target Overhead**: ≤5ms (achieved: 162μs average)
- **Supported Platforms**: Windows x64, Linux x64, Linux ARM64

[Unreleased]: https://github.com/ShibbityShwab/lightspeed/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.1.0
