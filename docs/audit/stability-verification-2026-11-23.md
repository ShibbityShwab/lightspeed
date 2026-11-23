# Stability verification audit — 2026-11-23

**Task:** `task-111` — why did the QA 84/84 matrix pass while a real settlement /
server transition produced `FWP_E_IN_USE` and stale registration errors?
**Owner:** stability-verifier (independent audit; read-only).
**Worktree:** `.agents/worktrees/windivert-stability-audit`
(branch `audit/windivert-stability`, base merge `6e30a65`).
**Date:** 2026-11-23.

> **Status: audit only. No pass, fix or release claim is made here.**
> This round did not modify production code, tests, workflows or Cargo files,
> did not start or stop the LightSpeed GUI, and did not touch WinDivert, WFP or
> any running process. Every Windows/driver observation below is quoted from
> stored evidence that was produced earlier by other agents or by the owner's
> own runs; none was re-executed by this audit.
>
> Deliverables for this task: this document,
> [`docs/training/stability-verification-training.md`](../training/stability-verification-training.md),
> and the evidence notes in `wat/evidence/2026-11-23/stability-audit/`.

---

## 0. Short answer

**The matrix and the real failure are not competing observations of the same
thing; they are observations of different systems.** The 84/84 run is a
*clean-boot, fixture-only, marker-absence* matrix over roughly fourteen scenario
families. The field failure happened in a state the matrix is explicitly
forbidden to measure (a kernel and app state left behind by previous sessions),
after session churn the fixture cannot produce (two game-server re-detections
35 s apart, relay demotion after four consecutive no-reply windows, repeated
in-place re-registrations), on a binary whose identity is not recorded in the
trace.

Six findings drive that answer; severities and open questions are in §§7–8:

| # | Severity | Finding |
|---|---|---|
| F1 | **High** | The 84/84 matrix ran on build `84e15c3`, not on the release HEAD (`01438e0`/merge `6e30a65`) that contains the "stale registration error" fix and the release notes. No real-driver evidence exists for the binary the release would ship. |
| F2 | **High** | The harness *refuses* to run unless the WinDivert service is `NOT_INSTALLED` and WFP is `0/0/0/0` — i.e. it excludes by construction the kernel state the product itself leaves after a run and the state the user's machine was in. Dirty-kernel recovery is not a row; it is a preflight failure. |
| F3 | **High** | The fixture cannot exercise a game-server transition or a working return path. The synthetic game always sends to one fixed blackholed address (`8.8.4.4:40000`); all 30 judged windows in the saved QA traces are `verdict=no_reply_from_relay, injected=0, from_relay=0, share=0.0%`. The real trigger context (stale server re-detect → re-registration → relay demotion → stop/restart) has no row. |
| F4 | **Medium-High** | Pass criteria are trace-marker absences, not behaviour. No row can express "interception failed while the control plane registered" — the exact state at 06:49:39–06:49:43 — and no row observes the GUI error/registration cards. |
| F5 | **Medium** | "84/84" is 84 assertions over ~14 scenario families (30 Stop→Optimize cycles and 20 relaunches inflate the count). It is not 84 distinct risks, and the failing-before control row was excluded from the count. |
| F6 | **Medium** | The design assumption in `windivert_handle.rs` ("with a pin held, `FWP_E_IN_USE` must be another process and cannot be retried") is contradicted by the trace; and — by source reading, no GUI run — the GUI has **no production path that clears a generic command error on success**: `SessionCommandState::finish` clears only the in-flight marker, `clear_error()` is never called in production, and the 1.12.5 fix clears only errors whose detail contains `control-plane registration failed`. The "stale error" class the user hit (a WinDivert remedy card) is not covered. |
| F7 | **Medium** | The trace is a multi-build file with no build stamp. Two of its segments log `handle open retry … pin_active=true`, a combination no committed revision can emit (`should_retry_fwp_in_use` requires `!pin_active` since `9e73987`). The 06:49 segment matches the committed 1.12.5 retry policy, but attributing it to the 1.12.5 binary is unproven. |
| F8 | **Low-Medium** | QA evidence completeness: kernel-state snapshots exist for setup, the 20 quits, force-kill and update only; saved trace slices exist only for the 20 quit runs. Scope-change, game-restart, relay-change, TCP and hold phases are asserted in-memory and cannot be re-analysed from the evidence directory; the 84/84 run contains no UI capture. |

**What the audit can state as fact:** the 84 rows did pass on that binary in
that environment; the trace shows a real failure on a real machine; the two do
not refute each other. **What the audit cannot state:** that 1.12.5 fixes or
introduces the field fault, that the field failure reproduces on the release
binary, or that any row in the matrix covers the 06:49 scenario.

---

## 1. Evidence base and method

### 1.1 Inputs

| Evidence | Identity |
|---|---|
| Trace slice `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log` | 1,400 lines, 225,050 B, SHA256 `5DFFF05824AE65FC20D4D7281C454E1DC5E52B14CA0B4477DEF562882B317331` |
| Full trace `%LOCALAPPDATA%\Lightspeed\gui-trace.log` | 106,658 lines, 14,776,101 B, SHA256 `F48EE174A16ACCA4EEA2AF2B2FBF5140DD5B5C1A8108BD72D7BB6A0A59757F1B`, last write 2026-11-23 13:50:15 +07 |
| 84/84 matrix run `.agents/worktrees/pinned-driver-1.12.5/wat/evidence/2026-11-23/task-107-matrix-84e15c3v2-20261123-115633/` | 84 rows, `matrix.jsonl` SHA256 `8F879E42D43D623CABD49749B066C0678E74DFAA05606952D4CF93BD52FCC284`, started 11:56:33 +07, finished 12:09:01 +07 |
| QA report `.agents/worktrees/pinned-driver-1.12.5/wat/evidence/2026-11-23/task-107-release-report.md` | SHA256 `2289E5CE9E8D70F257593E29AA7F848D65B25C520F1BF903FE053A5B97679415` |
| Harness `scripts/windivert-pinned-harness.ps1` / `-lib.ps1` (pinned worktree) | SHA256 `9E5B1BA4…` / `06BBA0DE…` (match the QA report's round-2 hashes), 2,216 / 1,460 lines |
| Source under audit | `.agents/worktrees/windivert-stability-audit` at merge `6e30a65` (contains `9e73987`, `84e15c3`, `01438e0`) |

### 1.2 Slice provenance (verified)

The slice is the **last 1,400 lines of the full trace**, line-for-line: after
stripping non-ASCII characters **all 1,400 lines are identical**, and every
difference is a non-ASCII glyph stored as double-encoded mojibake (`ðŸŽ¯` for
`🎯`; arrows likewise). Reading order, timestamps, counts and ASCII text are
unaffected. The slice
therefore begins at 2026-11-23T05:24:23.641472Z and ends at the last line the
full trace ever wrote, 2026-11-23T06:50:15.173465Z; the GUI has not run since.
Timestamps carry `Z` and are UTC; the host is UTC+07, cross-checked against the
in-log `at_unix_ms=1795416333454` ↔ `2026-11-23T06:45:33.454Z`. Local times in
this document are UTC+07 where stated.

`-2144206838` is the signed 32-bit rendering of `0x8032000A`, i.e. **`FWP_E_IN_USE`**
("The object is referenced by other objects so cannot be deleted"); the app's own
retry line spells it `after=FWP_E_IN_USE(0x8032000A)` (`user-server-transition-trace.log:307`).

### 1.3 Method

1. Reconstruct the exact event order from the slice and independently cross-check
   it against the full trace (frequency by date, absence of later runs).
2. Read the QA harness, the 84 rows, the saved QA traces and the WFP/tool
   evidence to determine what each row actually exercised and asserted.
3. Read the shipped source (`client/`, `client-gui/`) for the lifecycle/error
   paths the log lines come from, to separate what the code does from what the
   report says it does.
4. Classify every claim as **fact** (directly observable in stored evidence) or
   **inference** (a mechanism that fits the facts but was not directly observed).
5. Produce the minimum acceptance matrix and instrumentation that would close
   each gap, with pass criteria that are not "no error".

No command that changes state was run: all commands were `git show/log/status`,
file reads, `Get-FileHash` and read-only text processing.

---

## 2. Evidence timeline around 06:48–06:50, and the context that makes it legible

All timestamps are UTC (host local +07 in parentheses). Line numbers refer to
`user-server-transition-trace.log` (the 1,400-line slice). Quoted text is verbatim
except where `…` elides the long WinDivert filter string.

### 2.1 Facts, in order

| # | UTC | Local | Line | Observed fact |
|---:|---|---|---:|---|
| 1 | 06:41:21.880 | 13:41 | 535 | `LightSpeed GUI starting` after a **30 min 39 s gap** from the previous quit (06:10:42). |
| 2 | 06:41:35.50 | 13:41 | 572 | pin opens: `WinDivert handle open: id=1 kind=pin … filter="false" -> ok`; intercept/inject/shadow then open `-> ok` (`:580,:583,:586`). |
| 3 | 06:41:50–06:42:29 | 13:41–13:42 | 617–843 | **Four fast Stop→start cycles**, each: `Shutdown signal received`, diverting handles (ids `2/3/4`, `5/6/7`, `8/9/10`, `11/12/13`, `14/15/16`) close `-> ok` and reopen `-> ok`, `diverting handles closed; driver pin active=true`; no FWP. Final `Quit requested` at `:840` releases pin `id=1`. |
| 4 | 06:42:34.42–06:42:38.34 | 13:42 | 844–917 | GUI starts, requests elevation (`:879`), elevated instance starts with `--force` (`:881`, `:894`), unelevated process confirms and exits (`:911`). Both instances register (`:877` session=1996858841, `:916` session=1883557116). The "Single-instance guard bypassed" line is the **elevation handshake**, not a second user launch. |
| 5 | 06:43:34.53 | 13:43 | 925–926 | `Game presence changed: robrun`; `Game present and relay connected; starting interception for robrun`. |
| 6 | 06:43:34.775 | 13:43 | 941 | pin opens **`id=1`**, held until quit at 06:50:15. |
| 7 | 06:43:34.99–06:43:35.44 | 13:43 | 983–996 | Hold armed, server locked `178.107.14.146:36701`, relay choice keeps `207.246.106.36:4434` "applies next session", **two registrations 0.21 s apart** (`session=779093578`, then `session=2910297239`). |
| 8 | 06:43:45–06:47:58 | 13:43–13:47 | — | Traffic-bearing normal session: `Relay reply window judged … verdict=healthy` every 8 s, shares 51–97 %, `SERVER_LEG_*`/`CLIENT_TRACE` lines, `session token rotated` at 06:45:35 and 06:47:35. |
| 9 | **06:47:59.324** | 13:47 | 1239 | `🔄 Locked server stale - re-detecting`. |
| 10 | 06:48:00.36–06:48:01.10 | 13:48 | 1245–1253 | New server locked `178.107.14.45:36501`; relay kept; `Registered with proxy: session=2427923240, token=692112952`. |
| 11 | 06:48:20.157 | 13:48 | 1266 | `Relay reply window judged: … verdict=no_reply_from_relay … server=178.107.14.45:36501` (streak 1). |
| 12 | **06:48:34.177** | 13:48 | 1278–1288 | `Locked server stale - re-detecting` **again**; new server `178.107.13.95:36605`; relay kept; `Registered with proxy: session=381747847, token=1826986767`. Two server re-detections 34.9 s apart; four relay registrations in the same session (two at start, one per re-detect). |
| 13 | 06:48:42.669–06:49:26.691 | 13:48–13:49 | 1295, 1307, 1320, 1331 | Four consecutive judged windows `verdict=no_reply_from_relay` (streak 1→4), `intercepted=34→22 injected=0 from_relay=0 share=0.0%`. |
| 14 | 06:48:59.677 / 06:49:13.184 / 06:49:26.691 | 13:48–13:49 | 1308, 1321, 1332 | `WARN Relay return path is starving the session; keeping the relay and warning the player (no automatic path switch)`; `fault=Some(NoReplyFromRelay)`. |
| 15 | 06:49:00.022 | 13:49 | 1309 | `The return-path watchdog found no reply for this server; the relay is demoted for the next ranking. The session keeps relaying and the player has been warned`. |
| 16 | **06:49:38.417** | 13:49 | 1340 | `🛑 Shutdown signal received` — a **true Stop** (no `Quit requested`), 11.7 s after the last starvation warning. |
| 17 | 06:49:38.56 | 13:49 | 1344–1349 | Firewall rule removed; intercept/inject/shadow `id=2/3/4` close `-> ok`; **`WinDivert diverting handles closed; driver pin active=true`** (pin `id=1` still held). |
| 18 | **06:49:39.827–06:49:39.84** | 13:49 | 1350–1362 | A **new start attempt** begins 1.27 s after the Stop: `ProcessScanner: found ROB RUN.exe (PID=6908) with 7 routes`, `Telemetry flush started`, intercept filter printed, `Connecting QUIC control plane`, `Built-in detection target`, `Opening WinDivert intercept handle...`. No presence-transition line and no command/actor line precedes it. |
| 19 | **06:49:39.841** | 13:49 | 1363 | `WARN WinDivert handle open: id=5 kind=intercept attempt=1 … -> FAILED (The object is referenced by other objects so cannot be deleted. (os error -2144206838))`; **no `handle open retry` line follows** (pin is active → committed policy does not retry). |
| 20 | 06:49:39.8419 | 13:49 | 1364–1365 | `ERROR ❌ WinDivert intercept open failed: … A previous LightSpeed run left its WFP filter/callout registered…`; `ERROR start_interceptor failed: AppError { kind: Tunnel, detail: Some("WinDivert has a stale WFP filter from a previous run. Close any stale LightSpeed process and restart Windows to clear it before retrying. …") }`. |
| 21 | 06:49:40.275 | 13:49 | 1368 | `Registered with proxy: session=2861874939, token=2467109933, node=relay-lax-1, region=us-west` — **registration succeeds after the interceptor start failed**. |
| 22 | **06:49:42.676–06:49:42.690** | 13:49 | 1369–1382 | A **second full start attempt** 2.83 s after the first: ProcessScanner, telemetry flush, filter, `Connecting QUIC`, target, `Opening WinDivert intercept handle...`, `id=6 … -> FAILED (FWP_E_IN_USE)`. Again no retry line. |
| 23 | 06:49:42.6908–06:49:43.129 | 13:49 | 1383–1386 | Same two ERROR lines; then `Registered with proxy: session=8752802, token=864653562` (note: a 7-digit session id, an order of magnitude smaller than the other 2026-11-23 registrations, which are 9–10 digits). |
| 24 | 06:49:43–06:50:14.9 | 13:49–13:50 | 1387–1395 | No further start attempts, no presence lines, no stop lines. Only `Auto-detected game: ROB RUN` (~every 5 s) and `Latency comparison` with **frozen values** (`shadow_samples=7`, `relay_samples=1`, same relay_ms range) until `Quit requested - stopping engine` at **06:50:14.905**. |
| 25 | 06:50:15.173 | 13:50 | 1398–1400 | `WinDivert handle close: id=1 kind=pin -> ok`; `📌 WinDivert driver pin released (id=1); the driver may now unload`; `Interception driver pin released after handle teardown`. This is the last line in the full trace. |

### 2.2 Earlier context that changes the reading (05:24–06:10)

| UTC | Line | Fact |
|---|---|---|
| 05:24:21–05:25:55 | 105245 (full trace), 3, 95, 116, 136 (slice) | The **same `FWP_E_IN_USE`** appears 5 times in this window: once from a pre-slice engine run, then after each attempted (re)start. |
| 05:24:34–05:24:35 | 44–68 | `elevation requested` → elevated instance starts → unelevated exits. |
| 05:55:03.647 | 208–210 | A build **with the pin** opens `id=1 kind=pin`; the very next intercept open (`id=2`) fails `FWP_E_IN_USE` at 05:55:03.656. |
| 06:01:02.880 / 06:03:24.580 | 306–311, 385–390 | The same failure, now with the bounded retry loop: **4 attempts fail in ≤ 550 ms, each with `pin_active=true`** in the retry line. |
| 06:09:48.66 / 06:10:24.77 | 460–469, 520–529 | The same failure **with `pin_active=false` and no pin-open line at all** — i.e. in a fresh process that never opened a pin in that run. |
| 06:10:42.074 → 06:41:21.880 | 534 → 535 | 30 min 39 s with no LightSpeed lines at all; the next run works for 8 minutes. |

**Reading:** `FWP_E_IN_USE` on an intercept open is not new to the pinned build
(21 occurrences on 2026-11-21, 4 on 2026-11-22, 12 on 2026-11-23 in the full
trace, many before the pin existed), it can appear with no pin in the process,
and it can appear with the pin held immediately after the app's own Stop. Those
three facts bound what the pin can be responsible for.

### 2.3 Facts vs inference

**Facts (observed):**

- The pin was held (`id=1`) from 06:43:34.775 to 06:50:15.173, across the Stop at
  06:49:38 and both failed restarts.
- The Stop logged all three diverting handles closed `-> ok` and explicitly
  recorded `driver pin active=true`.
- The next start began 1.27 s later; the intercept open failed on the first
  attempt with `FWP_E_IN_USE`; there was no retry line; the start failed; the
  control plane registered 0.43 s later.
- A second full start attempt 2.83 s later failed the same way and registered a
  third session.
- The preceding session had two server re-detections 34.9 s apart, four
  registrations in the same session (two 0.21 s apart at start, then one per
  re-detect), four consecutive no-reply judged windows and a relay-demotion
  warning.
- The user quit 31 s after the second failure; nothing was retried in between.

**Inference (plausible, not proven):**

- *Who initiated the restarts.* The trace records no command, actor or reason for
  either start attempt. `poll_game_presence` would have logged
  `Game present and relay connected; starting interception for robrun`
  (`client-gui/src/app.rs:2144-2155`) — that line is absent. A user double-click
  of Optimize, an app-internal retry, or an unattended recovery path all fit. The
  2.83 s gap is more consistent with two user clicks or a two-attempt retry than
  with an unbounded loop, but this is an inference.
- *Why `FWP_E_IN_USE` appeared after the app's own clean close.* Candidate
  mechanisms, none observed directly: (a) deferred/retained WFP object references
  after the last `WinDivertClose` (the close returns before the driver finishes
  releasing objects); (b) the 1.27 s start racing that release; (c) a stale WFP
  provider/sublayer from an earlier process/update in this long-lived machine
  session; (d) a second holder (another process/handle) that the trace cannot
  see. The trace supports "the app's own Stop preceded it"; it does not establish
  which mechanism.
- *Why the QA environment never saw it.* Steady-state, slow (≥2.7 s after Stop),
  same-server, healthy-window cycles; a single blackholed destination; a clean
  kernel at session start; no in-place re-registrations. See §3.
- *What happened between 06:10:42 and 06:41:21.* A reboot, an app update (the
  1.12.5 updater now runs `sc.exe stop WinDivert` before copying driver files),
  or a manual `sc stop` would all clear a stale kernel state and explain the
  8-minute healthy run that followed. This is the **most likely** reason the
  06:41 run opened handles cleanly after the 06:10 failures, but it is not in the
  trace. Windows event logs / update logs would answer it; this audit did not
  read them (read-only, and they are outside the evidence set).
- *The user-visible "stale registration error".* The trace contains **no**
  `control-plane registration failed` line and **no** registration-failure
  warning anywhere in the 11-23 window; every registration in the slice
  succeeded. If a red registration/error card was on screen, it is a UI-state
  fact that the log does not record (the GUI never logs error-card show/clear).
  The phrase cannot be confirmed or refuted from this evidence set.

### 2.4 Challenges to the current narrative

1. **"Auto-start FWP."** The trace shows a start, not an auto-start. The
   presence-path line that would prove auto-start is absent; only
   `ProcessScanner` evidence exists. Both a user click and an internal retry
   remain open (Q1 in §8).
2. **"A previous LightSpeed run left its WFP filter."** At 06:49 the previous
   run was *this process*, whose diverting handles had closed 1.27 s earlier and
   whose pin the app itself held open on purpose. The remedy text is inherited
   from `driver_open_remedy()` (`client/src/interceptor/windivert_handle.rs:498-521`)
   and is factually wrong for the self-induced case; it sends the user to
   "restart Windows" for a state the product creates.
3. **"The pin removes the unload/reload churn path."** The pin does keep the
   driver loaded, but the trace shows the failure both with the pin held and
   without it, and shows five Stop→reopen cycles with the pin held between
   06:41:50 and 06:42:01 plus another successful pin-held open at 06:43:34.
   Pin-active is neither necessary nor sufficient for `FWP_E_IN_USE`;
   the release note's narrower claim ("the churn path is removed by
   construction") is true, but it is not the fault mechanism.
4. **"The matrix passed, so the fix is good."** The matrix tested a different
   binary from the release HEAD (F1) in a state (F2) and under a scenario set
   (F3) that excludes the observed failure context.
5. **"84/84."** The number is real as a row count, misleading as a coverage
   statement (F5, §3.1).

---

## 3. QA harness and 84-row coverage audit

### 3.1 What the 84 rows are

84 is the count of assertions written to `matrix.jsonl`; the run is
`Matrix` phase only (no control). Composition:

| Row family | Rows | Distinct scenario |
|---|---:|---|
| preflight, build identity, first pin, first traffic, pin identity, phase-evidence, cleanup, cleanup-WFP | 8 | 2–3 |
| same-scope Stop→Optimize cycles | 30 | **1** |
| Quit→relaunch at varied delays | 20 | **1** |
| scope/game/profile changes | 10 | **1** (same mechanism, 4 profiles) |
| game restarts (auto-stop/auto-start) | 5 | **1** |
| relay change while running, TCP profile Stop, Stop during hold, force-kill active, force-kill idle | 5 | 5 |
| self-update contract, app update while active | 2 | 2 |
| row-family summaries | 4 | — |
| **Total** | **84** | **~14 scenario families** |

84/84 PASS is therefore accurate *as rows*, and it means "all 84 assertions the
harness defines held", not "84 risks were exercised". The failing-before control
row was not part of this run (it lives in a separate control run, BLOCKED).

### 3.2 The preflight gate excludes the field state

`scripts/windivert-pinned-harness.ps1:240-265` refuses to run when:

- `sc query WinDivert` state is `RUNNING` → *"stale kernel from an earlier
  forced kill; reboot required"*;
- the service has a non-zero exit code → *"reboot required"*;
- `netsh wfp show state` reports any provider/sublayer/filter/callout → *"stale
  WinDivert WFP objects at baseline … reboot required"*;
- `WinDivertTool` sees any handle → *"stale kernel; reboot required"*;
- any `lightspeed-gui`, `lightspeed-proxy`, ROB RUN or known-game process, or
  any other catalogue game, is running.

This is a **correct measurement-control rule** (you cannot measure driver
lifecycle from an unknown kernel), but it has a product consequence: the only
states the matrix ever measures are states in which the product has just booted
into a clean kernel. The product's own post-run state (WinDivert service loaded,
WFP provider/sublayer `1/20` after every Quit — documented in the QA report §4
and the 84/84 summary tail) is **defined as a preflight failure**, not as a
scenario. Recovery from a dirty kernel is therefore structurally untestable in
this harness. The trace shows exactly such states: 5 failures at 05:24–05:55 and
at 06:09–06:10 in which the machine was not clean, and a 30-minute gap before
the next clean run.

### 3.3 The fixture cannot produce the trigger context

- `scripts/windivert-pinned-lib.ps1:29` fixes the synthetic game server to
  `$script:LS_GAME_SERVER = '8.8.4.4'`; `Start-GameDouble`
  (`harness:500-526`) launches a renamed PowerShell host sending 64 B UDP every
  100 ms to that one literal; a `/32` blackhole route keeps it local
  (`harness:419-446`). Every phase therefore has **one fixed server endpoint**;
  the scope-change phase changes the *game profile* (name + port), never the
  server *within* a session; the game-restart phase restarts the same endpoint.
- Consequently the app's server-transition paths are never triggered: in the 20
  saved `trace/quit-run-*.log` files there are **0** occurrences of
  `Locked server stale`, and `Relay chosen for` never appears. The status line
  `Pinned relay: re-arming the relay for the new game server …` appears 51 times
  only because each new session re-arms the same fixed server.
- Because the destination is blackholed and the loopback relays have no game
  peer, the **return path is always starved**: all **30/30** judged windows in
  those traces are
  `verdict=no_reply_from_relay injected=0 from_relay=0 share=0.0%`.
  Two representative lines:

  ```text
  Relay reply window judged: intercepted=7072 injected=0 from_relay=0 share=0.0%
    streak=1 verdict=no_reply_from_relay … server=8.8.4.4:40000   (quit-run-1.log)
  Relay reply window judged: intercepted=8072 injected=0 from_relay=0 share=0.0%
    streak=1 verdict=no_reply_from_relay … server=8.8.4.4:40000   (quit-run-1.log)
  ```

  The real trace's failure context is precisely the *continuation* of this
  condition: `no_reply_from_relay` streaks 1→4, the starvation warning, and a
  relay demotion. The matrix stops each session after the first judged window
  (`harness:809`), so it never accumulates a streak and never exercises the
  watchdog/demotion/re-registration path that the field failure followed.
- "Relay change while running" moves the control plane between two loopback
  relays for the same fixed server and asserts *no handle recreation* — it is
  a relay-selection test, not the app's server-driven re-registration path
  (`Pinned relay: re-arming…`, `Keeping the live session…`).

### 3.4 Rows that are misleading as named

| Row as reported | What it actually establishes | Why the name overstates it |
|---|---|---|
| `traffic-bearing proof (judged window) … intercepted=7072` | 7,072 outbound packets were intercepted and attributed to the fake server; the relay forwarded them. | There is no return traffic: `injected=0 from_relay=0 share=0.0% verdict=no_reply_from_relay`. It is an **outbound capture/forward** proof, not a two-way tunnel proof. |
| `same-scope cycle N … relay_forwarded_delta=8000` (×30) | Outbound packets reached the loopback relay; handles reopened; quiet-window delta 0. | Same return-path gap; and the reopen happens ≥2.7 s after teardown because of a 700 ms sleep plus a 2,000 ms quiet-window settle (`harness:683-694, 825-828`), so "Stop→Optimize" is measured at a much slower cadence than the 1.27 s field restart. |
| `relay change while running … trafficA/B_delta=1000` | Control-plane re-registration on relay A/B; forwarding counter moved. | Return path absent throughout; no server change; the app's server-driven re-arm (`Pinned relay: re-arming…`) is not what this row tests. |
| `preflight clean environment` (counted as a row) | The harness environment was clean. | A *precondition* counted as a product row inflates the pass total and hides that the precondition excludes field states. |
| `app update while active` (PASS) | Swap-script contract + `sc stop WinDivert` remedy + relaunch. | Semantics were redefined in round 2: "sys locked after quit" moved from FAIL to expected; the row now asserts the remedy, not that an update from a published asset works (none exists). |
| `cleanup leaves no diverting WFP filters/handles` | filters=0, callouts=0, tool handles=0; provider/sublayer remain `1/20` (driver loaded). | Correct as scoped, but a reader can take it as "no WFP residue"; the quota was captured in the summary tail, not in the row name. |
| `84 rows: 84 PASS` | 84 assertions over ~14 families. | Reads as 84 scenarios to a lay reader. |

The one genuinely strong family is the pin/quit lifecycle itself (30 cycles, 20
relaunches, force-kill active/idle): the handle-pairing, pin-open-once and
per-kind counters are good instrumentation and the base lifecycle claims are
properly evidenced by it (`trace/quit-run-*.log`, `wfp-summary.jsonl`,
`windiverttool-summary.jsonl`).

### 3.5 The tested binary is not the release HEAD

- The 84/84 run's `run.json` records the GUI as
  `…\ls-pinned-qa-target\debug\lightspeed-gui.exe`, SHA256
  `F8ECE5824D674B186D2DB6C2D91703858F597B37B8A7FA3F317B4A6936EA8DFC`, which
  the QA report identifies as commit **`84e15c3`** (11:43:05 +07).
- The release commit **`01438e0`** ("release: 1.12.5 pinned WinDivert driver and
  stale error fix", 12:27:28 +07) is **later** and changes
  `client-gui/src/app.rs` (`poll_health` now calls
  `clear_stale_registration_error`, `app.rs:1535-1538`),
  `client/src/app/commands.rs:64`, `client/src/app/dto.rs:447`, and adds one
  unit test (`dto.rs:655-674`). `git show 84e15c3:client/src/app/commands.rs`
  has no such symbol. The merge **`6e30a65`** (12:36:09 +07) is HEAD.
- The matrix finished at **12:09:01**, i.e. before `01438e0` existed. **The
  "stale registration error" fix was never exercised on the real driver or in
  the GUI; its only evidence is one unit test.** The release notes and WAT state
  present the 84/84 result for a binary that cannot contain that fix.

### 3.6 Implementation-level assumptions the evidence contradicts

1. **Retry policy / remedy.** `should_retry_fwp_in_use` returns
   `!pin_active && is_fwp_in_use(err)` (`windivert_handle.rs:327-329`) and
   `open_with_pin_retry` skips the retry when a pin is held
   (`:622-656`). The stated reason is "with the pin held the driver is already
   loaded, so the same code is a persistent stale filter from another process".
   The 06:49 evidence is the app's own pin + its own just-closed handles. The
   no-retry decision turns a possibly transient self-state into an immediate
   user-facing "restart Windows" error, and the two attempts 2.83 s apart show
   the state persisted at least that long.
2. **Error persistence.** `SessionCommandState::finish(command)`
   (`dto.rs:425-430`) clears only `in_flight`; it does not clear `last_error`.
   `AppCore::clear_error()` (`commands.rs:57-59`) has **no production call
   site** (verified by repository grep: only its definition and one unit-test
   use). `clear_stale_registration_error()` (`commands.rs:64-68`) clears only
   errors whose detail contains `control-plane registration failed`, and is
   called only while `status.control_registered` is true (`app.rs:1535-1538`).
   The error card precedence (`app.rs:2937-2960`: last command error first)
   means a failed `StartBoost` WinDivert remedy remains the visible error until
   the app restarts or a different error replaces it — the user's 06:49 card is
   in that class. This is a *code-reading* finding; no GUI run was performed to
   observe it (read-only audit).
3. **Registration vs interception consistency.** Registration is started by the
   same start pipeline (QUIC connect → register) while the interceptor open
   fails first. The log shows a successful registration 0.43 s after
   `start_interceptor failed`. Nothing in the GUI lowers a registration success
   into a failed interception state or vice versa; the two cards can disagree
   and neither is logged.
4. **No actor/reason on lifecycle events.** The trace has rich handle-level
   instrumentation but no event saying *who* asked for the start/stop, *why*,
   or which command id/session it belongs to. That is why the 06:49 actor is
   unknown (§2.3).

### 3.7 Coverage gap table

| Risk / scenario | Exercised by 84/84? | Evidence |
|---|---|---|
| Clean-boot first start, pin once | Yes | preflight/setup rows |
| Steady Stop→Optimize, same server, healthy window | Yes (30×, slow cadence) | cycles rows |
| Quit→relaunch (20 delays) | Yes | quit rows + traces |
| Game exit/restart auto stop/start | Yes, fixed endpoint | restart rows |
| Relay A↔B control-plane switch | Yes, fixed endpoint | relay row |
| Game-server transition / settlement / POI change | **No** | harness has one fixed server; 0 `Locked server stale` |
| Stale-server re-detect → re-registration in place | **No** | same |
| Return-path starvation streak + relay demotion | **No** (always streak ≤1; row stops at first judged window) | 30/30 no_reply windows |
| Stop while degraded/starving = field 06:49 context | **No** | no phase |
| Restart ≤1.5 s after Stop (fast reopen) | **No** (≥2.7 s by design) | cycle timing |
| Repeated start after a failed start (error → retry) | **No** | no phase |
| Presence flicker (lost/regained, latch behavior) | **No** | no phase |
| Dirty-kernel first start / recovery | **No — preflight refuses** | preflight code |
| Error-card show/clear and stale-card recovery | **No** (no UI/state assertion) | 84/84 has no UIA/screenshot |
| Build identity of the running trace | **No** (not logged by the app) | trace |
| Long session (≥30/60 min) with many transitions | **No** (8 min max in-run) | traces/phases |
| Force-kill during a transition/starving session | **No** (idle/active only) | force-kill rows |

---

## 4. Minimum acceptance matrix for the next fix

Rules for this matrix (they are deliberately stricter than "no error"):

1. **Build identity first.** A row only counts if the candidate binary's GUI,
   proxy, `WinDivert.dll` and `WinDivert64.sys` SHA256 and the source commit are
   recorded in the run and logged by the app at startup. The candidate is the
   exact release HEAD, not an earlier commit.
2. **Two-way traffic.** Every "traffic-bearing" row requires at least two
   consecutive judged windows after the transition with `injected > 0`,
   `from_relay > 0` and `share ≥ 1/8` (the product's own
   `RETURN_SHARE_DENOMINATOR`), plus the relay counter deltas in both directions.
   Outbound-only evidence is labelled as such.
3. **State ledger.** Every row checks the handle ledger (`pin` open count = 1,
   every intercept/inject/shadow open id has exactly one close id), the
   registration ledger (each registration has session/token and a close or
   supersede event; at most one active session), and a WFP/handle snapshot at
   the phase boundary.
4. **UI truthfulness.** Each transition row captures the GUI state (semantic
   dump and screenshot) and asserts the visible primary state matches the
   engine facts; a failed interception with a successful registration must not
   present as "connected & optimizing".
5. **Error lifecycle.** A failure row asserts how the error card is cleared:
   same-class success clears it within 1 s, unrelated errors are preserved, and
   the card text names the failed component and a correct remedy.
6. **Recovery bound.** No row may require a reboot to pass. If a reboot is
   genuinely required, that is a **FAIL** row and the product must say so before
   the failure, not after.
7. **Independence.** A second agent re-runs the matrix on the final tree and
   signs it; the author of the fix does not sign its own evidence.

### Matrix

| ID | Scenario (minimum) | Steps | Pass criteria beyond "no error" | Required evidence |
|---|---|---|---|---|
| A1 | Clean-boot first start | Clean boot → launch → game present → relay connect | pin opens once; first session gets 2 healthy judged windows (`injected>0`, share≥1/8); WFP baseline 0/0/0/0 before, filter/callout 0 at idle; build identity logged | trace with build stamp; WFP+tool snapshots; UI state |
| A2 | Traffic-bearing settlement/POI switch | Fixture with ≥2 reachable game-server endpoints (different ports/IPs) that send and receive; drive the game to switch between them mid-session | `Locked server stale - re-detecting` logged with old+new endpoint; relay choice reason logged; registration ledger shows the old session superseded and exactly one active session; interception continues (or reopens with paired ids) with 0 `FWP_E_IN_USE`/0 failed opens; ≥2 consecutive healthy judged windows after the switch | trace; relay-side session log; handle/registration ledger; WFP snapshot after switch |
| A3 | Region/auth transition | Change relay/region while a server is locked, then switch server again | same ledger rules; no duplicate/overlapping registration; data plane continuity; error card never shows; relay demotion state consistent | trace + relay logs + UI |
| A4 | Stop while degraded (field shape) | Drive 4 consecutive no-reply windows → relay demotion → Stop → immediate Start (≤1.5 s) and Stop → start at 3 s / 10 s | reopen succeeds every time (0 FWP, 0 failed opens) **or** a documented lockout with a truthful, non-reboot remedy and automatic recovery; handle ledger paired; error lifecycle clears on the successful retry | trace; WFP snapshots before/after each attempt; UI |
| A5 | Fast-restart timing matrix | Optimize at 250 ms, 1 s, 2 s, 5 s after Stop, 10× each | every open either succeeds or fails into a *documented* state; no non-deterministic mix across repetitions; ids increment correctly | trace with timing annotations; harness timings |
| A6 | Repeated long-session transitions | ≥60 min boosting, ≥10 server transitions, ≥20 in-place re-registrations, ≥5 relay switches | 0 FWP; 0 failed opens; handle/fd/handle counts stable; registration active-session count ≤1; error card count 0; healthy-window share maintained after each transition; no monotonic growth in handles/threads/RSS | 60 min trace; periodic WFP/tool/handle snapshots; resource samples |
| A7 | Presence flicker and auto stop/start | Game exits/returns 3× at 0.5–2 s gaps; rapid presence name changes; scan error injected | presence transitions logged with cause; latch behaves (no restart storm); pin unchanged; each auto-start either succeeds with traffic or produces exactly one truthful failure; no stale error | trace; UI captures; registration ledger |
| A8 | Stale error recovery (functional) | Induce a controlled StartBoost failure (test hook or real condition), then a successful StartBoost | error card reflected the failure; on success it clears within 1 s; unrelated error survives; registration card and route card consistent | UI semantic dumps before/after; trace with error lifecycle events |
| A9 | Quit/relaunch | Quit at idle, during boost, during a transition, during a starving session; relaunch at 500 ms–5 s | pin released exactly once after teardown; relaunch: one pin, clean intercept open, 2 healthy windows; no reboot needed; WFP filter/callout 0 at idle | trace + WFP snapshots + UI |
| A10 | Force-kill | Kill during boost, during a transition, during a starving session, while pin held; relaunch | no orphan handles; first relaunch either starts cleanly or reports a truthful recoverable state; next-start recovery marker detects the unclean shutdown; 0 FWP; WFP has no diverting filters/callouts | trace + WFP/tool snapshots + recovery log |
| A11 | Dirty-kernel recovery (the inversion) | Start from the state the product itself leaves: driver loaded, WFP provider/sublayer `1/20`, no process holding handles; then launch | product opens cleanly on first attempt (or after a bounded, logged retry) **without** reboot/`sc stop`; if it cannot, it must (a) detect the state, (b) name it accurately, (c) offer a bounded automated remedy, (d) log the evidence; "restart Windows" is not acceptable as the only remedy | pre-state WFP/tool snapshot; trace; UI; post-state snapshot |
| A12 | Build/artifact identity | Run the row set on the exact release HEAD; verify the shipped installer would contain the same binary | app logs version+commit+driver hashes; matrix blocked on mismatch; release notes carry the tested hashes | run.json + startup log line + package diff |

**Exit criteria for the next fix:** A1–A12 all PASS on the exact release HEAD,
with two consecutive independent full runs, a build-identity match, no row that
requires a reboot, and an explicit owner-accepted U-list for anything not run.
A matrix that excludes the dirty-kernel and transition rows (A2, A4, A11, A12)
cannot sign off a stability fix.

---

## 5. Instrumentation and log fields to add

The handle-level instrumentation already in `windivert_handle.rs` is good and
should be kept. What is missing is the **lifecycle-level** layer: who/what/why,
and a stable machine-readable shape instead of prose.

### 5.1 Required fields (v1)

All events as one line with `event=<name>` plus key=value fields (no payloads,
no process memory; the existing privacy posture is fine):

| Event | Fields |
|---|---|
| `app_start` | `version`, `commit`, `windivert_dll_sha256`, `windivert_sys_sha256`, `windivert_version`, `elevated`, `host_os`, `boot_id`, `session_id`, `prev_shutdown=clean|unclean|unknown` |
| `interceptor_state` | `from`, `to` (`idle|starting|boosting|stopping|failed`), `reason` enum (`user_start`, `user_stop`, `presence_start`, `presence_stop`, `relay_switch`, `server_transition`, `watchdog_demote`, `error_retry`, `quit`, `update`, `auto_recover`), `actor` (`user|auto|retry|internal`), `command_id`, `attempt`/`max_attempts`, `pin_id`, `pin_active`, `handle_ids=[intercept:…,inject:…,shadow:…]`, `locked_server_from`/`to`, `relay_from`/`to`, `duration_ms` |
| `windivert_open` | existing fields + `reason`, `attempt`, `pin_active`, `fwp_code`, `elapsed_ms`, `snapshot=…` (see 5.2) |
| `control_registration` | `attempt`, `outcome` (`ok|fail|timeout`), `session`, `token_id` (opaque), `node`, `region`, `reason`, `prev_session`, `superseded=true|false`, `close_reason` |
| `presence` | `from`, `to`, `cause` (`process_started|process_exited|name_match|scan_error|disabled`), `scan_ms`, `latch` |
| `watchdog` | `verdict`, `streak`, `fault`, `action` (`keep|demote|switch_direct`), `relay`, `server` |
| `ui_error` | `op` (`show|clear|replace`), `kind`, `detail_key`, `source_command`, `source_state`, `cleared_by` (`retry_success|state_reconcile|user|other`), `age_ms` |
| `shutdown` | `reason` (`quit|update|panic|elevation_handoff|kill`), handle ids closed, pin released, `clean=true|false` |

Every line should carry `build=…` and `command_id=…`, so a trace can be sliced per
build and per action. This alone would have answered Q1/Q2 in §8.

### 5.2 Snapshot-on-failure (automatic, rate-limited)

On any WinDivert open failure, the app should capture, once per 60 s and into a
support bundle directory (not the log):

- its own handle table (ids/kinds/filters) and the pin slot state;
- `sc query WinDivert` state and exit code (read-only);
- WFP summary counts for the WinDivert provider (read-only enumeration);
- `WinDivertTool`-style handle listing if available;
- the last 200 lifecycle events, the UI state dump and a screenshot.

That is the difference between "a stale WFP filter from a previous run" (a guess)
and an evidence-backed diagnosis.

### 5.3 How to capture it automatically

1. **Structured trace schema** (`event=`/`phase=`/ids) instead of parsing prose;
   keep the human messages for support.
2. **Startup build stamp** (version/commit/driver hashes) as line 1 of every run.
3. **Unclean-shutdown marker** written at start and cleanly removed at Quit;
   next start logs `prev_shutdown=unclean` and includes the marker contents.
4. **Support bundle command** (`lightspeed-gui --support-bundle`) that collects
   trace, WFP/service enumeration, config (redacted), UI dump and screenshot;
   the GUI offers it from the error card.
5. **Harness assertions on the new fields**, replacing prose regexes; the QA
   harness reads `event=interceptor_state`, `event=windivert_open`,
   `event=control_registration`, `event=ui_error` and asserts ledgers.
6. **Lifecycle counters in telemetry** (no payloads): starts/stops/failures by
   reason, registration supersedes, recovery from dirty kernel, error-card age.
   Dashboards/alerts: `fwp_in_use_per_100_starts`, `registration_supersede_rate`,
   `dirty_kernel_recovery_failures`.
7. **Regression corpus**: keep each field trace (anonymised) and replay it
   against the parser and against a fault-injection harness.

---

## 6. Continuous stability roadmap

**H0 — before the next release claim (this week).**
Land the fields in §5.1–5.3 (start with `build`, `interceptor_state`,
`control_registration`, `ui_error`, snapshot-on-failure); build the two-endpoint
fixture (A2) and the dirty-kernel recovery row (A11); re-run A1–A12 on the exact
candidate binary; publish traces + signatures; correct the release/WAT text so
the verified binary and the shipped binary are the same artifact.

**H1 — next 2–4 weeks.**
Soak matrix: 60–120 min runs with ≥10 transitions nightly (one host, synthetic);
force-kill injection at 5 lifecycle points; presence-flicker module; a
dirty-kernel matrix that starts from a *known product-created* state instead of
refusing it; error-lifecycle assertions and a UI-state harness (semantic dump +
screenshot diff); WinDivert version matrix (2.2.x) and EDR/firewall interference
matrix (owner hardware needed — owner-gated).

**H2 — quarter.**
Telemetry-backed stability SLOs (failure per 1,000 starts by reason; dirty-kernel
recovery success; stale-error age p95); canary rollout of driver-affecting
changes to the owner first; a public "known lifecycle states" support doc; a
yearly challenge of the core assumptions (pin necessity, no-retry-with-pin,
"reboot required" remedies) with experiments.

**Ongoing practices.**
Every fix of this class needs: a failing-before reproduction from the *field
shape* (transition/starving/dirty), a mutation check that the reproduction fails
without the fix, a two-run independent matrix on the final binary, and a
regression trace kept in the evidence corpus. Treat "reboot required" as a
defect signal, not a support script.

---

## 7. Findings and severity

| ID | Severity | Finding | Impact | Suggested owner |
|---|---|---|---|---|
| F1 | High | Matrix build `84e15c3` ≠ release HEAD `01438e0`/`6e30a65`; stale-error fix unverified on hardware | Release claim covers an untested artifact | Lead + qa |
| F2 | High | Preflight excludes the dirty-kernel state; no recovery row | The product's own residue is untestable; field failures become "environment" | qa + rust-dev |
| F3 | High | Fixture: one blackholed server, no return path, no server transition | The field trigger context (stale server, starvation, demotion, re-registration) is not exercised | qa |
| F4 | Medium-High | Pass criteria are marker absences; no UI/registration consistency assertion | A "registered but not intercepting + stale error card" state can pass | qa + interface |
| F5 | Medium | 84/84 = 84 assertions over ~14 families; control excluded | Overstates coverage; decisions built on it | Lead |
| F6 | Medium | No-retry-with-pin assumption + wrong remedy text; no success path clears generic command errors | User is told to reboot for a self-induced state; stale error card persists | rust-dev + interface |
| F7 | Medium | Trace has no build stamp and mixes builds; 06:49 attribution unproven | Cannot sign a fix for a reproduced fault without a build-stamped rerun | rust-dev |
| F8 | Low-Medium | Phase evidence/trace slices incomplete for most phases; no UI capture in 84/84 | Evidence cannot be independently re-analysed for 5 phases | qa |

**Severity of the product issue itself:** the trace is a real user-visible
failure (boost fails, registration says connected, remedy says restart Windows).
Whether it is a 1.12.5 regression is **unknown and unproven**; it is at minimum a
pre-existing class (21+4+12 occurrences across three days) that the pinned design
did not remove. That is a release-risk decision for the Lead/owner, not a
conclusion of this audit.

---

## 8. Open questions (with the evidence that would answer each)

| ID | Question | Why it is open | Evidence needed |
|---|---|---|---|
| Q1 | Who/ what initiated the two starts at 06:49:39.8 and 06:49:42.7? | No command/actor line; presence line absent; user absent from the evidence | `event=interceptor_state` with `reason`/`actor`/`command_id`; owner recollection |
| Q2 | Which binary produced each segment of the trace? | No build stamp; retry lines inconsistent with every committed revision | startup build stamp; rebuild/re-run with SHA logged |
| Q3 | Does `FWP_E_IN_USE` reproduce on the release binary with pin held after Stop + server transition + starving session? | Never reproduced in QA; trace not attributable | controlled A2+A4+A11 runs |
| Q4 | What cleared the kernel state between 06:10:42 and 06:41:21? (reboot? updater `sc stop WinDivert`? manual?) | Not in the trace | Windows event log / update log; updater log; boot id |
| Q5 | What are the duplicate/overlapping registrations in one session (06:43:35 ×2, 06:47:59→06:48:34 ×3, 06:49:40/43), and what does `session=8752802` mean? | Only success lines are logged; no supersede/close event; session ids differ by magnitude | `control_registration` lifecycle events with `prev_session`/`superseded` |
| Q6 | Was a stale registration/error card actually on screen at 06:49, and what did it say? | UI state is not logged and no screenshot exists in the evidence set | UI error lifecycle events; screenshot; owner report |
| Q7 | Does a successful StartBoost clear a previous StartBoost error card? | Code reading says no (`finish` does not clear `last_error`; `clear_error` unused) | unit test + A8 UI evidence |
| Q8 | Is the "no retry with pin held" rule correct, or should the app retry its own pin-induced opens? | Contradicted by the 06:49 sequence; not experimentally isolated | A4/A5 + a fault-injection experiment |

---

## 9. What would change this audit's verdict

- A controlled, build-stamped reproduction of the 06:49 state on the release
  HEAD, with the new instrumentation, that **passes** A2, A4, A5, A11, A12 and
  the A8 error-lifecycle row.
- A release artifact proven byte-identical to the tested binary, with the
  identity recorded in `run.json` and the startup log.
- Evidence that the 06:49 starts were user-initiated and that the product
  behaved within a documented contract (would lower F6/F7 but not F1–F3).
- Conversely: a single reproduction on the release binary turns F6/F7 into a
  High release blocker and invalidates the release notes' narrow claim.

---

## 10. Non-claims and limitations

- Not verified: that the field fault reproduces on 1.12.5, that 1.12.5 fixes it,
  that the 06:49 starts were auto or manual, that any particular WFP mechanism
  caused the failure, that the release binary behaves like `84e15c3`.
- Observed only: stored logs, QA evidence, source code. No GUI was launched, no
  driver/WFP state was read live, no reboot/update history was inspected.
- The 84/84 rows are quoted from `matrix.jsonl`/`summary.txt`; this audit did not
  re-run the harness (read-only task; and re-running needs admin + a clean boot).
- The trace slice's mojibake is a storage artefact only (§1.2); no bytes of the
  original full trace were modified.

---

## Appendix A — trace line index used in this document

Line numbers are the 1-based lines of
`wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log`
(the last 1,400 lines of the full GUI trace). Ranges are inclusive.

| Lines | Content |
|---|---|
| 1–4 | 05:24:23 pre-existing engine run hits `FWP_E_IN_USE` |
| 6–8 | 05:24:24 registration, then `Quit requested` |
| 9, 39–98 | GUI start / elevation handshake / registrations |
| 154–162 | token rotation; quit 05:29 |
| 163–220 | 05:54 start; 05:55:03 pin `id=1` then intercept `id=2` `FAILED` |
| 254–317 | 06:00:38 Quit; 06:00:45 start; 06:01:02 pin + 4 failed intercept attempts with `pin_active=true` retries |
| 328–396 | 06:03 another cycle; same failure |
| 410–471 | 06:09 run; failures with `pin_active=false` (no pin in the run) |
| 474–531 | 06:10 run; same |
| 534–617 | 06:10:42 Quit; 06:41:21 GUI start after 30 min 39 s |
| 572–626 | 06:41:35 pin `id=1`, first session, first Stop (handle ids 2/3/4) |
| 633–772 | 06:41:50 / 06:41:54 / 06:41:57 fast Stop→start cycles (`id=5–10`, `11–13`) |
| 774–843 | 06:42:00 cycle; 06:42:29 Quit releases pin |
| 844–917 | 06:42:34 GUI start; elevation; two registrations |
| 941–996 | 06:43:34 pin `id=1`; start; server lock; double registration |
| 995–1238 | healthy session, token rotations, server-leg traces |
| 1239–1288 | 06:47:59 and 06:48:34 stale-server re-detections and re-registrations |
| 1295–1332 | four `no_reply_from_relay` windows, starvation warnings, relay demotion |
| 1340–1349 | 06:49:38 Stop: handles `id=2/3/4` closed, pin active |
| 1350–1368 | 06:49:39 first start attempt: `id=5` `FAILED`, start failure, registration `session=2861874939` |
| 1369–1386 | 06:49:42 second start attempt: `id=6` `FAILED`, registration `session=8752802` |
| 1387–1395 | 31 s of no retry; stale cached latency/game lines |
| 1396–1400 | 06:50:14 Quit; 06:50:15 pin `id=1` released — last line of the full trace |

## Appendix B — QA evidence identity

- Run: `.agents/worktrees/pinned-driver-1.12.5/wat/evidence/2026-11-23/task-107-matrix-84e15c3v2-20261123-115633/`
  (`run.json`, `matrix.jsonl`, `summary.txt`, `preflight.json`, `wfp-summary.jsonl`,
  `windiverttool-summary.jsonl`, `phase-evidence.jsonl`, `trace/quit-run-1..20.log`).
- Build: GUI SHA256 `F8ECE582…`, source `84e15c3` (per QA report).
- Harness: `scripts/windivert-pinned-harness.ps1` SHA256 `9E5B1BA4…`,
  `scripts/windivert-pinned-lib.ps1` SHA256 `06BBA0DE…`.
- Report: `.agents/worktrees/pinned-driver-1.12.5/wat/evidence/2026-11-23/task-107-release-report.md`
  (authoritative run per §9; round-1 numbers are history).
- 30/30 judged windows `no_reply_from_relay` in the 20 saved traces; 0
  `Locked server stale`; 0 `FWP_E_IN_USE`; 20 pin opens / 20 pin releases.

## Appendix C — commands for the fix round (not executed here)

These are the read-only/admin commands the **next** verification round should
script; this audit did not run them:

```powershell
# identity
Get-FileHash <gui.exe> -Algorithm SHA256
(Get-Item <gui.exe>).VersionInfo
# kernel/service state (read-only)
sc.exe query WinDivert
netsh wfp show state file=<tmp>\wfp.xml
# handle listing
& <WinDivertTool.exe> handles
# trace check on the new structured fields
Select-String -Path gui-trace.log -Pattern 'event=(app_start|interceptor_state|windivert_open|control_registration|ui_error)'
```

The acceptance matrix in §4 is the contract for the next fix; the training brief
in `docs/training/stability-verification-training.md` explains how to execute and
evidence it without repeating the gaps found here.
