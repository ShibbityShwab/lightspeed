# WinDivert lifecycle — in-house training brief

Audience: LightSpeed agents (engine, GUI, QA) who touch, test or debug the
Windows interception lifecycle. Read with the audit
`docs/audit/windivert-lifecycle-2026-11-23.md`; this is the working brief, not a
substitute for evidence.

Hard context: the product bundles **official WinDivert 2.2.2** (dual
LGPLv3/GPLv2). The kernel driver is third-party, signed, and shared with any
other WinDivert app on the machine. We do not patch it; its teardown error
reporting is not visible to us.

---

## 1. Ten rules

1. **A handle is not a WFP filter.** The pin and the inject handles use filter
   `"false"`: they install **no** callout and **no** filter. Only the intercept
   (`udp and outbound …`) and shadow (`udp and inbound …`) handles own WFP
   objects. Never argue about WFP state from a "false" handle.
2. **`CloseHandle` success ≠ WFP objects removed.** `WinDivertClose` is
   `CloseHandle`; the driver deletes filters/callouts in its own destroy
   callback, reports failures only to a debug log compiled out of the release
   driver, and returns nothing. "close -> ok" in the trace proves the handle
   closed, nothing else.
3. **The driver does not unload on the last close.** WinDivert marks its kernel
   service for deletion while it runs; provider + 20 sublayers stay registered
   until service stop or reboot. A persistent pin is not what keeps the driver
   loaded — do not build a theory that it is.
4. **`FWP_E_IN_USE (0x8032000A)` = a WFP object-lifetime conflict**, including
   "callout already being registered/unregistered in another thread". It is
   **not** a synonym for "a previous LightSpeed run left a filter". The
   product's remedy sentence is a fixed string, not a diagnosis.
5. **One open attempt is not a diagnosis either.** `WinDivertOpen` =
   CreateFile + INITIALIZE + STARTUP; the single errno does not say which step
   or which WFP call failed. Capture WFP state, don't guess.
6. **Never suppress retries because a pin is held.** The current code does
   exactly that; it turns a sub-second transient into "restart Windows". Any
   change here must keep a *bounded* retry (attempts + wall-clock), never an
   unbounded loop.
7. **Long sessions are the untested regime.** The QA matrix cycles were
   ≈12 s; the field failure followed a 364 s traffic session. Any WinDivert
   change needs at least one ≥10-minute traffic-bearing Stop→start run with WFP
   capture.
8. **Timing evidence beats timing intuition.** Always compute the close→open
   gap from timestamps. Measured in the field trace: successful cycles 0.567–
   0.577 s, failure 1.279 s, second failure 4.128 s. "Too fast" is not
   supported; state your gap.
9. **A Stop is a user/product decision, not a server event.** In the 2026-11-23
   trace, two server re-detects changed no handle and no pin. Do not attribute a
   Stop to a region/settlement change without a log line naming the action.
10. **No fix claim without a failing-before and an after run on the real
    driver.** A mock pin seam, a unit test or a 84-row matrix that never
    combines a long session with Stop→start is not evidence for this failure
    class.

---

## 2. Lifetime model (memorise the layers)

```
process
 └─ WinDivert device handle (file object)        ← the pin lives here
     └─ per-handle dynamic WFP engine session    ← created at CreateFile, closed in destroy
         └─ WFP callout object + filter          ← created at IOCTL_STARTUP (intercept/shadow only)
             └─ runtime callout registration     ← FwpsCalloutRegister0, per handle, per layer
driver (WinDivert.sys)
 └─ provider + 20 sublayers (DriverEntry)        ← shared, persist until service stop/reboot
```

A pin raises one handle reference in the top layer. The failure lives in the
per-handle session/callout layer and in the shared driver layer. **A handle in
layer 1 cannot lock or await teardown in layer 2/3.** That is the whole reason a
held pin does not guarantee the next open.

---

## 3. Log-line glossary (what each line proves)

| Line | Proves | Does not prove |
|---|---|---|
| `WinDivert handle open: id=N kind=… layer=Network priority=0 flags=0x0000 filter="…" -> ok` | that handle id N is open | that WFP filter was installed (check `filter="false"`) |
| `-> FAILED (… os error -2144206838)` | `WinDivertOpen` failed with `FWP_E_IN_USE`; the step is unknown | a stale object from another run; which call failed |
| `WinDivert handle open retry: … pin_active=… backoff_ms=…` | retry policy and the pin state at decision time | that a retry will help |
| `WinDivert handle close: id=N kind=… -> ok` | `CloseHandle` returned TRUE | WFP filter/callout deletion |
| `WinDivert diverting handles closed; driver pin active=true` | all three diverting handles were closed and the pin slot is live | WFP is clean; next open will succeed |
| `📌 WinDivert driver pinned … the driver stays loaded` | a `"false"` handle is open | WFP state ownership |
| `🛑 Shutdown signal received` | the interceptor shutdown thread got the stop signal | who asked for the Stop or why |
| `Relay reply window judged …` / `return path is starving` | tunnel data-plane state | any WinDivert state |

Never quote an app-composed remedy sentence as a finding.

---

## 4. Triage playbook

**S1 — "Open failed with 0x8032000A"**

1. Get the exact line(s): id, kind, attempt, `pin_active`, timestamp.
2. Compute the gap from the last `handle close` of that kind.
3. Determine whether any other WinDivert client exists (WinDivertTool handle
   scan; `tasklist /m WinDivert.dll`).
4. Capture WFP state *now*: `netsh wfp show state file=…` plus the
   filters/callouts/providers views the host's `netsh wfp show` supports.
5. Only then classify: leftover object (H2/H3) vs registration race (H1) vs
   unknown. Do **not** write "stale filter" as a conclusion.
6. Check whether the build SHA in the trace matches the build under test. If the
   trace has `pin_active=true` retries it is not v1.12.5 (see audit F7).

**S2 — "Start fails only after a Stop"**

- Record: session duration, traffic volume, close→open gap, retry count,
  WFP callout/filter counts after close (should be 0 for the closed handles;
  provider/sublayers may remain).
- Reproduce with T2/T3 in the audit matrix; a retry/pin-cycle experiment (T8/T9)
  decides transient vs persistent.

**S3 — "Interception is active but the UI shows an error"**

- Out of WinDivert scope; capture the control-plane/registration state and hand
  to the task that owns the GUI state machine. Do not conflate it with FWP.

**S4 — "Errors after region/server change"**

- First check the trace: did handles or the pin change? In 1.12.5 relay changes
  are data-plane only; if `handles_unchanged=True` the WinDivert lifecycle is
  not implicated.
- Then check server lock re-detect / registration / tunnel state.

**S5 — "After a crash/force-kill the next start fails"**

- Capture whether a WinDivert client handle is orphaned
  (`tasklist /m WinDivert.dll`, WinDivertTool) and WFP objects remain.
- Recovery ladder: graceful Quit and wait → app restart → service reset
  (`sc stop WinDivert`, only with the shared-driver warning) → reboot. Record
  which step worked.

---

## 5. Evidence to collect (read-only unless stated)

Minimum bundle for any WinDivert lifecycle bug:

1. Full GUI trace (`%LOCALAPPDATA%\Lightspeed\gui-trace.log`) — plus a copy of
   the slice around the event.
2. Build identity: GUI SHA256 / commit (log it with the report).
3. Handle lifecycle lines (open/close/retry/pin) for the whole process.
4. WFP state before start, after start+traffic, before Stop, 10 ms/100 ms/1 s/5 s
   after close, before/after the retry.
5. Driver/service state: `Get-Service WinDivert`, `sc.exe qc WinDivert`,
   `Get-CimInstance Win32_SystemDriver -Filter "Name='WinDivert'"`.
6. WinDivert load/unload events: `wevtutil qe System
   /q:"*[System[Provider[@Name='WinDivert']]]" /f:text /c:20`.
7. Handle scan (WinDivertTool 2.2) while idle, and any other client found.
8. Exact wall-clock timestamps and timezone; the trace is UTC, hosts are often
   UTC+7.
9. What the user/harness clicked (if reproducible) — the GUI does not log it
   today; note this gap.

Redaction: no packet payloads, no process command lines, no keys. Handle ids,
filters and WFP GUIDs are fine.

Never do during an audit: start/stop the GUI, open a WinDivert handle, run
`sc stop WinDivert`, or edit app state, unless the task explicitly authorises it
and the owner gate is satisfied.

---

## 6. Anti-patterns (all observed or implied in prior work)

| Anti-pattern | Why it fails | Do instead |
|---|---|---|
| "The pin guarantees the next open" | pin owns no WFP object (rule 1) | measure WFP state |
| "Close -> ok, therefore WFP clean" | driver teardown is void/debug-only (rule 2) | capture WFP after close |
| "The pin removes driver unload churn" | driver does not unload on last close (rule 3) | state the real lifetime |
| "FWP_E_IN_USE means a previous run left a filter" | it is a family of lifetime conflicts (rule 4) | classify with WFP capture |
| "With a pin, don't retry" | converts transients to hard failures (rule 6) | bounded retry + pin-cycle |
| "Reboot to clear it" as first-line advice | loses evidence, hides cause, user-hostile | capture, retry, then service reset |
| "The server switch tore down the interceptor" | trace shows it did not (rule 9) | cite the Stop action |
| 12 s QA cycles prove lifecycle stability | field failure needed 364 s (rule 7) | long-session row |
| "84/84 rows pass" as release proof | matrix has structural gaps; update row stops the service itself | scope the claim to what ran |
| Patching WinDivert to add teardown ack | GPLv2 + signature + signing cost | owner gate; prefer recovery/design options |

---

## 7. When may we restart/Stop capture?

- **Legal:** explicit user Stop/Optimize; game exit (presence transition);
  Quit; elevation restart; self-update when no pin is held; scope/game change
  requiring a new capture filter.
- **Not legal as a reflex:** server/region change, relay change, latency fault,
  one failed packet sample, a UI error card. These are in-place state changes
  (relay changes already are).
- **Never** auto-restart on a timer or on a failed start without a bounded
  policy and a visible reason.
- If a Stop is required, the next start must be allowed a bounded retry and must
  not depend on a reboot.

---

## 8. Code-review checklist for any WinDivert handle change

- [ ] Does it change handle open/close ordering, ids, or retry policy?
- [ ] Does it claim a WFP guarantee? If so, is WFP state verified (not just
      close success)?
- [ ] Are failures classified (create vs initialize vs startup vs WFP call)
      without inventing a cause?
- [ ] Is the retry bounded by both attempts and wall clock, and does it cover
      the pinned path?
- [ ] Is there a recovery path other than reboot, with a user-visible
      consequence warning?
- [ ] Does the log carry build identity and the reason for the action?
- [ ] Is there a test for long traffic session → Stop → immediate start
      (T2/T13), not just short cycles?
- [ ] Are docs updated to the real lifetime (no "callout per handle", no
      "pin protects WFP")?
- [ ] Does the change affect the self-update path (pin held → sys file locked)?

---

## 9. Quick severity guide (QA)

| Symptom | Severity | First action |
|---|---|---|
| Capture cannot restart without reboot/Quit after a normal Stop | Critical | capture bundle + WFP state; no code claim |
| Start fails after a crash/force-kill | High | check orphan handles; recovery ladder |
| Interception active but UI error card | Medium (GUI/control-plane) | hand to the state-machine owner |
| Relay/region change breaks latency but capture stays | Medium (data plane) | not a WinDivert bug |
| Pin/update conflict at Quit | Low | check `self_update.rs` gating |

---

## 10. Questions that must not be answered by intuition

- Which call returned `0x8032000A`? (unknown; needs debug driver/ETW)
- Does retry or pin-cycle clear it? (needs T8/T9)
- Is the trigger session length, traffic, starvation, or close sequence? (needs
  T2–T4)
- Is the pin removable without loss? (needs T7)
- Was the field Stop automatic? (the GUI does not log it; the auto-start latch
  says no)

Keep these open until measured. An honest "unknown" is the correct expert answer
here, and it is more useful than another close-path patch.
