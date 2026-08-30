# Community Relay Network

> How to run a relay, publish it, and let clients discover it — end to end.

LightSpeed is self-hosted: the relay nodes that actually reduce ping are run by
the community, not by a central operator. This document ties the whole flow
together — **deploy → register → discover** — so a new relay operator can
onboard in one page.

---

## How it works

```
Relay operator                          LightSpeed client
     │                                        │
     │  provision a VPS (or BYO host)          │
     │  → proxy + Ed25519 node key            │
     │                                        │
     │  POST /register (invite token)         │
     ▼                                        │
  Registry (Cloudflare Worker + KV) ── signed node list ──▶ verify signature
     ▲                                                       │ probe, pick fastest
     │                                                       ▼
  Operator signs list (Ed25519)                      connect + tunnel
```

The registry is a small signed JSON document. Clients verify the operator's
Ed25519 signature against a key they trust, drop revoked nodes, probe the
survivors, and connect to the fastest — the same "probe and pick" logic that
already runs against configured proxies.

**Trust model (v1):** *registered + healthy + not revoked.* There is no
transitive reputation or Sybil resistance yet.

---

## For relay operators

### 1. Pick a region

Network position is everything: the relay must have a *better-peered, lower-latency*
path to the game server than the player's home ISP. Pick the region **closest to
the game servers you want to serve** — see the table in
[`infra/README.md`](../infra/README.md#choosing-a-region-network-position).

### 2. Deploy a relay

Provision any small Linux VPS with a public IPv4 address in your chosen region —
any provider works; the proxy is a single static binary. Then deploy it with the
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

Registration is gated by an invite token so strangers can't poison the list.

---

## For players

The client fetches + verifies the registry and probes its nodes alongside any
locally configured proxies:

```bash
lightspeed --probe-proxies --registry https://<your-worker>/nodes
```

Or in `lightspeed.toml`:

```toml
[registry]
url          = "https://<your-worker>/nodes"
operator_key = "<base64-ed25519-public-key>"   # the operator's public key
```

> The client only trusts nodes whose signatures verify against `operator_key`,
> and it skips any node whose public key is in the registry's revocation list.

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
- **A community relay can see and MITM your game traffic** — the same as any
  paid optimizer's relay. Don't imply privacy; document it.
- **Relaying UDP can look like a VPN/proxy to your cloud provider** and risk a
  ToS ban. Keep the relay token-gated so it's demonstrably not an open relay.

---

## Operator tooling

Generate the operator key and sign a node list offline (no Worker needed for a
static-file registry):

```bash
ssh-keygen -t ed25519 -N "" -f operator.key -C "lightspeed-operator"
openssl pkcs8 -topk8 -nocrypt -in operator.key -out operator.pk8
cargo run -p lightspeed-client --example sign_registry -- operator.pk8 registry.json
```

The signing logic (`sign_registry` / `verify_registry`) lives in
`client/src/registry.rs`; the reference Worker is in `infra/registry/`.

---

## Current status & known gaps

- ✅ Code complete and tested: relay provisioning, destination allowlisting,
  signed node-list + Ed25519 verify, client fetch + discovery. (117 client +
  30 proxy unit tests, clippy clean.)
- ⏳ **Needs live smoke-testing** (not runnable in CI): the automated
  provisioning script against a real provider account, and the Worker against
  a real Cloudflare account.
- ⚠️ **Key-format note:** the Rust signer uses PKCS8; the reference Worker's
  WebCrypto `importKey` expects the raw 32-byte Ed25519 seed. Reconcile these
  (or move signing fully offline and serve a static file) during deployment.
- The `[registry] operator_key` should be **compiled into the client** for
  production (config is fine for bring-up).

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| `--probe-proxies` reports no nodes | Registry URL wrong, or `operator_key` missing/mismatched |
| "registry fetch failed" | Worker down, or signature/JSON malformed |
| Node shows ❌ in probe | Relay not running — check `curl http://<ip>:8080/health` |
| Node rejected / banned | It was revoked, or its destination allowlist/rate-limit tripped |
