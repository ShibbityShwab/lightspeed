# Deploy a LightSpeed Proxy

> Deploy your own proxy relay on any Linux VPS. You own the node, you run the node. LightSpeed also ships a community relay network that works out of the box, but this guide is for running your own dedicated node. This is self-hosting, full stop.

## Overview

A **proxy node** is a small relay server that sits between your game client and the game server. Your client tunnels game packets to the node over UDP, and the node forwards them toward the game server over its own high-speed backbone connection. The result is a shorter, less congested path than your ISP's default route, which usually means lower and more stable ping.

The model is **self-hosted**: you provision a VPS, install the proxy binary, and point your client at it. Nobody else uses your node, and you don't rely on anyone else's. Run one node for yourself, or spin up a few in different regions and let the client pick the fastest automatically (see [Multi-Node Mesh](#multi-node-mesh)). If you would rather not run your own, the client uses the community relay network by default (see [Community Relay Network](community-network.md)).

A single node is tiny, roughly **~500KB of actual RAM**, so it fits comfortably on even the smallest VPS.

The full deployment options (native binary, Docker, automated script) are documented in depth in **[`infra/README.md`](../infra/README.md)**. This guide gives you the essentials and the operational details you need day to day. Don't duplicate what infra already covers; follow the links.

### What the node exposes

| Port | Protocol | Purpose |
|------|----------|---------|
| `4434` | UDP + TCP | Data plane: relayed game traffic (UDP, or TCP via the `--tcp` tunnel) |
| `4433` | QUIC | Control plane: client registration and auth |
| `8080` | HTTP | Health check and Prometheus metrics |

All three ports are configurable via the `[network]` section - see [Configurable ports](#configurable-ports).

## Prerequisites

Before you start, you need:

- **A Linux VPS.** Any provider works. A single small instance is plenty.
- **SSH access** to the VPS.
- **Open firewall ports.** Allow inbound UDP `4434` (data) and TCP `4434` (TCP tunnel), UDP `4433` (QUIC control), and TCP `8080` (health/metrics). Ports are configurable via the `[network]` section - see [Configurable ports](#configurable-ports). The exact commands depend on your provider's firewall and `ufw`/`firewalld` on the box.
- **A way to run the proxy.** Either the Rust toolchain to build from source, or Docker. See the options below.

## Quick Deployment

There are three supported ways to get a node running. Full, step-by-step instructions for each live in **[`infra/README.md`](../infra/README.md)**. Here's the short version.

### Option A: Native binary + systemd (recommended)

Relays use a **versioned release layout** so a bad binary can always be rolled back:

```
/opt/lightspeed/releases/<version>/lightspeed-proxy   # one immutable dir per version
/opt/lightspeed/current -> releases/<version>         # active release (symlink)
```

The systemd unit runs `/opt/lightspeed/current/lightspeed-proxy`, so switching versions is an atomic `ln -sfn` repoint, not an in-place file swap. The three most recent releases are kept; older ones are pruned only after a new release is activated and passes its health gate.

For a fresh node, use the one-shot setup script (it uploads the binary, writes the config and unit, installs the first release, and verifies health):

```bash
# Build locally (the quic feature is required for the control plane)
cargo build --release -p lightspeed-proxy --features quic

# Provision the node (ip, node-id, region)
bash infra/scripts/setup-new-node.sh YOUR_VPS_IP relay-1 us-east
```

The same layout is used under the hood by `relay-install.sh`, which you can also run directly on a node to install a staged binary:

```bash
# On the relay, with a staged binary already on disk
bash relay-install.sh --binary /tmp/lightspeed-proxy.staged --version 20260101T000000Z-abc1234
```

`relay-install.sh` verifies the file is an executable ELF for the host arch, runs `--check` against `/etc/lightspeed/proxy.toml`, repoints `current`, restarts the service, and polls `/health` for up to 15s. If the new release never becomes healthy it repoints `current` back to the previous release, restarts, and exits non-zero.

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
export LIGHTSPEED_NODES='{"relay-1":{"ip":"1.2.3.4"},"relay-2":{"ip":"5.6.7.8"}}'
./infra/scripts/deploy.sh
```

The project's own relays are deployed through the **Deploy Proxy** GitHub
Actions workflow (`.github/workflows/deploy.yml`), which builds the proxy and
runs `deploy.sh` on every push to `master` that touches `proxy/**`,
`protocol/**`, or the deploy scripts. The proxy must be built with
`--features quic` for the control plane to work.

## Security & Authentication

The proxy has a QUIC control plane (port `4433`) that gates the data plane. As of
**v1.1.0**, **`require_auth` defaults to `true`**: the proxy will not relay data-plane
packets from a client until that client has registered over the QUIC control plane.

### How it works

The control plane is **QUIC with a self-signed server certificate pinned trust-on-first-use
(TOFU) by the client**. Registration is anonymous: there is no client certificate. The
first connection records the certificate fingerprint and every later connection to that
address must present the same one, so a changed certificate is rejected as a possible MITM.
The flow is:

1. A client connects to the QUIC control port (`4433`) and sends a `Register` message.
2. The proxy checks capacity, generates a random `session_token`, and records the client's IP.
3. The proxy replies with a `RegisterAck` carrying the `session_token`.
4. The client stamps that token into **every** data-plane packet header.
5. The proxy validates an **(IP, data-plane port, token)** triple on every data-plane packet, dropping any that do not match.

This raises the bar for abuse: a random UDP sender cannot relay through your node without
first completing a QUIC registration. The token is 32-bit (defense-in-depth alongside the
IP binding); the enforcement path is covered by `proxy/tests/integration_security.rs`.

### Requirements for token auth to work

- The **proxy** must be built with the `quic` feature (the Docker image and release
  binaries already are).
- The **client** must be built with the `quic` feature so it can register (the release
  binaries already are). A client built without `quic` sends token `0` and will be
  rejected by an authenticated proxy - build with `cargo build --features quic`.

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
abuse, and FEC pipeline applies to TCP traffic - it is not a weaker path. The TCP tunnel
uses length-prefixed framing with a hard frame-size cap and connection limits.

## Multi-Node Mesh

For best results, deploy 2-3 nodes in different regions. The client probes every configured proxy on startup and auto-selects the fastest route, so more regions means a better chance of a low-latency path to your game server.

RTT depends on your location, ISP, and the game-server region, so measure it
from your own connection instead of trusting a fixed table. Run
`lightspeed --probe-proxies` to probe every relay in the signed registry and
see real numbers for your network, then pick the region closest to the game
server.

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
# Which release is active, and what exists on disk
readlink -f /opt/lightspeed/current
ls -la /opt/lightspeed/current/lightspeed-proxy /opt/lightspeed/releases/

# Verify the config parses
cat /etc/lightspeed/proxy.toml
```

A missing or malformed config is the usual culprit. Confirm the config file exists at the path passed to `--config`. If a recent deploy left the node unhealthy, `current` will have been rolled back to the previous release automatically; check `readlink -f /opt/lightspeed/current` to see which version is running.

### Updating the proxy

Rebuild and redeploy using the scripts in **[`infra/scripts/`](../infra/scripts/)**:

```bash
./infra/scripts/deploy.sh                 # all nodes
./infra/scripts/deploy.sh relay-nrt       # one node
```

`deploy.sh` builds the binary, uploads it to a staging path on each relay, and runs `relay-install.sh` over SSH. Each deploy gets a unique release version (a UTC timestamp plus the short git SHA; override with `LIGHTSPEED_VERSION`), so the previous version is preserved:

- `relay-install.sh` refuses to activate a binary that is not an executable ELF for the host arch, or whose `--check` fails.
- Activation is an atomic repoint of `/opt/lightspeed/current`, followed by a `/health` poll of up to 15s.
- On a failed health gate the installer repoints `current` back to the previous release, restarts the service, and exits non-zero, so a bad deploy leaves the node running the last known-good version.
- The newest 3 releases are retained; older ones are pruned only after a healthy activation.

For a mesh-wide rollout, `deploy-all.sh` iterates the node inventory. To roll a node back manually, repoint the symlink and restart:

```bash
ln -sfn /opt/lightspeed/releases/<previous-version> /opt/lightspeed/current
systemctl restart lightspeed-proxy
```

The installer's layout/rollback logic is covered by `infra/scripts/test_relay_install.sh`, which runs it against a fixture root with `systemctl`, `curl`, and `file` stubbed out.

### Flipping the control-plane ALPN (versioned control plane)

> **Fleet status (2026-11-16):** the project's eight community relays run 1.11.0
> and are all set to `control_alpn = "required"`. A client older than 1.11.0
> cannot register there and must update; a 1.11.0 client negotiates
> `lightspeed-control/1` on the first attempt. Self-hosted relays keep the
> default `"off"` unless their operator opts in using this procedure.

LightSpeed 1.11.0 adds a versioned ALPN name to the QUIC control plane
(`lightspeed-control/1`). The relay's policy is one config key:

```toml
[network]
control_alpn = "off"        # default; byte-for-byte the pre-1.11.0 behaviour
```

`"off"` sends no ALPN and accepts the ALPN-less handshake every older client
makes. `"required"` advertises `lightspeed-control/1` and **rejects** clients
that do not offer it (TLS alert 120, `no_application_protocol`). The background,
the compatibility matrix and the rollout plan are in
[quic-alpn-rollout.md](quic-alpn-rollout.md).

**Order matters: client first.** A current client (1.11.0+) offers
`lightspeed-control/1` first and, only on alert 120, retries once with an empty
offer — so it still registers against an old relay (one extra handshake per
process and relay). A pre-1.11.0 client never offers ALPN and cannot recover
from a `required` relay: it keeps retrying the control plane, never registers,
and cannot relay game traffic through that relay until it updates or switches
relay. Therefore:

- **Updating a relay to the stage-2 build is inert.** `control_alpn` defaults to
  `"off"`, which is exactly today's behaviour. Update relays at any time.
- **Enabling ALPN is one line and one restart.** `required` is the only
  behavioural change, and it is gated per relay on its owner's approval plus the
  canary window below.
- **Rollback is the same line and a restart.** `"off"` restores the previous
  behaviour immediately; clients reconnect on their supervisor's normal backoff
  (250 ms to 5 s). No client release is needed for a relay rollback.
- **Blast radius of a flip is that relay's pre-ALPN clients only.** Peers on the
  path that do not carry ALPN (QUIC-intercepting middleboxes) fail either way;
  see the alert-120 entry in [troubleshooting.md](troubleshooting.md).

#### The flip command

```bash
# 1. Plan the change (read-only: reads /health, changes nothing)
./infra/scripts/relay-control-alpn.sh --node relay-lax --mode required --dry-run

# 2. Flip the canary relay (one relay, over SSH)
./infra/scripts/relay-control-alpn.sh --node relay-lax --mode required

# 3. Verify from the outside
curl -s http://<relay>:8080/health | jq .control_alpn      # -> "required"
curl -s http://<relay>:8080/metrics | grep lightspeed_control_handshakes_total
```

`--node` accepts a node id, region, or IP from the signed registry
(`web/registry.json`); `--ip` overrides the resolved address for a relay that is
not in the registry. `--config` overrides the relay config path (default
`/etc/lightspeed/proxy.toml`).

The script is fail-closed and rolls back by itself:

- It reads the relay's `/health` **before** changing anything. A build without
  the `control_alpn` field is refused with "deploy the stage-2 build first" —
  otherwise the key would be inert and the flip unprovable.
- It keeps a timestamped backup next to the config
  (`proxy.toml.bak.YYYYMMDDTHHMMSSZ`), writes the edit atomically, checks that
  exactly one active key remains under `[network]`, and runs the deployed
  binary's `--check --config` over SSH **before** restarting.
- After the restart it polls `/health` (default 30 s) until it reports the new
  mode. On a failed `--check`, a failed restart, or a health poll that never sees
  the new mode, it restores the backup, restarts the service, proves the previous
  mode is back, and exits non-zero with the evidence.
- Requesting the mode already in effect reports and exits 0 without touching the
  config or restarting the service.

The same flip can be dispatched from GitHub Actions ("Deploy Proxy" →
Run workflow): set `control_alpn_node` and `control_alpn_mode`, and type
`control_alpn_confirm` as exactly `flip <node> <mode>`. A dispatch with a node
runs only the single-relay flip job and never redeploys the fleet; a dispatch
without a node is a normal deploy.

#### Canary gate (plan §7.4)

A relay flipped to `required` is watched before the next relay is touched:

- **Metric:** `M1 = lightspeed_control_handshakes_total{outcome="rejected_no_application_protocol"}`
  on that relay. It is a **count**, not a latency; every pre-ALPN client that
  still exists increments it once when it tries to connect.
- **Window:** at least **200 accepted control handshakes** on the busiest hour of
  that relay's day (`negotiated_v1` + `no_alpn` from the same metric family), so
  "clean" is a meaningful sample of the population that can break.
- **Pass:** `M1 == 0` across the window, and no new `rejected_other` class of
  handshake errors.
- **Fail:** any non-zero `M1`. Then `outcome="no_alpn"` counts tell you how many
  pre-ALPN clients remain: roll that relay back (`--mode off`) and wait, or take
  the owner decision to accept breaking them. Per-relay rollback is one command
  and does not need a client release.

Only after the canary's clean window may the remaining relays be flipped, one at
a time, each with its own approval and its own window.
## Publishing a release (and the manual fallback)

A release is cut by pushing a version tag (`vX.Y.Z`). The `Release` workflow
builds the cargo-dist artifacts, publishes the GitHub Release, and then bumps
the packaging manifests (Homebrew, Chocolatey, Scoop) and verifies them. This
section is the operator contract for that last step: what the automation does,
what to do when it cannot publish, and how to check the logic without cutting
a release.

### What the workflow does now (2026-11-17)

The publish step is `infra/scripts/publish-release.sh`, called by the `host`
job. It is idempotent because on 2026-11-16 the generated
`gh release create` line failed with `HTTP 403: Resource not accessible by
integration` on `POST /releases`, the release had to be published by hand, and
the packaging bumps were skipped (they required the `host` job to succeed).

- **Release missing:** `gh release create <tag> --target <sha> --title ...
  --notes-file ...`, then `gh release upload <tag> artifacts/* --clobber`.
- **Release already exists** (created by a previous attempt, a re-run, or by
  hand): no second create; the assets are uploaded with `--clobber` and the
  title/notes/draft metadata is refreshed. A create race (`already_exists`)
  falls back to the upload path instead of failing.
- **Bounded retry:** HTTP 5xx/429, timeouts, connection resets, the
  "Egress is over the account limit" 503, and a 403 are retried on one budget
  of four attempts with 5 s / 15 s / 30 s backoff. Every attempt is logged.
- **Persistent 403:** the step stops with an actionable `::error::` block - the
  repository workflow-permission setting, the workflow `permissions:` block,
  an org/App cap, rulesets, the failing run URL, and the optional
  `RELEASE_TOKEN` fallback below - instead of only "Resource not accessible by
  integration".
- **Complete release + read-only token:** if the release already has every
  expected asset and the token cannot rewrite them, the step warns and exits 0
  and reports `writable=false`; the note trim is skipped. A usable release is
  not turned red because a retry could not re-upload identical assets.
- **`release-state` job:** after `host`, it checks that the release is
  published and carries the six assets the packaging steps read (the four
  client tarballs, the Windows client zip and the GUI MSI). It reads release
  metadata only, so it adds no artifact egress. `bump-homebrew`,
  `bump-chocolatey`, `bump-scoop`, `verify-bumps` and `winget-releaser` are
  gated on *that* (`ready=true`), not on `host` success, so a red publish step
  no longer skips packaging when the release exists - and they still never run
  before those assets exist.
- **Pull requests never publish:** `publishing` is true only for a tag push.
  A PR runs `dist plan` (plus whatever build-on-PR jobs cargo-dist asks for) and
  the publish-logic self-test, but never the publish step. `workflow_dispatch`
  is a read-only probe (below).

The 2026-11-17 token probe (throwaway branch, run `43989309532`, workflow
`permissions: contents: write`) created a tag ref and a draft release with the
Actions token, so the 2026-11-16 403 is not reproducible under the current
repository config; the same window also hit an artifact-egress 503. The
workflow therefore assumes publish works and survives a repeat - it does not
re-add permissions that were already present.

### Verifying the publish logic without a release

The publish path only runs on a tag, so it is tested with a stateful `gh`
stub: `bash infra/scripts/test_release_publish.sh`. The
`Release publish self-test` job runs it on every workflow event, including
`pull_request`; 64 assertions cover create/upload, the create race, the
transient retry, the
persistent 403 verdict, the complete-release tolerance, `--dry-run`, and the
PR refusal. It needs bash + jq and no credentials, so it is CI-only on the
Windows dev host.

Read-only probe / dry run:

```text
Actions -> Release -> Run workflow -> tag = v1.11.0
```

The `Release publish probe (read-only)` job reports the token and repository
permission state and prints what the publish step would do for that tag,
without creating, editing or uploading anything. Locally (Linux/macOS, or CI):

```bash
bash infra/scripts/publish-release.sh --dry-run --tag v1.11.0
```

### If the Release workflow cannot publish

Publishing a release is an owner gate. Prefer fixing the token path to
publishing by hand, because the workflow also bumps every package manifest.

1. **Read the error.** The step prints the failing `gh` output and, on a
   persistent 403, the settings to check in order:
   `Settings -> Actions -> General -> Workflow permissions` must allow the
   workflow token to write (the workflow itself keeps
   `permissions: contents: write`), no org/App policy may cap Actions tokens at
   read, and no ruleset may block release/tag creation.
2. **If the Actions token is capped:** create a fine-grained PAT with
   "Contents: read and write" on this repository and store it as the
   `RELEASE_TOKEN` repository secret. The publish step prefers it when set and
   falls back to `GITHUB_TOKEN` when it is absent. This is an owner/credential
   action - never commit a token.
3. **Alternatively, publish manually from the run's own artifacts** (this is
   what was done for `v1.11.0`; the commands work in PowerShell too):

   ```text
   gh run download <run-id> --dir release-artifacts
   # verify one archive against sha256.sum before uploading anything
   gh release create vX.Y.Z --target <tag-commit-sha> \
     --title "X.Y.Z - YYYY-MM-DD" --notes-file release-notes.md
   gh release upload vX.Y.Z release-artifacts/* --clobber
   ```

   The body should be the `CHANGELOG.md` section for the version
   (`bash infra/scripts/release-notes.sh X.Y.Z > release-notes.md`).
4. **Then re-run the Release workflow** (Actions -> Release -> Re-run all
   jobs). The publish step finds the release and uploads with `--clobber`, or
   accepts a complete release it cannot write, and `release-state` lets the
   packaging bumps run. Verify the outcome with
   `gh release view vX.Y.Z --json isDraft,isLatest,assets`, by checking that
   `Formula/lightspeed.rb`, `dist/chocolatey/lightspeed.nuspec` and
   `dist/scoop/lightspeed.json` moved to the new version, and by the
   `verify-bumps` job.

Rollback: `gh release delete vX.Y.Z --cleanup-tag --yes` removes the release,
its assets and the tag - it is destructive and needs owner approval; re-tag
the correct commit afterwards. The packaging bumps are normal commits on
`master` and can be reverted with `git revert`.
