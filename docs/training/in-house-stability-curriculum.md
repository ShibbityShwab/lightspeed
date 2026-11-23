# In-house stability curriculum — Windows interception, transitions and evidence

> **Audience:** LightSpeed in-house agents (engine, GUI, QA, support) who will
> touch, test, triage or review the Windows capture/tunnel lifecycle.
> **Status:** training material only. It contains no fix and no verified result;
> every example is traced to the 2026-11-23 evidence set.
> **Prerequisites:** `AGENTS.md`, `wat/rules.md`, skill `lightspeed-verify`, and
> the three audit briefs this curriculum synthesizes
> ([`windivert-lifecycle`](windivert-lifecycle-training.md),
> [`game-transition`](game-transition-training.md),
> [`stability-verification`](stability-verification-training.md)).
> **Companion:** the prioritised roadmap
> [`docs/design/windivert-stability-roadmap-2026-11-23.md`](../design/windivert-stability-roadmap-2026-11-23.md).
> **Evidence:** `wat/evidence/2026-11-23/stability-audit/` (trace slice
> `user-server-transition-trace.log`, 1,400 lines, SHA256 `5DFFF058…`).
> **Time convention:** trace timestamps are UTC (`Z`); the owner's host is
> UTC+07. The 06:49 failure window is 13:49 local.

**Learning outcomes.** After this curriculum an agent can:

1. Name the **five planes** a live session spans and say which plane an event
   belongs to.
2. Apply the **WinDivert/WFP lifecycle rules** without inventing a guarantee.
3. Tell a **transition fingerprint** from a **failure fingerprint** in a trace,
   line by line.
4. Explain and audit the **error-card lifecycle** (generation-blind status,
   sticky command errors, masking, stale cards).
5. Choose the correct rung of the **recovery ladder** and refuse to skip rungs.
6. Produce an **evidence bundle** that a second agent can re-check, and mark
   `U` honestly.
7. Recognize and reject the **wrong assumptions** this incident produced.

---

## Module 0 — The five-plane model (the first hour)

A LightSpeed session is not one object. It is five state planes that change for
different reasons. **The single most important rule: a change in one plane must
not silently move the capture plane.**

| Plane | What lives there | Typical events | Code home (1.12.5) |
|---|---|---|---|
| **P1 Presence** | whether the game process exists; 5 s scan | launch, exit, **scan error** | `client-gui/src/app/presence.rs`, `app.rs:2063-2163` |
| **P2 Capture** | WinDivert pin + per-run intercept/inject/shadow handles + the hold gate | start, stop, fatal handle fault | `client/src/interceptor/windows.rs`, `engine.rs:1579-2096` |
| **P3 Destination** | the locked game server (the UDP endpoint the game sends to) | lock, stale, re-lock | `client/src/interceptor/order.rs`, `windows.rs:1492-1559` |
| **P4 Route/Session** | current relay, per-path token, QUIC control-plane supervisors/generations | relay choice, registration, re-registration, rotation | `client-gui/src/app.rs:910-1263`, `client/src/quic/mod.rs`, `engine.rs:704-876` |
| **P5 Presentation** | the state word, banner/card, action button, tray | any of the above, rendered | `client-gui/src/app/route_surface.rs` |

**How to use the model in triage:** when a user report contains two systems
("switching server restarted it and then it couldn't start"), split it into one
sentence per plane and ask for the trace line that proves each crossing.

- A destination change (P3) may re-register the control plane (P4) and change
  what the user sees (P5). It must not close a handle (P2).
- A registration error (P4) is not proof of a capture fault (P2).
- A capture fault (P2) does not imply the route is bad (P4): in the 2026-11-23
  trace the control plane registered successfully 0.43 s after each failed
  start.
- A presence scan error (P1) is not a game exit. Today a failed scan can look
  like `none`, which is a known defect.

**Drill D1 (15 min):** classify each line into a plane and say what it proves:
(a) `Game presence changed: robrun`; (b) `Locked game server: 178.107.13.95:36605`;
(c) `Keeping the live session on 207.246.106.36:4434`;
(d) `WinDivert diverting handles closed; driver pin active=true`;
(e) `Registered with proxy: session=2861874939`.
*Model answer:* (a) P1, the only presence transition in the window; (b) P3, new
lock; (c) P4, a relay decision deliberately not applied; (d) P2, handles closed
— **not** proof that WFP objects are gone; (e) P4, a registration succeeded on
the wire — **not** proof that interception works.

---

## Module 1 — WinDivert/WFP lifecycle rules (the second hour)

Memorise the layer diagram before the rules; the rules are consequences of it.

```
process
 └─ WinDivert device handle (WDF file object)        ← the pin lives HERE
     └─ per-handle dynamic WFP engine session        ← created at CreateFile
         └─ WFP callout + filter                     ← installs only for a real filter
             └─ runtime callout registration         ← per handle, per layer
driver (WinDivert.sys)
 └─ provider + 20 sublayers (DriverEntry)            ← persist until service stop/reboot
```

### The ten rules

1. **A handle is not a WFP filter.** The pin and the inject handles use filter
   `"false"` and install **no** callout/filter; only intercept and shadow own
   WFP objects. Never infer WFP state from a `"false"` handle.
2. **`CloseHandle` success ≠ WFP objects removed.** Driver-side teardown is
   void and debug-only in the release driver; `close -> ok` proves the handle.
3. **The driver does not unload on the last close.** Provider + 20 sublayers
   stay until service stop/reboot. The pin is not what keeps the driver loaded.
4. **`FWP_E_IN_USE (0x8032000A)` is a family**, not a diagnosis: a management
   object reference conflict, or a callout registration/unregistration race.
   It is **not** a synonym for "a previous run left a filter".
5. **One failed open is not a diagnosis.** `WinDivertOpen` = CreateFile +
   INITIALIZE + STARTUP; one errno for four round-trips. Capture WFP state.
6. **Never suppress retries because a pin is held.** The 1.12.5 code does; the
   trace shows the resulting one-attempt failure becoming "restart Windows".
   Any retry must be **bounded by attempts and wall clock**.
7. **Long sessions are the untested regime.** QA cycles were 11.8–12.2 s; the
   field failure followed a 363.9 s traffic session. Any WinDivert change needs
   a ≥10-minute traffic-bearing Stop→start with WFP capture.
8. **Timing evidence beats timing intuition.** Measure the close→open gap.
   Field: successful cycles 0.567–0.577 s; failures at 1.279 s and 4.128 s
   after the close. "Too fast" is contradicted; state your anchor
   (stop-signal→open vs close→open).
9. **A Stop is an action, not a server event.** The two server re-detects in the
   trace closed no handle. Never attribute a Stop to a transition without a log
   line naming the action.
10. **No fix claim without failing-before and after on the real driver.** A mock
    pin seam, a unit test, or a matrix that never combines a long session with
    Stop→start is not evidence for this failure class.

**Drill D2 (20 min):** a teammate says "the pin guarantees the next open, and
close returned ok so WFP is clean". List every rule they violated and what
evidence you would demand instead.
*Model answer:* rules 1–3 and 6: the pin owns no WFP object; close success says
nothing about teardown; no guarantee exists; the code additionally suppresses
retries. Demand a WFP snapshot (`netsh wfp show state`) before/after the close,
the close→open gap, and the build hash proving which binary produced the line.

---

## Module 2 — Transition fingerprint vs failure fingerprint (the third hour)

Reading a trace starts with pattern recognition **before** mechanism.

### 2.1 Transition fingerprint (P2 must not move)

```
🔄 Locked server stale - re-detecting                    ← P3, expected
🔍 Locked game server: <new ip:port>                     ← P3, new lock
Destination-aware relay choice for <ip>: <relay> …       ← P4, ranking
Keeping the live session on <relay>; … applies next session   ← P4, deliberate keep
Connecting QUIC control plane to <relay>:4433            ← P4, re-register
QUIC connection established; Registered with proxy: session=…  ← P4, wire success
[control-plane registered …]                             ← P4, only for the current generation
```

- Hard assertion while reading: **no `WinDivert handle close` and no
  `Shutdown signal received` between the stale line and the next second.** If
  one appears, it is a defect (invariant I1), not a transition.
- "…applies next session" means the product chose not to move the relay. It is
  not an error. It is also not free: the only way to apply it today is
  Stop→Optimize.

### 2.2 Failure fingerprint (P2 fault)

```
🛑 Shutdown signal received                              ← Stop, actor NOT logged
WinDivert handle close: id=2/3/4 … -> ok                 ← handles, not WFP
WinDivert diverting handles closed; driver pin active=true
⚡ Opening WinDivert intercept handle...
WARN WinDivert handle open: id=5 kind=intercept attempt=1 … -> FAILED (os error -2144206838)
[no "handle open retry" line]                            ← pinned no-retry policy
ERROR ❌ WinDivert intercept open failed: … restart Windows …
ERROR start_interceptor failed: AppError { kind: Tunnel, … }
Registered with proxy: session=…                         ← route alive while capture dead
```

### 2.3 The three traps in this evidence set

1. **The same failure can be a transition symptom or a capture fault.** The user
   said "switching settlements restarted it and then it couldn't start". The
   trace shows the transitions did **not** stop capture; the Stop came 64 s
   after the last transition, and the failure is in P2. Split the report.
2. **A stop can be an implicit pre-stop.** `engine.start_interceptor_request`
   stops any existing run before starting. So a bare teardown without a Quit /
   elevation / update / presence line is either a UI Stop **or** the pre-stop of
   a Start action. The trace cannot distinguish them today (no action log).
3. **Frozen lines after a failure are not liveness.** `Auto-detected game` and
   `Latency comparison` repeated with identical values for 31 s after the failed
   restart. Repetition ≠ progress; check that counters/ids move.

**Drill D3 (20 min):** open `user-server-transition-trace.log` lines
`1239–1288` and `1340–1386`. Produce two lists: "transition evidence" and
"failure evidence". Then answer: did a transition close a handle?
*Model answer:* transitions at 06:47:59 and 06:48:34 show stale/lock/choice/
registration lines and **zero** close lines. The closes are at 06:49:38.56, 64 s
after the second transition. No transition closed a handle.

**Drill D4 (15 min):** compute the close→open and stop-signal→open gaps from
lines 1340–1365 and 1369–1386. Which gap values are measured, which are
narrative?
*Model answer:* close (06:49:38.561251) → first open (06:49:39.840319) = **1.279 s**;
stop signal (06:49:38.417500) → open = **1.423 s**; close → second open
(06:49:42.689567) = **4.128 s**; first open failure → second attempt start =
**2.836 s**; open-to-open = **2.848 s**
(narrative "2.83 s" and "1.4 s" are anchor-dependent, not wrong).

---

## Module 3 — The error-card lifecycle (the fourth hour)

The observed user-visible defect: **ERROR + "Registration failed" + only
STOP OPTIMIZING on screen while the Relayed row moved.**

### 3.1 The mechanism (code-certain, per `[GT]` §3.5)

1. A start spawns a registration supervisor; the engine awaits its first attempt.
2. The first tunnel send reports the destination; `quic::report_destination`
   retires that supervisor and starts a **new generation**.
3. The retired generation completes on the wire but fails to publish; on exit it
   reports `Initial::Done(0)` → the engine writes
   `control_registered=false`, `registration_error=Some("control-plane registration failed")`.
4. The replacement registers successfully — **but nothing writes success to
   `EngineStatus`** (only per-path token/session state).
5. `word_for` gives `Error` precedence over `Optimizing`, so P5 renders failure
   while P4 is healthy.
6. Generic command errors are sticky: `finish()` clears only `in_flight`;
   `AppCore::clear_error()` has **no production caller**; the 1.12.5
   `clear_stale_registration_error` clears only command errors containing
   `control-plane registration failed` and is gated on `control_registered=true`
   — which the race sets false. The **engine** field is what `engine_error()`
   reads.

### 3.2 The required contract (audit synthesis)

- **Generation guard:** only the current supervisor generation may publish
  status. `Superseded` is not `Failed`.
- **Registration ledger:** every registration has session/token and is either
  superseded or closed; at most one active session; a registration without a
  close/supersede is a stale-registration signal.
- **Error record:** `{source, epoch, kind, detail, auto_clear}`; clears when the
  same source reports healthy at the same or newer epoch.
- **No masking:** while capture is healthy, route/registration errors are
  banners *in addition to* the live word; only a latched capture fault may be
  the ERROR word.
- **Observable:** `ui_error show/clear/replace` and `ui.word` change events in
  the trace.
- **Copy:** name the failed subsystem and offer a remedy that exists. Never show
  "Registration failed" while traffic is returning.

### 3.3 What to do in triage today

- Do **not** restart capture to clear a card — wrong plane, and after an active
  session it is the action that can hit the FWP failure.
- Capture the trace window; check whether two `Connecting QUIC` lines appear
  ~0.2 s apart during a start and only one `control-plane registered`; check
  whether the card predates the last successful registration. That fingerprint
  is the stale-registration defect.

**Drill D5 (15 min):** from `trace:986, 994–996`, identify which registration
"failed" and which survived, and explain why the failure was published.
*Model answer:* the 06:43:34.992 destination report superseded the start
supervisor; its 06:43:35.231 wire success was discarded (no `control-plane
registered`); the engine waiter recorded the retired generation's `Done(0)` as a
failure; the replacement at 06:43:35.442 registered successfully but wrote no
`EngineStatus`. The failure is a lost-update/attribution bug, not a wire failure.

---

## Module 4 — The recovery ladder (the fifth hour)

The product must never respond to a recoverable capture fault with "restart
Windows" as the first or only remedy. Learn the ladder, its budgets, and whose
authority each rung needs.

| Rung | Action | Budget / precondition | Who can trigger | Notes |
|---|---|---|---|---|
| **R0** | Capture evidence | on every failure: handle table, pin state, `sc query WinDivert`, WFP provider/filter/callout counts, UI dump + screenshot, build hashes | automatic (rate-limited) | read-only; writes only evidence files |
| **R1** | Bounded retry of the same open while pinned | 4–6 attempts over ~5 s, exponential backoff | automatic | must be attempt **and** wall-clock bounded |
| **R2** | Pin-cycle: release pin, reopen pin, retry once | once per failure episode | automatic | cheap test of whether the pin's own session is implicated |
| **R3** | **Reset interception driver** (`sc stop WinDivert`) | explicit user action; prove no other WinDivert client holds a handle first; after our handles are closed | user, with elevation + shared-driver warning | the only proven userspace reset in the QA evidence (`service_stop_issued=True`, `wfp_clean_after_stop=True`) |
| **R4** | Reboot | only if R3 is unavailable/fails and an external holder cannot be excluded; copy must say why | user | last resort; collect the support bundle first |

**Rules of the ladder.** Never skip a rung because it is faster to reboot. Never
loop R1/R2 unbounded. Never take the driver down while another app's handle is
open. Record which rung worked; that record is the next patch's data.

**Drill D6 (20 min):** a report says: "after a long session and a Stop, Optimize
fails with `FWP_E_IN_USE`; second click fails too; I restarted Windows". Which
rungs existed, which were skipped, and what would you collect if it happened on
the current build?
*Model answer:* R1 was deliberately disabled by the `!pin_active` gate, R2 never
ran, R3/R4 were not offered; the app jumped from failure to reboot advice. On the
current build: R0 bundle — trace, `sc query WinDivert`, WFP counts, tool handles,
build hashes, close→open timestamps, session duration/traffic, and whether the
retry budget was attempted.

---

## Module 5 — Evidence and runbook discipline (the sixth hour)

### 5.1 The evidence bundle for any lifecycle bug

1. Full trace (`%LOCALAPPDATA%\Lightspeed\gui-trace.log`) + the slice around the
   event; note that a shared log may mix binaries.
2. Build identity: GUI SHA256/commit, `WinDivert.dll`/`WinDivert64.sys` hashes.
3. Handle lifecycle lines for the whole process (open/close/retry/pin).
4. WFP snapshots: before start, after start+traffic, before Stop, +10 ms/+100 ms/
   +1 s/+5 s after close, before/after retry.
5. Service/driver state: `Get-Service WinDivert`, `sc.exe qc WinDivert`,
   `Win32_SystemDriver`; WinDivert driver events.
6. WinDivertTool handle scan (idle and at failure); `tasklist /m WinDivert.dll`.
7. Exact UTC timestamps and the host zone.
8. UI state (screenshot/semantic dump) whenever an error card is involved.
9. What the user/agent did — and if the product does not log it, say so.

**Privacy:** no payloads, no process command lines, no keys/tokens in public
evidence. IPs and handle ids are operational data; relay session tokens are
credentials.

### 5.2 Fact vs inference note format

```markdown
# <task> — <claim>
Status: PASS | FAIL | U | BLOCKED
Build: <exe SHA256> = <commit>; driver DLL/SYS <sha256>; harness <sha256>
Environment: <host/boot/clean-kernel gate result>
Rows: <exercised>/<total>; <n> PASS, <n> FAIL, <n> U/BLOCKED
Facts: <timestamped lines/measurements>
Inferences: <labelled, with what would confirm>
Not covered / U: <scenarios + why>
Evidence index: <paths + hashes>
```

Rules: never write PASS for a row you did not run; a summary written by another
agent is a claim, not evidence; name the binary and environment in the first
five lines; "U because the fixture has no return path" beats a fake PASS.

### 5.3 Detection drills

**Drill D7 (20 min):** given a trace whose first 900 lines contain
`WinDivert handle open retry … pin_active=true`, is that segment attributable to
1.12.5? What do you write?
*Model answer:* no — committed `should_retry_fwp_in_use` is `!pin_active &&
is_fwp_in_use`; that line shape cannot come from any committed revision, so the
file spans at least two binaries. Write "build identity: U" and scope claims to
the segment that matches the committed policy.

**Drill D8 (20 min):** a matrix reports 84/84 PASS. Name three claims it does
**not** support without reading the harness.
*Model answer:* (i) that a long traffic session then fast Stop→start works (no
row; 11.8–12.2 s sessions, ≥2.7 s cadence); (ii) that game-server transitions or
return-path starvation→demotion are covered (one fixed blackholed server, 0
stale-lock lines, 30/30 no-reply windows); (iii) that recovery from the product's
own kernel residue works (preflight refuses that state), or that the release
binary was tested (matrix ran `84e15c3`; release HEAD `01438e0` came later).

**Drill D9 (20 min):** write the acceptance criteria for the row "Optimize 250 ms
after Stop" so it is not a "no error" row.
*Model answer:* identity recorded; Stop command time, last close time, start
command time, open result + duration; all repetitions deterministic (success, or
a documented lockout with a truthful bounded remedy); handle ids pair; ≥2 healthy
two-way judged windows after; registration ledger consistent; UI shows the real
state; no reboot required.

**Drill D10 (25 min, capstone):** produce a one-page evidence note for the
06:49 sequence, labelling at least five facts and three inferences, and file it
against the right finding IDs.
*Model answer anchors:* facts — Stop 06:49:38.417 with no presence/quit line;
handles 2/3/4 close `ok` 06:49:38.561; `driver pin active=true`; `id=5` FAILED
with `-2144206838` at 06:49:39.841 and no retry; `Registered with proxy
session=2861874939` at 06:49:40.275; `id=6` FAILED at 06:49:42.690. Inferences —
actor of the Stop/starts (unknown), cause of the FWP status (unknown), what the
UI showed (unknown). Findings — capture-plane fault `[GT]` F2/`[WD]` F1–F2;
missing instrumentation `[SV]` F4/`[GT]` F6; remedy text `[WD]` F9/`[GT]` F11.

---

## Review checklist for future agents (pin this)

Use before approving **any** change, evidence note or release claim in this area.
Tick every box or write `U` next to it.

**Claims and identity**
- [ ] Build/commit and driver DLL/SYS hashes are recorded **and equal to the
      shipped/tested artifact**.
- [ ] The trace has a build stamp; every segment cited is attributable to a build.
- [ ] Every "traffic-bearing" claim names both directions (`injected>0`,
      `from_relay>0`), not just outbound counters.
- [ ] "84/84", "matrix passed", "verified" are scoped to environment + binary +
      scenario set; no upgrade from "row passed" to "risk covered".

**Capture plane**
- [ ] No claim that a `"false"` handle protects WFP state.
- [ ] No claim that `close -> ok` means WFP objects are removed.
- [ ] Retries are bounded by attempts **and** wall clock, and cover the pinned path.
- [ ] The failure path has a recovery that is not a reboot; the ladder rungs and
      their budgets are stated.
- [ ] WFP callout/filter counts are captured around the transition; provider/
      sublayers persisting is not called "residue".

**Transitions**
- [ ] A destination/region/relay change is asserted to close **zero** handles
      (not merely to end green).
- [ ] Registration status is generation-scoped; `superseded` is not `failed`.
- [ ] The hold is installed before the first tunnel send; `server_chosen` is not
      set before a choice.
- [ ] Presence: `ScanError` is not treated as absence; absence is debounced.

**Presentation**
- [ ] A non-fatal route/registration warning never replaces the live state word.
- [ ] A same-class success clears the error within the contract time; unrelated
      errors survive.
- [ ] Error copy names the subsystem and a remedy that exists; no "restart
      Windows" as the first-line remedy for a self-induced state.
- [ ] UI state was captured (screenshot/semantic dump) for every state claimed.

**Evidence**
- [ ] Failing-before reproduction exists and matches the field shape.
- [ ] Mutation check (revert fails / restore passes) was run.
- [ ] An independent verifier re-ran it; the author did not sign alone.
- [ ] U-rows are named with what would verify them; no hidden U presented as PASS.
- [ ] No secrets, payloads, tokens or personal data in the evidence.

---

## Common wrong assumptions from this incident

| Wrong assumption | Why it is wrong | What to say instead |
|---|---|---|
| "The pin guarantees the next filter open / prevents `FWP_E_IN_USE`" | Contradicted by the field evidence: pin held, next open failed; the pin owns no WFP filter and cannot serialize per-handle teardown (`[WD]` F1/F3/F4) | "The pin keeps the driver referenced; it is not a WFP-state lock." |
| "A previous LightSpeed run left its WFP filter" (the app's own copy) | At 06:49 the only run was this process, whose own handles had closed 1.279 s earlier; the sentence is a hard-coded remedy, not a diagnosis (`[WD]` F2, `[WD-claims]` C2) | "An unidentified WFP lifetime conflict; capture WFP state." |
| "The pin removes the unload/reload churn path" | WinDivert's driver stays loaded after the last close; provider + 20 sublayers persist until service stop/reboot (`[WD]` F5; QA update row) | "The pin does not change driver load/unload in the observed environment." |
| "Close returned ok, so WFP is clean" | Driver teardown is void/debug-only; `CloseHandle` proves only the handle (`[WD]` F4) | "Handles closed; WFP state unverified — snapshot it." |
| "The restart was too fast; wait longer" | Short cycles succeeded at 0.567–0.577 s; the 1.279 s and 4.128 s attempts failed (`[WD]` §2.3, `[WD-timeline]` §2) | Name the measured gap and anchor; the differentiator was the 363.9 s session. |
| "The server/settlement transition tore down the interceptor" | Transitions closed no handle; the Stop came 64 s later with no causal line (`[GT]` §2–3, `[WD]` C6) | "Transition = P3/P4; the teardown is a separate action or a pre-stop." |
| "It was an automatic restart" | The presence auto-start line is absent; the latch cannot re-arm after a manual stop; two separate open invocations exist (`[GT]` §2.3, `[WD]` §2.4) | "At least two start invocations; actor unlogged — add action logging." |
| "The stale registration error was a wire failure" | Every registration in the window succeeded; the failure is a superseded-generation attribution bug (`[GT]` F1/§3.5) | "Generation-blind status write; the route was healthy." |
| "Restart capture to clear the error card" | Wrong plane; after an active session the restart is the action that can hit the FWP fault (`[GT]` §3.1/§4) | "Capture the trace; fix the status write; do not cycle capture." |
| "84/84 proves the fix / the release" | Different binary (`84e15c3` vs release HEAD `01438e0`), different state (clean preflight), no transitions/return path, marker-absence assertions (`[SV]` F1–F5, `[EV-02]` §8–9) | "84 assertions passed on that build in that environment; coverage gap listed." |
| "Reboot is the fix" | It loses evidence and hides cause; the product's own QA shows service stop is the reset (`[WD]` §6.5; `[SV]` A11) | "Evidence, bounded retry, pin-cycle, driver reset; reboot last." |
| "A retry would obviously clear it" | Unproven; the trace's only pinned retries were on a different poisoned binary/build. The mechanism is unmeasured (`[WD]` C5/Q8) | "Unknown until T8/T17; the retry is still the right bounded mitigation." |

---

## Competency assessment and escalation

**Pass standard for an in-house agent:** owns D3, D4, D7, D8, D10 unaided;
explains Modules 0–5 in review; writes an evidence note whose facts/inferences an
independent verifier accepts; never states a guarantee the code cannot keep.

**When to escalate (do not improvise):**
- Any live WFP/WinDivert query or driver/service action (`sc stop`), any reboot,
  or any GUI launch/stop — needs an explicit task authorisation and the owner
  gate; the 2026-11-23 audits were read-only for this reason.
- Any claim that needs the real driver or a clean-boot host → `qa-engineer` +
  `stability-verifier`.
- Any change to retry/recovery policy, pin retention, or stop semantics →
  `windivert-expert` review before code.
- Any change to transition/registration/presentation semantics →
  `game-networking-expert` review before code.
- Any user-facing copy change → `ux-lead` / owner (9 locales).
- Any spend, signing, publish or fleet action → owner only (`[COST_STUB]`,
  owner gates); never assume.

**Where to go next:** the roadmap's P0 list
([`docs/design/windivert-stability-roadmap-2026-11-23.md`](../design/windivert-stability-roadmap-2026-11-23.md)
§3.1) is the current work queue; every item has a test gate and an owner persona.
