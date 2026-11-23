# LightSpeed 1.12.5 — a pinned WinDivert driver and a true Stop

**Status: release candidate, 2026-11-23.** A Windows interception-lifecycle
change: one non-diverting WinDivert handle is pinned for the app lifetime, so a
normal Stop no longer unloads and reloads the driver. No protocol, wire, config
or CLI change. Tagging and publishing remain the owner's gate.

## Why

The 1.12.x line closed a series of stale-WinDivert failures from the close side
(synchronous handle close, relay switches without a restart, fail-closed
teardown). The failure that started that line — after one interception session
the next start could see an orphaned WinDivert WFP provider even though no
process held a handle — was never reproduced in a controlled environment.

This release changes the shape of the problem instead of adding another close
path. A normal Stop closes the intercept, inject and shadow handles; with no
live handle, nothing keeps the WinDivert driver/service loaded for the next
start, and the load/unload churn path that this line of releases was chasing
lives there. 1.12.5 keeps one permanent **non-diverting** handle open (WinDivert
filter `"false"`, normal flags, Network layer, priority 0) for the whole app
lifetime. A live WinDivert handle keeps the driver/service loaded, so a Stop no
longer releases it and Optimize only reopens the diverting handles. The matrix
does not try to prove the old build actually unloaded the driver; it verifies
the new lifecycle, and the pin removes the churn path by construction.

## The design

- **One pin, for the app lifetime.** `pin_driver()` opens exactly one
  non-diverting handle before the first diverting handle. It is idempotent: a
  second call while a pin is held opens no second handle. The pin carries no
  filter that diverts traffic and owns no tunnel.
- **A true Stop.** Stop closes the intercept, inject and shadow handles through
  their normal teardown, cancels the start-of-match hold watcher, and clears the
  per-run state (hold, relay choice, locked server). Nothing keeps intercepting
  or tunnelling after a Stop: a stopped engine has no interceptor handle at all,
  and the relay-forwarding delta stays zero in the quiet window after every
  Stop.
- **Optimize reuses the pin.** A later Optimize opens only the diverting
  handles; the pin is not reopened, released or duplicated.
- **The pin ends only with the app.** Quit / explicit app shutdown runs the true
  Stop first and then releases the pin (after handle teardown, never before);
  dropping the engine and process exit release it too. A user Stop deliberately
  keeps it.
- **Bounded retry, no retry loop.** If an open fails with `FWP_E_IN_USE` and
  **no** pin is held, it is retried a small fixed number of times with a bounded
  backoff — the driver may still be winding down from a previous process. With
  the pin held, the stale state is not a cold start, so the open fails
  immediately with the actionable remedy.
- **Instrumentation.** Every logical WinDivert open is logged with a monotonic
  per-process id and its role (`pin`, `intercept`, `inject`, `shadow`,
  `legacy`), layer, priority, flags (`0x0000` normal, `0x0001` sniff, `0x0008`
  send-only) and filter; the matching close line uses the same id. Per-kind
  counters record open attempts, opens, closes, failures and live handles. Only
  handle metadata is logged — never packet payloads or process data.
- **Windows only; a no-op elsewhere.** On builds without the
  `windivert-redirect` feature the pin functions are no-ops, so the engine and
  the shared app core can call them unconditionally. Linux and macOS
  interception paths are unchanged.

This is a *pin*, not a pause gate. It does not keep traffic intercepted and it
does not replace the per-session handles: a Stop still releases every filter
that diverts or injects.

## UI: a recovered registration clears its error card

A command that failed while the control plane was still coming up could leave
the red Registration failed card on screen after the background retry had
succeeded. The card is now cleared when the engine reports the control plane
registered; unrelated command errors are left untouched.

## Updater: stop the driver before replacing its files

WinDivert 2.2 keeps its driver loaded after the last handle closes (the DLL
marks the service for deletion but never stops it), so `WinDivert64.sys` stays
locked after a Quit. The previous self-update script copied the new
`WinDivert*.dll`/`.sys` over the installed files and could abort on that lock,
leaving the app closed and not updated.

- The GUI refuses to start an in-app update while the driver pin is held, and
  runs a full engine shutdown (true Stop + pin release) before the elevated swap
  script.
- The swap script stops the `WinDivert` service and waits up to 10 s for it to
  stop **before** copying the runtime driver files, then copies the GUI. A
  `finally` block relaunches the installed `lightspeed-gui.exe` when it is
  present, so a runtime-file copy failure cannot leave the app closed.

## Verification

Real WinDivert driver, clean boot, real loopback relays and a synthetic game
client with real traffic. Build: branch `fix/pinned-driver-1.12.5`, HEAD
`84e15c3` (the updater fix), GUI SHA256
`F8ECE5824D674B186D2DB6C2D91703858F597B37B8A7FA3F317B4A6936EA8DFC`.

**84/84 matrix rows PASS, 0 FAIL, 0 BLOCKED, 0 SKIP.**
Evidence:
`wat/evidence/2026-11-23/task-107-matrix-84e15c3v2-20261123-115633/`;
report: `wat/evidence/2026-11-23/task-107-release-report.md` (round 2).

| Matrix area | Result |
|---|---|
| one pin opened on the first app run; traffic-bearing judged window | PASS |
| same-scope Stop → Optimize traffic cycles | **30/30** |
| Quit → relaunch at varied delays | **20/20** |
| scope/game/profile changes | **10/10** |
| game restarts | **5/5** |
| relay change while running (A → B → A) | PASS |
| TCP-profile Stop | PASS |
| Stop during the start-of-match hold | PASS |
| force-kill while active / while idle | PASS |
| self-update path (locked `.sys`, service stop, relaunch) | PASS |

Across the run: **0** `FWP_E_IN_USE`, **0** stale-WFP lines, **0** failed handle
opens and **0** open retries. Every Stop→Optimize cycle carried intercepted
traffic (7,072–9,071 packets in the judged window), opened a new
intercept/inject/shadow id set that was closed by id on the next Stop, never
reopened or released the pin, and showed a zero relay-forwarding delta in the
2 s quiet window after each Stop. Each of the 20 Quit/relaunch runs opened
exactly one pin, released it after the last diverting close, and relaunched into
traffic; no quit left a diverting filter, callout or handle behind.

## Accepted evidence gap: the old fault was not reproduced

The gate asks for a failing-before control on the old build. On a clean boot the
1.12.3 build (tag `v1.12.3`, commit `37ff8d0`) ran the same 30-cycle traffic
loop and reported **30/30 cycles, 0 failure hits**; the control run recorded
5 PASS rows and **1 BLOCKED** row:
`wat/evidence/2026-11-23/task-107-control-1123-20261123-120914/`.

The historic failure needed conditions this driven loop does not produce
(forced-kill/unload-reload races and the original user sequence), so the lab
**did not reproduce the control**. The owner explicitly accepted this evidence
gap on 2026-11-23.

Therefore this release does **not** claim the pin "fixes the original fault".
What is established is narrower and checkable: the pin keeps the driver loaded
across Stop, so the unload/reload churn path is removed by construction, and a
traffic-bearing real-driver matrix exercising the lifecycle shows no
`FWP_E_IN_USE` and no failed opens.

## Upgrade

- **In-app:** update from the update sheet as usual. The new updater stops the
  WinDivert service before replacing driver files and always restarts LightSpeed.
  A running boost is shut down first; the install refuses rather than killing a
  live filter.
- **Manual/MSI/package installs:** close LightSpeed, then either reboot or run
  `sc.exe stop WinDivert` as administrator before replacing `WinDivert64.sys` or
  `WinDivert.dll`; after any LightSpeed run the driver stays loaded until the
  service is stopped or the machine restarts.
- LightSpeed needs to run **as administrator** for interception, as before.
- No config migration: the 1.12.4 config file and relay pin store are unchanged,
  and a clean 1.12.4 install can be replaced in place.

## Rollback

If anything regresses, reinstall the previous 1.12.4 build. There is no kernel
state to migrate back. A machine that has run any 1.12.x build keeps the
WinDivert driver loaded after Quit until a reboot or `sc.exe stop WinDivert`, so
the reinstaller must stop the service (or reboot) before changing driver files.
Rolling back does not recreate anything beyond 1.12.4's own unload-on-Stop
behaviour.

## Remaining risks and known limitations

- **The original field fault is still unreproduced.** The accepted control gap
  above is this release's main evidence limit.
- **The driver stays loaded after Quit.** With WinDivert 2.2 the WFP
  provider/sublayer objects remain (1 / 20) and `WinDivert64.sys` stays locked
  until the service is stopped or the machine restarts. This is unchanged by
  design, is not a boot-zero guarantee, and the 1.12.5 updater is built around
  it.
- **Matrix scope.** One Windows 11 host, one clean boot, synthetic game clients
  and loopback relays. A live match on a real game server, other Windows/driver
  versions, and third-party EDR/firewall interference are not covered.
- **Windows only.** The pin is a Windows/WinDivert mechanism; the Linux/macOS
  interception paths are unchanged and not part of this verification.
- **Unclean teardown still latches.** A teardown that cannot be acknowledged
  fails the Stop closed and requires an app restart; the pin does not remove
  that rule.
- **Update-from-published-asset path.** The matrix validated the swap-script
  contract and the service-stop/relaunch steps on the real driver, but no
  published 1.12.5 asset exists yet, so a full in-app update from a GitHub
  release was not exercised end to end.

## Release-record correction (2026-11-23)

Reading the 1.12.3/1.12.4 evidence for this release exposed record problems,
corrected in the living documentation without rewriting history:

- `v1.12.3` and `v1.12.4` were tagged while their in-tree release notes still
  said "release candidate"; the release body (the CHANGELOG section) did not
  carry that status, so the published record read as finished.
- The 1.12.4 round's real-driver evidence was four rapid start/stop cycles, and
  those cycles carried **zero intercepted and zero tunnelled packets**. The
  1.12.4 real-driver claim was therefore not established.
- The 1.12.3 failing-before control never reproduced (30/30 cycles, 0 failure
  hits, 1 BLOCKED row), and the attribution of the orphaned provider to the
  1.12.3 probe handle was never proven.

The tags and their release bodies are not rewritten. The dated corrections are
in `docs/release-notes-1.12.3.md`, `docs/release-notes-1.12.4.md`, the
corresponding CHANGELOG notes and `wat/state/decisions/2026-11.md`.
