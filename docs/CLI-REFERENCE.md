# CLI Reference

> `lightspeed` — Reduce your ping. Free. Forever.

## Quick Start

```bash
lightspeed --game rust --proxy YOUR_PROXY_IP:4434
```

## All Flags

### Core

| Flag | Description |
|------|-------------|
| `-c, --config <PATH>` | Path to config file (default: `lightspeed.toml`) |
| `-g, --game <GAME>` | Game to optimize: `fortnite`, `cs2`, `dota2`, `rust`, `apex`, `valorant`, `ow2`, `lol`, `pubg` |
| `-p, --proxy <ADDR>` | Proxy server address (`host:port`). Auto-selects from config if omitted |
| `-v, --verbose` | Enable verbose logging |

### Operation Modes

| Flag | Description |
|------|-------------|
| `--dry-run` | Show what would happen without capturing packets |
| `--tunnel-test` | Send test packets to verify proxy connectivity |
| `--quic-test` | Test QUIC control plane (connect, register, ping, disconnect) |
| `--live-test` | Run comprehensive integration test against configured proxies |
| `--echo-addr <ADDR>` | Echo server for `--live-test` data relay phase |
| `--demo` | Interactive demonstration of architecture and latency projections |
| `--smoke-test` | Full E2E smoke test (starts echo server + interceptor, needs root) |

### Redirect Mode

| Flag | Description |
|------|-------------|
| `-s, --server <ADDR>` | Game server address (`ip:port`) for redirect mode |
| `--local-port <PORT>` | Local port for redirect mode (default: same as game server) |

### Interceptor

| Flag | Description |
|------|-------------|
| `--start-interceptor` | Start the TrafficInterceptor for live MITM |
| `--watch` | Watch for game process and auto-start interceptor |
| `--server-addr <ADDR>` | Proxy address for interceptor mode |

### Capture Mode

| Flag | Description |
|------|-------------|
| `--capture` | Enable pcap capture mode (needs `pcap-capture` feature + elevated privileges) |
| `--interface <IFACE>` | Network interface for capture (e.g. `eth0`) |

### FEC (Forward Error Correction)

| Flag | Description |
|------|-------------|
| `--fec` | Enable FEC for packet loss recovery (~25% bandwidth overhead) |
| `--fec-k <K>` | FEC block size: data packets per parity packet (2-16, default: 4) |

### Routing & WARP

| Flag | Description |
|------|-------------|
| `--route <STRATEGY>` | Route selection: `nearest` or `ml` |
| `-w, --warp` | Enable Cloudflare WARP for improved routing |
| `--no-warp` | Disable WARP even if previously enabled |
| `--warp-status` | Show WARP status and exit |
| `--probe-proxies` | Probe all configured proxies and display latencies |

### Diagnostics

| Flag | Description |
|------|-------------|
| `--list-interfaces` | List available network interfaces for capture |
| `--list-games` | List supported games with default ports |
| `--write-default-config` | Write a default `lightspeed.toml` to current directory |
| `--check` | Run environment checks (nftables, proxy, game detection) |
| `--status` | Show detailed system state (OS, interceptor, games, nftables) |
| `--benchmark` | Run latency benchmark (direct vs LightSpeed routing) |
| `--target <ADDR>` | Target server for `--benchmark` (`ip:port`) |
| `--scan-processes` | Scan for running game processes |

## Examples

```bash
# Basic usage
lightspeed --game fortnite --proxy proxy.example.com:4434

# With FEC for packet loss recovery
lightspeed --game cs2 --proxy proxy.example.com:4434 --fec

# Probe all configured proxies
lightspeed --probe-proxies

# Live test against a proxy
lightspeed --live-test --echo-addr proxy.example.com:9999

# Run the demo
lightspeed --demo

# Environment check
lightspeed --check

# Write a default config
lightspeed --write-default-config

# Watch for game and auto-intercept
sudo lightspeed --watch --game rust --proxy proxy.example.com:4434

# Benchmark latency
lightspeed --benchmark --target game-server.example.com:27015 --proxy proxy.example.com:4434
```

## See Also

- [User Guide](user-guide.md) — step-by-step setup
- [Troubleshooting](troubleshooting.md) — common issues
- [Deploy Proxy](deploy-proxy.md) — run your own proxy
