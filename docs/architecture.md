# ⚡ LightSpeed Architecture

> Last updated: 2026-02-23 — Reflects FEC, WARP, redirect mode, and Vultr mesh deployment

---

## System Overview

```
┌──────────────────────┐   UDP Tunnel    ┌──────────────────────┐   Direct UDP   ┌──────────────┐
│                      │ (20B header +   │                      │               │              │
│   Game Client PC     │  optional FEC)  │   Proxy Node         │──────────────▶│  Game Server  │
│   + LightSpeed       │────────────────▶│  (Vultr Cloud,       │               │  (Epic/Valve) │
│   Client (Rust)      │                 │   ~500KB RAM)        │◀──────────────│              │
│                      │◀────────────────│                      │               │              │
└──────────────────────┘                 └──────────────────────┘               └──────────────┘
     User PC                               Vultr VPS                           Game Infra

     QUIC Control ◄──────────────────────► QUIC Control
     (quinn, port 4433)                    (quinn, port 4433)
```

**Key Principles**:
- Data plane (game packets) uses raw UDP for minimum latency
- Control plane uses QUIC for reliability
- No encryption on the data plane — transparency is a feature, not a bug
- FEC adds loss recovery without retransmission (protocol v2)
- WARP integration provides free local route optimization

---

## Live Infrastructure (2-Node Mesh)

```
┌──────────────────────────────────────────────────────────────────┐
│                         Vultr Cloud                              │
│                                                                  │
│  ┌────────────────────┐          ┌────────────────────┐         │
│  │  proxy-lax          │          │  relay-sgp          │         │
│  │  149.28.84.139      │◄────────▶│  149.28.144.74      │         │
│  │  US-West (LA)       │  178ms   │  Asia (Singapore)   │         │
│  │                     │          │                     │         │
│  │  UDP :4434 (data)   │          │  UDP :4434 (data)   │         │
│  │  HTTP :8080 (health)│          │  HTTP :8080 (health)│         │
│  │  504KB RAM          │          │  496KB RAM          │         │
│  │                     │          │                     │         │
│  │  Role: Primary      │          │  Role: FEC multipath│         │
│  │  proxy for US games │          │  + SEA relay        │         │
│  └────────────────────┘          └────────────────────┘         │
│                                                                  │
│  Deployment: Native binary + systemd (no Docker overhead)        │
└──────────────────────────────────────────────────────────────────┘

User (Bangkok) ─── 206ms direct ──▶ proxy-lax
User (Bangkok) ─── 31ms ──────────▶ relay-sgp ─── 178ms ──▶ proxy-lax
User (Bangkok) + WARP ── 193ms ──▶ proxy-lax  (5-10ms improvement via CF NTT backbone)
```

---

## Module Dependency Graph

### Client (`lightspeed-client`)

```
main.rs ──────────────────────────── CLI, orchestration, test modes
  ├── config.rs                      ← TOML configuration management
  ├── error.rs                       ← Centralized error types (thiserror)
  ├── warp.rs                        ← Cloudflare WARP detection + management
  ├── redirect.rs                    ← UDP redirect proxy (game integration)
  │
  ├── tunnel/                        ← Core UDP tunnel engine
  │   ├── mod.rs                     ← TunnelEngineState, TunnelStats, TunnelPacket
  │   ├── header.rs                  ← Re-exports lightspeed_protocol header
  │   ├── capture.rs                 ← CapturedPacket, CaptureFilter, PacketCapture trait
  │   └── relay.rs                   ← UdpRelay: async send/recv, keepalive, RTT measurement
  │
  ├── capture/                       ← Platform-specific packet capture
  │   ├── mod.rs                     ← Factory: create_default_capture(), list_interfaces()
  │   ├── pcap_backend.rs            ← Cross-platform pcap (feature: pcap-capture)
  │   ├── windows.rs                 ← WFP backend (planned)
  │   ├── linux.rs                   ← AF_PACKET backend (planned)
  │   └── macos.rs                   ← BPF backend (planned)
  │
  ├── route/                         ← Proxy selection & failover
  │   ├── mod.rs                     ← ProxyNode, RouteSelector trait, ProxyHealth
  │   ├── selector.rs                ← NearestSelector (latency), MlSelector (Random Forest)
  │   ├── multipath.rs               ← Multi-path routing config
  │   └── failover.rs                ← Automatic failover state machine
  │
  ├── quic/                          ← QUIC control plane (feature: quic)
  │   ├── mod.rs                     ← ControlClient (register, ping, disconnect)
  │   ├── discovery.rs               ← ProxyDiscovery (static, DNS, peer exchange)
  │   └── health.rs                  ← HealthChecker, periodic proxy probing
  │
  ├── ml/                            ← ML route prediction
  │   ├── mod.rs                     ← RouteModel wrapper (load/save/predict/train)
  │   ├── data.rs                    ← Synthetic training data generation
  │   ├── features.rs                ← NetworkFeatures (11 inputs), LatencyTracker
  │   ├── predict.rs                 ← Inference: linfa RF or heuristic fallback
  │   └── trainer.rs                 ← Model training pipeline (Random Forest)
  │
  └── games/                         ← Game-specific configuration
      ├── mod.rs                     ← GameConfig trait, detect_game(), auto_detect()
      ├── fortnite.rs                ← Fortnite: EAC, ports 7000-9000
      ├── cs2.rs                     ← CS2: VAC, ports 27015-27050, SDR
      └── dota2.rs                   ← Dota 2: VAC, ports 27015-27050, SDR
```

### Proxy (`lightspeed-proxy`)

```
main.rs ──────────────────────────── CLI, task spawning, graceful shutdown
  ├── config.rs                      ← ProxyConfig (server, security, rate_limit, metrics)
  ├── relay.rs                       ← RelayEngine: session mgmt, packet forward, FEC decode
  ├── auth.rs                        ← Authenticator: per-client token auth
  ├── metrics.rs                     ← ProxyMetrics: atomic counters, Prometheus export
  ├── health.rs                      ← HealthResponse: HTTP /health endpoint (JSON)
  ├── rate_limit.rs                  ← RateLimiter: per-client PPS/BPS limits
  ├── abuse.rs                       ← AbuseDetector: amplification/reflection, private IP block
  └── control.rs                     ← QUIC control server (feature-gated)
```

### Protocol (`lightspeed-protocol`)

```
lib.rs ──────────────────────────── Crate root, re-exports
  ├── header.rs                      ← TunnelHeader: 20-byte binary, v1/v2, encode/decode
  ├── control.rs                     ← ControlMessage: binary QUIC messages
  └── fec.rs                         ← FecEncoder/FecDecoder: XOR parity, FecHeader, FecStats
```

---

## Data Flow

### Outbound (Client → Game Server) — Redirect Mode

```
1. Game client configured to connect to localhost:LOCAL_PORT
2. [redirect]  UdpRedirect receives game packet on local socket
3. [tunnel]    TunnelHeader created (20 bytes: version, seq, timestamp, orig addrs)
4. [fec]       If FEC enabled: FecEncoder groups K packets, generates parity
5. [relay]     Header + payload sent to proxy via UDP (port 4434)
6. [proxy]     Proxy receives, strips header, validates session
7. [proxy]     Original UDP packet forwarded to game server
8. [game]      Game server sees user's ORIGINAL IP (preserved!)
```

### Outbound (Client → Game Server) — Capture Mode

```
1. Game sends UDP to game server IP:port normally
2. [capture]   pcap/WFP intercepts packet on network interface
3. [capture]   Packet parsed → CapturedPacket { src, dst, payload }
4. [route]     RouteSelector picks optimal proxy (Nearest or ML)
5. [tunnel]    TunnelHeader created (20 bytes)
6. [fec]       If FEC enabled: FecEncoder adds parity packets
7. [relay]     Header + payload sent to proxy via UDP (port 4434)
8-9. Same as redirect mode
```

### Inbound (Game Server → Client)

```
1. Game server sends UDP response to proxy (via session mapping)
2. [proxy]   Proxy wraps response in TunnelHeader
3. [proxy]   Wrapped packet sent back to client
4. [relay]   Client receives wrapped packet
5. [fec]     If FEC enabled: FecDecoder reassembles, recovers any lost packets
6. [tunnel]  Header stripped, latency measured from timestamp
7. [deliver] Original response delivered to game client
```

### FEC Data Flow (Forward Error Correction)

```
Sender (Client):
  Packet 1 ─┐
  Packet 2 ─┤  XOR together
  Packet 3 ─┤  ──────────▶  Parity Packet P
  Packet 4 ─┘

  Send: [P1, P2, P3, P4, P_parity]  (K+1 packets per group)

Receiver (Proxy / Client):
  Received: [P1, __, P3, P4, P_parity]  (P2 lost!)
  Recovery: P2 = P1 ⊕ P3 ⊕ P4 ⊕ P_parity  (XOR recovery in ~3ms)

  Without FEC: P2 lost → game retransmits → +400ms penalty
  With FEC:    P2 recovered instantly → no visible impact to gameplay
```

### WARP Integration Flow

```
Without WARP:
  User → True ISP → SBN/AWN SGP → HGC SGP (+29ms detour!) → Pacific → Vultr LA
  Total: ~203-206ms

With WARP:
  User → CF BKK PoP (4ms) → CF backbone → NTT (36ms) → Pacific (166ms) → Vultr LA
  Total: ~193-197ms  (bypasses HGC detour via Cloudflare NTT peering)
```

### Control Plane (Parallel)

```
[quic/health]     Periodic health checks → update ProxyHealth
[quic/discovery]  Discover new proxy nodes → update proxy list
[route/failover]  Monitor keepalive → trigger failover if needed
[ml/predict]      Collect latency feedback → update route predictions
[warp]            Detect/manage WARP state → optimize local routing
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
| **rand** | RNG | Used in FEC, ML synthetic data, session tokens |

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
- **Capture**: Npcap via pcap crate (MVP), WFP native (planned)
- **Redirect**: Local UDP proxy — no admin privileges needed
- **Anti-cheat**: Must not trigger EAC/VAC. pcap is passive capture, not modification
- **Admin**: Packet capture requires Administrator privileges; redirect mode does not
- **Binary**: Single `.exe`, no runtime dependencies (except Npcap for capture)
- **WARP**: Cloudflare WARP CLI auto-detected if installed

### Linux
- **Capture**: libpcap (MVP), AF_PACKET with PACKET_MMAP (planned)
- **Proxy**: Primary proxy deployment target (Vultr Ubuntu)
- **Permissions**: Requires `CAP_NET_RAW` capability or root for capture; redirect mode unprivileged
- **Deployment**: Native binary + systemd, ~500KB RAM

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
| **FEC** | XOR parity | Simple, low overhead, recovers single packet loss per group |
| **WARP** | Optional integration | Free 5-10ms improvement, auto-detected |
| **Infra Provider** | Vultr | $300 free credit, good Asia peering, native deployment |
| **No Docker** | Native binary | 350x less RAM (500KB vs 175MB), simpler systemd |
| **Redirect Mode** | Local UDP proxy | No admin/root needed, game connects to localhost |
