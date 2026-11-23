# Stability verification training — Windows interception/tunnel lifecycle

**Audience:** in-house QA engineers and AI agents who verify LightSpeed's
WinDivert/interceptor/control-plane lifecycle work.
**Companion audit:** [`docs/audit/stability-verification-2026-11-23.md`](../audit/stability-verification-2026-11-23.md).
**Date:** 2026-11-23.

**Outcome after this brief you should be able to:** read a lifecycle trace and
separate fact from inference; explain what a green matrix does and does not
prove; design and evidence a lifecycle acceptance row that is stronger than "no
error"; and refuse to sign a release claim built on a differently-built binary.

---

## 1. Why this training exists

On 2026-11-23 the pinned-driver QA matrix reported **84/84 PASS** on build
`84e15c3`, and the same day the owner's own machine logged a real failure while
boosting a live game: `FWP_E_IN_USE` on a restart after a Stop, two registrations
that succeeded while interception failed, and a red "WinDivert has a stale WFP
filter … restart Windows" error. Both facts are true. The failure was not "QA is
wrong" — it was **the matrix and the field measuring different states**.

The bug class here is not a wrong line of code; it is a **verification gap**:
a clean-boot fixture that cannot represent the state a real user reaches. This
brief trains the reflexes that catch such gaps before they become a release note.

---

## 2. Vocabulary: what a "pass" means

| Claim | Means | Does **not** mean |
|---|---|---|
| "Row PASS" | The harness's stated check for that row held in that run | The scenario is risk-free or representative |
| "84/84 PASS" | 84 assertions the harness defined held | 84 different risks were covered |
| "Matrix green" | In a clean-boot, clean-desktop, synthetic-fixture environment, the defined rows held | The shipped binary was tested; the product behaves on a dirty machine |
| "Verified" | Someone else can re-run the check and get the same result from the recorded evidence | The author believes it |
| "The trace shows X" | A line with a timestamp says X | Why X happened, or which code path produced it |

**Rule 1 — never upgrade a claim.** "The matrix passes" does not become "the
lifecycle is stable". The correct sentence names the environment, the binary and
the scenario set.

---

## 3. Lesson 1 — read the trace like an investigator

### 3.1 Facts vs inference, always separated

For every minute of a trace, fill two lists:

- **FACTS:** line number, timestamp, verbatim text, and what it mechanically
  implies (e.g. "`handle close id=2 -> ok` = the close call returned success").
- **INFERENCE:** the mechanism that fits (e.g. "the driver released its WFP
  objects late"). Write "inference" in the note; never cite an inference as
  evidence later.

Worked example from the 06:49 window:

| Fact (line) | Inference (never cite as fact) |
|---|---|
| 06:49:38.561 `diverting handles closed; driver pin active=true` (`:1349`) | The close returned OK; WFP objects are therefore fully released (NOT proven) |
| 06:49:39.841 `intercept … id=5 … FAILED (os error -2144206838)` (`:1363`) | A stale filter from another process kept a reference (possible, unproven) |
| 06:49:40.275 `Registered with proxy: session=2861874939` (`:1368`) | "The UI showed Registration OK" (the UI is not logged) |
| no `handle open retry` line after the failure | The retry policy is the committed `!pin_active` one (likely; still check the build) |
| start sequence begins with no presence/command line | Auto-start (unproven; a user click fits equally) |

### 3.2 Three traps that fooled this evidence set

1. **Double-encoded logs.** The provided slice stores emoji as mojibake
   (`ðŸŽ¯` = 🎯). ASCII facts (timestamps, error codes, "FAILED") are unaffected;
   don't "fix" the file, note the artefact.
2. **A shared log across builds.** One `gui-trace.log` can contain several
   binaries. Compare log lines that should be impossible for a given commit
   (e.g. `handle open retry … pin_active=true` while the committed code refuses
   to retry with a pin held) to detect it.
3. **Frozen "healthy" lines after a failure.** `Latency comparison` and
   `Auto-detected game` keep repeating with identical values for 31 s after the
   failed restart. Repetition is not liveness; check that counters/ids move.

### 3.3 The read-through checklist

- [ ] Is there a build stamp? If not, write `build identity: U` and do not
      attribute behaviour to a release.
- [ ] Do handle ids pair (every open id has one close id)? Is there exactly one
      pin per process run?
- [ ] Does the error code match the message? (`-2144206838` = `0x8032000A` =
      `FWP_E_IN_USE`.)
- [ ] Did registration and interception agree with each other?
- [ ] Is the failing state *after* a state transition (Stop, server change,
      relay change, presence change)? Look 60 s before the failure, not only at
      the error line.
- [ ] Which lines would be different if the app had *not* been broken? If none,
      the "evidence" is worthless.

---

## 4. Lesson 2 — harness literacy

Before trusting a matrix, read the harness. Four questions always:

1. **What does it refuse to run on?** `Invoke-Preflight` refuses a RUNNING
   WinDivert service, non-zero service exit code, any WFP provider/sublayer/
   filter/callout, any WinDivert handle, and any game/lightspeed process. That is
   good measurement hygiene — and it means **dirty-kernel states are never a
   row**. If the product can leave such a state, the harness is blind to exactly
   the bug class.
2. **What is the fixture topology?** Here: one fixed synthetic server
   (`8.8.4.4:40000`), `/32` blackhole route, two loopback relays, a renamed
   PowerShell game double. Consequences: **no server transition**, **no return
   traffic**, **no real relay fleet**.
3. **What exactly is asserted?** `Test-CycleInvariants` asserts absence of FWP
   text/open-FAILED/retries/pin changes, presence of close/open lines, quiet-window
   delta 0 and a judged line. It never asserts UI state, registration ledgers, or
   the *both directions* of traffic.
4. **How fast is the action relative to the field?** The cycle waits 700 ms after
   Stop plus a 2,000 ms quiet-window settle before clicking Optimize
   (≥2.7 s). The field failure came 1.27 s after teardown. Timing is a variable,
   not a constant.

**Rule 2 — a fixture that cannot produce the failing state cannot clear it.**
Ask "what state does this fixture make impossible?" and write it down as a U-row.

---

## 5. Lesson 3 — acceptance criteria beyond "no error"

A lifecycle row passes only when **all** of these hold:

1. **Identity:** build/commit and driver DLL/SYS hashes recorded in the run and
   logged at app start; the tested binary equals the candidate release artifact.
2. **Two-way traffic:** ≥2 consecutive judged windows after the transition with
   `injected > 0` and `share ≥ 1/8`, plus relay counter movement. Outbound-only
   evidence must be labelled outbound-only.
3. **Ledgers:** pin opened exactly once per app run; every intercept/inject/
   shadow open id closed exactly once; each registration has a session and is
   either closed or superseded, with ≤1 active session.
4. **Kernel state:** WFP filter/callout counts 0 at idle; a snapshot at each
   phase boundary; no row requires a reboot to pass.
5. **UI truth:** the visible primary state matches the engine facts
   (semantic dump + screenshot). "Connected" plus a failed boost is a FAIL;
   "restart Windows" as the only remedy for a self-induced state is a FAIL.
6. **Error lifecycle:** a failure card is shown for the failure, cleared within
   1 s of a same-class success, and unrelated errors are not silently cleared.
7. **Recovery bound:** after a force-kill or dirty start, the next start either
   succeeds without a reboot or reports a truthful, bounded, automated remedy.
8. **Independence:** someone other than the author re-runs the row set on the
   final tree and signs it.

Use the minimum matrix in the audit (§4, rows A1–A12) as the template.

---

## 6. Lesson 4 — the stale-error / stale-state trap

There are three different "stale" things and they are easy to conflate:

| Stale thing | What it is | How to verify |
|---|---|---|
| Stale kernel/WFP state | WFP provider/sublayer/filter from a previous process; a reboot or service stop clears it | pre-state WFP/handle snapshot; reproduce the dirty state deliberately |
| Stale UI error | A red card whose condition has recovered | `ui_error` lifecycle event; assert clear-on-success; check that `finish()` clears `last_error` |
| Stale registration | A session/token superseded by a newer registration | registration ledger with supersede/close events |

In this codebase today: retry policy refuses to retry with the pin held; the
error card's generic success path calls `finish()` which does **not** clear
`last_error`; `clear_error()` has no production call site; the 1.12.5 fix clears
only `control-plane registration failed` details. Treat all three as
test-required, not documentation.

---

## 7. Lesson 5 — writing the evidence note

Template (copy this):

```markdown
# <task> — <claim>
Status: PASS | FAIL | U (unverified) | BLOCKED
Build: <exe SHA256> = <commit>; driver DLL/SYS <sha256>; harness <sha256>
Environment: <host/boot/clean-kernel gate result>
Rows: <row id> exercised / total; <n> PASS, <n> FAIL, <n> U/BLOCKED
Facts: <timestamped lines or measurements>
Inferences: <explicitly labelled, with what would confirm them>
Not covered / U: <scenarios, with why>
Evidence index: <paths + hashes>
```

Rules:

- Never write "PASS" for a row you did not run; write `U` and why.
- A summary written by another agent is not evidence; a command transcript, test
  output with counts, a screenshot of the real window, or a raw log line is.
- No secrets, no payloads, no process memory in evidence.
- Name the binary and the environment in the first five lines.

---

## 8. Lesson 6 — exercises (use the 2026-11-23 evidence)

1. **Fact/inference.** From `user-server-transition-trace.log` lines 1340–1386,
   list five facts and three inferences. Which of the inferences can be answered
   by a log field, and which needs a rerun?
   *Model answer:* facts — Stop 06:49:38.417, handles `id=2/3/4` closed OK,
   `driver pin active=true`, `id=5` FAILED with `-2144206838` at 06:49:39.841,
   `Registered… session=2861874939` at 06:49:40.275. Inferences — actor of the
   restart, cause of `FWP_E_IN_USE`, what the UI showed.
2. **Misleading row.** Which of the 84 rows is weakest as named, and what would
   you rename it to? *Model answer:* `traffic-bearing proof (judged window)` —
   in every saved QA trace the same window is `injected=0 from_relay=0
   share=0.0% verdict=no_reply_from_relay`; rename to "outbound interception and
   relay-forward proof".
3. **Fixture gap.** Name two product paths the fixture cannot reach and the
   fixture change that would reach them. *Model answer:* game-server transition /
   stale re-detection (needs ≥2 reachable endpoints that reply) and dirty-kernel
   recovery (needs a start from a deliberately product-created kernel state
   instead of a preflight refusal).
4. **Build mismatch.** The matrix finished at 12:09 on `84e15c3`; release HEAD
   `01438e0` was committed at 12:27. What claim is invalid? *Model answer:* any
   claim that the 1.12.5 release binary's error fix is real-driver verified, and
   any "no stale error in 1.12.5" statement.
5. **Fast restart.** Design the row for "Optimize 250 ms after Stop". What
   timestamps do you record, and what is a pass? *Model answer:* Stop command
   time, last handle-close time, start command time, intercept open result and
   duration; pass = every repetition behaves the same, handles pair, 2 healthy
   windows after, or a documented deterministic lockout with a truthful remedy.
6. **Dirty-kernel inversion.** Write the row that starts from driver loaded,
   WFP `1/20`, zero handles. Why must it be a row and not a preflight failure?
   *Model answer:* because it is the state the product itself leaves; if the
   product cannot recover from its own residue, users are one bad restart away
   from "restart Windows".

---

## 9. Escalation and claims policy

- **U is a first-class result.** Mark unverified work U, with what would verify
  it. "U because the fixture has no return path" is better than a fake PASS.
- **Never sign a claim on an unverified artifact.** If the tested binary's
  SHA/commit is not in the run record and not equal to the shipped artifact, the
  matrix signs a commit, not a release.
- **One author cannot be the only verifier.** The fix author writes the
  reproduction; QA re-runs it on a different worktree/binary and signs.
- **"Reboot required" is a defect signal.** If the only remedy is a reboot or a
  service stop the product does not perform itself, open a bug and add the
  recovery row; do not document it as expected unless the owner accepts it
  explicitly in writing.
- **A green matrix never deletes a field trace.** File it as a regression trace
  in the evidence corpus with the open questions it raised.

---

## 10. Continuous stability roadmap (QA view)

| Horizon | Practice |
|---|---|
| Every fix | Failing-before from the field shape → fix → mutation check (revert fails, restore passes) → two-run independent matrix on the exact release binary |
| Weekly | One 60-min soak with ≥10 transitions and periodic WFP/handle snapshots; one dirty-kernel recovery run; one force-kill-at-transition run |
| Monthly | Re-read the design assumptions in code against new evidence (retry policy, remedy text, error lifecycle); review flake/quarantine list; verify evidence completeness (screenshots, ledgers, build stamps) |
| Quarterly | Re-run the full minimum matrix on the supported Windows/WinDivert matrix; refresh the regression trace corpus; review stability SLOs with the Lead |
| Always | Structured lifecycle events (`event=…`, ids, reason) as the first-class evidence, not prose parsing; support bundle on error; no payload logging |

---

## 11. Red-flag checklist (pin this next to your desk)

- [ ] Matrix ran on a different commit than the release HEAD.
- [ ] Preflight forbids the exact state the product leaves behind.
- [ ] Fixture has one fixed server (no transitions) or no return traffic.
- [ ] Row name says "traffic" but only outbound counters moved.
- [ ] Cycle waits longer between Stop and start than the field does.
- [ ] No UI assertion; error/registration cards are not checked.
- [ ] No build/version stamp in the trace.
- [ ] Retry/remedy text tells the user to reboot a state the app created.
- [ ] A success path does not clear the error it fixed.
- [ ] A pass depends on a kernel state only a reboot can produce.

If any box is ticked, the claim is **U**, not PASS.
