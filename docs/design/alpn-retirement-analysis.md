# ALPN §5 retirement — analysis and recommendation

**Recommendation: wait for a condition — do not retire the empty-offer/legacy
path now, and do not rule it out forever.** Recommendation only: no code, fleet,
relay or policy change was made; the fleet/removal decision remains the owner's.

## 1. Scope of "§5 retirement"

The plan reserves it as a separate decision: §4 stage 5 is "(later, separate
decision) client stops falling back / relay drops `/1` — a client-adoption
decision this plan does not make". Retirement here = removing the row C/C′
compatibility path added by §5.2/§5.3:

- **Client** ([`client/src/quic/mod.rs`](../../client/src/quic/mod.rs)):
  `AlpnMemory` (`:94-129`), the one-shot empty-offer retry on alert 120
  (`should_retry_with_empty_alpn` `:239-250`, loop `:378-435`), the alert-120
  verdict (`:253-280`), and accepting a no-ALPN handshake
  (`register_on_connection` `:437-451`).
- **Relay** ([`proxy/src/config.rs`](../../proxy/src/config.rs) `:66-141`,
  [`proxy/src/control.rs`](../../proxy/src/control.rs) `:284-300`): the
  `control_alpn = "off"` mode (empty `alpn_protocols`, the default), leaving
  `required` unconditional.
- **Not in scope:** the `lightspeed-control/1` name (plan §2 keeps it), a future
  `/2` migration, or the `required` policy (owner decision, fleet memo).

## 2. What breaks

Matrix from plan §3, with the retirement effect:

| row | client | relay | today | after retirement |
|---|---|---|---|---|
| A | `[]` (pre-1.11) | `off` | works | `off` gone (becomes B) |
| B | `[]` (pre-1.11) | `required` | rejected | unchanged |
| C | `[v1]` (1.11+) | `off` | rejected, recovered by C′ | **permanent failure** |
| C′ | `[]` (retry) | `off` | works | removed |
| D | `[v1]` (1.11+) | `required` | works | unchanged |
| E | `[v2]` only | `[v1]` | hard failure | unchanged |

On the 8/8-`required` first-party fleet no client path changes: C/C′ never fire.
The break is **new clients against relays that are not `required`** —
self-hosted/third-party relays on `<1.11.0` or rolled back to `off`, outside
project telemetry. Asymmetry: a relay-side removal is rollback-able (`off` +
restart); a **client-side removal cannot be rolled back in the field** — only a
later release restores the fallback.

## 3. Who is affected

- **Pre-1.11 clients:** already permanently rejected (row B); retirement changes
  nothing. M1 (98–99 % of handshakes) counts every peer without the ALPN name —
  pre-1.11 clients plus scanners, not attributable from public counters — so it
  is not the population retirement protects.
- **1.11+ clients vs old/`off` relays:** the real blast radius, reachable only
  through relays the project does not run; unmeasurable today (P2/P5).
- **Self-hosters:** `off` is the relay default and the stage-2 inert upgrade;
  removal makes a routine upgrade breaking for their pre-1.11 clients unless an
  absent/`off` key maps to explicit `required` with a loud warning.
- **Mixed fleets / mercy lever:** today `off` is the only mode compatible with
  both generations (A + C′), which is why the fleet memo's mercy rollback was
  safe. After retirement `off` serves only old clients and **breaks updated
  clients** — the rollback lever inverts.
- **Telemetry:** the M1 counters must stay; they are the only measure of the
  refusal population.

## 4. Measured state (snapshot 2026-11-18T12:05Z)

- **8/8 relays reachable**, healthy, `version=1.11.2`, `control_alpn=required`;
  uptimes 786–1,289 s after a graceful 1.11.1→1.11.2 handoff (`result=ok`,
  11:49Z).
- **Current-process counters: 4,370 alert-120 rejects vs 80 accepted (98.2 %),
  0 `rejected_other`, 4 sessions**; 2 of 8 relays carried traffic. M1 is
  ~16,200/h. The accepted count includes a post-handoff reconnect burst (80 in
  ~16 min vs ~103/h fleet-average earlier), so only the share is meaningful.
- Prior windows agree: 99.400 % over the 38.5 h lifetime window at 05:27Z
  (673,495/677,561) and 98.69 % in the clean 429 s pair at 08:17Z.
- **No clean multi-day M1 series:** 4 field-bearing stats snapshots (08:10,
  08:17, 09:07, 11:00Z), two containing reset bookings from the 1.11.1 rollout;
  each release resets counters (1.11.2 at 11:49Z; 1.11.3 in preparation).
- **Adoption is not measurable:** the client sends `client_version` in the opt-in
  telemetry body ([`client/src/telemetry.rs`](../../client/src/telemetry.rs)
  `:292`), but relays export no version distribution. Downloads (not installs)
  are the only proxy: GUI MSI v1.11.0 187, v1.11.1 17, v1.11.2 1; in-app
  `-update` assets single digits.

## 5. Preconditions

| # | Precondition | Observable test | Status |
|---|---|---|---|
| P1 | All first-party relays `required`, continuously | 8/8 `required` across ≥1 full release/deploy cycle | **not met** (15-min uptime, next release pending) |
| P2 | Minimum client adoption (≥1.11.0) | privacy-safe version or fallback aggregate | **not measurable today** |
| P3 | Legacy tail bounded and attributed | M1 stable/declining over multi-day, plus relay-log source-IP attribution | **not met** (releases reset series) |
| P4 | Self-hoster migration defined | deprecation notice (owner gate) + `off`/absent handling + docs | not started |
| P5 | Fallback use observable | client-side fallback-taken / `alpn=none` aggregate (no first-party relay is `off`) | not available |

## 6. Decision matrix

| | Retire now | Wait for condition (recommended) | Never |
|---|---|---|---|
| **Ships** | client drops retry; relay drops `off` | instrumentation + deprecation only | nothing |
| **Breaks** | new clients × old relays; self-hosters; mercy lever; client side irreversible | nothing on first-party paths | nothing |
| **Preconditions** | none met | P1–P5 | n/a |
| **Rollback** | relay yes; client no (next release only) | n/a | n/a |
| **Cost/benefit** | ~130 lines saved; high unmeasured risk | small cost; evidence before breaking | permanent branch; defers `/2` rework |
| **Verdict** | reject | **recommend** | fallback if instrumentation is refused |

## 7. Staged plan (if "wait") with rollback

- **0 — record (no code, owner gate):** decision + deprecation intent; keep M1.
- **1 — instrument + warn:** privacy-safe client-side fallback/no-ALPN aggregate
  in opt-in telemetry; log/GUI deprecation notice on fallback; release.
  Rollback: telemetry flag off / next release.
- **2 — measure:** one migration window; gate on P1–P3 and P5.
- **3 — relay default:** new relay builds default `required`; `off` accepted with
  a startup warning; docs/notice. Rollback: `off` + restart (unchanged).
- **4 — remove (only with P1–P5 + explicit owner yes):** client stops retrying
  and refuses no-ALPN; relay rejects `off` with a migration message. Rollback:
  relay = previous release; client = restore path in a later release only.

## 8. Recommendation

**Wait for a condition.** Retirement now has no first-party benefit — on an
8/8-`required` fleet C/C′ never execute — while its blast radius (old/`off`
relays and self-hosters) is unmeasurable, its client step is irreversible in the
field, and it removes the only mode compatible with both generations, which is
what makes a mercy rollback safe. Waiting is nearly free: the fallback costs one
extra handshake only against a non-`required` relay, and there is none on the
project fleet. Retire only when P1–P5 hold. If the owner will not add the
privacy-safe fallback/version telemetry, then "never" is more honest than
retiring blind — keep the fallback as cheap insurance.

**What would change this:** a multi-day M1 series that is flat or falling *and*
source-IP attribution showing the residual stream is scanners rather than
clients; a client-side fallback count near zero over a migration window; or an
owner decision to accept breaking self-hosted old relays deliberately.
