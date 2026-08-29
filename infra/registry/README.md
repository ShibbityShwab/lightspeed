# Community Proxy Registry

A signed node list + revocation list that lets LightSpeed clients discover and
connect to community-run relay nodes — without trusting the transport.

## How it works

1. **Nodes** generate an Ed25519 identity on first boot (`infra/scripts/provision-oci.sh`
   writes `/etc/lightspeed/node.key`).
2. **Operators** submit their node (`node_id`, `region`, `data_addr`, `health_url`, `pubkey`)
   to the registry, gated by an invite token.
3. The **registry** (a Cloudflare Worker) stores the node list in KV and serves it signed
   with the operator's Ed25519 key. The signature covers the exact registry JSON bytes.
4. **Clients** fetch `/nodes`, verify the signature against the operator's embedded public
   key, drop revoked nodes, probe the survivors, and connect to the fastest.

The client-side verification lives in `client/src/registry.rs`
(`verify_registry` / `sign_registry`). The operator public key is compiled into the client.

## Setup

```bash
cd infra/registry
npm install -g wrangler
wrangler login
wrangler kv namespace create REGISTRY_KV        # copy the id into wrangler.toml
wrangler secret put OPERATOR_KEY_B64            # base64(PKCS8 Ed25519 private key)
wrangler secret put INVITE_TOKEN                # random bearer token
wrangler deploy
```

Generate the operator key and sign a registry by hand (offline path):

```bash
ssh-keygen -t ed25519 -N "" -f operator.key -C "lightspeed-operator"
openssl pkcs8 -topk8 -nocrypt -in operator.key -out operator.pk8
cargo run -p lightspeed-client --example sign_registry -- operator.pk8 registry.json
```

## Trust model (v1)

Trust = *registered + healthy + not revoked*. Nodes are authenticated at registration
(invite token); health is scored from client probe reports; revocation is an explicit
operator action. There is no transitive reputation or Sybil resistance yet — that is
deferred. A community relay can observe and MITM your game traffic (same as any paid
optimizer's relay); document this rather than implying privacy.
