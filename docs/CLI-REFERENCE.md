# CLI Reference

> `lightspeed` - Reduce your ping. Free. Forever.

## Quick Start

```bash
lightspeed --game rust --proxy YOUR_PROXY_IP:4434
```

## All Flags

### Core

| Flag | Description |
|------|-------------|
| `-c, --config <PATH>` | Path to config file (default: `lightspeed.toml`) |
| `-g, --game <GAME>` | Game to optimize: `fortnite`, `cs2`, `dota2`, `rust`, `apex`, `valorant`, `ow2`, `lol`, `pubg`, `maplestory`, `genshin`, `rocketleague`, `wot`, `deadbydaylight`, `bodycam`, `csgo`, `roblox`, `zomboid`, `wardogs` (run `--list-games` for the full list) |
| `-p, --proxy <ADDR>` | Proxy server address (`host:port`). Auto-selects from config if omitted |
| `-v, --verbose` | Enable verbose logging |

### Operation Modes

| Flag | Description |
|------|-------------|
| `--dry-run` | Show what would happen without capturing packets |
| `--tunnel-test` | Send test packets to verify proxy connectivity |
| `--quic-test` | Test QUIC control plane (connect, register, ping, disconnect) |
| `--live-test` | Run comprehensive integration test against configured proxies |
| `--echo-server <ADDR>` | Echo server for `--live-test` data relay and FEC phases |
| `--demo` | Interactive demonstration of architecture and latency projections |
| `--smoke-test` | Full E2E smoke test (starts echo server + interceptor, needs root) |
| `--diagnose` | Measure one server's direct vs relayed path and print an honest saved/not-helped verdict, then exit. Uses `--target` (or the running game's server). |

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

### Transport

| Flag | Description |
|------|-------------|
| `--tcp` | Use TCP for the client→proxy leg (for networks that block or throttle UDP). The proxy must have its TCP listener enabled (on by default). |
| `--dscp` | Mark tunnel packets with DSCP EF (46) so the local router/ISP may prioritise them. Off by default; only helps on the player's local hop and is ignored on Windows. Also configurable as `dscp = true` under `[tunnel]`. |

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
| `--write-config` | Write a documented `lightspeed.toml` to the current directory |
| `--check` | Run environment checks (nftables, proxy, game detection) |
| `--status` | Show detailed system state (OS, interceptor, games, nftables) |
| `--benchmark` | Run latency benchmark (direct vs LightSpeed routing) |
| `--target <ADDR>` | Target server for `--benchmark` and `--diagnose` (`ip:port`) |
| `--scan-processes` | Scan for running game processes |
| `--registry <URL>` | Community registry URL (signed node list) for relay discovery; with `--probe-proxies` its nodes are probed too |

### Telemetry

| Flag | Description |
|------|-------------|
| `--telemetry` | Force telemetry on even if the config disables it (it is on by default) |
| `--no-telemetry` | Disable telemetry (hard override, always wins) |

Telemetry is **on by default** since v1.6.5. It sends anonymous aggregate metrics (p50/p95/p99 RTT, jitter, FEC counters, and the direct/relayed/saved latency numbers) to your own relay every 15 minutes. No IP address, identifier, or packet content is sent, and cells with fewer than 3 reports are suppressed. Disable it with `--no-telemetry` or `telemetry = false` under `[general]` in `lightspeed.toml`. See [Privacy](privacy.md).

## Examples

```bash
# Basic usage
lightspeed --game fortnite --proxy proxy.example.com:4434

# With FEC for packet loss recovery
lightspeed --game cs2 --proxy proxy.example.com:4434 --fec

# Probe all configured proxies
lightspeed --probe-proxies

# Live test against a proxy
lightspeed --live-test --echo-server proxy.example.com:9999

# Run the demo
lightspeed --demo

# Environment check
lightspeed --check

# Write a documented config
lightspeed --write-config

# Watch for game and auto-intercept
sudo lightspeed --watch --game rust --proxy proxy.example.com:4434

# Benchmark latency
lightspeed --benchmark --target game-server.example.com:27015 --proxy proxy.example.com:4434

# TCP tunnel for UDP-restricted networks
lightspeed --game rust --proxy proxy.example.com:4434 --tcp
```

## Latency diagnosis (`--diagnose`)

`lightspeed --diagnose --target <ip:port>` answers the only question that
matters for your connection: is the relay actually helping?

It measures two things over a short bounded window (about five seconds):

- **Direct (ICMP) RTT** to the server IP, using the same burst the client
  already runs for its "RTT saved" metric.
- **Relayed (tunnelled) RTT**, by sending probes through your relay to the
  server and timing the replies, using the same measurement code as a live
  session.

It then prints both medians, their difference, and one line you can act on:
`LightSpeed is saving you about N ms here`, or `the relay is not helping on
this path; connect directly`.

It is honest by design:

- The server comes from `--target <ip:port>`, or `-s/--game-server`, or the
  active server route of the running game (`--game <name>` or auto-detect).
- It is strictly local. It never builds or sends a telemetry report, whether or
  not telemetry is enabled. The probes go only to your relay and the target
  server.
- It is bounded to a few seconds and always exits.
- If either path gets fewer than three replies it says `not enough samples`
  instead of guessing. A game server often will not answer a probe, so the
  relayed number may be unavailable; point `--target` at a host that echoes
  (for example the `tools/echo_server.py` used by `--benchmark`).
- The direct figure is ICMP and the relayed figure is a tunnelled application
  probe. They are different instruments, so read the difference as an estimate.
- Reading ICMP needs no root on Linux and macOS (unprivileged datagram
  sockets). On Windows it needs an elevated session (the client already runs
  elevated for WinDivert). Relayed probing needs a registered session token,
  which the mode obtains automatically.

Examples:

```bash
# Diagnose against a specific game server
lightspeed --diagnose --target 203.0.113.7:28015

# Diagnose the running game's active server
lightspeed --diagnose --game rust

# Diagnose against an echo host for a like-for-like relayed comparison
lightspeed --diagnose --target echo.example.com:9999 --proxy proxy.example.com:4434
```

## Proxy Configuration

The proxy reads `proxy.toml`. Its ports are configurable via a `[network]` section:

```toml
[network]
data_port    = 4434    # UDP + TCP data plane
control_port = 4433    # QUIC control plane
health_port  = 8080    # HTTP health/metrics

# TCP tunnel (client→proxy over TCP)
tcp_enabled = true
tcp_max_connections = 256
```

> On the **proxy** side the QUIC control-plane port is `[network] control_port`. On the **client** side the matching setting is `[proxy] quic_port`. Both default to 4433 and must agree.

See [Deploy Proxy](deploy-proxy.md) for the full reference.

## See Also

- [User Guide](user-guide.md) - step-by-step setup
- [Troubleshooting](troubleshooting.md) - common issues
- [Deploy Proxy](deploy-proxy.md) - run your own proxy
