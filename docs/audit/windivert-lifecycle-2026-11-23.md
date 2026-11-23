# WinDivert lifecycle audit — LightSpeed 1.12.5 pinned driver

**Status:** read-only expert audit, 2026-11-23. No fix, no release, no
"verified" claim. No production file, Cargo manifest, workflow, app state or
runtime driver/WFP object was touched. All conclusions are bounded by the
evidence listed in §1 and by the explicit open questions in §9.

Worktree: `...\.agents\worktrees\windivert-stability-audit` @ `6e30a65`
(tag `v1.12.5`). Evidence notes:
`wat/evidence/2026-11-23/stability-audit/windivert-lifecycle/`.

---

## 0. Findings, severity and confidence

| # | Severity | Confidence | Finding |
|---|---|---|---|
| F1 | **Critical (product)** | Certain | A held pin did not prevent `FWP_E_IN_USE` on the next filter open: pin active 06:43:34→06:50:15, first post-Stop open failed at 06:49:39.841 and again at 06:49:42.691. The shipped guarantee is false in the field. |
| F2 | **High** | Certain | The pinned path **deliberately disables the retry** (`windivert_handle.rs:322-329,636-651`). A transient WFP lifetime conflict becomes a hard start failure with a reboot recommendation. No experiment supports that policy. |
| F3 | **High** | Certain | The pin owns **no WFP filter and no callout**. Filter `"false"` analyses to zero layer flags (`dll/windivert_helper.c:2124-2136`) and installs nothing (`sys/windivert.c:1591-1631`). The pin keeps a device handle, not WFP state. |
| F4 | **High** | Certain | The failure "layer" is per-handle: each `WinDivertOpen` gets its own dynamic WFP session (`sys/windivert.c:1548-1551`), own callouts/filters (`:1732-1839`), and teardown runs in the file-object destroy callback with **void, debug-only** error reporting (`:1844-1946`, `:65-84`). `CloseHandle` success (the app's "close -> ok") proves nothing about WFP object removal. |
| F5 | **High** | High | The pin's stated rationale — removing a driver unload/reload churn path — is contradicted by WinDivert 2.2 itself and by LightSpeed's own evidence: the driver stays loaded after the last handle closes; only service stop/reboot removes it (`docs/release-notes-1.12.5.md:74`; QA `summary.txt:253-254`). There is no unload churn to remove. |
| F6 | **Medium** | High | The "server/settlement transition caused the automatic Stop/start" narrative is not supported by the trace. The two re-detects produced no Stop; the auto-start latch cannot re-arm after a manual stop; the Stop→restart is two actions 1.423 s apart. |
| F7 | **Medium** | Certain | Evidence provenance: the trace mixes at least two binaries (retries with `pin_active=true` exist at 06:01/06:03 and are impossible in v1.12.5). No build SHA is logged. |
| F8 | **Medium** | High | The QA matrix is a 12 s-session matrix; it never exercised a multi-minute traffic session followed by Stop→start, and its update row passes only because the **harness** stops the WinDivert service (`service_stop_issued=True`, `wfp_clean_after_stop=True`). |
| F9 | **Medium** | High | Documentation now carries false/over-broad statements: "a WFP callout/filter for each open handle" (`docs/troubleshooting.md:199`), "nothing keeps the driver loaded" (`release-notes-1.12.5.md:11-25`), reboot-only remedy. They will mislead the next investigation. |
| F10 | **Low** | Certain | The pin is process-lifetime; the app must refuse in-app updates while it is held (`client-gui/src/self_update.rs:41`, `release-notes-1.12.5.md:80-81`). This is a user-visible update friction added by a mechanism that F3/F5 show is unnecessary. |
| F11 | Info | Certain | No WinDivert release after 2.2.2 exists at read time; 2.2.1 fixed a crash deadlock, 2.2.2 a FLOW-layer close BSOD. Nothing upstream addresses this class. |

"Certain" = directly printed by the product/source. "High" = strongly supported
by converging evidence but not directly instrumented. No claim in this document
rests on the prior narrative.

---

## 1. Evidence base

| Source | What it is |
|---|---|
| `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log` | 1400-line ANSI-coded slice, 05:24:23.641472Z–06:50:15.173465Z, line-cited as `slice:N` |
| `%LOCALAPPDATA%\Lightspeed\gui-trace.log` | full 106,658-line trace, 2026-11-13→2026-11-23 06:50:15Z |
| LightSpeed `6e30a65` (v1.12.5) | `client/src/interceptor/{windivert_handle,windows,mod}.rs`, `client/src/engine.rs`, `client-gui/src/app.rs`, `self_update.rs`, docs |
| WinDivert **v2.2.2** `1789526ecfb9ff5397c94f9f54c1a3dc2fb60440` | official source at `github.com/basil00/WinDivert`, cloned read-only; line-cited as `file:line` |
| `pinned-driver-1.12.5` worktree evidence | matrices `task-107-matrix-release-20261123-112550/` (82 PASS/1 FAIL/1 BLOCKED) and `task-107-matrix-84e15c3v2-20261123-115633/` (84 PASS), `windiverttool/` outputs, `wfp/` dumps |
| MS Learn | WFP error codes, `FwpsCalloutUnregisterByKey0`, `FwpsCalloutRegister0`, `FwpmFilterAdd0`, `FwpmEngineOpen0`, `FwpmEngineClose0`, `FwpmTransactionCommit0` |

Timestamp convention: trace is UTC (`Z`); the host is UTC+7. The owner's
"13:34:17 cold boot" is 06:34:17Z and is not in the trace. Detailed timeline and
gap arithmetic: `timeline-facts.md`. Upstream source map: `upstream-source-evidence.md`.
Claim-by-claim verdicts: `claims-and-contradictions.md`.

---

## 2. What happened, minute by minute (facts)

The slice's early hours are a **machine-wide poisoned period** (every
filter-bearing open fails from 05:24 to 06:10); they are not a baseline. After
the 06:34:17 cold boot the product starts clean at 06:41:21.

### 2.1 First post-boot process (06:41:21–06:42:29)

- 06:41:35.504 pin `id=1` opens; 06:41:35.511 intercept `id=2` opens; inject
  `id=3`; shadow `id=4` (slice:572-587).
- Four Stop→Optimize cycles, all successful: starts at 06:41:50.798 (`id=5`),
  06:41:54.301 (`id=8`), 06:41:57.807 (`id=11`), 06:42:01.334 (`id=14`);
  every stop closes ids 2/3/4 … 14/15/16 and logs
  `driver pin active=true` (slice:626, 679, 731, 783, 839).
- 06:42:29.658 Quit → pin `id=1` released after teardown (slice:840-843).

### 2.2 Second process: the long session (06:42:38–06:50:15)

- 06:43:34.533 `Game present and relay connected; starting interception for
  robrun` (slice:926); pin `id=1` 06:43:34.775; intercept `id=2`, inject `id=3`,
  shadow `id=4` 06:43:34.78 (slice:941-955).
- Session runs **363.8 s** with real traffic, two stale-lock re-detects
  (06:47:59.325 → lock 178.107.14.45; 06:48:34.177 → lock 178.107.13.95) and
  three "Relay return path is starving" warnings with `injected=0`
  (06:48:59.677, 06:49:13.184, 06:49:26.692; slice:1308, 1321, 1332).
- 06:49:38.417 `🛑 Shutdown signal received` (slice:1340). Threads exit; at
  06:49:38.561 the three diverting handles close and the product logs
  `WinDivert diverting handles closed; driver pin active=true`
  (slice:1346-1349. The pin `id=1` is **not** closed).
- 06:49:39.840 restart action: intercept `id=5` `WinDivertOpen` fails
  `FWP_E_IN_USE (0x8032000A)`; **one attempt only** (slice:1362-1365).
- 06:49:42.689 a second restart action: intercept `id=6` fails identically
  (slice:1380-1384).
- The two failures are **two separate start invocations**, not one invocation
  retrying internally: each has its own plan/process-scan log
  (`06:49:39.827353`, slice:1350; `06:49:42.676960`, slice:1369) ~13 ms before
  its `Opening WinDivert intercept handle` line, and each makes exactly one
  `WinDivertOpen` (pinned policy, §4.5). Control-plane registration succeeds
  0.43 s after each failed start (`06:49:40.275474`, `06:49:43.129417`;
  slice:1368,1386): the route plane was alive while the capture plane was dead.
- 06:50:14.906 Quit; 06:50:15.173 pin released (slice:1396-1400). No later
  start attempt exists, so recovery after Quit is **unobserved**.

### 2.3 Measured timing (from the same trace)

| Case | close→next open | result |
|---|---|---|
| cycle 1 | 0.567 s | ok |
| cycle 2 | 0.571 s | ok |
| cycle 3 | 0.573 s | ok |
| cycle 4 | 0.577 s | ok |
| field Stop→restart | 1.279 s (1.423 s from stop signal) | FWP_E_IN_USE |
| retry | 4.128 s | FWP_E_IN_USE |

The successful cycles used the **shortest** gaps; waiting longer did not help.
The only structural difference is the **session length and workload**: the four
clean cycles ran ≈2.9 s (first session 14.7 s) with little/no traffic, whereas
the failing session ran 363.8 s with traffic, carrier re-locks and a
return-path starvation streak. This is a correlation, not yet a proven cause
(§4.8).

### 2.4 What the trace does not support

- The two server re-detects did **not** stop or restart anything; the relay
  choice lines say "Keeping the live session … applies next session"
  (slice:1248, 1285).
- There is no "automatic restart" trace: after a manual stop the GUI's
  once-per-presence latch is not re-armed (`app.rs:1693-1707` vs `:2106-2155`),
  and game presence continued on its 5 s cadence. The 1.4 s restart is
  **INFERENCE: a second user/harness action**, because no automatic path can
  fire there and the GUI does not log clicks.
- The Stop's cause is not logged at all: `Shutdown signal received` is emitted
  by the interceptor's internal shutdown thread (`windows.rs:1316-1323`), not by
  the code that requested it.

### 2.5 The 06:49 failure in one line

> Three per-handle WFP callout owners were closed 1.279 s earlier; the pin,
> which owns no WFP object, was still open; the next `WinDivertOpen` returned
> the WFP lifetime status `0x8032000A`; the app's pinned-path policy turned
> that into a permanent failure without one retry.

---

## 3. What `FWP_E_IN_USE` actually is (and is not)

**F1. It comes from `WinDivertOpen` only.** The app logs
`last_os_error()` after `WinDivertOpen` returns `INVALID_HANDLE_VALUE`
(`windivert_handle.rs:705-717`). `WinDivertOpen` = CreateFile +
`IOCTL_WINDIVERT_INITIALIZE` + `IOCTL_WINDIVERT_STARTUP`
(`dll/windivert.c:530-612`); the single errno collapses four kernel round-trips.
The logged remedy text is a hard-coded sentence (`windows.rs:2410-2424`) — not a
diagnosis.

**F2. `0x8032000A` is a WFP object-lifetime conflict.** MS defines it as "the
object is referenced by other objects, so it cannot be deleted". WinDivert's
teardown calls `FwpsCalloutUnregisterByKey0`, which returns
`STATUS_FWP_IN_USE` when "the callout is already in the process of being
registered or unregistered in another thread". So the status family covers both
a management-object reference conflict and a **registration/unregistration
race** — it does not mean "a previous process left a filter", which is only one
of several possible states.

**F3. Which call in the failed open produced it is unknown.** Candidates, in
code order, are `FwpsCalloutRegister0` (S4 `:1784`), `FwpmTransactionBegin0`
(`:1797`), `FwpmCalloutAdd0` (`:1803`), `FwpmFilterAdd0` (`:1809`),
`FwpmTransactionCommit0` (`:1815`), or — if the failure occurred before
STARTUP — `FwpmEngineOpen0` inside `windivert_create` (`:1550`). Microsoft's
pages list `FWP_E_IN_USE` explicitly only for the delete/unregister family and
otherwise say "other NTSTATUS codes", so the trace's single errno cannot
discriminate. Upstream has no callout-teardown-ordering change after 2.2.2.
**This must be measured, not argued** (§7, §9 Q1).

---

## 4. Does a held pin guarantee the next filter open? No — and why

### 4.1 The pin is a device handle with an empty WFP session

- `PIN_FILTER = "false"` (`windivert_handle.rs:335-337`); opened on
  `WINDIVERT_LAYER_NETWORK`, priority 0, flags `0x0000` (`:419-460`).
- `WinDivertAnalyzeFilter` returns **0** for a provably false filter
  (`dll/windivert_helper.c:2130-2136`); `windivert_install_callouts` then
  registers nothing (`sys/windivert.c:1601-1631`, `i == 0`).
- The inject handle is also `filter="false"` and installs nothing. Only the
  intercept (`udp and outbound …`) and shadow (`udp and inbound …`) handles own
  WFP callouts/filters.
- QA shows the pin as a real handle only (`windiverttool/after-same-scope.txt`:
  `filter="false"`), while idle WFP callouts/filters are 0
  (`summary.txt:253-254`).

Therefore the pin cannot "hold" the WFP state whose teardown is racing. It holds
one WDF file object, which keeps `WinDivert.sys` referenced — a layer the
failure is not in.

### 4.2 Who owns the WFP objects: per-handle dynamic sessions

Each open creates a file-object context with its **own dynamic WFP engine
session** (`sys/windivert.c:1548-1551`), then registers its own callouts and
filters in that session (`:1732-1839`). On close, the runtime callout unregister
and filter/callout deletes happen in `windivert_destroy` (`:2106-2110`) — a
driver-side path that returns **void** and reports failures only through
`DEBUG_ERROR`, which is compiled out of the release driver (`:65-84`). The app's
`close()` is `CloseHandle` (`dll/windivert.c:718-721`); its "ok" log means the
handle closed, not that filter/callout deletion succeeded.

This is the ownership gap: the pin is in the *device-handle* domain, while the
contended objects are in the *per-handle dynamic-session/callout* domain. A pin
cannot serialize or await that domain, and there is no public WinDivert API to
do so.

### 4.3 Pin ordering and reference counts

Ordering is correct and consistent: engine pins before the first diverting
handle (`engine.rs:1769-1771`), and the backend repeats the idempotent pin before
its own opens (`windows.rs:1060-1075`). Evidence: pin 06:41:35.504 →
intercept .511; pin 06:43:34.775 → intercept .782. The pin is idempotent and
held once per process (`windivert_handle.rs:407-460`).

But ordering is not the failing dimension. A reference count that *would*
matter is the WDF file-object/driver reference, and the driver stays loaded
without it anyway (4.4). There is no user-visible WFP reference count to raise.

### 4.4 Driver/service state: the pin's premise is false here

WinDivert's service is started once and then `DeleteService`d while running
(`dll/windivert.c:340-373`). The driver's WFP provider and 20 sublayers are
installed in `DriverEntry` (`sys/windivert.c:1153-1276`) and removed only in
`EvtDriverUnload` (`:1316-1418`), i.e. at service stop/reboot. Official docs:
the driver is uninstalled "during the next machine reboot"
(`doc/windivert.html:229-234`). QA confirms: after all LightSpeed handles are
gone, no client handles exist but WFP still shows provider=1 / sublayers=20 /
callouts=0 / filters=0 (`summary.txt:253-254`). LightSpeed's own release note
states it (`docs/release-notes-1.12.5.md:74`).

So the "unload on last handle close → reload race" that the pin was designed to
remove does not exist in the observed environment. The pin is **redundant for
driver-load stability** and cannot affect WFP object teardown. The pre-1.12.5
stale-state reports are better explained by WFP objects/state persisting while
the driver stays loaded (or by a failed service/update transition), not by
driver re-load churn. `sc stop WinDivert` is the reset — the QA update row does
exactly that and gets `wfp_clean_after_stop=True`
(`task-107-matrix-84e15c3v2-.../summary.txt` update row).

### 4.5 Retry budget: the pinned path is the *least* resilient

`FWP_IN_USE_MAX_RETRIES = 3` with 50/150/350 ms backoff (≈550 ms plus call
time; `windivert_handle.rs:296-330`), but `should_retry_fwp_in_use` returns
**false whenever the pin is active** (`:322-329`). The pinned restart therefore
makes exactly **one** attempt (06:49:39.841) and, after a second action 2.8 s
later, gives up. Each of the two failed starts is a separate invocation with one
`WinDivertOpen`; neither is an internal retry. The rationale ("with a pin the
stale state is external and persistent; retrying cannot clear it") is an
assumption, not a measurement (claims C5). A transient WFP registration race — a
documented `FWP_E_IN_USE` meaning — is exactly the case a bounded retry exists
for.

### 4.6 Teardown and the fail-closed latch: what the ack proves

`stop_interceptor` waits up to 1500 ms for the four owner-thread acks, then the
close hook runs inside `stop_and_wait` before it returns
(`engine.rs:1998-2027`, `windows.rs:2282-2320`). The trace shows all acks and
three successful `CloseHandle` calls before the restart, and the fail-closed
latch was therefore not set (`engine.rs:1581-1590,2014-2019`). But:

- the ack proves no LightSpeed thread will touch the handles;
- the close proves `CloseHandle` returned TRUE;
- **neither proves the driver deleted its filter/callouts** (§4.2).

So the latch cannot guard the state that actually failed. The product's
`diverting handles closed; driver pin active=true` line is a truthful statement
about handles and a misleading one about WFP.

### 4.7 The 1.4 s stop→start race: not the discriminator

The measured gaps (§2.3) fail the two simplest race hypotheses:

- "reopen too fast": the 0.567–0.577 s gaps all succeeded; the 1.279 s gap failed.
- "just needs a moment": a second attempt at 4.128 s failed identically.

A *concurrent teardown* hypothesis (the old file object's destroy path still
running when the new open registers) remains possible — WDF destroy timing and
the `FwpsCalloutUnregisterByKey0` "another thread is registering/unregistering"
status make it plausible — but if it were a simple race it should have failed
the earlier short cycles too. The trace points instead at **state accumulated
during a 363.8 s traffic session** (or at workload during the close) as the
differentiator. That is why the next step is instrumentation, not a sleep.

### 4.8 Ranked mechanism hypotheses (INFERENCE; each with the test that decides it)

| # | Hypothesis | Support | Against | Decide by |
|---|---|---|---|---|
| H1 | Long-session callout lifecycle (in-flight classify/queue drain) leaves an unregister pending, and the next register/add sees `FWP_E_IN_USE` | Only failure is after 363.8 s traffic; upstream unregister returns IN_USE for concurrent register/unregister; teardown errors invisible | QA cycles had traffic too (but only 12 s); mechanism not proven | WFP state capture + a debug-build WinDivert or ETW WFP audit around the failure (§7) |
| H2 | The close path leaked a WFP object/session (deletes failed; void + debug-only) | Release post-mortems report provider/objects left registered; close failures are unobservable | Driver dynamic session should clean session-owned objects; no capture at 06:49 | Same: `netsh wfp show state` before Stop / after Stop / after failed start, plus a modified debug driver build (not shippable) |
| H3 | External/other WinDivert user on the machine | The 05:24–06:10 period proves such states exist; a stale callout from another process can block opens | Single-instance trace, only 3 handles in-process, no other client evidence | WinDivertTool scan + `netsh wfp show state` at failure |
| H4 | BFE/driver state degradation (e.g. transaction/session leak) after many sessions | Pre-reboot failures suggest sustained state | Only four sessions before the long one in this process; driver persists across processes | WFP object counts per session; BFE event log; repro with fixed sequence |
| H5 | Binary/provenance artefact (trace from non-final build) | Proven for 06:01/06:03; no SHA logged | 06:49 behaviour matches v1.12.5 code (no retry) | Ship with build SHA in every log line |

None of these is established. A "pin-cycle" or retry may clear H1-type transients
and will not clear H2/H3; only measurement separates them.

---

## 5. Upstream WinDivert 2.2.2 research (cited)

1. **Version:** the bundled binaries are official **2.2.2**
   (`client/windivert/README.md`; `WinDivert.dll`, `WinDivert64.sys`), dual
   licensed **LGPLv3 or GPLv2** (same file). Newest upstream tag is v2.2.2
   (2022-09-17, commit `1789526`).
2. **Multiple handles:** supported and tested upstream (`test/test.c:967-969`,
   `:1130-1137`). There is no documented interaction rule for "one handle stays
   open while another is closed and a third opens" beyond the normal close/open.
3. **Close/open:** `WinDivertClose = CloseHandle`; all WFP teardown is in the
   driver's file-object destroy callback and is void/debug-only
   (`dll/windivert.c:718-721`; `sys/windivert.c:1844-1946,2062-2122,65-84`).
   The user-mode library has no "flush/await WFP deletion" API.
4. **Driver unload/service:** demand-start kernel service, `DeleteService`d
   immediately after start; unload is service-stop/reboot, not last-close
   (`dll/windivert.c:296-391`; `sys/windivert.c:998,1308-1418`;
   `doc/windivert.html:229-234`). A persistent handle is therefore not needed to
   keep the driver loaded on 2.2.2 in this environment.
5. **`FWP_E_IN_USE`:** see §3. Upstream CHANGELOG has no 2.2.x fix for callout
   teardown/registration ordering; 2.2.1 = crash-deadlock fix, 2.2.2 = FLOW
   close BSOD fix. No upstream issue/PR matching this failure was found in the
   upstream GitHub issue search (`FWP_E_IN_USE`, `IN_USE` → 0 results).
6. **Documented ordering:** callout register → callout add → filter add
   (`FwpmFilterAdd0` remarks); WinDivert follows it inside a transaction. There
   is no documented "wait for the previous handle's unregister" step.
7. **What upstream does not promise:** that a non-diverting handle makes
   subsequent opens immune; that close is synchronous with WFP object removal;
   that `0x8032000A` means a stale previous process.
8. **Practical constraint:** WinDivert 2.2.2 is the newest release; there is no
   drop-in upstream fix to adopt. Patching the driver (to add teardown
   acknowledgement) would invalidate the signed binary and trigger GPLv2
   distribution obligations — an owner-budget/signing decision, not a normal
   patch (§6.8).

---

## 6. Design options, trade-offs and recommendation

Options are judged against the confirmed requirement: *a Stop must be followed
by a start that succeeds without rebooting, including after a long,
traffic-bearing session, without touching other WinDivert users.*

### 6.1 Persistent capture — never Stop on transition

Keep the per-session handles open and only flip the tunnel gate ("pause/resume";
the draft PR #214 direction). **Pro:** removes the close/open sequence entirely,
so the failure window cannot occur for same-scope transitions; also removes the
driver-resource churn. **Con:** does not fix a failed *start* or a scope/game
change (still needs teardown + reopen); changes Stop semantics (after Stop the
handles still exist, so "no interception" is a gate, not a closed filter);
a long-lived session is exactly the workload that correlates with the field
failure, so a pause would hide, not fix, accumulated WFP state; no recovery if
the session's callouts go bad. **Verdict:** useful for transition safety (task-110
owns that decision), but **not a fix for this failure**. It would not even have
prevented the observed 06:49 event if the Stop was manual (which the trace
supports, F6).

### 6.2 Pin + real open gate/backoff

Keep the pin (or drop it, §6.3) and make the open resilient: always retry
`FWP_E_IN_USE` with bounded exponential backoff while pinned (e.g. 4–6 attempts
over ~5 s), distinguish the status family, and only then fail. **Pro:** small,
testable, no driver change, addresses a transient registration race (H1) and
gives the driver time to finish destroy-path work. **Con:** heuristic; does not
establish *why*; must not loop forever (a bounded budget is mandatory); a
persistent leak still fails after the budget. **Verdict:** recommended as the
immediate mitigation (cheap, reversible), together with the diagnostics in §7.

### 6.3 Close pin before open, then reopen (pin-cycle)

On a failed or suspected open, release the pin, reopen it, and retry. **Pro:**
closes and reopens the last remaining dynamic session/handle in-process; cheap;
can be tried automatically before surfacing an error; directly tests whether the
pin's own session state is implicated. **Con:** theory-light; if the contention
is another handle's runtime callout, it may not help; must be serialized and
bounded. **Verdict:** include in the immediate mitigation as step 2 after the
bounded retry budget, gated behind an experiment (§7 T11).

### 6.4 Helper service (or elevated helper process) owning the handles

Move capture handles into a long-lived elevated helper; GUI Stop/restart and
crashes no longer churn WinDivert handles. **Pro:** isolates the lifecycle from
the GUI; can own a single session across Optimize; central place for WFP
diagnostics; survives GUI restarts. **Con:** new IPC/security surface; installer
and service lifecycle; user Stop semantics (the capture process must release
filters for a true Stop); still depends on WinDivert internals; does not remove
the per-handle teardown race on scope changes; significant engineering. **Verdict:**
defer until the mechanism is measured; revisit only if H2/H4 is confirmed.

### 6.5 True stop + driver state reset (service stop)

On Stop (or only on failure), after closing handles, `sc stop WinDivert` to
unload the driver and remove its provider/sublayers; the next Optimize reloads
it. **Pro:** the only userspace action proven to clear WFP state in the product's
own QA (`service_stop_issued=True`, `wfp_clean_after_stop=True`); strong recovery
path; no driver modification. **Con:** the WinDivert driver is shared with any
other app using WinDivert (stopping it affects them); requires admin (the GUI
already needs elevation for capture); adds a driver load/unload cycle and its
latency; unload can fail while other clients hold handles. **Verdict:**
recommended as an explicit, user-visible **recovery action** ("Reset
interception driver") — not on every Stop — with a check that no other WinDivert
client is present. This replaces the current "restart Windows" advice.

### 6.6 Direct WFP/NDIS or a patched WinDivert driver

Own the kernel component. **Pro:** full control of callout/session lifetime and
a proper teardown acknowledgement; removes the third-party lifecycle. **Con:**
kernel development and WHQL/signing cost (EV code-signing certificate + Microsoft
attestation; hardware token; recurring cost — an owner-budget gate, incompatible
with the project's $0 constraint without explicit approval); modified WinDivert
driver loses the upstream signature and carries GPLv2 obligations; much larger
test surface. **Verdict:** long-term only; not justified before the mechanism is
measured and a cost gate is approved.

### 6.7 Keep the pin, remove its two harmful side effects

If the pin is retained: (a) never let it suppress retries (delete
`!pin_active` from the retry decision); (b) state the truth in the pin doc/log
("keeps the driver referenced; owns no WFP filter; not a WFP state lock"); (c)
keep the updater gating. **Pro:** minimal change, keeps whatever small benefit
exists. **Con:** the pin's stated benefit is unproven (F5); keeping it preserves
the update-lock friction. **Verdict:** acceptable short-term if changing the
pin is out of scope, but it must not be sold as a guarantee.

### 6.8 Signing / licensing / cost constraints (practical)

- **Bundling the official driver** is what ships today: dual LGPLv3/GPLv2
  (`client/windivert/README.md`), unmodified signed `WinDivert64.sys`. No cost,
  no source obligation beyond the license notice. **Any** option that keeps the
  official binary is zero-cost.
- **Patching/replacing the kernel component** requires (1) own code-signing
  (EV certificate and Microsoft attestation signing for kernel drivers; hardware
  token; annual fee), (2) GPLv2 source obligations and loss of the upstream
  signature, (3) owner approval because it adds spend — forbidden by the
  project's zero-cost rule without an explicit owner gate. Do not plan it
  without that gate.
- **Helper service (user-mode)** still needs installation and elevation; a
  signed executable is advisable but not legally mandatory; it adds installer and
  service work that packaging owns. It does not by itself fix the WFP race.
- **Persistent capture / retry / pin-cycle / service-stop recovery** are all
  zero-cost, userspace-only changes.

### 6.9 Recommendation (no fix claimed)

1. **Do not treat the pin as a guarantee.** Correct the code comments, logs and
   docs so the next investigation does not inherit the false premise. Scope the
   claim to "keeps the driver referenced", not "keeps WFP state safe".
2. **Re-enable bounded retry while pinned** and add a single pin-cycle retry
   (release+reopen) before surfacing failure. Keep the budget short and bounded.
3. **Replace the reboot-only remedy** with a distinct failure status and a
   user-approved **"Reset interception driver"** action (`sc stop WinDivert`
   after confirming no other WinDivert client) as the recovery path; document
   its side effect.
4. **Instrument before redesigning:** capture WFP state around the Stop/start
   and identify the failing call (§7). Only then choose between persistent
   capture (transition safety), helper service, or a patched/own driver.
5. **Keep the current pin only if changing it now is out of scope**, but remove
   its retry suppression and its over-claims. The pin is *not* "fundamentally
   wrong" as a device handle; it is **the wrong mechanism for this failure**
   because it lives in a different ownership layer from the WFP callouts, and
   because the driver unload it was meant to prevent does not happen in the
   observed environment.

---

## 7. Test / reproduction matrix

All rows are proposals; none was executed by this audit. Every row must capture
WFP state at each marked point, not just the trace log. Commands (Windows
PowerShell 5.1, elevated for `netsh wfp`):

```
netsh wfp show state file="$env:TEMP\wfp-state.xml"
netsh wfp show filters file="$env:TEMP\wfp-filters.xml"     # if supported by the host build
netsh wfp show callouts file="$env:TEMP\wfp-callouts.xml"   # if supported
netsh wfp show providers file="$env:TEMP\wfp-providers.xml" # if supported
netsh wfp show sublayers file="$env:TEMP\wfp-sublayers.xml" # if supported
Get-Service WinDivert | Format-List Name,Status,StartType
sc.exe qc WinDivert
Get-CimInstance Win32_SystemDriver -Filter "Name='WinDivert'" | Format-List *
wevtutil qe System /q:"*[System[Provider[@Name='WinDivert']]]" /f:text /c:20
tasklist /m WinDivert.dll
# WinDivertTool 2.2 (basil00) for a handle scan, as used by QA
```

Capture timestamps: S0 before start, S1 after successful start+10 s traffic,
S2 the instant before Stop, S3 +10 ms / +100 ms / +1 s / +5 s after the close
ack, S4 the instant before the start attempt, S5 after success/failure, S6 after
the retry budget. The trace must additionally log handle ids (already), the
**build SHA**, the **reason** for Stop/start, and the retry/pin-cycle decisions.

| ID | Scenario | Steps | Pass criteria | Evidence |
|---|---|---|---|---|
| T0 | Clean-boot baseline | fresh boot, one start, no traffic | pin 1, intercept/inject/shadow open; WFP filters/callouts = 0 while idle; provider/sublayer present after start | S0/S1/S6, `windiverttool/baseline` equivalent |
| T1 | Short-cycle control | 30 × (start 12 s → Stop → start) | all starts ok, no retry needed; WFP filters/callouts back to 0 after each Stop | trace + S2/S3/S5 |
| T2 | **Long-session Stop/start (primary repro)** | start, 10 min real traffic, Stop, start after 1.0–1.5 s | start succeeds without reboot; WFP shows no orphan filter/callout left by ids 2–4 | all S points + WFP XML diff |
| T3 | Same, long delay | as T2 but start after 10 s | distinguishes timing race from persistent state | all S points |
| T4 | Stop during return-path starvation | reproduce `injected=0` streak, Stop, start | start succeeds; queue drain/starve state not implicated | trace + S2/S3 |
| T5 | Stale-lock / settlement switch workload | ≥3 server re-detect + relay re-choice, then Stop/start | start succeeds; no WFP change during re-detect | trace + S points around re-detect |
| T6 | Relay change then Stop/start | relay A→B→A while running, then Stop/start | handles unchanged during switch; start ok | trace |
| T7 | Pin-held vs pin-released comparison | same long scenario; variant where Quit/relaunch releases pin before start | both pass or the difference is measured — this is the pin-hypothesis discriminator | trace + S points |
| T8 | Retry-with-pin experiment | build with retries allowed while pinned; long session then Stop/start | count attempts until success/failure; show whether budget clears it | trace attempts + WFP at each attempt |
| T9 | Pin-cycle recovery | on failure: release pin, reopen pin, retry once | record whether that alone clears FWP | trace + WFP before/after |
| T10 | Driver reset recovery | on failure: `sc stop WinDivert`, verify WFP clean, start | WFP provider/sublayer/filters/callouts = zero; start succeeds; other WinDivert clients absent | S points + `wevtutil` |
| T11 | Scope/game change | profile/scope switch, then Stop/start | starts ok; pin count = 1 | trace |
| T12 | Force-kill control | kill active/idle process, relaunch, start | no orphan handles/filters; start ok (QA already covers; keep as control) | `windiverttool` + WFP |
| T13 | Repeated-transition soak | 20 × (re-detect → Stop → start) over 30+ min | zero FWP failures; WFP object counts stable; handle ids monotonic | trace + per-cycle WFP summary |

Pass/fail rules: a row fails on any `FWP_E_IN_USE` that survives the chosen
bounded recovery, on any orphan WinDivert filter/callout after all handles are
closed, or on any recovery that needs a reboot. Expected normal state: provider
and 20 sublayers **may persist** while no handles exist; callouts and filters
must be 0.

---

## 8. Roadmap (no release claim)

1. **R1 — instrument** (days): log GUI action ids + reasons for Stop/start,
   build SHA, retry/pin-cycle decisions; add the WFP capture harness (§7). No
   behavioural change.
2. **R2 — resilience** (small): bounded retry while pinned (remove
   `!pin_active`), single pin-cycle retry, distinct failure status; tests with a
   mock pin seam (exists) + a real-driver long-session run.
3. **R3 — recovery** (small): "Reset interception driver" action with explicit
   shared-driver warning, replacing the reboot advice in
   `docs/troubleshooting.md:197-212`; owner-visible copy.
4. **R4 — truth in docs** (small): pin doc/log/release-note language
   (`windivert_handle.rs:19-38`, `docs/release-notes-1.12.5.md:11-25`,
   `docs/troubleshooting.md:199`), and a note that provider/sublayers persist
   until service stop/reboot.
5. **R5 — long-session CI/QA** (medium): add T2/T4/T13 to the matrix; require a
   ≥10-minute traffic-bearing session for any WinDivert change; capture WFP XML
   at transitions.
6. **R6 — design decision after R1/R2** (owner gate): persistent capture
   (pause/resume) vs helper service vs keep true Stop + recovery. Decide with
   measurements, not with the current narrative.
7. **R7 — only with budget**: patched/own kernel component; do not start without
   the signing/licensing gate (§6.8).

---

## 9. Open questions

- **Q1.** Which kernel call returned `0x8032000A` at 06:49:39.841, and which WFP
  object was the reported conflict? (Needs a debug WinDivert build or ETW WFP
  audit + `netsh wfp show state`; not answerable from the release binary.)
- **Q2.** Why did the 06:49:42.689 attempt fail as well, and would a retry budget
  or a pin-cycle have cleared it? (T8/T9.)
- **Q3.** Is the trigger the 363.8 s session duration, the traffic volume, the
  return-path starvation state, or the close sequence? (T2/T3/T4.)
- **Q4.** Does any stale WinDivert filter/callout exist between S2 and S4 on a
  failing run? (S3 capture.)
- **Q5.** Is `sc stop WinDivert` safe to expose in-app while other WinDivert
  clients may exist? What is the detection primitive? (Product decision.)
- **Q6.** Was the 06:49 Stop user-initiated or harness-initiated? The GUI logs
  no action; product should log a stop reason.
- **Q7.** Which binary produced the pre-cold-boot sessions (the
  `pin_active=true` retry lines)? Add a build SHA to every run.
- **Q8.** Does the pinned path's single attempt ever succeed on retry in the
  field (i.e. is the failure transient)? Current evidence: unknown.
- **Q9.** Does the upstream project consider this a bug? No matching upstream
  issue was found; a minimal repro would need the debug driver anyway.
- **Q10.** Can the pin be removed without losing anything measurable? F5 says
  its benefit is unproven; T7 is the experiment.

---

## 10. References (exact)

Local source (v1.12.5): `client/src/interceptor/windivert_handle.rs:19-38,
296-330, 322-329, 335-337, 407-460, 462-482, 600-656, 665-731, 857-907`;
`client/src/interceptor/windows.rs:1060-1075, 1120-1135, 1296-1302,
1316-1323, 2282-2320, 2365-2367, 2410-2424`; `client/src/engine.rs:606-623,
1579-1590, 1757-1772, 1943-1960, 1998-2027, 2067-2108`;
`client-gui/src/app.rs:1614-1686, 1693-1707, 2050-2163, 2673-2691`;
`client-gui/src/self_update.rs:41`; `client/windivert/README.md`;
`docs/release-notes-1.12.5.md:11-25, 74, 80-81`; `docs/troubleshooting.md:197-212`.

Trace: slice lines named in §2 and `timeline-facts.md`; full trace path in §1.

Upstream (v2.2.2, commit `1789526`):
`dll/windivert.c:296-391, 454-617, 715-721`;
`dll/windivert_helper.c:2124-2136`;
`sys/windivert.c:65-84, 970-1030, 1153-1276, 1308-1418, 1465-1586,
1591-1631, 1732-1839, 1844-1946, 2062-2122, 2085-2110`;
`doc/windivert.html:229-234`; `include/windivert.h:191-196`;
`test/test.c:967-969, 1130-1137`; `CHANGELOG` (2.2.1, 2.2.2).

MS Learn: WFP error codes
<https://learn.microsoft.com/en-us/windows/win32/fwp/wfp-error-codes>;
`FwpsCalloutUnregisterByKey0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/nf-fwpsk-fwpscalloutunregisterbykey0>;
`FwpsCalloutRegister0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpsk/nf-fwpsk-fwpscalloutregister0>;
`FwpmFilterAdd0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpmk/nf-fwpmk-fwpmfilteradd0>;
`FwpmCalloutAdd0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpmk/nf-fwpmk-fwpmcalloutadd0>;
`FwpmEngineOpen0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpmk/nf-fwpmk-fwpmengineopen0>;
`FwpmEngineClose0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpmk/nf-fwpmk-fwpmengineclose0>;
`FwpmTransactionCommit0`
<https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/fwpmk/nf-fwpmk-fwpmtransactioncommit0>.

QA evidence: `<pinned-driver-1.12.5>\wat\evidence\2026-11-23\
task-107-matrix-release-20261123-112550\{summary.txt, windiverttool\*.txt,
wfp\*.xml}` and `...\task-107-matrix-84e15c3v2-20261123-115633\summary.txt`.

---

## Appendix A — Falsifiers (what would change the conclusions)

- A captured WFP state showing a **leftover filter/callout owned by a dead
  session** at 06:49:39 would confirm H2 and make "pin-cycle + reset" the
  priority; it would not restore the "pin guarantees opens" claim.
- A captured WFP state showing **no** leftover object, with the failure
  reproducible only in a debug driver, would point to H1 (registration race) and
  make bounded retry the correct mitigation.
- A reproduction on v1.12.5 with a **short** session and an immediate Stop/start
  would refute the long-session correlation.
- A demonstration that `0x8032000A` at 06:49 came from `FwpmEngineOpen0` (before
  STARTUP) would move the problem to dynamic-session/BFE state and away from
  callouts entirely.
- Proof that the field binary was not v1.12.5 would invalidate F6–F8 as
  statements about the shipped build (but not F3/F4/F5, which are source facts).
