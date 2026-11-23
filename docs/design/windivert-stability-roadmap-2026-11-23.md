# WinDivert stability roadmap — 2026-11-23

> **Status:** synthesis + prioritised proposal. **No fix, test pass or release is
> claimed here.** No production code, Cargo file, workflow, GUI process, WinDivert
> handle, WFP object or machine state was changed by this round.
> **Author:** training/roadmap lead for `task-112`; design owner once accepted:
> `architect`. **Source tree:** `master` `6e30a65` (merge of
> `fix/pinned-driver-1.12.5`, tag `v1.12.5`), audit worktree
> `.agents/worktrees/windivert-stability-audit`. **Trace:** the owner's
> `user-server-transition-trace.log` slice is evidence, not a reproduction.
> **Inputs:** three independent audits and their training briefs
> ([`windivert-lifecycle`](../audit/windivert-lifecycle-2026-11-23.md),
> [`game-transition-state-machine`](../audit/game-transition-state-machine-2026-11-23.md),
> [`stability-verification`](../audit/stability-verification-2026-11-23.md)); evidence
> notes under [`wat/evidence/2026-11-23/stability-audit/`](../../wat/evidence/2026-11-23/stability-audit/README.md).
> **Companions:** [`in-house-stability-curriculum`](../training/in-house-stability-curriculum.md),
> personas `windivert-expert`, `game-networking-expert`, `stability-verifier`,
> and the item→finding map in
> [`wat/evidence/2026-11-23/stability-audit/synthesis.md`](../../wat/evidence/2026-11-23/stability-audit/synthesis.md).

---

## 0. How to read this document

Three labels are used consistently. They are not interchangeable:

| Label | Meaning | Example |
|---|---|---|
| **FACT** | Directly observable in the stored trace, QA evidence, git, or source | "All three diverting handles closed `-> ok` at 06:49:38.56Z" |
| **INFERENCE** | Mechanism/reconstruction that fits the facts but was not directly instrumented; confidence stated | "The 06:49 restarts were external actions (medium-high)" |
| **OWNER** | A decision only the owner/project can make (spend, user-facing copy, release, risk acceptance) | "Expose `sc stop WinDivert` in-app" |

Citation keys: `[WD]` = WinDivert lifecycle audit, `[GT]` = game-transition audit,
`[SV]` = stability-verification audit, `[EV-xx]` = evidence notes (`01`–`03`,
`GT-notes`, `WD-claims`, `WD-timeline`). Section numbers are the audits' own.
The `T`-series test IDs in this document are the **game-transition** audit's
`T1–T17`; the lifecycle audit's `T0–T13` are cited as `WD-T*`, and the
stability audit's acceptance rows as `A1–A12`. This alias rule exists because
three documents independently used "T" for different matrices.

**Non-claims.** This document does not establish the root cause of
`FWP_E_IN_USE`, does not verify 1.12.5, does not state that any listed change
works, and does not authorise a release, publish or patch. Every change below is
a proposal with a verification plan.

---

## 1. Situation in one page

### 1.1 What actually happened (FACT unless marked)

On 2026-11-23, in one elevated GUI process (log UTC, host UTC+07):

1. **06:43:34.775** — WinDivert pin `id=1` opens; intercept `id=2`, inject
   `id=3`, shadow `id=4` open; the session runs **≈363.9 s** with real traffic
   (`06:43:34.533` → Stop `06:49:38.417`).
2. Inside that session: two `Locked server stale - re-detecting` events
   **34.9 s apart** (`06:47:59.324`, `06:48:34.177`), **four** relay
   registrations (two 0.21 s apart at start, one per re-detect), four
   consecutive `no_reply_from_relay` judged windows (streak 1→4), three
   "return path is starving" warnings, and a relay demotion for the *next*
   ranking. **The re-detects closed no handle and stopped nothing** — this is
   agreed by all three audits and is visible in the trace.
3. **06:49:38.417** — `Shutdown signal received` with **no**
   presence/Quit/elevation/update/reason line. All three diverting handles
   close `-> ok` at **06:49:38.561**, pin still held (`driver pin active=true`).
4. **06:49:39.841** — first start attempt, **1.279 s** after the last close
   (**1.423 s** after the stop signal): intercept `id=5` fails
   `FWP_E_IN_USE (0x8032000A, os error -2144206838)`; **no retry line follows**
   (committed 1.12.5 policy retries only when `pin_active == false`).
   **06:49:42.690** — a second, separate start invocation: intercept `id=6`
   fails identically (**4.128 s** after the close).
5. The control plane **registered successfully 0.434 s after each failed
   start** (`session=2861874939`, `session=8752802`): route plane alive, capture
   plane dead. The two registration cards can disagree because they are written
   by different layers.
6. **06:49:43 → 06:50:14.905** — no retry for 31 s; only 5 s scan heartbeats
   and frozen `Latency comparison` values. Quit at 06:50:14.905; pin released
   06:50:15.173 — the last line the full trace ever wrote.
7. Context that bounds the diagnosis: the same failure class appears
   **05:24–06:10** in the same slice with **and without** a pin (the failure
   predates the pinned build; full-trace counts by day: 21 / 4 / 12 for
   2026-11-21 / 22 / 23), then a **30 min 39 s** gap (`06:10:42` →
   `06:41:21`) precedes the healthy run. What cleared the state is not in the
   log (`[WD]` F5/Q4, `[SV]` Q4).
8. Counter-evidence to the two simplest hypotheses: in the same post-boot
   window, **four Stop→Optimize cycles succeeded with close→open gaps of
   0.567 / 0.571 / 0.573 / 0.577 s**, and **six pin-held intercept opens**
   succeeded across the two post-boot processes (`[WD]` §2.3, `[EV-01]` §3).
   "The restart was too fast" and "the pin always breaks the next open" are
   both contradicted.

### 1.2 What the QA matrix did and did not cover (FACT from stored evidence)

- The 84/84 run (started **2026-11-23 11:56:33 +07**, finished **12:09:01 +07**)
  ran GUI SHA256 `F8ECE582…` = commit **`84e15c3`** (11:43:05 +07).
- The release HEAD **`01438e0`** ("stale error fix", **12:27:28 +07**) is later;
  the merge `6e30a65` (12:36:09 +07) is later still. `clear_stale_registration_error`
  is **absent** at `84e15c3` and present at HEAD (verified with `git show`).
  **The stale-error fix was never exercised on the real driver or the GUI.**
- 84 rows = **84 assertions over ~14 scenario families**, dominated by 30
  same-scope cycles and 20 relaunches. The matrix preflight **refuses** any
  dirty kernel state (RUNNING service / nonzero WFP objects / handles), so
  recovery from the product's own residue is untestable by construction.
- The fixture is one blackholed server (`8.8.4.4`); **all 30** judged windows in
  the 20 saved quit traces are `injected=0 from_relay=0 share=0.0%
  verdict=no_reply_from_relay`, **0** `Locked server stale` lines occur, and the
  cycle cadence after a Stop is **≥2.7 s** by construction (700 ms + 2,000 ms
  settle) — versus **1.279 s** in the field.
- The update row passes because the **harness** stops the WinDivert service
  (`service_stop_issued=True`, `wfp_clean_after_stop=True`); the matrix knows
  service stop is the reset and never exercises the app doing it.
- The trace **has no build stamp** and contains lines no committed revision can
  emit (`WinDivert handle open retry … pin_active=true` at 06:01/06:03), so it
  spans at least two binaries. Only the 06:42:38–06:50:15 elevated session
  matches the committed 1.12.5 policy by line shape (`[WD]` F7, `[SV]` F7,
  `[EV-01]` §6).

### 1.3 What is genuinely unknown

| Unknown | Why it is unknown | What answers it |
|---|---|---|
| Which kernel call returned `0x8032000A`, and which object held the reference | One errno for four round-trips; driver teardown is void/debug-only | WFP snapshot + debug driver/ETW around the open (blocked by a test-signing gate) |
| Whether the trigger is session length, traffic volume, starvation state, or close sequence | Only failing session was long; no WFP capture; no fast-restart row | T17 / WD-T2/T3/T4/T8 |
| Who requested the 06:49 Stop and the two starts | No action/reason fields exist; presence auto-start line absent | `interceptor_state` + `gui.action` instrumentation |
| Whether the trace's 06:49 segment is the shipped 1.12.5 binary | No build stamp; mixed builds | Build stamp + build-stamped reproduction on release HEAD |
| What the user saw on screen at 06:49 | GUI never logs error/word transitions; no screenshot in the evidence set | `ui_error`/`ui.word` events + support bundle |

**Verdict:** the failure is real and user-visible; its cause is unmeasured; the
release claim attached to it is not supported. The path forward is not "one more
close-path patch" — it is *make it measurable, make the failure recoverable,
make the user-facing story true, then decide the architecture with data*.

---

## 2. Settled disputes (the five decisions the audits left open)

Each row states the audit positions, then this document's settlement. Settlements
bind the roadmap; where a call needs the owner it is flagged.

### D1 — Is the pin salvageable?

**Positions.** `[WD]` §4/F5: a held pin owns **no WFP filter/callout**
(`filter="false"` installs nothing), it cannot serialize the per-handle WFP
teardown, and the driver-unload churn it was meant to prevent **does not happen**
in the observed environment (driver stays loaded after the last close; provider
+ 20 sublayers persist until service stop/reboot). It concludes the pin is "the
wrong mechanism for this failure" but "not fundamentally wrong as a device
handle", and offers T7 as the removal experiment. `[SV]` §2.4: pin-active is
"neither necessary nor sufficient" for `FWP_E_IN_USE` (failures occur with and
without it). `[GT]` F2: the pin matters only because its presence **suppresses
the retry**, turning a possibly transient state into a terminal one.

**Settlement — salvageable, but demoted from guarantee to handle.** Keep the pin
for the P0 patch; it is not the cause, removing it would broaden the change, and
the observed failure has a recovery problem, not a pin problem. Strip its two
harmful roles immediately: (a) **never suppress retries because a pin is held**;
(b) never describe it as protecting WFP state ("keeps the driver referenced" is
the whole claim). Its long-term retention is decided by data (T7, WD-T7), not by
narrative. *Confidence: high that the pin is not the root cause; medium on
eventual retention.*

**Consequence:** P0-2 (retry) and P0-5 (truth pass) are required even if the pin
later disappears; a "remove the pin" track is P1-6, gated on the T7 comparison.

### D2 — Should Stop be removed for transitions?

**Positions.** `[GT]` §5 invariant I1: "a game-server/region change must never
change the capture plane"; server/region/relay changes are destination/route
events, and the trace shows the transitions themselves did **not** stop capture.
`[WD]` §6.1 evaluates "persistent capture — never Stop on transition" and
concludes it **is not a fix for this failure** (it would not have prevented the
06:49 event if the Stop was manual, and a long-lived session is exactly the
workload correlated with the fault; a pause gate would hide accumulated WFP
state). `[SV]` makes no capture-semantics recommendation.

**Settlement — remove the *coupling*, not the Stop.** A transition must never
induce a capture teardown (I1/I2 enforced and test-asserted), and the product
must not force the user through Stop→Optimize to apply a relay decision (D3).
But true Stop stays a first-class user intent, and persistent-capture/pause-gate
is **not** adopted as the primary fix. Rationale: it removes the failure window
only for transitions, leaves the fault class untouched for real stops, and
converts a truthful "capture is off" into a hidden gate. *Confidence: high.*

**Consequence:** P0-4 (no implicit pre-stop / explicit restart), P1-1 (in-place
relay), P1-2 (presentation). Do **not** queue "pause/resume interception" as the
stability fix; if ever pursued, it is a product change with its own proof.

### D3 — Is in-place relay switching the primary fix?

**Positions.** `[GT]` F3 (High): every live-session destination change logs
"Keeping the live session … applies next session"; the in-place primitive
`commands.switch_relay → engine.switch_relay` exists and **the GUI never calls
it**; applying the choice means Stop→Optimize — exactly the action that then hit
`FWP_E_IN_USE`. It ranks this P1. `[WD]` §6.1/§6.9 treats transition safety as a
different task owner and explicitly says it is "not a fix for this failure".
`[SV]` F3: the fixture cannot exercise transitions at all.

**Settlement — primary fix for the *transition path*, not for `FWP_E_IN_USE`.**
Two-layer answer, and both halves are binding:

- **Yes** for the user-visible transition trauma: wiring `switch_relay` (plus
  I5/I9 ordering fixes) removes the forced Stop→Optimize that exposed the
  capture fault. That makes it **P1-1, the first item of the next release
  train**, because it removes a user path into the failure.
- **No** as the capture-fault fix. The P0 items remain retry/recovery,
  diagnostics, and the error/registration state machine. A relay switch cannot
  repair a WFP open failure; the trace shows the route plane registering
  successfully while capture was dead.

*Confidence: high.*

### D4 — What error/registration state machine is needed?

**Positions.** All three audits converge; this is the most code-certain finding in
the set. `[GT]` §3.5/F1: a retired supervisor publishes `Initial::Done(0)` and
the engine writes `control_registered=false` +
`registration_error=Some("control-plane registration failed")`; the surviving
replacement registration **never writes `EngineStatus`**, so the card persists
while traffic relays. `[GT]` F4/F5: `Error` outranks `Optimizing`, and generic
command errors are sticky (`finish()` does not clear `last_error`;
`clear_error()` has no production caller). `[SV]` F6/Q7 says the same by source
reading and marks it a *code-reading* finding. `[GT]` §5.2/§5.4 I5–I7/I13 and
`[SV]` §5.1 `ui_error` both prescribe the fix shape.

**Settlement — a generation-scoped, source-tagged error/registration lifecycle is
mandatory P0.** Required contract:

1. **Generation guard.** Every registration write to shared status carries a
   supervisor generation; only the current generation may publish. A retired
   generation's result is discarded (`superseded=true`), never mapped to failure.
2. **Registration ledger.** Each registration has `{relay, destination,
   generation, session, outcome}` and a `prev_session`/`superseded`/
   `close_reason`; at most one active session; every session is resolvable to a
   record.
3. **Error records carry source + epoch + auto-clear rule.**
   `{source: Capture|Route|Registration|Config|Game, epoch, kind, detail,
   auto_clear}`; a same-source healthy result at the same or newer epoch clears
   it. Generic command errors must either clear on the same command's success or
   be explicitly dismissible — not latch for the process lifetime.
4. **No masking.** While capture is healthy, non-fatal route/registration
   errors render as a banner *in addition to* the live word; only a latched
   capture fault may use the ERROR word.
5. **Observable lifecycle.** `ui_error show/clear/replace` and `ui.word` changes
   are logged with cause, so the next incident can be triaged from the trace.
6. **Truthful copy.** Error text names the subsystem that failed ("the game
   capture", "the relay link") and a remedy that is actually available.

*Confidence: high (mechanism [C] in `[GT]`; card identity [P]).*

### D5 — What recovery replaces "restart Windows"?

**Positions.** `[WD]` §6.2/§6.3/§6.5/§6.9 recommends: bounded retry while
pinned, then a single pin-cycle (release/reopen the pin), and an explicit
user-approved **"Reset interception driver"** (`sc stop WinDivert` after
checking no other WinDivert client) as the recovery action, replacing the
reboot-only advice. `[WD]` F2: the pinned path currently makes **one** attempt.
`[SV]` A11/exit criteria: no acceptance row may require a reboot; "reboot
required" is a defect signal unless the owner accepts it. `[GT]` F11/I15: the
in-app remedy text is a hard-coded sentence that misattributes a same-process
condition to "a previous run" and says restart Windows.

**Settlement — a four-rung recovery ladder with published budgets.** Reboot is
the last rung, never the first:

| Rung | Action | Budget / condition | User-visible |
|---|---|---|---|
| R1 | Bounded retry of the same open while pinned | 4–6 attempts over ~5 s, exponential backoff (start 50 ms, cap ~350–500 ms); never unbounded | "retrying…" not ERROR |
| R2 | Single pin-cycle: release pin, reopen pin, retry once | once per failure episode | silent unless it works; log event |
| R3 | **Reset interception driver**: `sc stop WinDivert` after proving no other WinDivert client holds a handle (and after closing our handles) | explicit user action + shared-driver warning | "Reset interception driver" button/card |
| R4 | Reboot | only if R3 is unavailable/fails *and* an external holder cannot be excluded; copy must say why | last resort |

R1/R2 are zero-cost user-space changes. R3 requires elevation (the GUI already
requires it for capture) and a **shared-driver warning** because WinDivert is
used by other applications; whether to expose it at all is **OWNER** decision
OD-1. R4 copy must name the reason, and the app must offer the support bundle
first. *Confidence: high on direction; OD-1 gates the R3 surface.*

---

## 3. Prioritised roadmap

Priorities mean: **P0 = next patch; nothing in a stability claim ships before
these** · **P1 = next release train** · **P2 = architecture/quarter**. Every item
is a proposal; none is implemented or verified. Owner persona = who should hold
the task; reviewer persona = who should sign it (per the new personas).

### 3.1 P0 — next patch (stop the unrecoverable path; make it measurable)

| ID | Change | Rationale (audit source) | Depends on | Owner / reviewer | Risk | Test gate |
|---|---|---|---|---|---|---|
| **P0-1** | **Lifecycle instrumentation v1**: startup build stamp (`version`, `commit`, DLL/SYS SHA256, `boot_id`, `prev_shutdown`); `event=interceptor_state` with a **closed** `reason`/`actor`/`command_id` enum; extend `windivert_open` with `reason`, `pin_active`, `ms_since_last_close`, `fwp_code`, snapshot id; `control_registration` ledger fields; `presence` state (present/none/error); `watchdog` verdict fields; `ui_error`/`ui.word`; a stable `lifecycle.jsonl`; rate-limited support bundle on open failure (WFP/service/handle read-only snapshot + UI dump) | All three audits: the 06:49 actor, binary, holder, and card state are unanswerable today (`[SV]` §5, `[EV-03]` §1–3; `[GT]` F6; `[WD]` R1). Without this, no later gate is verifiable | none | `rust-dev` / `stability-verifier` | Medium: log volume, PII discipline (no payloads/tokens in public evidence) | T15 + A12 + schema review; a real run whose trace contains the new fields |
| **P0-2** | **Restart resilience**: remove the `!pin_active` condition from the `FWP_E_IN_USE` retry decision; bounded retry (4–6 attempts / ~5 s) even with a pin; one pin-cycle rung; typed capture fault (`FAULT_LATCHED`) with close-age context; recovery ladder R1/R2; truthful non-reboot copy | `[WD]` F2/§4.5/§6.2–6.3; `[GT]` F2/P0-3; `[SV]` F6/Q8. The failure is recoverable-looking; the policy makes it terminal after one attempt | P0-1 for acceptance evidence; OD-1 only for R3 text | `rust-dev` / `windivert-expert` | Medium: a retry loop that burns CPU or hides a persistent leak; must be attempt- and wall-clock-bounded; may delay the error up to budget | T7 + T17; WD-T8/T9; A4/A5; failing-before on a fault-injected release of the WFP reference (`[WD]` §7) |
| **P0-3** | **Error/registration state machine** (D4 contract): generation-scoped status writes; superseded ≠ failed; registration ledger; source+epoch error records with clear-on-recovery; no live-state masking; generic command-error clear/dismiss | `[GT]` F1/F4/F5, §5.2/§5.4 I5–I7/I13; `[SV]` F6/Q7; `[EV-03]` §2.4/§2.7. The observed card persisted while traffic relayed — a truthfulness defect independent of FWP | P0-1 (events), interface copy owner (OD-7) | `rust-dev` + `interface-designer` / `game-networking-expert` | Medium: touches shared status used by CLI/GUI; must not clear unrelated errors (test-pinned) | T10/T11/T13; A8; unit tests for superseded generations + error epochs |
| **P0-4** | **Start/Stop semantics**: no implicit pre-stop of a healthy capture from a start request (explicit `restart` only, or reject "already optimizing"); every capture-crossing action logs `source` + `command_id`; fix the hold ordering (hold state installed before the first tunnel send) | `[GT]` §3.2/P0-3/I2/I3/I11/I14; `[EV-GT]` §3.2. `engine.start_interceptor_request` pre-stops any existing run — a start during a live session is a silent teardown + reopen | P0-1 | `rust-dev` / `qa-engineer` | Medium: changes user-visible command behaviour; a rejected Optimize needs a clear message | T12/T15; unit for "start while capturing is a no-op/rejection"; A1 |
| **P0-5** | **Truth pass (docs/code comments/logs/copy)**: pin claim scoped to "keeps the driver referenced; owns no WFP object"; remove "a WFP callout/filter for each open handle" (`docs/troubleshooting.md` lifecycle section); replace the hard-coded reboot remedy with the R1–R4 wording; state that provider + 20 sublayers persist until service stop/reboot; correct release-note/state over-claims about the matrix | `[WD]` F5/F9, `[WD-claims]` C1–C5; `[SV]` F1/F6; `[GT]` F11. These sentences are the reason the next investigation inherits a false premise | D1, D5 text; OD-7 for user-facing copy | `architect` / `windivert-expert` | Low: docs-only, but user-facing copy needs owner sign-off (9 locales) | Doc review against the audit claims; no "guarantee"/"prevents" wording; copy reviewed by owner (OD-7) |
| **P0-6** | **Evidence gate for the patch**: candidate binary == release HEAD by hash (GUI, proxy, DLL/SYS) recorded in `run.json` **and** logged at startup; add A2 (two-endpoint transition fixture) and A11 (dirty-kernel recovery) to the matrix; add fast-restart (A4/A5) and long-session (A6) rows; a claimed detection of the WFP reference holder must be captured, not inferred | `[SV]` §3/F1–F4, A-row rules, exit criteria; `[WD]` F8; `[EV-02]` §8. Current matrix structurally cannot cover the field state | P0-1..P0-5 for green rows; OD-6 for host time | `qa-engineer` + `stability-verifier` / Lead | Medium: harness work, clean-boot host time, admin; re-runnability of phase evidence | A1–A12 (at minimum A1/A2/A4/A5/A8/A11/A12); two consecutive independent full runs; no row may require reboot to pass |

**P0 exit criteria:** all six items landed on the release branch; the candidate
artifact hashes match the tested binary; the P0 gate rows pass twice under an
independent verifier; the U-list is explicit and owner-accepted; no stability
claim is attached to `84e15c3`. **Until then, no tag/publish of a stability fix,
and no 1.12.5 "verified" claim.**

### 3.2 P1 — next release train (make transitions safe by construction)

| ID | Change | Rationale | Depends on | Owner / reviewer | Risk | Test gate |
|---|---|---|---|---|---|---|
| **P1-1** | **In-place relay update**: on `ServerLocked` while capturing, call `commands.switch_relay` with the destination; publish relay/token only after registration succeeds; keep the hold-based switch only for the pre-first-packet window; route the manual dropdown through the same command; fix `server_chosen`/hold ordering (I9) | D3; `[GT]` F3/F10/§3.4/P1-5; `[EV-GT]` §3.5. Removes the forced Stop→Optimize that exposed the capture fault | P0-3 (generation status), P0-4 | `rust-dev` / `game-networking-expert` | Medium-High: live data-plane move; old-token grace and rollback must be tested | T1/T3/T12; A2/A3; assert **zero handle close** during the transition, not just a green final state |
| **P1-2** | **Presentation split**: live word never replaced by a non-fatal route/registration warning; banner + details; action slot offers in-place switch where meaningful; error footer names the failing subsystem | `[GT]` F4, §5.6; `[SV]` A8/UI-truth rule | P0-3 | `interface-designer` / `ux-lead` | Medium: copy/i18n cost, 9 catalogs; easy to over-clutter at 320 px | T13 + A8 UI captures; semantic dump + screenshot per state |
| **P1-3** | **Presence semantics**: worker returns `Present` / `None` / `ScanError` (three-way); absence requires N=3 clean `None` or observed process exit; scan errors counted, never treated as exit | `[GT]` F7/§5.2 I8 | P0-1 | `rust-dev` / `qa-engineer` | Low-Medium: a missed exit leaves capture running; a debounce that is too long delays stop | T8/T9; A7; unit + injected scan fault |
| **P1-4** | **Bounded in-place failover on confirmed starvation**: opt-in policy flag (preserve the 2026-11-20 "never take the route silently" owner decision as the default); ranked candidate; registration-first; K=2 then warn-only; one-click "switch relay" fallback. Wire or delete `client/src/route/failover.rs` | `[GT]` F9/§5.5; owner decision 2026-11-20 recorded in `wat/state/decisions/2026-11.md` | P1-1, OD-3 | `game-networking-expert` (design) + `rust-dev` / `net-eng` | Medium: taking the route without a registration success was the original hazard; must never stop capture | T5; A6 (starving session); relay-side logs |
| **P1-5** | **Soak + transition evidence**: 60–120 min traffic runs with ≥10 server transitions and ≥20 in-place re-registrations; periodic WFP/handle snapshots; dirty-kernel matrix that starts from a known product-created state; per-phase trace slices and UI captures retained; regression trace corpus | `[SV]` H1/§6; `[WD]` R5; `[EV-02]` §7. The only failing session was 364 s; QA sessions were 11.8–12.2 s | P0-6, OD-6 | `qa-engineer` / `stability-verifier` | Medium: host time, flake management, storage; must not become a checkbox | A6 + T1–T6/T9/T14/T16; two-run independence; evidence-completeness checklist |
| **P1-6** | **Pin-retention decision experiment**: T7 pin-held vs pin-released on the same long scenario; record whether the pin changes any measured outcome (opens, failures, latency, update friction); decide keep/remove with data | D1; `[WD]` Q10/T7 | P0-1, P0-2 | `windivert-expert` + `qa-engineer` / `architect` | Medium: removing the pin changes self-update gating and the Stop story; OD-2 | WD-T7; decision record in `wat/state/decisions/` with evidence |

### 3.3 P2 — architecture/quarter (only after P0/P1 data)

| ID | Change | Rationale | Depends on | Owner / reviewer | Risk | Test gate |
|---|---|---|---|---|---|---|
| **P2-1** | **Auth/menu provisional destinations**: classify the first lock as provisional until sustained bidirectional traffic; never let it pin a relay decision; re-rank on the match server | `[GT]` F8/P1-7 | P1-1 | `game-networking-expert` | Medium: misclassification could delay the only working route | T2; A3 |
| **P2-2** | **Helper service / elevated capture owner** (only if H2/H4 is confirmed): long-lived elevated process owns capture handles; GUI restart no longer churns WinDivert | `[WD]` §6.4 — explicitly "defer until the mechanism is measured" | P0-1 evidence | `architect` + `rust-dev` / `sec-ops` | High: IPC/security/installer surface; does not by itself fix the per-handle teardown race | design review + threat model + A10/A11 |
| **P2-3** | **Own/patched kernel component** (teardown acknowledgement) | `[WD]` §6.6/§6.8: the only way to get synchronous WFP teardown evidence | P0-1 data; **OD-5 cost/signing gate** | `architect` / owner | Very high: EV signing + MS attestation + GPLv2 obligations + new test surface | not plannable before OD-5 |
| **P2-4** | **Telemetry-backed stability SLOs + canary**: failure per 1,000 starts by reason; dirty-kernel recovery success; stale-error age p95; canary driver-affecting changes to the owner first | `[SV]` H2; `[EV-03]` §3.4 | P0-1, P1-5 | `dev-ops` + `stability-verifier` | Medium: zero-cost hosting constraint (`[COST_STUB]`); local counters first | counters live in a real run; no PII |
| **P2-5** | **Unify the two WinDivert lifecycles**: legacy redirect backend (`client/src/capture/windivert_redirect.rs`) and the main interceptor must share one policy, or the legacy one is retired | `[GT]` §3.7: the same re-detect wording exists in a second loop; a fix can silently cover only one | P0-2 | `rust-dev` / `windivert-expert` | Medium: two code paths, feature-gated builds | feature-gated tests on Windows (`--features windivert-redirect`) |
| **P2-6** | **Support/known-states docs**: a public "known interception states" page (what each remedy means, when to reset the driver), plus a parser/replay tool for lifecycle traces | `[SV]` H2/§5.3; `[WD]` R4; keeps support from re-learning the ladder | P0-1, P0-5 | `architect` + `ux-lead` | Low | doc review; locale parity if user-facing |

### 3.4 Dependency graph (critical path)

```
P0-1 instrumentation ──┬─► P0-2 retry/recovery ──┬─► P1-6 pin decision
                       ├─► P0-3 gen-scoped status ─┼─► P1-1 in-place relay ──┬─► P1-4 failover
                       ├─► P0-4 start/stop sem. ───┘                         └─► P2-1 provisional dest
                       └─► P0-6 evidence gate ──► P1-5 soak/transition evidence ──► P2-4 SLOs
P0-5 truth pass ──► P2-6 support docs
P0-2/P0-1 data ──► P2-2 helper service (only if H2/H4) ──► P2-3 kernel (owner gate)
```

P0-2 and P0-3 can be implemented in parallel; **their acceptance is blocked on
P0-1**, because without reason/build/holder fields a green row cannot be
distinguished from a lucky one. P1-1 must not start before P0-3, or the GUI will
trade one stale card for another.

---

## 4. Phases (entry/exit, not dates)

### 4.1 Short term — next patch (P0)

**Entry:** this roadmap accepted; OD-1/OD-2/OD-7 answered. **Work:** P0-1…P0-5
implemented; P0-6 harness extended. **Exit:** candidate = release HEAD by hash;
P0 gate rows green twice under an independent verifier; the next field trace is
attributable (build, actor, reason, holder snapshot, card events); recovery never
shows "restart Windows" as the first or only remedy; docs/copy claims match the
code. **Still open by design:** root cause (may remain unknown), transition
safety, relay in-place switching.

### 4.2 Medium term — next release train (P1)

**Entry:** P0 exit met on a real driver. **Work:** P1-1…P1-6. **Exit:** A2/A3/A6
pass with zero capture teardown on transitions and two-way traffic after each
transition; a 60–120 min soak with ≥10 transitions and ≥20 re-registrations has
0 `FWP_E_IN_USE`; starvation failover (if approved) moves the route only after
registration; pin-retention decision recorded with evidence; phase evidence
re-runnable from the stored directory.

### 4.3 Long term — architecture (P2)

**Entry:** P1 exit met **and** the mechanism question answered by P0-1 data, or
explicitly accepted as unknown. **Work:** P2-1…P2-6, selected by evidence, not by
symmetry. **Exit:** stability SLOs in place from local counters; the capture
architecture choice (pin + true Stop / helper service / own kernel) is recorded
as an owner decision with measured trade-offs; support has a truthful known-states
ladder; the "restart Windows" class of advice is gone.

---

## 5. The T1–T17 matrix (synthesized)

Rules that apply to every row (adopted from `[SV]` §4 and its training brief):
build/commit/driver hashes recorded and logged; **two-way traffic** for any
"traffic-bearing" claim (≥2 consecutive judged windows with `injected > 0`,
`from_relay > 0`, share ≥ 1/8); handle + registration ledgers; WFP snapshot at
phase boundaries; UI truthfulness capture; error lifecycle asserted; **no row may
require a reboot to pass**; a row that cannot run is `U`/`BLOCKED`, never PASS.
The `A`/`WD` columns are the crosswalk to the other two audits' matrices; `—`
means that audit has no equivalent row.

| ID | Scenario | Given / When / Then | Invariants | Level | Crosswalk | Phase gate |
|---|---|---|---|---|---|---|
| **T1** | Settlement/POI switch | live capture; server changes A→B within 30 s → zero handle closes; `destination.changed`; in-place re-registration; relay choice either applied via `switch_relay` or kept with a route banner; no ERROR word and no stale card | I1,I4,I5,I7 | U+I+M | A2, WD-T5 | P1 |
| **T2** | Login menu → match | presence only; first lock is an auth/lobby flow; match server appears later → capture never restarts; provisional destination yields; relay re-ranked; no registration error card | I1,I8,I9 | U+I+M | A3 (partial) | P2 |
| **T3** | Region shift across continents | server IP changes region while capturing → relay switch awaited and registered with the destination, published only on success; capture untouched; old-token grace respected | I4,I12 | U+I+M | A3 | P1 |
| **T4** | Reconnect (control-plane loss) | drop QUIC mid-session → supervisor reconnects with backoff; capture untouched; status CONNECTING→REGISTERED; no stale card; data plane keeps the current relay | I1,I5,I12 | I+M | A3 | P1 |
| **T5** | Relay failover after starvation | 2 judged windows with no reply and a ranked alternative → one in-place switch, registration first, capture untouched; if it starves too, one bounded second attempt then warn-only | I4,I10 | U+I+M | A6 (starving), WD-T4 | P1 (OD-3) |
| **T6** | Server lock stale on the same server (loading screen) | 5 s silence, same server returns → `destination.stale` then re-lock the same server; no relay re-choice, no restart, no error | I1,I9 | U+I | A2 (subcase) | P1 |
| **T7** | Rapid Stop→Optimize after an active session | capture traffic → Stop → Optimize at 1 s / 4 s / 10 s → either bounded retry succeeds within budget, or a typed capture error with Retry and close-age context; never "restart Windows" for a same-process condition | I2,I11,I15 | M | A4,A5; WD-T2/T3/T8 | **P0** |
| **T8** | Presence scan failure | inject `ScanError` → `presence.scan result=error`; no stop, no state change; counter increments | I8 | U+I | A7 | P1 |
| **T9** | Presence flicker | one/two `None` results then `Present` → no stop (debounce), no restart loop; stop only after N absences | I8 | U+I | A7 | P1 |
| **T10** | Stale registration error clears | supersede a supervisor during start, replacement registers → no ERROR word; superseded ignored and success published; visible in trace | I5,I6,I13 | U+I | A8 | **P0** |
| **T11** | Superseded supervisor is not a failure | unit: start supervisor's generation retired before its first attempt completes → no `registration_error`; `superseded_total` increments | I5,I13 | U | A8 (unit) | **P0** |
| **T12** | Hold ordering | start with an already-connected relay; first packet arrives before the hold is installed → no packet tunnelled before the hold; no destination-report supervisor churn at start; exactly one start registration | I3,I9,I13 | U+I | A1 (partial), WD-T0 | P0-4 gate |
| **T13** | Error presentation while capturing | live capture + forced route error + forced registration error → word stays OPTIMIZING (or warning), banner shows, action stays STOP; clearing the source restores the word without a restart | I6,I7 | U+I+M | A8 UI-truth | **P0** |
| **T14** | Game exit | process disappears → after debounce capture stops, state cleared, relay disconnected, no ERROR card | I8,I11 | I+M | A7/A9 | P1 |
| **T15** | Action audit | every button/tray/shortcut/menu action → exactly one `gui.action` line with source and state transition; Stop attribution possible from the trace alone | I14 | U+M | `[SV]` §5.1 only | **P0-1 gate** |
| **T16** | Elevation/update handshake | elevation restart or self-update while capturing → true Stop, pin release ordering, no stranded handles, no stale WFP object | I11,I15 | M | A9/A10; WD-T12 | P1 |
| **T17** | FWP close-age matrix | Stop with (a) no traffic, (b) light traffic, (c) starved burst; reopen at 0.5 / 1 / 3 / 10 s with the pin held → record close→open outcome, correlate, define the retry budget from data | I15 (supports F2) | M | A4/A5; WD-T2/T3/T8/T9 | **P0** |

**P0 gate rows:** T7, T10, T11, T12, T13, T15, T17 (plus A1/A2/A4/A5/A8/A11/A12). **P1
gate rows:** T1–T6, T9, T14, T16 (plus A2/A3/A6 and the soak). T2/T8 are
P2-gated by the provisional-destination work. A failure that only changes the
final UI state does **not** pass T1–T6: the row must assert the **absence of
teardown events** during the transition (`[GT]` §7 note).

---

## 6. Test gates per item and release criteria

### 6.1 Per-change gates (every item, no exceptions)

1. **Failing-before reproduction from the field shape** (transition / starving /
   dirty kernel / fast restart), not from a mock of the fix.
2. **Mutation check**: revert the fix → the reproduction fails; restore → passes
   (`lightspeed-verify`).
3. **Evidence**: command transcript or raw trace, named path under
   `wat/evidence/<YYYY-MM-DD>/`, identity block (build/commit/driver hashes,
   host/clean-boot state).
4. **Independent verifier**: the fix author does not sign its own evidence — the
   `stability-verifier` re-runs on a different worktree/binary.
5. **No U-row hidden as PASS**: any unexercised scenario is written `U` with what
   would verify it.

### 6.2 P0 patch gate

- Candidate binary identity == release HEAD (GUI, proxy, `WinDivert.dll`,
  `WinDivert64.sys` SHA256) in `run.json` **and** in the startup log.
- Rows: A1, A2, A4, A5, A8, A11, A12 + T7, T10, T11, T12, T13, T15, T17 PASS.
- Two consecutive full runs by an independent verifier.
- No row that requires a reboot to pass; if a reboot is genuinely unavoidable,
  the product must say so **before** the failure and the owner must accept it.
- New instrumentation present in a real run (`app_start`, `interceptor_state`,
  `windivert_open`, `control_registration`, `ui_error`, `presence`).
- Docs/copy corrected; release notes name the tested artifact and the U-list.

### 6.3 Release-train gate (P1)

- A1–A12 + T1–T6, T9, T14, T16 PASS; T5 only if OD-3 is approved.
- A2/A3: two-endpoint fixture, two-way traffic after each transition, **zero**
  handle closes during the transition, registration ledger consistent.
- A6: 60–120 min, ≥10 transitions, ≥20 in-place re-registrations, 0
  `FWP_E_IN_USE`, stable handle/fd/thread counts and WFP object counts.
- P1-5 evidence is re-runnable from the stored directory (per-phase slices, UI
  captures, hashes).
- Owner-accepted residual U-list.

### 6.4 Long-term gate (P2)

- Stability SLOs measured from local counters over a defined window.
- The architecture decision record (pin vs helper vs own kernel) names the
  measurement that decided it and its falsifier.
- Support/marketing copy makes no guarantee the code cannot keep.

---

## 7. Open owner decisions and cost/signing gates

| ID | Decision | Recommendation | Blocks | Cost/impact |
|---|---|---|---|---|
| **OD-1** | Expose **"Reset interception driver"** (`sc stop WinDivert`) in-app, with a shared-driver warning and a no-other-client check? | Yes — it is the only proven userspace reset (`[WD]` §6.5) and replaces reboot-only advice; keep it explicit, never automatic | P0-2 R3 rung, P0-5 copy | $0; risk to other WinDivert apps on the machine (mitigated by detection + warning + explicit action) |
| **OD-2** | Long-term pin retention after the T7 experiment | Keep through P0/P1; decide with T7 data. Do not remove on narrative | P1-6, P2-2 | $0; update/self-update gating friction if kept |
| **OD-3** | Automatic failover on confirmed starvation (currently warn-only by the 2026-11-20 owner decision) | Opt-in bounded in-place switch (K=2), default off until T1/T5 evidence; preserves "never take the route silently" | P1-4 | $0; needs user-visible policy + copy |
| **OD-4** | Release policy: a stability claim requires A1–A12 on the **exact** release artifact with identity match; 84/84 on `84e15c3` cannot be cited for `01438e0` | Yes; also publish corrected release notes/state text (no silent rewrite of published tag bodies) | P0-6, any tag/publish | $0; may delay a tag until the gate is green |
| **OD-5** | Kernel component ownership/patched driver (teardown acknowledgement) | Not now. Revisit only if P0-1 evidence confirms H2/H4; requires explicit spend approval under `[COST_STUB]` | P2-3 | **EV code-signing cert + Microsoft kernel attestation + hardware token (annual fee), GPLv2 source obligations, loss of upstream signature**, new test surface |
| **OD-6** | Test-host + owner time for clean-boot matrices, 60–120 min soaks, dirty-kernel rows, debug/test-signed driver experiments | Schedule dedicated clean-boot windows; the current harness needs admin + pristine kernel | P0-6, P1-5, P2 mechanism experiments | $0 cash; owner machine time; test-signing mode on an isolated host for a debug WinDivert build (not shippable) |
| **OD-7** | User-facing copy changes: error text that names the failed subsystem, recovery wording, removal of "restart Windows" as first-line advice (9 locales) | Approve in the P0 patch; keep "full shutdown" as a documented fallback, not the headline | P0-3, P0-5, P1-2 | $0 cash; localization + UX review effort |
| **OD-8** | If the mechanism is still unknown when the P0 patch is ready: ship the bounded-recovery patch with the cause documented as unknown? | Yes — recovery + truth + instrumentation are independently valuable; do **not** claim a fix | P0-6, release messaging | $0; a known-issue entry instead of a false guarantee |

**Zero-cost guards.** Bundling official WinDivert 2.2.2 stays zero-cost with the
license notice. Retry/pin-cycle/in-place switch/presentation/evidence work are
userspace-only. The only paths that add spend are P2-3 (kernel/signing) and any
paid telemetry hosting; neither starts without an explicit owner gate. Do not
patch the bundled signed driver as a "temporary" measure: it loses the signature
and triggers GPLv2 distribution obligations (`[WD]` §6.8).

---

## 8. What this document does not claim

- It does not claim the field fault reproduces on 1.12.5, nor that any change
  above fixes it.
- It does not claim the QA matrix was dishonest or useless: the 84 rows are real
  and the handle lifecycle family is the strongest evidence in the set
  (`[SV]` §3.4, `[EV-02]` §9).
- It does not attribute the 06:49 Stop to the server transitions: the trace
  contradicts that; the actor remains unknown (`[GT]` §2.3, `[WD]` §2.4,
  `[SV]` §2.3).
- It does not claim the pin is harmful; it claims the pin's *guarantee* is false
  and its retry suppression is harmful.
- It does not select the kernel-component option; it prices it and defers it.

## 9. References

- `[WD]` [`docs/audit/windivert-lifecycle-2026-11-23.md`](../audit/windivert-lifecycle-2026-11-23.md)
- `[GT]` [`docs/audit/game-transition-state-machine-2026-11-23.md`](../audit/game-transition-state-machine-2026-11-23.md)
- `[SV]` [`docs/audit/stability-verification-2026-11-23.md`](../audit/stability-verification-2026-11-23.md)
- Training briefs: [`windivert-lifecycle`](../training/windivert-lifecycle-training.md),
  [`game-transition`](../training/game-transition-training.md),
  [`stability-verification`](../training/stability-verification-training.md)
- Evidence notes: [`EV-01`](../../wat/evidence/2026-11-23/stability-audit/01-trace-facts-timeline.md),
  [`EV-02`](../../wat/evidence/2026-11-23/stability-audit/02-qa-coverage-audit.md),
  [`EV-03`](../../wat/evidence/2026-11-23/stability-audit/03-instrumentation-and-open-questions.md),
  [`EV-GT`](../../wat/evidence/2026-11-23/stability-audit/game-transition-audit-notes.md),
  [`WD-claims`](../../wat/evidence/2026-11-23/stability-audit/windivert-lifecycle/claims-and-contradictions.md),
  [`WD-timeline`](../../wat/evidence/2026-11-23/stability-audit/windivert-lifecycle/timeline-facts.md)
- Trace: `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log`
  (1,400 lines, SHA256 `5DFFF05824AE65FC20D4D7281C454E1DC5E52B14CA0B4477DEF562882B317331`)
- Item→finding map: [`wat/evidence/2026-11-23/stability-audit/synthesis.md`](../../wat/evidence/2026-11-23/stability-audit/synthesis.md)
- Code anchors are the audits' references; this document added none and ran no
  build, test, GUI or driver command.
