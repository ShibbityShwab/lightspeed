# ⚡ LightSpeed Architecture

> WF-001 Step 1 Deliverable — Client & Proxy Architecture Design
> Agent: Architect | Date: 2026-02-21

---

## System Overview

```
┌──────────────────────┐   UDP Tunnel    ┌──────────────────────┐   Direct UDP   ┌──────────────┐
│                      │  (Unencrypted)  │                      │               │              │
│   Game Client PC     │────────────────▶│   Proxy Node         │──────────────▶│  Game Server  │
│   + LightSpeed       │                 │  (Oracle Cloud       │               │  (Epic/Valve) │
│   Client (Rust)      │◀────────────────│   Always Free)       │◀──────────────│              │
│                      │                 │                      │               │              │
└──────────────────────┘                 └──────────────────────┘               └──────────────┘
     User PC                               Free Tier VPS                       Game Infra

     QUIC Control ◄──────────────────────► QUIC Control
     (quinn, port 4433)                    (quinn, port 4433)
```

**Key Principle**: Data plane (game packets) uses raw UDP for minimum latency. Control plane uses QUIC for reliability. No encryption on the data plane — transparency is a feature, not a bug.

---

## Module Dependency Graph

### Client (`lightspeed-client`)

```
main.rs
  ├── config.rs           ← TOML configuration management
  ├── error.rs            ← Centralized error types
  │
  ├── tunnel/             ← Core UDP tunnel engine
  │   ├── mod.rs          ← TunnelEngineState, TunnelStats
  │   ├── header.rs       ← Protocol header encode/decode (20 bytes)
  │   ├── capture.rs      ← CapturedPacket, CaptureFilter, PacketCapture trait
  │   └── relay.rs        ← UdpRelay: async send/recv via tokio
  │
  ├── capture/            ← Platform-specific packet capture
  │   ├── mod.rs          ← Factory: create_default_capture()
  │   ├── pcap_backend.rs ← Cross-platform pcap (feature: pcap-capture)
  │   ├── windows.rs      ← WFP backend (Phase 2)
  │   ├── linux.rs        ← AF_PACKET backend (Phase 2)
  │   └── macos.rs        ← BPF backend (Phase 2)
  │
  ├── route/              ← Proxy selection & failover
  │   ├── mod.rs          ← ProxyNode, RouteSelector trait, ProxyHealth
  │   ├── selector.rs     ← NearestSelector (MVP), MlSelector (Phase 2)
  │   ├── multipath.rs    ← Multi-path routing (Phase 2)
  │   └── failover.rs     ← Automatic failover state machine
  │
  ├── quic/               ← QUIC control plane (feature: quic)
  │   ├── mod.rs          ← ControlClient
  │   ├── discovery.rs    ← ProxyDiscovery (static, DNS, peer exchange)
  │   └── health.rs       ← HealthChecker, periodic proxy probing
  │
  ├── ml/                 ← ML route prediction (feature: ml)
  │   ├── mod.rs          ← RouteModel wrapper
  │   ├── predict.rs      ← Real-time inference (< 1ms target)
  │   └── features.rs     ← NetworkFeatures: 11 input features
  │
  └── games/              ← Game-specific configuration
      ├── mod.rs          ← GameConfig trait, detect_game(), auto_detect()
      ├── fortnite.rs     ← Fortnite: EAC, ephemeral ports
      ├── cs2.rs          ← CS2: VAC, ports 27015-27050, SDR
      └── dota2.rs        ← Dota 2: VAC, ports 27015-27050, SDR
```

### Proxy (`lightspeed-proxy`)

```
main.rs
  ├── config.rs           ← ProxyConfig, RateLimitConfig, MetricsConfig
  ├── relay.rs            ← RelayEngine: session management, packet forwarding
  ├── auth.rs             ← Authenticator: per-client authorization
  ├── metrics.rs          ← ProxyMetrics: atomic counters, Prometheus export
  ├── health.rs           ← HealthResponse: HTTP health check endpoint
  ├── rate_limit.rs       ← RateLimiter: per-client PPS/BPS limits
  └── abuse.rs            ← AbuseDetector: amplification/reflection detection
```

---

## Data Flow

### Outbound (Client → Game Server)

```
1. Game sends UDP to game server IP:port
2. [capture] pcap/WFP intercepts packet on network interface
3. [capture] Packet parsed → CapturedPacket { src, dst, payload }
4. [route]   RouteSelector picks optimal proxy (NearestSelector for MVP)
5. [tunnel]  TunnelHeader created (20 bytes: version, seq, timestamp, orig addrs)
6. [relay]   Header + payload sent to proxy via UDP (port 4434)
7. [proxy]   Proxy receives, strips header, validates auth
8. [proxy]   Original UDP packet forwarded to game server
9. [game]    Game server sees user's ORIGINAL IP (preserved!)
```

### Inbound (Game Server → Client)

```
1. Game server sends UDP response to user's IP
2. [proxy]   Response arrives at proxy (via routing/NAT)
3. [proxy]   Proxy wraps response in TunnelHeader
4. [proxy]   Wrapped packet sent back to client
5. [relay]   Client receives wrapped packet
6. [tunnel]  Header stripped, latency measured from timestamp
7. [deliver] Original response delivered to game client
```

### Control Plane (Parallel)

```
[quic/health]     Periodic health checks → update ProxyHealth
[quic/discovery]  Discover new proxy nodes → update proxy list
[route/failover]  Monitor keepalive → trigger failover if needed
[ml/predict]      Collect latency feedback → update route predictions
```

---

## Interface Definitions (Rust Traits)

### `PacketCapture` — Platform Packet Capture
```rust
pub trait PacketCapture: Send + Sync {
    fn start(&mut self, filter: &CaptureFilter) -> Result<(), CaptureError>;
    fn stop(&mut self) -> Result<(), CaptureError>;
    fn next_packet(&mut self) -> Result<CapturedPacket, CaptureError>;
    fn is_active(&self) -> bool;
}
```

### `RouteSelector` — Proxy Selection
```rust
pub trait RouteSelector: Send + Sync {
    fn select(&self, game_server: SocketAddrV4, proxies: &[ProxyNode])
        -> Result<SelectedRoute, RouteError>;
    fn feedback(&mut self, proxy_id: &str, observed_latency_us: u64);
    fn strategy(&self) -> RouteStrategy;
}
```

### `GameConfig` — Per-Game Settings
```rust
pub trait GameConfig: Send + Sync {
    fn name(&self) -> &str;
    fn process_names(&self) -> &[&str];
    fn ports(&self) -> (u16, u16);
    fn anti_cheat(&self) -> &str;
    fn typical_pps(&self) -> u32;
    fn packet_size_range(&self) -> (usize, usize);
}
```

---

## Crate Selection Rationale

| Crate | Purpose | Why This One |
|-------|---------|-------------|
| **tokio** | Async runtime | Industry standard, full-featured, excellent for networking |
| **bytes** | Zero-copy buffers | Efficient packet handling without allocation |
| **pcap** | Packet capture | Cross-platform libpcap binding, well-maintained |
| **quinn** | QUIC client/server | Pure Rust, built on rustls, active development |
| **linfa** | ML toolkit | Native Rust ML — no Python dependency, fast inference |
| **clap** | CLI parsing | Derive-based, excellent UX, industry standard |
| **tracing** | Structured logging | Async-aware, spans for latency tracking, filterable |
| **serde/toml** | Config parsing | Standard serialization, human-readable config format |
| **thiserror** | Error types | Ergonomic custom error enums |
| **anyhow** | Error handling | Flexible error propagation for application code |
| **socket2** | Advanced sockets | Low-level socket options for UDP performance tuning |
| **prometheus** | Metrics | Standard metrics format, compatible with free monitoring |

### Feature Gating

Heavy dependencies requiring a C compiler are behind cargo features:
- `pcap-capture` — requires Npcap (Windows) or libpcap (Linux)
- `quic` — requires ring (via rustls) → needs C compiler
- `ml` — requires linfa ecosystem → may need BLAS
- `full` — enables all of the above

Default build (no features) compiles with just the Rust toolchain.

---

## Platform Considerations

### Windows (Primary Target)
- **Capture**: Npcap via pcap crate (MVP), WFP native (Phase 2)
- **Anti-cheat**: Must not trigger EAC/VAC. pcap is passive capture, not modification
- **Admin**: Packet capture requires Administrator privileges
- **Binary**: Single `.exe`, no runtime dependencies (except Npcap for capture)

### Linux
- **Capture**: libpcap (MVP), AF_PACKET with PACKET_MMAP (Phase 2)
- **Proxy**: Primary proxy deployment target (Oracle Cloud ARM/Ubuntu)
- **Permissions**: Requires `CAP_NET_RAW` capability or root

### macOS
- **Capture**: libpcap via pcap crate
- **Permissions**: Requires root or BPF group membership
- **Priority**: Lower priority, but supported via cross-platform pcap

---

## Decisions Made

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Language** | Rust | Zero-cost abstractions, memory safety, excellent async |
| **Async Runtime** | Tokio | Most mature, best ecosystem support |
| **Protocol** | Custom UDP + QUIC control | Minimum overhead on data path |
| **No Encryption** | Deliberate | Anti-cheat friendly, no overhead, IP preservation |
| **Feature Gating** | Cargo features | Allow compilation without C compiler for dev |
| **IP Preservation** | Header carries original IP | Game servers see real user IP |
| **ML Framework** | linfa | Native Rust, no Python, fast inference |
| **Config Format** | TOML | Human-readable, Rust ecosystem standard |

---

## Next Steps

| Step | Agent | Deliverable |
|------|-------|-------------|
| WF-001 Step 2a | RustDev | Implement tunnel engine (pcap capture + UDP relay) |
| WF-001 Step 2b | NetEng | Finalize protocol specification |
| WF-001 Step 3 | RustDev | Implement proxy server relay loop |
| WF-001 Step 4 | RustDev | QUIC control plane (quinn integration) |
