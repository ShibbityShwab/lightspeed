# Deploy a LightSpeed Proxy

> Deploy your own zero-cost proxy relay on any Linux VPS with a free tier. You own the node, you run the node. There is no shared "LightSpeed network". This is self-hosting, full stop.

## Overview

A **proxy node** is a small relay server that sits between your game client and the game server. Your client tunnels game packets to the node over UDP, and the node forwards them toward the game server over its own high-speed backbone connection. The result is a shorter, less congested path than your ISP's default route, which usually means lower and more stable ping.

The model is **self-hosted**: you provision a VPS, install the proxy binary, and point your client at it. Nobody else uses your node, and you don't rely on anyone else's. Run one node for yourself, or spin up a few in different regions and let the client pick the fastest automatically (see [Multi-Node Mesh](#multi-node-mesh)).

A single node is tiny, roughly **~500KB of actual RAM**, so it fits comfortably inside any free-tier VPS.

The full deployment options (native binary, Docker, automated script) are documented in depth in **[`infra/README.md`](../infra/README.md)**. This guide gives you the essentials and the operational details you need day to day. Don't duplicate what infra already covers; follow the links.

### What the node exposes

| Port | Protocol | Purpose |
|------|----------|---------|
| `4434` | UDP + TCP | Data plane: relayed game traffic (UDP, or TCP via the `--tcp` tunnel) |
| `4433` | QUIC | Control plane: client registration and auth |
| `8080` | HTTP | Health check and Prometheus metrics |

All three ports are configurable via the `[network]` section — see [Configurable ports](#configurable-ports).

## Prerequisites

Before you start, you need:

- **A Linux VPS.** Any provider with a free tier works: Vultr, Hetzner, Oracle Cloud (OCI), AWS Lightsail, DigitalOcean, Linode. A single small instance is plenty.
- **SSH access** to the VPS.
- **Open firewall ports.** Allow inbound UDP `4434` (data) and TCP `4434` (TCP tunnel), UDP `4433` (QUIC control), and TCP `8080` (health/metrics). Ports are configurable via the `[network]` section — see [Configurable ports](#configurable-ports). The exact commands depend on your provider's firewall and `ufw`/`firewalld` on the box.
- **A way to run the proxy.** Either the Rust toolchain to build from source, or Docker. See the options below.

## Quick Deployment

There are three supported ways to get a node running. Full, step-by-step instructions for each live in **[`infra/README.md`](../infra/README.md)**. Here's the short version.

### Option A: Native binary + systemd (recommended)

Build the proxy, copy it to the VPS, install a systemd unit, and enable it:

```bash
# Build locally
cargo build --release -p lightspeed-proxy

# Copy binary and config to the VPS
scp target/release/lightspeed-proxy root@YOUR_VPS_IP:/usr/local/bin/
scp proxy/proxy.toml.default root@YOUR_VPS_IP:/etc/lightspeed/proxy.toml

# Install and start the systemd service (see infra/README.md for the full unit)
ssh root@YOUR_VPS_IP 'systemctl daemon-reload && systemctl enable --now lightspeed-proxy'
```

This is the recommended path: ~500KB RAM, sandboxed with `DynamicUser`, `ProtectSystem=strict`, and `NoNewPrivileges`.

### Option B: Docker / GHCR

```bash
# Pull from GitHub Container Registry
docker pull ghcr.io/shibbityshwab/lightspeed-proxy:latest
docker run -d --name ls-proxy \
  -p 4434:4434/udp -p 4434:4434/tcp -p 4433:4433/udp -p 8080:8080 \
  ghcr.io/shibbityshwab/lightspeed-proxy:latest

# Or build from source
docker build -f infra/docker/Dockerfile -t lightspeed-proxy .
docker run -d --name ls-proxy \
  -p 4434:4434/udp -p 4434:4434/tcp -p 4433:4433/udp -p 8080:8080 \
  lightspeed-proxy
```

Note the control plane port `4433` is published alongside the data plane, and the data port is published for both UDP and TCP (the TCP listener supports the `--tcp` tunnel). The `infra/README.md` Docker examples predate the control plane; make sure you publish all ports so registration and the TCP tunnel work.

### Option C: Automated deploy script

Configure your nodes and run the deploy script:

```bash
export LIGHTSPEED_NODES='{"proxy-1":{"ip":"1.2.3.4"},"proxy-2":{"ip":"5.6.7.8"}}'
./infra/scripts/deploy.sh
```

## Security & Authentication

The proxy has a QUIC control plane (port `4433`) that gates the data plane. As of
**v1.1.0**, **`require_auth` defaults to `true`**: the proxy will not relay data-plane
packets from a client until that client has registered over the QUIC control plane.

### How it works

The control plane is **QUIC with mTLS**. The flow is:

1. A client connects to the QUIC control port (`4433`) and sends a `Register` message.
2. The proxy checks capacity, generates a random `session_token`, and records the client's IP.
3. The proxy replies with a `RegisterAck` carrying the `session_token`.
4. The client stamps that token into **every** data-plane packet header.
5. The proxy validates an **(IP, token)** pair on every data-plane packet, dropping any that do not match.

This raises the bar for abuse: a random UDP sender cannot relay through your node without
first completing a QUIC registration. The token is 8-bit (defense-in-depth alongside the
IP binding); the enforcement path is covered by `proxy/tests/integration_security.rs`.

### Requirements for token auth to work

- The **proxy** must be built with the `quic` feature (the Docker image and release
  binaries already are).
- The **client** must be built with the `quic` feature so it can register (the release
  binaries already are). A client built without `quic` sends token `0` and will be
  rejected by an authenticated proxy — build with `cargo build --features quic`.

### Setting the config

```toml
[security]
require_auth = true   # default
```

To disable auth (dev mode only, **not** recommended for a public node), set it to `false`.

The proxy prints the auth state at startup, so you can confirm the setting took effect:

```text
Auth enabled: true
```

## Configurable ports

All three ports are configurable via a `[network]` section in `proxy.toml`:

```toml
[network]
data_port     = 4434    # UDP + TCP data plane
control_port  = 4433    # QUIC control plane
health_port   = 8080    # HTTP health/metrics

# TCP tunnel (client→proxy over TCP)
tcp_enabled           = true
tcp_max_connections   = 256
tcp_read_timeout_secs = 10
```

The `--data-bind`, `--control-bind`, and `--health-bind` CLI flags override the config
(e.g. to bind a specific interface instead of `0.0.0.0`).

## TCP tunnel

For networks that block or throttle UDP, the client↔proxy leg can run over TCP instead.
On the client, pass `--tcp` (or set `tunnel.transport = "tcp"` in the client config); the
proxy accepts both UDP and TCP on its data port by default. The same auth, rate-limit,
abuse, and FEC pipeline applies to TCP traffic — it is not a weaker path. The TCP tunnel
uses length-prefixed framing with a hard frame-size cap and connection limits.

## Multi-Node Mesh

For best results, deploy 2-3 nodes in different regions. The client probes every configured proxy on startup and auto-selects the fastest route, so more regions means a better chance of a low-latency path to your game server.

Typical RTT from each region to Bangkok:

| Region | Typical RTT to Bangkok |
|--------|----------------------|
| Singapore | ~31 ms |
| Tokyo | ~85 ms |
| Los Angeles | ~206 ms |
| Frankfurt | ~170 ms |
| Sydney | ~115 ms |

Add all your proxies to the client config:

```toml
[proxy]
servers = [
    "YOUR_SGP_IP:4434",
    "YOUR_LAX_IP:4434",
]
```

The client probes all proxies on startup and auto-selects the fastest route.

## Monitoring

Every node exposes two HTTP endpoints on port `8080`:

```bash
# Health check
curl http://YOUR_PROXY_IP:8080/health

# Prometheus metrics
curl http://YOUR_PROXY_IP:8080/metrics
```

`/health` returns a small JSON document with the node's status, uptime, active sessions, and total packets relayed. `/metrics` exports Prometheus-format metrics including relayed/dropped packet counters, active connections, relay latency, FEC recoveries, auth rejections, and abuse blocks.

For the full dashboard experience, stand up the **Prometheus + Grafana** stack in **[`infra/monitoring/README.md`](../infra/monitoring/README.md)**:

```bash
cd infra/monitoring
docker compose up -d
# Grafana: http://localhost:3000 (admin / lightspeed)
# Prometheus: http://localhost:9090
```

The dashboard auto-loads with 20 panels across 6 sections (mesh overview, traffic, latency, FEC, security, sessions) and ships with 10 alert rules, including `ProxyNodeDown` and `HighAuthRejections`.

## Troubleshooting

### Health check failing

If `curl http://YOUR_PROXY_IP:8080/health` times out or errors:

```bash
ssh root@<vps-ip>
systemctl status lightspeed-proxy
journalctl -u lightspeed-proxy --tail 50
ss -ulnp | grep 4434
```

Check that the firewall allows TCP `8080` and that the service is actually running.

### Binary not starting

```bash
# Check permissions
ls -la /usr/local/bin/lightspeed-proxy

# Verify the config parses
cat /etc/lightspeed/proxy.toml
```

A missing or malformed config is the usual culprit. Confirm the config file exists at the path passed to `--config`.

### Updating the proxy

Rebuild and redeploy using the scripts in **[`infra/scripts/`](../infra/scripts/)**:

```bash
cargo build --release -p lightspeed-proxy
./infra/scripts/deploy.sh
```

For a single node, `deploy.sh` rebuilds, copies the binary, and restarts the service. For a mesh, `deploy-all.sh` rolls the update out across every configured node.
