# Community Relay Network

> How to run a relay, publish it, and let clients discover it, end to end.

LightSpeed runs a **community relay network**: five sponsor-funded relays,
hosted by the community, that any client can use with zero configuration. You
can also self-host your own relay. This document ties the whole flow together
(**deploy → register → discover**) so a new relay operator can onboard in one
page.

---

## How it works

```
Relay operator                          LightSpeed client
     │                                        │
     │  provision a VPS (or BYO host)          │
     │  → proxy + Ed25519 node key            │
     │                                        │
     │  sign registry offline (Ed25519)        │
     ▼                                        │
  Registry (static signed JSON on           │
  GitHub Pages) ── signed node list ──────▶ verify signature
     ▲                                                       │ probe, pick fastest
     │                                                       ▼
  Operator signs list (Ed25519)                      connect + tunnel
```

The registry is a small signed JSON document. Clients verify the operator's
Ed25519 signature against a key compiled into the binary, drop revoked nodes,
probe the survivors, and connect to the fastest. That is the same "probe and
pick" logic that already runs against configured proxies.

**Trust model (v1):** *registered + healthy + not revoked.* There is no
transitive reputation or Sybil resistance yet.

### The live network

Five relays are live, covering the major game-server regions:

| Region | Location |
|--------|----------|
| US-West | Los Angeles |
| US-East | New Jersey |
| AP-Southeast | Singapore |
| EU-Central | Frankfurt |
| AP-Northeast | Tokyo |

The default registry is a **static signed file** served from GitHub Pages:

```
https://shibbityshwab.github.io/lightspeed/registry.json
```

This is a plain static file, not the Cloudflare Worker. The Worker in
`infra/registry/` remains available as an optional dynamic registry, but the
static file is the current default. The operator's Ed25519 public key is
compiled into the client, so discovery works with no config at all.

---

## For relay operators

### 1. Pick a region

Network position is everything: the relay must have a *better-peered, lower-latency*
path to the game server than the player's home ISP. Pick the region **closest to
the game servers you want to serve**. See the table in
[`infra/README.md`](../infra/README.md#choosing-a-region-network-position).

### 2. Deploy a relay

Provision any small Linux VPS with a public IPv4 address in your chosen region.
Any provider works; the proxy is a single static binary. Then deploy it with the
provider-agnostic script:

```bash
cargo build --release -p lightspeed-proxy
./infra/scripts/setup-new-node.sh <ip> <node-id> <region>
```

> `infra/scripts/provision-oci.sh` is an optional example of fully-automated
> provisioning for one provider's API; `setup-new-node.sh` above is the
> supported default and works on any host.

### 3. Get your node identity

Every relay has an Ed25519 identity so the registry can revoke it individually.

- The automated provisioning script generates it at `/etc/lightspeed/node.key`
  (public key in `/etc/lightspeed/identity`).
- On a BYO host, generate one yourself: `ssh-keygen -t ed25519 -N "" -f node.key`.

Your **public** key is what you submit to the registry.

### 4. Register with the registry

The default registry is a static signed file, so "registration" means adding
your node to the JSON and re-signing it offline (see
[Operator tooling](#operator-tooling)). If you run the optional dynamic
Cloudflare Worker instead, you can register over HTTP:

```bash
curl -X POST https://<your-worker>/register \
  -H "x-registry-token: <invite-token>" \
  -H "content-type: application/json" \
  -d '{
        "node_id": "proxy-ewr",
        "region": "us-east-1",
        "data_addr": "1.2.3.4:4434",
        "health_url": "http://1.2.3.4:8080/health",
        "pubkey": "<base64-ed25519-public-key>",
        "note": ""
      }'
```

Worker registration is gated by an invite token so strangers can't poison the
list.

---

## For players

Discovery is **zero-config by default**. The client ships with the default
registry URL and the operator's public key compiled in, so it can find and
probe the community relays without any setup.

To point at a different registry (your own, or a mirror), override it on the
command line:

```bash
lightspeed --probe-proxies --registry https://shibbityshwab.github.io/lightspeed/registry.json
```

Or in `lightspeed.toml`:

```toml
[registry]
url          = "https://shibbityshwab.github.io/lightspeed/registry.json"
operator_key = "<base64-ed25519-public-key>"   # the operator's public key
```

> The client only trusts nodes whose signatures verify against `operator_key`,
> and it skips any node whose public key is in the registry's revocation list.
> The default URL and key are compiled in for zero-config discovery; setting
> either here overrides them.

---

## Security: what operators must know

- **Destination allowlisting** is the defense that makes a community relay safe.
  Restrict your relay to game-server prefixes so it can't be aimed at arbitrary
  public IPs (DDoS reflection/amplification):

  ```toml
  [security]
  require_auth = true
  destination_allowlist = ["104.26.0.0/16", "3.0.0.0/8", "52.0.0.0/8"]
  ```

  Empty (the default) = allow any public destination (your own private relay).
  Non-empty = only relay to these prefixes (community mode).
- **Token auth** ties every data-plane packet to a registered client; **rate
  limiting** and **anti-amplification** are on by default.
- **A community relay can see and MITM your game traffic**, the same as any
  paid optimizer's relay. Don't imply privacy; document it.
- **Relaying UDP can look like a VPN/proxy to your cloud provider** and risk a
  ToS ban. Keep the relay token-gated so it's demonstrably not an open relay.

---

## Operator tooling

Generate the operator key and sign a node list offline. The static-file
registry is the current default, so no Worker is needed:

```bash
ssh-keygen -t ed25519 -N "" -f operator.key -C "lightspeed-operator"
openssl pkcs8 -topk8 -nocrypt -in operator.key -out operator.pk8
cargo run -p lightspeed-client --example sign_registry -- operator.pk8 registry.json
```

The signing logic (`sign_registry` / `verify_registry`) lives in
`client/src/registry.rs`. The optional dynamic registry Worker is in
`infra/registry/`.

---

## Current status & known gaps

The community network is **LIVE** as of v1.4.0:

- ✅ **Five relays online:** Los Angeles (US-West), New Jersey (US-East),
  Singapore (AP-Southeast), Frankfurt (EU-Central), and Tokyo (AP-Northeast).
  All are community-hosted and sponsor-funded.
- ✅ **Registry hosted and signed:** the default registry is a static signed
  JSON file at
  `https://shibbityshwab.github.io/lightspeed/registry.json`, served from
  GitHub Pages. This static file is the current default, not the Cloudflare
  Worker.
- ✅ **Operator key compiled in:** the operator's Ed25519 public key is built
  into the client, so discovery is zero-config.
- ✅ **Key-format gap resolved:** the signer and verifier now agree on PKCS8
  Ed25519, so the sign/verify round-trip works end to end.
- ✅ **Code complete and tested:** relay provisioning, destination
  allowlisting, signed node-list + Ed25519 verify, client fetch + discovery.
  (117 client + 30 proxy unit tests, clippy clean.)

Known gaps:

- ⏳ **Live smoke-testing** of the automated provisioning script against a real
  provider account is still manual (not runnable in CI).
- The Cloudflare Worker in `infra/registry/` remains an optional dynamic
  registry; it is not the default path.
- **Trust model is v1:** registered + healthy + not revoked. There is no
  transitive reputation or Sybil resistance yet.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| `--probe-proxies` reports no nodes | Registry URL wrong, or `operator_key` missing/mismatched |
| "registry fetch failed" | Registry file unreachable, or signature/JSON malformed |
| Node shows ❌ in probe | Relay not running. Check `curl http://<ip>:8080/health` |
| Node rejected / banned | It was revoked, or its destination allowlist/rate-limit tripped |
