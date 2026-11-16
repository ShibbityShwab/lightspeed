# Versioned ALPN for the QUIC control plane — rollout plan

> **Status: plan only.** No production code and no wire change have been made for
> this document. Stages 1 and later in §4 are **owner-gated** because they change
> the TLS ClientHello on a released binary or the handshake policy of a relay.
> Evidence for every claim here: `wat/evidence/2026-11-16/alpn/` (gitignored, on
> the machine that produced this plan). Diagnosis background:
> [`docs/github-feedback-audit-2026-11-16.md`](github-feedback-audit-2026-11-16.md)
> §3 (`#140`/`#147`).

This plan answers one question: **how does the control plane move from "no ALPN"
to a versioned ALPN without breaking either end of the fleet?** The order is not a
preference — §3 shows that a relay cannot be configured first without breaking
every client already in the field.

---

## 1. Current state, and why it is a defect

Neither side of the QUIC control plane sets ALPN, so every LightSpeed handshake
completes with `alpn_protocol = None`. In rustls that is a silent negotiation
failure: the client sends no ALPN extension
(`ClientConfig::alpn_protocols` empty — "If empty, no ALPN extension is sent",
`rustls-0.23.46/src/client/client_conn.rs:165-167`), the relay configures none
(`ServerConfig::alpn_protocols` empty — "If empty we don't do ALPN at all",
`rustls-0.23.46/src/server/server_conn.rs:337-338`), and the strict-QUIC check
passes only because *both* lists are empty. RFC 9001 §8.1 requires QUIC endpoints
to agree an application protocol via ALPN and to close with transport error
`0x0178` (`no_application_protocol`, TLS alert 120) when negotiation fails; the
repository's own `rustls` source quotes that requirement and adds the client-side
MUST (`rustls-0.23.46/src/server/hs.rs:123-126`,
`rustls-0.23.46/src/client/hs.rs:629-634`). A conforming peer therefore rejects
the current ClientHello with exactly the alert the reporter saw — issue #140/#147
— and this repository's relay silently tolerates it. The opaque error is the
symptom; the missing ALPN is the defect. Nothing in the tree configures it:
`git grep -n alpn_protocols HEAD -- client proxy protocol` has no matches, and
the only `alpn` strings in the tree are the diagnostic comment, log line and
verdict added by `908b3ac` in [`client/src/quic/mod.rs`](../client/src/quic/mod.rs).

| Side | Code path (func; lines at `787a8b8`) | What it does today |
|---|---|---|
| Client | [`build_client_config`](../client/src/quic/mod.rs) — `client/src/quic/mod.rs:66-104` | builds a rustls config with the TOFU verifier and **no** `alpn_protocols`; logs `ALPN=[]` at line 101 |
| Client | [`connect_with_destination`](../client/src/quic/mod.rs) — `client/src/quic/mod.rs:192-284` | one `endpoint.connect(addr, "lightspeed-proxy")` (lines 203-208); any handshake error is mapped through `handshake_failure` and returned |
| Client | `handshake_failure` — `client/src/quic/mod.rs:106-133` | classifies `ConnectionClosed` with `crypto(0x78)` into a middlebox verdict; asserts "a LightSpeed relay does not require ALPN and cannot send this alert" |
| Relay | [`build_server_config`](../proxy/src/control.rs) — `proxy/src/control.rs:252-274` | builds a rustls server config with a self-signed cert and **no** `alpn_protocols` |
| Relay | accept loop — `proxy/src/control.rs:333-359` | `let Ok(conn) = incoming.await else { return; }` — a rejected handshake is **silent** (no log, no metric) |
| Shared | [`protocol/src/lib.rs`](../protocol/src/lib.rs) | no ALPN constant exists; this is where shared wire identifiers live |

Two properties make this a *wire-breaking* change rather than a config tweak:

1. **The ClientHello changes.** Offering ALPN adds an extension to a packet that
   middleboxes on the path already inspect (and one of them is the suspected
   cause of #140/#147, which is why the ALPN offer is itself a diagnostic).
2. **A relay that requires ALPN rejects clients that do not offer it** (§3,
   row B). There is no "optional ALPN" mode on a single QUIC endpoint: a relay
   with a non-empty list fails an ALPN-less client, and a relay with an empty
   list fails an ALPN-offering client. The list has to be one or the other.

**Scope of the change.** ALPN is negotiated on the control-plane QUIC connection
only (UDP control port, default 4433). The data plane is untouched: no tunnel
header field changes, no payload byte changes, no source-address rewriting, no
IP-masquerade change — the game server still sees the player's real IP and bytes
are forwarded unmodified. `[TRANSPARENCY_STUB]` and `[PRIVACY_STUB]` are
unaffected by construction, because the control plane carries registration,
ping and telemetry, not game packets.

---

## 2. Proposal: one versioned ALPN identifier

**Name:** `lightspeed-control/1` (20 bytes, ASCII).

The scheme is *versioned in the name*, not in an opaque token: a future protocol
revision adds `lightspeed-control/2` as a new name and keeps `/1` supported for
at least one migration window. The version is **independent of
`PROTOCOL_VERSION`** in [`protocol/src/header.rs`](../protocol/src/header.rs)
(the data-plane header version, currently `3`): the control plane and the data
plane evolve on their own schedules and coupling the numbers would force a
header bump for a TLS-only change.

**Where the constant lives:** [`protocol/src/control.rs`](../protocol/src/control.rs),
next to the `ControlMessage` definitions it governs, re-exported from
[`protocol/src/lib.rs`](../protocol/src/lib.rs) beside the existing
`control::{caps, disconnect_reason, game_id, ...}` re-export (line 47). Both the
client and the relay import it from there, so the two sides cannot drift:

```rust
/// Application protocol name for the QUIC control plane, revision 1.
///
/// Versioned wire identifier: a future revision adds a new name; `/1` is never
/// renamed and stays offered while relays in the field still speak it.
pub const QUIC_CONTROL_ALPN_V1: &[u8] = b"lightspeed-control/1";

/// Every control-plane ALPN this build offers, most preferred first.
pub fn quic_control_alpn() -> Vec<Vec<u8>> {
    vec![QUIC_CONTROL_ALPN_V1.to_vec()]
}
```

Constraints checked against the standards and the dependency API:

- RFC 7301 §3.1 defines `ProtocolName` as `opaque<1..2^8-1>` — non-empty,
  at most 255 bytes. 20 bytes is legal.
- rustls takes `Vec<Vec<u8>>` on both sides
  (`rustls-0.23.46/src/client/client_conn.rs:167`,
  `rustls-0.23.46/src/server/server_conn.rs:338`), so the helper above is the
  exact shape both configs need.
- **Selection order is the relay's list order**, not the client's: rustls
  iterates `config.alpn_protocols` (the server's list) and picks the first entry
  the client also offered (`rustls-0.23.46/src/server/hs.rs:99-108`). When the
  relay eventually supports `/2`, its list decides which revision a client that
  offers both gets — list newest-first.

**Config surface it implies:**

| Side | Key | Values | Default | Meaning |
|---|---|---|---|---|
| Relay | `[network] control_alpn` in [`proxy/src/config.rs`](../proxy/src/config.rs) | `"off"` \| `"required"` | `"off"` | `off` = current behaviour (empty list, ALPN-less handshakes accepted); `required` = list `[lightspeed-control/1]`, ALPN-less handshakes rejected |
| Relay | `--control-bind` | unchanged | `0.0.0.0:4433` | no new port, no new endpoint |
| Client | none (always offers) | — | offer `[lightspeed-control/1]`, retry `[]` | the fallback is unconditional; there is no user-facing knob |

There is deliberately **no `"optional"` value**: a single quinn `Endpoint` uses
one rustls `ServerConfig`, and a non-empty list makes the relay reject ALPN-less
clients. Serving both policies at once needs a second endpoint on a second port —
see §10 option D for why that is not the first move.

---

## 3. Compatibility matrix — why the client must ship first

`[]` = an empty ALPN offer/config (today's build). `[v1]` = `lightspeed-control/1`.
Rows cite the rustls branch that decides the outcome (rustls 0.23.46, resolved in
`Cargo.lock` alongside quinn 0.11.12 / quinn-proto 0.11.19).

| # | Client | Relay | Handshake | Alert 120 sent by | rustls branch |
|---|---|---|---|---|---|
| **A** | `[]` (today) | `off` (today) | OK, `alpn = None` | — | strict check false: `alpn none && (!relay_list.is_empty() \|\| client_offered.is_some())` = false (`server/hs.rs:122-137`) |
| **B** | `[]` (today) | **`required`** | **fails** | relay | same strict check, true because `relay_list` is non-empty (`server/hs.rs:130-137`) |
| **C** | `[v1]` (new) | `off` (today) | **fails on attempt 1** | relay | same strict check, true because the client offered (`server/hs.rs:130-137`) |
| **C′** | `[]` (the retry in C) | `off` | OK, `alpn = None` | — | identical to row A |
| **D** | `[v1]` (new) | `required` | OK, `alpn = lightspeed-control/1` | — | overlap found, `SingleProtocolName` selected (`server/hs.rs:100-113`) |
| **E** | `[v2]` only | `[v1]` | **fails** | relay | no overlap in the non-empty branch (`server/hs.rs:114-119`) |
| **F** | `[v2, v1]` | `[v2, v1]` | OK, `alpn = lightspeed-control/2` | — | relay order wins (`server/hs.rs:99-108`) |

Client side of the same moment: rustls raises 120 client-side **only** when the
client offered a non-empty list and got nothing (`client/hs.rs:635-640`), and a
server that selects a protocol the client did not offer is a fatal
`IllegalParameter`, not a silent downgrade (`client/hs.rs:620-626`). So:

- **Row B is the breaker.** A relay flipped to `required` first breaks every
  client built before this work, immediately and permanently: those clients never
  offer ALPN and have no fallback. They cannot self-heal; only a client update or
  a relay rollback fixes them.
- **Row C is recoverable, and only in one direction.** A new client that offers
  ALPN and, on `crypto(0x78)`, retries once with an empty list lands on row C′ and
  registers normally. Old relays keep working. This is the compatibility
  guarantee the whole plan rests on.
- **Row C also explains the cost** of the client-first order: one extra
  handshake per (process, relay) pair that has not upgraded — one client→relay
  round trip when the path is quiet, up to one Initial PTO (~1 s) when the
  offering ClientHello is lost, paid once per process because the client
  remembers that relay's answer. The estimator and sample-count discussion is in
  §9.
- **Row E stays a hard failure on purpose.** A client that offers only `/2`
  against a `/1` relay fails even after the empty-list retry, because that relay
  requires ALPN. That is version skew, not a bug: it must be loud and named (see
  §7.1), never silently downgraded.

**Conclusion.** Row B and row C are the *same alert produced in opposite
directions*; the difference is whether the receiver can recover. Therefore:

> **A client build that offers `lightspeed-control/1` and falls back to an empty
> list on `crypto(0x78)` must be published before any relay is configured with
> `control_alpn = "required"`.** Shipping them in the other order breaks the
> installed base with no recovery path.

---

## 4. Staged rollout

No calendar dates are promised. Each stage is gated on the observation named; the
owner decides when to move.

| Stage | Ships | Wire-visible? | Gate to proceed | Rollback |
|---|---|---|---|---|
| **0** | This plan; the test tiers in §6 (unit + matrix tests, smoke-script assertion) | no | tests green on `Feature Matrix (quic)` | revert the branch |
| **1** | **Client** offers `[lightspeed-control/1]`, retries once with `[]` on `crypto(0x78)`, logs the negotiated protocol, remembers a relay's answer; verdict text in `handshake_failure` gains the version-skew case | **yes** (ClientHello gains an extension) | ordinary release gates; no relay change required | publish a client that offers nothing (the previous build is already that) |
| **2** | **Relay** gains `[network] control_alpn` (`off` default) + startup log field + `/health` field + `/metrics` counters + handshake-rejection log | no behaviour change until configured | relay operators can update safely: default `off` is byte-for-byte today's behaviour | none needed (inert) |
| **3** | **One canary relay** set to `control_alpn = "required"` | yes, on that relay only | §7.4 clean window on that relay | set `"off"` and restart; clients recover on their next reconnect |
| **4** | **Remaining relays** set to `required` | yes, fleet-wide | same clean window per relay | same as stage 3, per relay |
| **5** | (later, separate decision) client stops falling back / relay drops `/1` | yes | a client-adoption decision this plan does not make | — |

Stage 1 and stage 2 are independent code changes in different crates and can be
developed in parallel; only their *releases* are ordered. Scope per crate is §5,
tests per tier are §6.

---

## 5. Exact changes this implies

### 5.1 `protocol/`

- Add `QUIC_CONTROL_ALPN_V1` and `quic_control_alpn()` to
  [`protocol/src/control.rs`](../protocol/src/control.rs); re-export from
  [`protocol/src/lib.rs`](../protocol/src/lib.rs).
- Tests: the name is byte-for-byte `b"lightspeed-control/1"`, length within
  RFC 7301 bounds, ASCII, no NUL. A rename must fail CI, because a rename is a
  wire break with no compile error on either side.

### 5.2 `client/`

- `build_client_config(addr, alpn: Vec<Vec<u8>>)` — set
  `crypto.alpn_protocols = alpn` before `QuicClientConfig::try_from`; log the
  offer instead of the hardcoded `ALPN=[]`
  (`client/src/quic/mod.rs:66-104`).
- `connect_with_destination` (`client/src/quic/mod.rs:192-284`) becomes two
  attempts: full offer, then — **only** on `ConnectionError::ConnectionClosed`
  with `error_code == TransportErrorCode::crypto(0x78)` — one retry with an empty
  offer. No retry on timeout, on a different alert, or on an application close:
  a middlebox that swallows the offer must not double the wait.
- Extract the alert-120 predicate used by both `handshake_failure`
  (`client/src/quic/mod.rs:106-133`) and the retry decision, so the classifier and
  the fallback cannot drift (the same "one place so the two entry points cannot
  drift apart" rule `client/src/route/relay_choice.rs` already states).
- Remember the answer **per relay, for the process lifetime**, in the existing
  per-relay supervisor state (`client/src/quic/mod.rs`, `supervisor::Entry` /
  `SupervisorContext` — the struct that already carries `current_token` and
  `destination`), so the doomed first attempt is not repeated on every reconnect.
  Do not add a global or an on-disk cache: a relay upgrade is picked up on the
  next process start, which is the right freshness for a capability bit.
- Read the negotiated protocol from `conn.handshake_data()`, downcast to
  `quinn::crypto::rustls::HandshakeData` and use its `protocol: Option<Vec<u8>>`
  (`quinn-proto-0.11.19/src/crypto/rustls.rs`), and attach it as a field on the
  existing `info!("QUIC connection established")` line.
- Reword `handshake_failure`'s verdict: the sentence "a LightSpeed relay does not
  require ALPN and cannot send this alert" becomes false at stage 3. New text
  must name both causes (a QUIC-intercepting middlebox, or a relay on a
  different ALPN revision) and say the empty-list retry already happened.
- `--test-control` (`client/src/modes/control_test.rs`) prints the negotiated
  ALPN in its summary, so the manual interop check in §6.5 is one command.

### 5.3 `proxy/`

- `build_server_config` (`proxy/src/control.rs:252-274`) takes the configured
  mode: `off` → leave `alpn_protocols` empty (today's behaviour), `required` →
  `quic_control_alpn()`. Read it from `ProxyConfig` (new field
  `[network] control_alpn`, default `off`); do not add a second endpoint or port.
- Startup line (`proxy/src/control.rs:305-314`): add `alpn=off` /
  `alpn=required (lightspeed-control/1)` to the existing
  `QUIC control plane listening on ...` line. This is the operator's primary
  self-check and the line the QUIC smoke script already greps.
- Accept loop (`proxy/src/control.rs:342-344`): replace the silent
  `let Ok(conn) = incoming.await else { return; }` with a `warn!` carrying
  `remote=` and `error=`, and count the rejection. Distinguish
  `crypto(0x78)` from other handshake errors.
- `handle_connection` (`proxy/src/control.rs:431`): read
  `HandshakeData.protocol` once per accepted connection and count
  `negotiated=v1` vs `negotiated=none`.
- Metrics: three `AtomicU64` counters on
  [`ProxyMetrics`](../proxy/src/metrics.rs) — negotiated, completed-without-ALPN,
  rejected — exported from `to_prometheus` (`proxy/src/metrics.rs:959`) as
  `lightspeed_control_handshakes_total{outcome=...}`; rejection split by reason
  (`no_application_protocol`, `other`).
- Health: add `control_alpn` to the `/health` JSON
  (`proxy/src/health.rs`, `/health` arm) so a monitor can read the mode without
  parsing the log.

### 5.4 Docs that must move with the code

- [`docs/deploy-proxy.md`](deploy-proxy.md) — the "Security & Authentication"
  section states the control plane is QUIC with TOFU. It must also state the ALPN
  mode, the client/relay pairing rule, and the `control_alpn` key.
- [`docs/troubleshooting.md`](troubleshooting.md) — the alert-120 entry must
  change meaning once relays can require ALPN: 120 can then be version skew, not
  only an interceptor.
- [`docs/protocol.md`](protocol.md) — the "Control Plane Protocol (QUIC)" section
  gains one line naming the ALPN identifier and its versioning rule.
- [`docs/CLI-REFERENCE.md`](CLI-REFERENCE.md) — it already documents the
  `[proxy] quic_port` ↔ `[network] control_port` pairing (line ~250); the
  `control_alpn` key joins it.
- [`docs/README.md`](README.md) — index entry for this document.

---

## 6. Test plan

Three tiers. Tier 1 is the proof and runs in today's CI; tier 2 is the
cross-crate end-to-end test and needs a CI plumbing change (§6.4); tier 3 is the
manual interop check an operator can run.

### 6.1 `protocol/` unit tests (tier 1)

| Test | Asserts |
|---|---|
| `control_alpn_name_is_stable` | `QUIC_CONTROL_ALPN_V1 == b"lightspeed-control/1"` byte for byte |
| `control_alpn_name_is_rfc7301_conformant` | length `1..=255`, printable ASCII, no NUL |

### 6.2 `client/` unit tests (tier 1) — classifier and fallback decision

All in `client/src/quic/mod.rs`'s existing `#[cfg(test)]` module, so they run
under `cargo test --workspace --lib --features quic`.

| Test | Asserts |
|---|---|
| `alert_120_selects_the_empty_list_retry` | `ConnectionClosed(crypto(0x78))` → retry decision `true` |
| `a_certificate_alert_does_not_retry` | `ConnectionClosed(crypto(0x2a))` → no retry |
| `a_timeout_does_not_retry` | `ConnectionError::TimedOut` → no retry (a swallowing middlebox must not double the wait) |
| `an_application_close_does_not_retry` | no retry |
| `the_120_verdict_names_version_skew_after_the_rollout` | the reworded message names both causes and the address; the existing two tests in the module are updated, not deleted |

### 6.3 `proxy/` matrix tests (tier 1) — the rows that decide the order

In `proxy/src/control.rs`'s test module (inside `mod inner`, so they are gated on
the `quic` feature and run in `cargo test --lib --features quic`). Each test
binds a real `ControlServer` on `127.0.0.1:0`, drives a real quinn client, and
asserts the handshake outcome. The existing test module already builds
`ControlState` fixtures (`proxy/src/control.rs:716-737`); the client side is the
`SkipVerify`-verifier endpoint pattern already used by
[`proxy/tests/phase0_integration.rs`](../proxy/tests/phase0_integration.rs), with
`alpn_protocols` as the only variable.

| Test | Row | Asserts |
|---|---|---|
| `an_off_relay_accepts_an_alpn_less_client` | A | handshake OK, `HandshakeData.protocol.is_none()`, `Register` answered |
| `an_off_relay_rejects_an_alpn_offering_client_with_120` | C | `ConnectionClosed` with `error_code == crypto(0x78)` — **pins the reason the client fallback exists** |
| `a_required_relay_negotiates_the_shared_name` | D | `HandshakeData.protocol == Some(b"lightspeed-control/1".to_vec())` |
| `a_required_relay_rejects_an_alpn_less_client_with_120` | B | alert 120 — **pins why stage 3 cannot precede stage 1** |
| `a_required_relay_rejects_a_different_revision_with_120` | E | alert 120 for `[v2]` vs `[v1]`; no silent downgrade |

Rows A, B, C, D carry the ordering argument; row E adds the loud-skew
property. If any of the five tests fails, the plan's order is wrong and the
rollout stops (§12).

### 6.4 CI jobs that cover it

At HEAD these execute tier 1 (paths in
[`.github/workflows/ci.yml`](../.github/workflows/ci.yml)):

| Job | Step | What it proves for this change |
|---|---|---|
| `Feature Matrix (quic)` (`ci.yml:255-281`) | `cargo test --workspace --lib ... --features quic` (`ci.yml:278`) | compiles and runs every tier-1 test above |
| `Feature Matrix (full)` (`ci.yml:255-281`; matrix entry `full` at `ci.yml:262`) | same, `--features full` | the same with the client's `full` feature set (which includes `quic`) |
| `Feature Matrix (quic)` clippy (`ci.yml:280-281`) | `cargo clippy ... --features quic` | no lint regression on the `#[cfg(feature = "quic")]` path |
| `Check & Test` (`ci.yml:38-39`) | `cargo test --workspace` (default features) | the non-`quic` build still compiles once the ALPN code is gated |
| `Proxy QUIC Smoke` (`ci.yml:41-58`) | [`infra/scripts/test_proxy_quic_smoke.sh`](../infra/scripts/test_proxy_quic_smoke.sh) | **extend it**: the release binary's startup line must contain `alpn=off` (and later `alpn=required`), so the shipped artifact proves its own mode — this is the only gate that exercises the release binary, not the test build |

**Gap this plan must close (its own task, §13 task C).** Every quic-gated
integration test file in the workspace is dead weight in CI today:

- `Cargo.lock`/manifests: `default = []` for both `lightspeed-client` and
  `lightspeed-proxy`.
- The six files `proxy/tests/integration_control.rs`,
  `proxy/tests/phase0_integration.rs`, `client/tests/control_reconnect.rs`,
  `client/tests/phase0_reconnect.rs`,
  `client/tests/quic_telemetry_capability.rs`,
  `client/tests/register_destination.rs` begin with
  `#![cfg(feature = "quic")]`.
- `Check & Test` runs default features (so those bodies compile to zero tests);
  the Feature Matrix jobs pass `--lib`, which excludes every `tests/` target.

So a cross-crate end-to-end ALPN test (real `ControlClient` ↔ real
`ControlServer`) placed in `proxy/tests/` **would not run anywhere today**. Two
changes, both small:

1. Put the tier-1 matrix tests in `src` unit-test modules (§6.3) so they run in
   the Feature Matrix job without any workflow change — this is the primary fix.
2. Add `--tests` to the Feature Matrix test step (or a dedicated
   `cargo test -p lightspeed-proxy -p lightspeed-client --features quic --tests`
   step) so the existing six suites finally execute, and put the cross-crate ALPN
   interop test there. Whether those six suites are green under `--features quic`
   is **unverified** — they have not been run in this task, so the workflow change
   must be validated on a branch before it is expected to pass.

### 6.5 Manual two-relay interop check (tier 3)

Two relay processes on one Windows host, two client config files. `--test-control`
derives the control port from `[proxy] quic_port` and ignores the port in
`--proxy` (`client/src/modes/control_test.rs:18-19`), so the two runs differ only
by config.

```powershell
# Build with the control plane enabled.
cargo build --release -p lightspeed-proxy --features quic
cargo build --release -p lightspeed-client --features quic

# Relay A — current behaviour (ALPN not required). No control_alpn key = "off".
@'
[server]
node_id = "alpn-a"
region = "test"
'@ | Set-Content alpn-a.toml
target\release\lightspeed-proxy.exe --config alpn-a.toml `
  --data-bind 127.0.0.1:14434 --control-bind 127.0.0.1:14433 --health-bind 127.0.0.1:18080

# Relay B — ALPN required.
@'
[server]
node_id = "alpn-b"
region = "test"

[network]
control_alpn = "required"
'@ | Set-Content alpn-b.toml
target\release\lightspeed-proxy.exe --config alpn-b.toml `
  --data-bind 127.0.0.1:15434 --control-bind 127.0.0.1:15433 --health-bind 127.0.0.1:19080

# Client configs differ only in the control port they target.
@'
[proxy]
quic_port = 14433
'@ | Set-Content client-a.toml
@'
[proxy]
quic_port = 15433
'@ | Set-Content client-b.toml

$env:RUST_LOG = "lightspeed_client=debug,rustls=debug,quinn=info"
# 1) new client vs OLD relay: must succeed on attempt 2 and log the fallback once.
target\release\lightspeed.exe --test-control --proxy 127.0.0.1:14434 --config client-a.toml
# 2) new client vs NEW relay: must succeed on attempt 1 with ALPN negotiated.
target\release\lightspeed.exe --test-control --proxy 127.0.0.1:15434 --config client-b.toml
# 3) relay self-check, both modes (the health port is the one bound above):
curl.exe -s http://127.0.0.1:18080/health    # relay A -> control_alpn: "off"
curl.exe -s http://127.0.0.1:19080/health    # relay B -> control_alpn: "required"
curl.exe -s http://127.0.0.1:19080/metrics | Select-String "control_handshakes_total"
```

Expected, recorded as the acceptance of the check:

1. Run 1 succeeds; the log carries one `offer: alpn=["lightspeed-control/1"]`
   line, one `retrying with an empty list` warning, then
   `QUIC connection established ... alpn=none` and `Registered with proxy`.
2. Run 2 succeeds; log carries `alpn="lightspeed-control/1"` and **no** fallback
   warning.
3. Relay A's startup line says `alpn=off`, relay B's says `alpn=required
   (lightspeed-control/1)`; `/health` on each reports the same mode.
4. **Optional but decisive (row B):** run the *previous* release binary (no ALPN
   offer) against relay B. It must fail with the alert-120 verdict and relay B's
   log must carry one `handshake rejected ... no_application_protocol` warning
   plus a non-zero `lightspeed_control_handshakes_total{outcome="rejected..."}`.
   This is the population that stage 3 breaks; step 4 is how a canary measures it.

**What is not verified by this plan:** no build, no run, and no interop test was
executed while writing it. The matrix in §3 is proven from the vendored rustls
sources (transcripts in `wat/evidence/2026-11-16/alpn/rustls-alpn-negotiation.txt`),
not from a live handshake.

---

## 7. Observability — proving which stage a relay is at

### 7.1 Client (`lightspeed-client`)

| Event | Level | Exact fields / text |
|---|---|---|
| offer built | `debug` | `QUIC control-plane TLS offer: alpn=["lightspeed-control/1"] sni=lightspeed-proxy tls=1.3` (replaces the hardcoded `ALPN=[]` at `client/src/quic/mod.rs:101`) |
| fallback taken | `warn` | `relay=<ip:port> relay rejected the ALPN offer with alert 120 (no_application_protocol); retrying with an empty offer` — once per (process, relay) |
| connection established | `info` | existing line gains `alpn=<negotiated>` or `alpn=none`; `none` is proof the relay has not been upgraded, `lightspeed-control/1` is proof it has |
| permanent failure | log | reworded `handshake_failure` verdict: alert 120 + address + both candidate causes + "the empty-offer retry also failed" |

### 7.2 Relay (`lightspeed-proxy`)

| Event | Level | Exact fields / text |
|---|---|---|
| startup | `info` | existing `QUIC control plane listening on <addr> (node=..., auth=..., alpn=off)` or `alpn=required (lightspeed-control/1)` |
| handshake rejected | `warn` | `remote=<ip:port> error=<quinn error> reason=no_application_protocol` (or `reason=other`) |
| per-connection outcome | counter | `lightspeed_control_handshakes_total{outcome=…}` on `GET /metrics`, with `outcome` ∈ `negotiated_v1`, `no_alpn`, `rejected_no_application_protocol`, `rejected_other` |
| mode | JSON | `control_alpn` field in `GET /health` |

`negotiated_v1 + no_alpn` is the relay's client-population split; `rejected_*` is
the breakage signal a canary watches (§7.4). The accept path is silent today
(`proxy/src/control.rs:342-344`), which is why stage 2 must add these before
stage 3 is allowed.

### 7.3 How a relay operator checks its own ALPN config

```powershell
# 1. The mode the process actually started with (single source: ProxyConfig):
Select-String -Path <proxy log or stdout> -Pattern "QUIC control plane listening"
#    -> ... (node=relay-fra, auth=enforced, alpn=required (lightspeed-control/1))

# 2. The same value as data, for a monitor:
curl -s http://<relay>:8080/health

# 3. Whether clients are being rejected because of it:
curl -s http://<relay>:8080/metrics | Select-String "control_handshakes_total"
```

A relay is at stage 2 iff the first line says `alpn=off`; at stage 3/4 iff it
says `alpn=required (lightspeed-control/1)`. Those are the only two states.

### 7.4 The stage-3 gate (what a clean window is)

Definition, because "looks fine" is not decidable:

- **Metric:** `M1 = lightspeed_control_handshakes_total{outcome="rejected_no_application_protocol"}`
  on the canary relay — a *count*, not a median. A single increment means at
  least one pre-ALPN client is still in the field and was cut off by the flip.
- **Window:** a window with at least **n = 200 accepted control handshakes** on
  the busiest hour of that relay's day (so `no_alpn` is a meaningful sample of
  the population that can break).
- **Pass:** `M1 == 0` across that window **and** `rejected_other` shows no new
  class of error.
- **Fail:** any non-zero `M1`. Then `outcome="no_alpn"` counts on that relay tell
  you how many pre-ALPN clients remain; the decision is to wait (roll back that
  relay) or to accept breaking them — an owner decision, not an operator one.
- A spiky link does not bias M1: it is a count of protocol outcomes, not a
  latency. It *does* inflate the registration-latency metric (M2, §9) and the
  fallback's cost, which is why the gate is on M1.

---

## 8. Rollout and rollback for community relays

Community relays may run older builds, be updated on their sponsor's schedule,
and are operated by people who are not in this repository's CI. The procedure is
built around that:

- **Updating a relay to the stage-2 build is inert.** `control_alpn` defaults to
  `off`, which is byte-for-byte today's behaviour. A relay can update at any
  time and stay at stage 2 indefinitely. Nothing in the client's behavior changes
  against it except the first-handshake fallback already described.
- **Enabling ALPN is one line and one restart.** `[network] control_alpn =
  "required"` in the relay's config, restart the process (the QUIC endpoint is
  bound at startup, `proxy/src/control.rs:252-316`). There is no migration
  script, no certificate change, and no port change. The relay's TOFU pin is
  unaffected: the pin is keyed by `ip:port` and the certificate does not change
  (`client/src/quic/pinning.rs:29-62`).
- **The client's empty-list fallback is the compatibility guarantee.** For any
  relay that has not upgraded, or has been rolled back, a current client still
  registers: it pays one failed handshake per process and then connects without
  ALPN (row C → row C′). Relays and clients therefore do not need to be released
  together, and a relay can revert without coordinating with anybody.
- **Rollback is the same line plus a restart:** `control_alpn = "off"`. Clients
  reconnect on the supervisor's existing backoff (250 ms–5 s,
  `client/src/quic/mod.rs` `Backoff`) and are accepted immediately. No client
  change, no client release.
- **Blast radius of a stage-3/4 flip is that relay's pre-ALPN clients only.** The
  precise consequence for a stale client: it keeps retrying the control plane
  forever (it can never register), so with `require_auth = true` (the default) it
  cannot relay game traffic through that relay at all — it can still use another
  relay or the direct path if the client's routing allows it. That blast radius
  is why stage 3 is one canary first, and why the gate is §7.4.
- **Do not change relay production configuration from this plan.** Stage 3/4 are
  fleet operations owned by the relay-operator task in §13 (task E), under the
  owner gate in §11.

---

## 9. Metric definition, estimator, and what the fallback costs

ALPN touches exactly one measurement surface, and it is worth stating it the way
the routing/FEC work states its metrics, because "one extra handshake" is easy to
mis-report.

- **Leg:** the client→relay **control-plane** connection on the UDP control port
  (default 4433). Not relay→server, not client→server total. The data-plane legs
  are untouched by this change, so no existing latency/jitter/loss/FEC number
  should move because of it. If a data-plane metric moves during a canary, that is
  a finding, not an expected effect.
- **M1 — handshake outcome (categorical, the rollout gate):**
  counts per relay per window of `negotiated_v1`, `no_alpn`,
  `rejected_no_application_protocol`, `rejected_other`. Estimator: **rate**
  (ratio of counts); n = the number of control handshakes in the window, minimum
  200 (§7.4). Categorical metrics are immune to link spikes by construction.
- **M2 — registration latency (continuous, the fallback cost):** client-side
  elapsed time from the start of `connect_with_destination`
  (`client/src/quic/mod.rs:192`) to the `RegisterAck`, microseconds, per attempt.
  Report **p50, p95, max and n**, plus the count of attempts that included a
  fallback; never p50 alone, because the fallback is a tail event.
- **Estimator behaviour on a spiky link:** the fallback's cost is *one client→relay
  round trip when the path is quiet* (the relay sends the alert as soon as it
  processes the ClientHello — the reported relay answered in ~30 ms in the
  evidence's example path) *and up to one Initial PTO (~1 s) when the offering
  ClientHello itself is lost*. A p50 over all attempts will hide that entirely: on
  a lossy mobile link a handful of attempts pay ~1 s while the median does not
  move. That is why M2 requires p95/max and a fallback count, and why the
  rollout gate is M1 (a count) rather than M2.
- **Sample-count honesty:** a canary that sees 12 handshakes proves nothing about
  a fleet. The n ≥ 200 / window definition exists so "clean" means something
  before stage 4 repeats the flip on seven more relays.
- **What the cost is not:** it is not per registration and not per ping. The
  supervisor remembers a relay's answer for the process lifetime, so the doomed
  attempt happens once per (process, relay) pair. A client that registers with
  one relay pays it at most once per launch while that relay is un-upgraded.

---

## 10. Alternatives considered

**A. Client-only fallback, no ALPN anywhere.** Ship the retry in the client but
never set `alpn_protocols` on either side. Rejected: it changes nothing on the
wire (an empty offer is still an empty offer), so RFC 9001 §8.1 conformance does
not improve, a strict peer still answers 120, and issue #140/#147 stays
unexplained-by-fix. It also leaves the client's retry dead code that can never
fire except against a foreign endpoint.

**B. Do nothing.** Rejected: the control plane stays non-conforming, every strict
QUIC peer on a user's path produces the opaque error, and the relay cannot even
tell a stale client from a middlebox once ALPN exists. The diagnosis in
[`docs/github-feedback-audit-2026-11-16.md`](github-feedback-audit-2026-11-16.md)
already committed the project to fixing the latent defect; this is the plan for
that commitment.

**C. Require ALPN on relays immediately (relay-first).** Rejected by §3 row B:
every client built before this work would be rejected permanently, including
users who would have to update before they can use the product again. Row C
exists precisely so the fleet never has to be in that state.

**D. Two control endpoints — one legacy, one ALPN-required on a second port.**
This is the only design that serves both policies at once and would give a true
"optional" mode. Rejected *for this rollout*: (i) the client learns a relay's
control port from config/registry, so a second port needs a registry schema field
and re-signing of `web/registry.json`, in [`client/src/registry.rs`](../client/src/registry.rs)
(`pre_pin_nodes` derives the control address as `ip:quic_port`); (ii) the TOFU
pin is keyed by `ip:port` (`client/src/quic/pinning.rs:29-33`), so a new port
means a fresh trust window per relay and a pre-pin path change; (iii) every relay
(including self-hosted ones) needs a new firewall/security-group opening, which is
a bigger fleet operation than a config flip; (iv) the client grows a
port-probing path on top of the ALPN fallback. Keep it as the fallback option if
the owner decides zero stale-client breakage is mandatory; the §3 matrix still
requires the client fallback to exist first either way.

**E. Relay-side ALPN without the client change.** Rejected as impossible: a QUIC
server cannot select a protocol the client did not offer; with a non-empty relay
list an ALPN-less client is rejected (row B). There is no server-side flag that
turns row B into row A in rustls 0.23.

**F. A private TLS extension instead of ALPN.** Rejected: ALPN is the mechanism
RFC 9001 §8.1 names, it is what strict peers and middleboxes inspect, and it is
what the reporter's error came from. A custom extension would be invisible to the
conformance problem it is meant to fix.

---

## 11. Owner gate

**What needs owner approval:**

1. **The stage-1 release.** It puts a new TLS extension on the wire in a released
   client. That is the wire-visible change this document exists to get approved.
   Each later stage's release (relay build, then a canary flip, then the fleet
   flip) is a separate approval at the time it happens; approving the plan is not
   approving the fleet flip.
2. **Any relay set to `control_alpn = "required"`** (stages 3 and 4). This is a
   fleet operation with a known blast radius (§8) and must be a crisp yes/no per
   relay.
3. **The public issue replies** on #140/#147 that describe the chosen order.

**What does not need approval:** this document; writing the code and tests in
§5/§6; the smoke-script and CI-invocation changes; updating the five docs in
§5.4. None of those change a released binary's wire behaviour or a relay's
config. The repository remains at owner spend $0.00: this plan adds no service,
no dependency, and no paid anything (`[COST_STUB]` unaffected).

**The decision being asked for:** *approve the client-first order in §4 so stage 1
and stage 2 can be implemented and released, with each relay flip gated
separately on §7.4.*

---

## 12. What would falsify this plan

- **F1 — The order is not actually required.** If a build that offers no ALPN can
  complete a handshake against a relay configured `required`, then rustls' strict
  branch does not behave as §3 says and stage 3 could precede stage 1. Detected by
  matrix test row B (`a_required_relay_rejects_an_alpn_less_client_with_120`).
- **F2 — The fallback does not rescue old relays.** If a current client cannot
  register against a stage-0 relay after one failed attempt, the compatibility
  guarantee is void and the client-first order does not buy what it claims.
  Detected by the manual check §6.5 step 1 and by a cross-crate interop test
  (§6.4 tier 2).
- **F3 — Version skew degrades silently.** If a `[v2]`-only client ever connects
  to a `/1` relay, or a relay ever accepts a client it should reject, the "loud
  failure" property is gone. Detected by matrix row E and by the client-side
  `IllegalParameter`/120 handling (`client/hs.rs:620-640`).
- **F4 — The canary gate passes for the wrong reason.** If `rejected_*` counters
  never increment because the relay never logs handshake failures, stage 3 will
  look clean while breaking clients. Detected by the row-B manual step (relay log
  + counter must be non-zero) being part of the canary procedure, not optional.
- **F5 — A data-plane metric moves.** ALPN must not change tunnel latency,
  jitter, loss or FEC recovery. If a canary shows a data-plane metric shifting
  beyond its normal spread, the change is not transparent and must be reverted
  and investigated before any further relay flips.
- **F6 — The reported 120 was a relay running this plan's `required` state.**
  Hypothetically possible after stage 3 (it is the old H2 hypothesis). The verdict
  text in §7.1 must therefore never claim "not this relay" without also naming
  version skew; if the two cannot be distinguished from the client, the diagnosis
  is incomplete and the wording must say so.

---

## 13. Follow-up tasks

| Task | Owner | Scope (write) | Acceptance |
|---|---|---|---|
| **A. Protocol + client ALPN offer, fallback, observability** | `rust-dev` | `protocol/src/`, `client/src/quic/`, `client/src/modes/control_test.rs` | constant + tests (§6.1); two-attempt connect with per-relay memory (§5.2); reworded verdict; §6.2 tests; `--test-control` prints negotiated ALPN; `cargo test --workspace --lib --features quic` green with counts reported |
| **B. Relay ALPN mode, rejection observability, matrix tests** | `rust-dev` | `proxy/src/control.rs`, `proxy/src/config.rs`, `proxy/src/metrics.rs`, `proxy/src/health.rs` | `control_alpn` off/required (§5.3); startup line, warn on rejection, `/health` field, `/metrics` counters; the five matrix tests (§6.3); smoke script asserts the mode |
| **C. CI coverage for quic integration suites** | `infra-dev` | `.github/workflows/ci.yml`, `infra/scripts/test_proxy_quic_smoke.sh` | Feature Matrix runs `--tests` for the quic feature (or a dedicated step); cross-crate interop test runs; whether the existing six suites pass under `--features quic` is established and reported before the step is made required |
| **D. Manual two-relay interop checklist + evidence** | `qa-engineer` | `wat/evidence/` only | §6.5 runs 1–3 (and 4 if a previous release binary is available); the four expected observations recorded verbatim with log lines |
| **E. Relay operator procedure + fleet flip** | `infra-dev` | `docs/deploy-proxy.md`, `docs/community-network.md` (procedure only; **no production config edits in the implementing task**) | the §8 procedure lands in the relay docs; the canary flip is proposed to the owner per relay with the §7.4 gate; rollback line documented |
| **F. User-facing docs** | `interface-designer` or `rust-dev` on docs | `docs/troubleshooting.md`, `docs/protocol.md`, `docs/CLI-REFERENCE.md`, `docs/README.md` | the four §5.4 updates; the alert-120 entry distinguishes middlebox from version skew |

Dependencies: A and B can run in parallel; C depends on A and B (it needs the
test to exist); D depends on A and B; E depends on B; F depends on the final
wording chosen in A. No task edits relay production config; stage 3/4 flips are
owner-gated operations, not code changes.

---

## Appendix A — the rustls branches that decide every row

All quotes are from `rustls 0.23.46` as resolved in `Cargo.lock`; full transcript
in `wat/evidence/2026-11-16/alpn/rustls-alpn-negotiation.txt`.

Server (`src/server/hs.rs`, `process_common`):

```rust
// ALPN
let our_protocols = &config.alpn_protocols;
if let Some(their_protocols) = &hello.protocols {
    cx.common.alpn_protocol = our_protocols
        .iter()
        .find(|ours| their_protocols.iter().any(|theirs| theirs.as_ref() == ours.as_slice()))
        .map(|bytes| ProtocolName::from(bytes.clone()));
    if let Some(selected_protocol) = &cx.common.alpn_protocol {
        ...
    } else if !our_protocols.is_empty() {
        return Err(cx.common.send_fatal_alert(
            AlertDescription::NoApplicationProtocol, Error::NoApplicationProtocol));
    }
}

if cx.common.is_quic() {
    // QUIC has strict ALPN, unlike TLS's more backwards-compatible behavior. RFC 9001
    // says: "The server MUST treat the inability to select a compatible application
    // protocol as a connection error of type 0x0178".
    if cx.common.alpn_protocol.is_none()
        && (!our_protocols.is_empty() || hello.protocols.is_some())
    {
        return Err(... NoApplicationProtocol ...);
    }
```

Client (`src/client/hs.rs`, `process_alpn_protocol`):

```rust
if common.is_quic() && common.alpn_protocol.is_none() && !offered_protocols.is_empty() {
    return Err(common.send_fatal_alert(
        AlertDescription::NoApplicationProtocol, Error::NoApplicationProtocol));
}
```

Read them together: emptiness on **both** sides is the only combination that
completes a QUIC handshake without an ALPN; any offer/config mismatch produces
120 in one direction or the other.

## Appendix B — evidence and how to reproduce it

`wat/evidence/2026-11-16/alpn/` (gitignored; present on the machine that produced
this plan):

| File | Contents |
|---|---|
| `README.md` | index of the directory, the exact commands, and what each command proves |
| `rustls-alpn-negotiation.txt` | the two rustls functions above, line-numbered, plus both `alpn_protocols` field docs |
| `tree-and-ci-state.txt` | `git rev-parse HEAD`, `git grep alpn_protocols` (no matches), every `cargo test` invocation in CI, the crate feature defaults, and the list of quic-gated test files |
| `code-paths.txt` | the seven code excerpts this document cites, with line numbers at `787a8b8` |

Reproduce the negotiation proof on any machine with the workspace's dependencies
fetched:

```powershell
$r = "$env:USERPROFILE\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\rustls-0.23.46\src"
Select-String -Path "$r\server\hs.rs" -Pattern "QUIC has strict ALPN" -Context 4,14
Select-String -Path "$r\client\hs.rs" -Pattern "QUIC clients MUST" -Context 4,10
```

Line numbers in this document are at `787a8b8`; the function names and file paths
are the stable anchors.

## Appendix C — standards references

- [RFC 9001 §8.1 — Protocol Negotiation](https://www.rfc-editor.org/rfc/rfc9001#section-8.1):
  QUIC endpoints agree an application protocol with ALPN and close with
  `no_application_protocol` (transport error `0x0178`) when negotiation fails.
- [RFC 7301 §3.1 — ALPN extension](https://www.rfc-editor.org/rfc/rfc7301#section-3.1):
  a protocol name is a 1–255 byte string; hence the length bound on
  `lightspeed-control/1`.

