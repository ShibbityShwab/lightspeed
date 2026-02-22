# Changelog

All notable changes to LightSpeed will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] — 2026-02-22

### 🎉 Initial MVP Release

The first release of LightSpeed — a zero-cost, open-source global network optimizer for multiplayer games.

### Added

#### Client (`lightspeed`)
- **UDP Tunnel Engine** — async packet relay with Tokio, keepalive, stats, and timeout handling
- **Tunnel Header Protocol** — efficient 24-byte binary header with encode/decode, session tokens, sequence numbers
- **QUIC Control Plane** — proxy discovery, health checks, and control messaging via quinn
- **Game Profiles** — built-in configurations for Fortnite, CS2, and Dota 2 (port ranges, server IPs, anti-cheat info)
- **Route Selection Framework** — nearest-proxy selector, multipath config, failover logic
- **ML Route Prediction Stubs** — feature extraction, model loading, and prediction interfaces for linfa integration
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
- **Binary Header Format** — 24-byte tunnel header: magic, version, flags, session token, sequence, timestamp, game ID, payload length
- **Control Messages** — Protobuf-defined control protocol (Ping, Pong, Auth, RouteRequest, RouteResponse, HealthCheck)
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
- **Tunnel Protocol**: Custom 24-byte UDP header, unencrypted for transparency
- **Control Plane**: QUIC via quinn (feature-gated)
- **Target Overhead**: ≤5ms (achieved: 162μs average)
- **Supported Platforms**: Windows x64, Linux x64, Linux ARM64

[0.1.0]: https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.1.0
