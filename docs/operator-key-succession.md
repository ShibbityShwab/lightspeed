# Operator Key Succession

> How the community registry is signed, how clients verify it, and what to do
> when the operator key is rotated, lost, or suspected compromised.

The single compiled-in operator key is the most fragile part of the community
relay network. If it is lost, no new registry can be published that existing
clients will trust. If it is compromised, an attacker can sign a registry that
points every client at attacker-controlled relays. This document describes the
current trust model exactly as the code implements it, then gives a concrete
rotation and recovery procedure, and finally states the gaps plainly.

Nothing here changes the code. Every command below maps to something that
already exists in the repository.

---

## Current trust model

### Where the signing key lives

The registry is signed offline with an Ed25519 key pair. The private key never
touches the client, the relay, or CI. It is a local file the operator holds:

```bash
ssh-keygen -t ed25519 -N "" -f operator.key -C "lightspeed-operator"
openssl pkcs8 -topk8 -nocrypt -in operator.key -out operator.pk8
```

- `operator.key` is the OpenSSH-format private key.
- `operator.pk8` is the PKCS8 form that the signing tool actually reads.
- The matching public key is base64-encoded and compiled into the client as
  `DEFAULT_OPERATOR_PUBKEY_B64` in `client/src/registry.rs:23`.

The current value is:

```
f5l9G2l4a3OrOX3NnWdMrNHQFbjOt27jN+st1z/J388=
```

There is exactly one key. There is no key ID, no key list, and no expiry.

### How the registry is signed

The registry is a small JSON document (`schema_version`, `published_at`,
`nodes`, `revoked`). Signing is a one-shot offline step:

```bash
cargo run -p lightspeed-client --example sign_registry -- operator.pk8 registry.json
```

`sign_registry` (`client/src/registry.rs:105`) serializes the registry to a
JSON string, signs the exact bytes with Ed25519, and emits a `SignedRegistry`:
the raw registry string plus a base64 signature. The signature covers the raw
bytes, so there is no canonicalization step and no ambiguity about what was
signed.

The output is committed to `web/registry.json` and served as a static file from
GitHub Pages at `https://shibbityshwab.github.io/lightspeed/registry.json`. The
Pages workflow (`.github/workflows/pages.yml`) republishes `web/**` on every
push to `master`, so a new signed registry goes live as soon as it is committed.

### How clients verify

The client fetches the signed registry over HTTPS and calls `verify_registry`
(`client/src/registry.rs:77`). That function:

1. base64-decodes the operator public key,
2. base64-decodes the signature,
3. verifies the Ed25519 signature over the exact registry bytes,
4. parses the registry JSON only after the signature checks out.

The key used is resolved in `client/src/main.rs:159` and `:1086`: an explicit
`--registry` / `config.registry.operator_key` wins, otherwise the compiled-in
`DEFAULT_OPERATOR_PUBKEY_B64` is used. `RegistryConfig.operator_key` is a single
`Option<String>` (`client/src/config.rs:130`), so a client trusts exactly one
key at a time.

After verification the client drops any node whose public key is in the
`revoked` list, probes the survivors, and connects to the fastest. The trust
model is v1: registered, healthy, and not revoked. There is no reputation or
Sybil resistance.

### What the key does and does not protect

- It authenticates the **node list**. A valid signature means "this list of
  relay addresses and node public keys came from the operator."
- It does **not** encrypt traffic. Game traffic is unencrypted by design.
- It does **not** protect a client that overrides `operator_key` with its own
  value; that client trusts whatever key it was given.
- Revocation is only as fresh as the last registry a client fetched. A client
  that never re-fetches keeps trusting the last validly signed list it saw.

---

## Rotation procedure

Rotation means: generate a new key, ship a client release that accepts the new
key, re-sign the registry with the new key, then retire the old key. The hard
constraint is that **old clients only trust the old key**, so the new key must
reach clients before the registry stops being signed by the old key.

### 1. Generate the new key pair

```bash
ssh-keygen -t ed25519 -N "" -f operator-new.key -C "lightspeed-operator-2026"
openssl pkcs8 -topk8 -nocrypt -in operator-new.key -out operator-new.pk8
```

Record the new public key in base64:

```bash
# Ed25519 public key is the last 32 bytes of the OpenSSH public key blob.
# The signing tool and client both use the raw 32-byte key, base64-encoded.
```

The exact base64 form is whatever `DEFAULT_OPERATOR_PUBKEY_B64` expects: the
raw 32-byte Ed25519 public key, standard base64. Derive it once and keep it with
the key material.

### 2. Add the new key as an accepted alternate in a client release

This is the step the current code does **not** support, and it is the crux of
the whole problem. See [Gaps](#gaps) below. Until a multi-key scheme exists,
the only way to make clients accept a new key is to change
`DEFAULT_OPERATOR_PUBKEY_B64` and ship a release. That is a hard cutover: every
client on an older release stops trusting the registry the moment the registry
is re-signed with the new key.

The smallest change that makes rotation safe is a key list. Instead of one
constant, the client would carry an ordered list of accepted operator keys and
`verify_registry` would accept a signature from any of them. The rotation then
becomes: ship a release with `[old, new]`, re-sign with `new`, then ship a
release with `[new]` only. No client is ever stranded.

### 3. Re-sign the registry with the new key

Once clients that accept the new key are in the field:

```bash
cargo run -p lightspeed-client --example sign_registry -- operator-new.pk8 registry.json
```

Commit the output to `web/registry.json`. The Pages workflow publishes it. From
this point the registry is signed by the new key only.

### 4. Retire the old key

- Remove the old public key from the client's accepted set in the next release.
- Delete `operator.key` / `operator.pk8` from every machine that held them.
- Revoke the old key in your own records. There is no on-chain or registry-level
  key revocation; "retired" means "no longer compiled into any client and no
  longer used to sign."

### Ordering rule

The order above is not optional. If you re-sign with the new key before clients
accept it, every existing client fails verification and falls back to whatever
it does when discovery fails. If you retire the old key before re-signing, the
old registry stays live and valid, which is fine, but you must not let the old
private key sign anything you would not want trusted.

---

## Recovery: key lost

If the private key is lost (disk failure, forgotten passphrase, departed
operator) and no backup exists, you cannot sign a new registry that existing
clients will trust. The recovery is:

1. Generate a new key pair (step 1 above).
2. Ship a client release with the new public key compiled in.
3. Re-sign the registry with the new key and publish it.
4. Accept that clients on old releases lose community discovery until they
   update. They can still use `--proxy` or a self-hosted relay.

There is no cryptographic recovery path. The compiled-in key is the root of
trust, and losing it means the root is gone. This is exactly why the key must be
backed up offline, in more than one place, and why the multi-key gap matters.

**Backup guidance:** keep the private key offline (not in the repo, not in CI
secrets, not in a password manager synced to the cloud if you can avoid it).
Store at least two encrypted copies in separate physical locations. Test that a
backup can actually sign a throwaway registry before you need it.

---

## Recovery: key suspected compromised

If the private key may have leaked, assume an attacker can sign any registry
they want, including one that lists attacker-controlled relays and revokes the
real ones. Clients cannot tell the difference: a valid signature is a valid
signature.

There is no way to revoke a compromised key for clients that already trust it,
because revocation itself would have to be signed by the compromised key. The
only real remedy is a client release that stops trusting the old key.

Procedure:

1. **Generate a new key pair immediately.**
2. **Ship a client release** that trusts the new key and no longer trusts the
   compromised one. This is the only step that actually stops the attack.
3. **Re-sign the registry** with the new key and publish it. Clients that have
   updated will follow it.
4. **Treat the window between compromise and the new release as hostile.** Any
   registry published in that window, even one that looks correct, must be
   considered attacker-controlled until re-signed with the new key.
5. **Audit what the compromised key could have signed.** The blast radius is
   the node list: an attacker could point clients at relays they control, which
   can see and modify unencrypted game traffic. They could not change client
   binaries or read the old private key's backups.

The uncomfortable truth: until every client updates, a compromised key keeps
working against un-updated clients. There is no server-side kill switch. This is
the strongest argument for the multi-key scheme and for short client update
cycles.

---

## Gaps

These are real limitations of the current code, stated plainly.

### 1. Single compiled-in key, no alternates

`DEFAULT_OPERATOR_PUBKEY_B64` is one constant (`client/src/registry.rs:23`) and
`verify_registry` takes a single `&str` key (`client/src/registry.rs:77`). There
is no way to accept two keys at once, so rotation cannot be done without a hard
cutover.

**Smallest change to close it:** change the constant to a slice of accepted
keys and have `verify_registry` try each until one verifies. The client config
`operator_key` would become a list. This is a small, self-contained change in
`client/src/registry.rs` and `client/src/config.rs`, with no protocol change.

### 2. No key ID or expiry

The registry carries no key identifier and no validity window. A client cannot
tell which key signed a registry, only that some accepted key did. Adding a
`key_id` field to `SignedRegistry` and an expiry to the registry payload would
let clients reject stale or wrong-key documents explicitly.

### 3. No revocation of the operator key itself

The `revoked` list revokes **node** public keys, not operator keys. There is no
mechanism to tell a client "stop trusting operator key X." The only lever is a
new client release.

### 4. Revocation freshness depends on re-fetch

A client that fetched a valid registry and then goes offline keeps trusting it
indefinitely. There is no max-age check on `published_at`. Adding a max-age
would bound how long a revoked node stays trusted by a disconnected client.

### 5. No automated signing in CI

Signing is deliberately offline, which is correct for key safety, but it means
there is no CI check that the committed `web/registry.json` verifies against the
compiled-in key. A test that loads `web/registry.json` and calls
`verify_registry` with `DEFAULT_OPERATOR_PUBKEY_B64` would catch a mismatched
key or a bad signature before it ships. (The test at `client/src/registry.rs:598`
does exactly this, but it runs in the client test suite, not as a deploy gate.)

---

## Summary

- The trust root is one Ed25519 public key compiled into the client.
- The private key signs the registry offline; clients verify against the
  compiled-in key.
- Rotation requires shipping a client that accepts the new key **before**
  re-signing, and the current single-key code makes that a hard cutover.
- Loss is unrecoverable without a client release; compromise is only stopped by
  a client release.
- The fix is a multi-key accepted set. It is small and self-contained, and it is
  the single highest-value change for the long-term survival of the network.
