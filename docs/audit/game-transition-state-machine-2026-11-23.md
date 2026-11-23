# Game-server transitions, reconnection and the interceptor lifecycle — audit and transition-safe state machine

**Status:** read-only audit. No production code was changed; nothing was fixed, released or deployed. This document specifies behavior, not a shipped change.

- **Date:** 2026-11-23
- **Source tree:** `master` `6e30a65` (merge of `fix/pinned-driver-1.12.5`, the 1.12.5 pinned-WinDivert/stale-error release)
- **Trace provenance:** the trace carries no build/version line, and it is not attributable to a single binary: the 06:01/06:03 `WinDivert handle open retry ... pin_active=true` lines cannot be produced by any committed revision of `should_retry_fwp_in_use` (`client/src/interceptor/windivert_handle.rs:320-329`). Only the elevated session from **06:42:38 to 06:50:15** is used for 1.12.5 behavior claims, and its line shapes match the committed policy. Commit attribution throughout this document is by line-shape inference, not by build identity; independent confirmation is in the sibling audits `docs/audit/stability-verification-2026-11-23.md` (§6 build provenance) and `docs/audit/windivert-lifecycle-2026-11-23.md`.
- **Related audits (same evidence set, different scope):** `docs/audit/stability-verification-2026-11-23.md` covers the QA matrix vs. field failure; `docs/audit/windivert-lifecycle-2026-11-23.md` covers the pin/Stop/`FWP_E_IN_USE` lifecycle in depth. This document owns the game-transition, reconnection, presence and presentation state machine; where scopes overlap it defers to those documents for the WFP/pin detail and to this one for the transition state machine.
- **Subject:** a ROB RUN (Roblox-title, Easy Anti-Cheat) session where the player switched settlements/POIs (Lennys, Looty Bay). The trace shows "Locked server stale - re-detecting", destination-aware relay choices that end with "Keeping the live session...", a Stop followed ~1.4 s later by start attempts that fail with `FWP_E_IN_USE`, and a stale "Registration failed / control-plane registration failed" card while the UI was actively relaying.
- **Evidence:**
  - `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log` (1400 lines, 225,050 bytes; SHA-256 `5DFFF05824AE65FC20D4D7281C454E1DC5E52B14CA0B4477DEF562882B317331`)
  - `%LOCALAPPDATA%\Lightspeed\gui-trace.log` (106,658 lines, 14,776,101 bytes; SHA-256 `F48EE174A16ACCA4EEA2AF2B2FBF5140DD5B5C1A8108BD72D7BB6A0A59757F1B`)
  - Extracted timeline: `wat/evidence/2026-11-23/stability-audit/game-transition-key-events.log`; audit notes: `wat/evidence/2026-11-23/stability-audit/game-transition-audit-notes.md`
- **Time convention:** the GUI log writes UTC (`Z`). Host zone is UTC+07:00, so 06:49:38 log time is 13:49:38 local. All times in this document are **log UTC**; a trace line is cited as `trace:<line>`.
- **Confidence labels:** **[C]** confirmed by code and trace; **[P]** probable (code explains the trace, one step not directly observable); **[H]** hypothesis / unknown, needs instrumentation.

---

## 1. Executive summary

The observed "server transition tears down the interceptor and then a restart fails" is **not** one causal chain. The trace separates into four different events with three different owners:

1. **Server/region transition (06:47:59 and 06:48:34) did *not* stop the interceptor.** The re-detect path is intra-run: `ServerTracker` releases a stale lock and re-locks the new server (`trace:1239, 1245-1253, 1278-1288`). No diverting handle was closed at either transition — the next close is 64 s later. The transition's real effects were: (a) a control-plane re-registration (supervisor replacement) with a new destination, (b) a relay choice that was *not applied* ("Keeping the live session ... applies next session"), and (c) a stale registration error set on a superseded supervisor generation.

2. **The Stop at 06:49:38 was a separate user/action-driven Stop**, ~64 s after the last transition and after the starvation warning had been raised three times across five judged windows (`trace:1266, 1295, 1307-1309, 1320-1321, 1331-1332`). No automatic path emits that line in this state: game-exit would log "Game exited; stopping the relay it was using"; Quit/elevation/update log their own lines. **[C]** for "not a transition, not game exit"; **[P]** that it was the user pressing the only action the stale error card offered (STOP OPTIMIZING).

3. **The "auto-restart 1.4 s later" is a misreading of the trace.** The two start attempts at 06:49:39.84 and 06:49:42.69 are not the game-presence auto-start path: that path logs "Game present and relay connected; starting interception for robrun" and is latched once per presence, and no presence transition appears anywhere between 06:43:34 and 06:50. The two distinct open attempts also cannot be one invocation's internal retry: with the pin held the committed policy opens once per start (`client/src/interceptor/windivert_handle.rs:320-329`). At least two start actions occurred; the actor is not logged (F6).

4. **The stale registration card is a real defect with a deterministic mechanism** (Section 3.5): during a start, the engine's registration supervisor is replaced by the first destination report; the retired generation publishes "not registered" to `EngineStatus` even though it was merely superseded, while the live replacement registers successfully and no code writes that success back. The GUI's 1.12.5 clearing fix only covers *command* errors and is gated on a flag the same race clears, so the card persists while traffic is relayed. Mechanism **[C]**; that this specific card is the user's screenshot **[P]** (card state is not logged; the mechanism is the only code path that produces that text while registrations succeed).

5. **The restart failure is real and unrecovered.** After a clean Stop with the driver pin held, two opens of the intercept handle failed `FWP_E_IN_USE` (0x8032000A, `trace:1363-1365, 1382-1384`). The 1.12.5 policy deliberately does not retry `FWP_E_IN_USE` while the pin is held (`client/src/interceptor/windivert_handle.rs:320-329`), and the resulting card tells the user to close a stale process and **restart Windows** — both wrong for a fault this process may be able to clear. The session ended in Quit at 06:50:14; the pin was released at 06:50:15 (`trace:1396-1400`). **[C]** for the failure; **[H]** for its cause (same-process WFP filter wind-down vs. external/EAC interference) — the same trace shows four 0.65 s Stop→Optimize cycles succeed when the capture had seen no game packets.

### Findings and severity

| ID | Severity | Finding | Confidence |
|---|---|---|---|
| F1 | **High** | Generation-blind registration status: a superseded supervisor publishes a "registration failed" result; the succeeding replacement is invisible; `EngineStatus.registration_error`/`control_registered` go stale while the control plane is healthy. This is the observed card. | [C] mechanism; [P] that it is the card in the screenshot (card show/clear is not logged; the screenshot is the only card evidence) |
| F2 | **High** | Stop→Optimize after an active session failed `FWP_E_IN_USE` twice; the pin-held no-retry rule turns a possibly transient WFP state into a terminal session ("restart Windows"), with no honest retry affordance. | [C] failure, [H] cause |
| F3 | **High** | Destination/region changes cannot move the relay in place in the GUI: every change logs "Keeping the live session ... applies next session"; the in-place primitive (`engine.switch_relay`, `client/src/engine.rs:745-783`) exists but the GUI never calls it. Applying the "next session" choice means Stop→Optimize, which is exactly the action that then hit F2. | [C] |
| F4 | **High** | Error presentation masks the live state: `Error` word takes precedence over `Optimizing` (`client-gui/src/app/route_surface.rs:289-307`) and the action slot then offers only STOP (`:404-449`). A non-fatal, stale route error removes the normal user model of an active session. | [C] |
| F5 | **Medium** | Command errors are sticky: `engine_error()` reads `session.last_error` first (`client-gui/src/app.rs:2937-2961`), `record_command_result` deliberately keeps it after later successes (`client/src/app/commands.rs:283-304`, test at `:369-372`), and `AppCore::clear_error` has no GUI caller. Any failed Start/Stop latches ERROR for the process lifetime. | [C] |
| F6 | **Medium** | No UI-action audit log: the Stop at 06:49:38 and both start attempts are unattributable from the trace. Stops have no `stop_requested source=...` line; Optimize/Stop/Disconnect/tray clicks are silent. | [C] |
| F7 | **Medium** | Presence scan failure is indistinguishable from game exit: `try_auto_detect_game` maps *any* `Err` to `None` (`client-gui/src/app/choice.rs:57-73`), and process-enumeration failure returns an empty list, which `auto_detect` reports as an error (`client/src/games/mod.rs:588-591, 661-691`). One failed scan can therefore stop interception; there is no debounce. | [C] code, not observed in this window |
| F8 | **Medium** | The product has no notion of auth/menu vs. match; capture starts on process presence alone and the relay is chosen for the first lockable UDP flow. A region-specific auth/lobby endpoint can be the first "server", and the relay decision made for it can survive into the match. | [C] code, [H] for this title |
| F9 | **Low/Medium** | Relay failover is absent in the shipped GUI path: the shared watchdog is warn-only by an owner decision (2026-11-20), `client/src/route/failover.rs` has no callers, and the demoted relay is only excluded from *future* rankings. The starved session keeps relaying. | [C] |
| F10 | **Low** | Startup hold window can close before the first relay choice: the pinned-relay branch sets `server_chosen` and opens the gate before any choice (`client-gui/src/app.rs:1171-1178`), and the auto choice's own `server_chosen` set happens only after its ranking (`:1215-1216`). In the trace a config save at 06:43:34.998 (`trace:988`) marks a relay-mode change in that 6 ms window, after which the destination-aware choice at 06:43:35.000 ran with the gate already open and logged "Keeping the live session". | [P] from trace 06:43:34.994-35.000 |
| F11 | **Low** | The FWP remedy text misattributes a same-process close/reopen collision to "a previous run" and instructs a Windows restart (`client/src/interceptor/windivert_handle.rs:498-504`). No close-age/pin context is logged with the failure. | [C] |

**Positive result worth keeping:** the destination re-detect state machine itself (`client/src/interceptor/order.rs`) behaves as designed: three packets in 1.5 s lock a server, five seconds of silence release it, a new attributable candidate replaces it, a non-attribute flow does not, and a new lock is published with the same interceptor run. The transition failure is at the session/control/presentation layers above it, not in server detection.

---

## 2. What the trace actually shows

### 2.1 Timeline (log UTC; `trace:` = line in the evidence log)

| Time | Event | Line |
|---|---|---|
| 06:41:21.88 | GUI process starts; control plane registers `relay-lax-1` | trace:540-566 |
| 06:41:35.48 | `ProcessScanner: found ROB RUN.exe (PID=2392) with 0 routes`; pin open id=1; interceptor starts (no game presence log and no "starting interception" log -> not the presence auto-start path) | trace:571-610 |
| 06:41:50.14 -> 06:42:01.33 | Four Stop→Optimize cycles, 0.65-0.66 s apart, all opens succeed; no game packets captured | trace:617-731, 670-785 |
| 06:42:21.47 | interceptor "No game packets have been captured since interception started" warning | trace:828 |
| 06:42:28.76 / 06:42:29.66 | Stop, then Quit; pin released | trace:830-843 |
| 06:42:34.42 | GUI relaunches (unelevated) | trace:844 |
| 06:42:36.89 | elevation handshake -> elevated instance; "Single-instance guard bypassed" | trace:879-894 |
| 06:42:38.75 | elevated instance registers | trace:916-917 |
| 06:43:34.02 | config saved (a user control changed) | trace:923 |
| 06:43:34.53 | `Game presence changed: robrun` + `Game present and relay connected; starting interception for robrun` (the only presence transition in the window) | trace:924-926 |
| 06:43:34.775 | pin id=1; intercept/inject/shadow ids 2/3/4; **first** control-plane connection | trace:941-960 |
| 06:43:34.893 | `Locked game server: 178.107.14.146:36701` | trace:978 |
| 06:43:34.992 | **second** control-plane connection (destination report replaces the startup supervisor) | trace:986 |
| 06:43:34.994 | `Pinned relay: re-arming the relay for the new game server ...` (pinned-relay branch) | trace:987 |
| 06:43:35.000 | `Destination-aware relay choice for 178.107.14.146: 45.77.32.236:4434 ...`, then `Keeping the live session on 207.246.106.36:4434; ... applies next session`, then `Relay chosen ... releasing the hold` | trace:989-991 |
| 06:43:35.231 | first registration succeeds on the wire (`Registered with proxy session=779093578 token=3440494843`) but its generation is already superseded; no `control-plane registered` line | trace:994 |
| 06:43:35.442 | replacement registers (`session=2910297239 token=490070053`) and logs `control-plane registered` | trace:995-996 |
| 06:45:35, 06:47:35 | session-token rotations on the live control connection | — |
| 06:47:59.32 | `Locked server stale - re-detecting` (old 178.107.14.146 silent > 5 s) | trace:1239 |
| 06:48:00.36 | `Locked game server: 178.107.14.45:36501` | trace:1245 |
| 06:48:00.66 | destination-aware choice; keep live session; control plane reconnects/re-registers | trace:1247-1253 |
| 06:48:34.18 | `Locked server stale - re-detecting` (again) | trace:1278 |
| 06:48:34.22-34.72 | lock 178.107.13.95:36605; choice keeps the live relay; re-registration registered | trace:1280-1288 |
| 06:48:59, 06:49:13, 06:49:26 | return-path starvation warnings, streak 2/3/4 (`intercepted=20-22, injected=0, from_relay=0`); watchdog demotes for the *next* ranking, no path switch | trace:1307-1309, 1320-1321, 1331-1332 |
| 06:49:38.417 | `Shutdown signal received` -> interceptor teardown | trace:1340 |
| 06:49:38.561 | diverting handles 2/3/4 closed; `driver pin active=true` | trace:1346-1349 |
| 06:49:39.827 | `ProcessScanner ... 7 routes` (start-plan scan completes; open attempt 13 ms later) | trace:1350 |
| 06:49:39.841 | intercept open id=5 fails `FWP_E_IN_USE` (-2144206838 = 0x8032000A) | trace:1363-1365 |
| 06:49:40.275 | a registration from the failed start succeeds on the wire, then is drained | trace:1368 |
| 06:49:42.676-42.69 | second start attempt; intercept open id=6 fails identically | trace:1369-1384 |
| 06:49:43.129 | another registration succeeds on the wire; drained | trace:1386 |
| 06:50:14.905 | Quit; pin id=1 released at 06:50:15.17 | trace:1396-1400 |

### 2.2 Three different "presence" events, told apart

| Event | Trace signature | Code | Present at 06:49? |
|---|---|---|---|
| **Game exit** | `Game presence changed: none` then `Game exited; stopping the relay it was using` | `client-gui/src/app.rs:2156-2159` | No |
| **Server/region transition** | `Locked server stale - re-detecting` + new `Locked game server:` + `Destination-aware relay choice` + `Keeping the live session` + new `Connecting QUIC …/control-plane registered`; **no handle close** | `client/src/interceptor/windows.rs:1549-1559`; re-registration: `client/src/quic/mod.rs:1262-1273` via `client/src/latency.rs:1526-1529` | Yes, twice (06:47:59, 06:48:34); not at 06:49 |
| **Presence-scan flicker** | `Game presence changed: none` (because a failed scan is `Err -> None`) | `client-gui/src/app/choice.rs:57-73`; `client/src/games/mod.rs:661-691` | Not observed in this window; code-reachable (F7) |
| **Action-driven Stop** | only `Shutdown signal received` + teardown; no presence/quit/elevation/update line | `client-gui/src/app.rs:2675-2691` (danger/toggle), `:1693-1707`, `:4413-4423` (tray), `:1583-1593` (Quit), `:1998-2012` (elevation), `:4096-4099` (manual redirect) | Yes, 06:49:38 [P] |
| **Start (auto)** | `Game present and relay connected; starting interception for …` | `client-gui/src/app.rs:2144-2155` | No |
| **Start (action)** | no GUI line; the engine then logs `ProcessScanner` and `Opening WinDivert intercept handle` | `client-gui/src/app.rs:2684-2690` -> `client/src/app/commands.rs:135-149` | Yes, twice [P] |

### 2.3 Why "auto-restart" is not what the trace supports

- The presence auto-start is latched once per presence (`auto_started_for_presence`, `client-gui/src/app.rs:2106-2162`) and logs its path. Neither is present at 06:49.
- No presence transition exists between 06:43:34 and 06:50, so the latch could not have been cleared by game exit.
- The trace shows **two distinct WinDivert open attempts** after the Stop (`id=5` at 06:49:39.841, `id=6` at 06:49:42.691; `trace:1363, 1382`). With the pin held the committed policy gives **one open per start invocation** (no retry, `client/src/interceptor/windivert_handle.rs:320-329`), so there were at least two start invocations, not one internal retry. The first open is 1.423 s after the Stop line; whether the Stop was the pre-stop inside the first invocation (one start action) or a separate user Stop followed by a start action cannot be decided from the log — but either way it is a UI/action path, not the presence auto-start, and the second invocation is necessarily a separate action.
- The plan work (`ProcessScanner`) completes just before each open (13 ms in both attempts); the total scan duration before that line is not logged, so the exact instant each invocation began is unknown. The reconstruction is therefore "Stop plus at least two start actions", not "stop and one auto-restart". **[P]** for the actor; **[C]** for two invocations.
- **[P]** Most consistent reading: the user pressed Stop (the only action the Error card offered, F4), then Optimize as the card changed to the FWP error, then Optimize again. The exact attribution needs the missing UI-action log (F6) or a reproduction with it added.

---

## 3. Code-path audit

### 3.1 Planes and their owners today

```
user / OS
   |
   +-- presence worker ..... client-gui/src/app/presence.rs (5 s scan) -> poll_game_presence (app.rs:2063)
   |        decision: start/stop the boost                          (app.rs:2113-2162)
   |
   +-- capture plane ....... WinDivert pin + per-run handles        (client/src/interceptor/windows.rs)
   |        start  -> engine.start_interceptor_request             (client/src/engine.rs:1579)
   |        stop   -> engine.stop_interceptor / stop_boost         (engine.rs:1998 / 2076)
   |
   +-- destination plane ... ServerTracker lock/re-detect           (client/src/interceptor/order.rs)
   |        publishes session::set_locked_destination               (windows.rs:1495)
   |        feeds latency::record_outbound -> quic::report_destination (latency.rs:1526-1529)
   |
   +-- route/control plane . relay choice (GUI) + QUIC supervisors  (client-gui/src/app.rs:910-1263; client/src/quic/mod.rs)
   |        connect() vs switch_relay()                             (engine.rs:704 / 745)
   |        status writes: registration_error/control_registered    (engine.rs:2217-2232)
   |
   +-- presentation ........ AppStatus.error -> word/action/card     (client-gui/src/app/route_surface.rs)
```

There is no single owner of "a game session". Capture, destination, route and presentation each have their own state, and four different layers can independently write "something is wrong" or "something changed". This is the structural reason a server transition can look like a failure: the transition is a *destination-plane* and *control-plane* event, but its consequences are rendered through a *presentation* object that cannot tell which plane the error came from.

### 3.2 Capture plane lifecycle

**Start** — GUI action `start_boost` (`client-gui/src/app.rs:1614-1686`) calls `AppCore::start_boost` (`client/src/app/commands.rs:135-149`), which calls `engine.start_interceptor_with_target` -> `start_interceptor_request`:

- `start_interceptor_request` first stops every supervised relay registration, then **stops any existing interceptor** before validating the new request: `client/src/engine.rs:1579-1593`. This is documented restart semantics; it means *any* start while a run exists is a teardown + restart.
- `plan_interceptor` runs the process scanner synchronously (the ~0.2-3.4 s blocking step on the UI thread; `engine.rs:1605-1748`, scanner at `:1616-1691`).
- `start_interceptor_plan` pins the driver, spawns a relay registration, starts WinDivert (`engine.rs:1757-1868`). On failure it drains the registration it just spawned (`:1818-1824`). On success it clears `registration_error` (`:1827-1851`) — note this is a *success-path* clear that the 06:49 failure never reached.
- The GUI then applies the start-of-match hold *after* the interceptor is already live and capturing (`client-gui/src/app.rs:1656-1685`). Between the first handles opening and `hold_tunnelling()`, packets can be classified against an open gate. The trace supports this: the tunnel task logs `Game client at 10.31.72.210:62050` at 06:43:34.992112 (`trace:985`), i.e. it is already processing a captured outbound packet, and the second control-plane connection 0.3 ms later (`trace:986`) is the destination report that only a *tunnelled* send can create (`client/src/latency.rs:1526-1529`). That report is what replaces the start supervisor (F1). **[P]** for the exact send timing, **[C]** for the report path.

**Stop** — the only paths that emit `Shutdown signal received` (`client/src/interceptor/windows.rs:1316-1323`) are the ones that call `stop_interceptor`:

- user Stop / toggle action (`client-gui/src/app.rs:2675-2691`, `:1693-1707`), tray Disconnect (`:4413-4423`), Quit (`:1583-1593`), elevation restart (`:1998-2012`), manual redirect switch (`:4096-4099`), game-exit (`:2156-2159`), update install, and `start_interceptor_request`'s implicit pre-stop.
- `stop_interceptor` waits up to 1500 ms for owner-thread acks and then closes the handles (`client/src/engine.rs:1998-2027`); the trace shows a clean 144 ms teardown (`trace:1340-1349`).
- The driver pin is deliberately kept on a user Stop (`client/src/interceptor/windivert_handle.rs:462-487`, release only on Quit/`shutdown`).

**Why it was torn down at 06:49:38:** not game exit (no line), not Quit/elevation/update (no line), not a server transition (no transition at that time). The in-process candidates are the user-facing actions above, or a start that pre-stopped. The trace's two later start attempts carry no auto-start log, so the action path is the consistent source; see Section 2.3 and open questions.

### 3.3 Locked server and re-detect

- `ServerTracker` (`client/src/interceptor/order.rs`): 3 packets within 1.5 s lock a server; 5 s of silence mark it stale (`TrackerConfig::default`, `order.rs:100-110`); a stale lock is replaced only by an attributable candidate (game process, or same hosting network when attribution lags) and the release is signalled as `Decision::ResetToDetection` (`order.rs:560-590`).
- The Windows receive loop maps that to `OutboundDecision::Redetect` and logs `Locked server stale - re-detecting`; it clears the reported server and inject cache and re-injects the packet unchanged (`client/src/interceptor/windows.rs:1549-1559`). A new lock logs `Locked game server:` (`:1492-1500`) and publishes `session::set_locked_destination` (`:1495`).
- A new tunnel send calls `latency::record_outbound` -> `route::destination::note_outbound` (250 ms rate limit, `client/src/route/destination.rs:495-515`) -> `quic::report_destination` (`client/src/latency.rs:1526-1529`; `client/src/quic/mod.rs:1262-1273`). That call is what replaces the per-relay supervisor and re-registers with the new destination.
- Consequences seen at 06:47:59/06:48:34: a new lock (correct), a relay choice (correct log), a re-registration (new session id), and no capture teardown (correct). This path is not itself defective. The defects are what the higher layers do with the event.

### 3.4 Relay selection and why the relay never changed

- `poll_relay_measurement` (`client-gui/src/app.rs:1091-1143`) and `follow_game_server` (`:1156-1263`) re-rank on every new locked server. A relay change is only applied when the tunnel is **held** (`switch_is_safe`, `client/src/app/watchdog.rs:826-837`); with tunnelling enabled they log exactly the sentence in the trace: "Keeping the live session ... is better for this server and applies next session" (`app.rs:1124-1135`, `:1240-1253`).
- The gate itself assumes a relay change is only safe while tunnelling is withheld. The engine already has a non-restarting primitive: `AppCore::switch_relay` (`client/src/app/commands.rs:199-212`) -> `engine.switch_relay` (`client/src/engine.rs:745-783`), which registers with the destination, publishes the relay and its per-path token only **after** the registration succeeds (`quic/mod.rs:1498-1512`), and never touches the interceptor handle. The GUI has no caller of `commands.switch_relay` (only tests). Its only mid-session relay paths are `connect_selected` -> `select_relay` -> `engine.connect` (`engine.rs:704-733`) and the manual relay dropdown (`client-gui/src/app.rs:3229-3233`), which also goes through `connect_selected`. `connect()` restarts the control plane and resets `connected`/`control_registered`; the code comment on `connect_selected` (`app.rs:791-796`) explicitly says restarting interception there would be wrong, so the relay move is in-place at the data plane, but the automatic gate still forbids it during a live session while a manual pin resets the route state instead of using the registration-gated switch.
- The 06:43:35.000 choice is worth separating: the hold had just been opened and the choice still saw an open gate (`trace:989-991`), so the player started the session on the pre-connected LAX relay. The hold-release watcher (`app.rs:1670-1681`) fires when `server_chosen && connected`; `server_chosen` is set by the pinned-relay branch *before* any choice (`app.rs:1171-1178`) and by the choice completion for the auto path (`:1215-1216`). The `Saved config` at 06:43:34.998 (`trace:988`) marks a user control change inside that 6 ms window; any relay-mode change there can close the only safe-switch window before the first choice runs, and the choice then keeps the live relay by construction. **[P]**
- Relay failover: absent. The shared watchdog is warn-only (owner decision 2026-11-20; `client/src/app/watchdog.rs:1-14, 190, 227-248`), it demotes `(relay, server)` in the history for the *next* ranking only (`client-gui/src/app.rs:1775-1797`), and `client/src/route/failover.rs` (a `FailoverState` helper) has no callers. So a confirmed starved session keeps relaying, which is what the trace shows for 178.107.13.95.

### 3.5 Control-plane registration (the stale-card mechanism)

Two different registrations exist and only one writes the UI status:

1. **Engine start registration** — `engine.spawn_registration` (`client/src/engine.rs:856-876`) awaits the supervisor's **first** attempt and maps it to `EngineStatus` via `apply_registration_result` (`engine.rs:2217-2232`): `Some(token)` -> `control_registered=true`, `registration_error=None`; `None` -> `control_registered=false`, `registration_error=Some("control-plane registration failed")`.
2. **Destination-report re-registration** — `quic::report_destination` (`client/src/quic/mod.rs:1262-1273`) -> `ensure_supervisor` with the new destination. If the relay already has a supervisor and its stored destination differs, the old supervisor is retired and a new generation starts (`quic/mod.rs:1145-1218`). Nothing in this path writes `EngineStatus`.

A retired supervisor that has not completed its first attempt publishes `Initial::Done(0)` when it exits (`quic/mod.rs:1405-1409`). A supervisor that completes registration after being superseded fails `publish_target` and breaks before logging `control-plane registered` (`quic/mod.rs:1350-1362`). So:

- At 06:43:34.992 the destination report retired the start supervisor; at 06:43:35.231 that retired supervisor's registration completed on the wire and was discarded (no `control-plane registered` line — `trace:994`); the engine's waiter therefore recorded a **failed** registration in status, ~0.2 s after a session that is in fact registered and relaying.
- The replacement's success at 06:43:35.442 (`trace:995-996`) has no writer to `EngineStatus`.
- The GUI's 1.12.5 fix `poll_health -> clear_stale_registration_error` (`client-gui/src/app.rs:1535-1538`; `client/src/app/commands.rs:61-67`) only clears a **command** error whose detail contains `"control-plane registration failed"`, and only while `status.control_registered` is true — which this race sets false. It cannot clear the engine field, which is what `engine_error()` returns (`app.rs:2944-2946`).
- Every later server change re-registers successfully (`trace:1252-1253, 1287-1288`); none of those successes clears the engine field either. The card persists while traffic flows — exactly the reported observation. **[C]**

A second, independent sticky path is F5: any failed command (`StartBoost`, `StopBoost`, `SelectRelay`, ...) records `SessionCommandState.last_error`, which `engine_error()` returns first, and no GUI code calls `AppCore::clear_error()` (`client/src/app/commands.rs:56-67`; only `clear_stale_registration_error` is called). A failed Optimize therefore pins the window to ERROR until the process restarts.

The two planes are also independent in the other direction: during the 06:49 failed starts, registration succeeded on the wire twice (06:49:40.275 and 06:49:43.129; `trace:1368, 1386`) while the capture plane could not open a handle. A route-plane success cannot repair a capture-plane failure, and neither plane's state may be inferred from the other; the presentation layer currently does exactly that inference (F4).

### 3.6 Error presentation: why the card survives and why only STOP is offered

- `AppStatus.error` is built from `engine_error()`: command error -> config recovery -> engine registration error -> capture/redirect/interceptor errors (`client-gui/src/app.rs:2721-2833, 2937-2961`).
- `word_for` gives `Error` precedence over `NoReturnPath`, `Holding`, `Optimizing`, `Connecting`, `Connected` (`client-gui/src/app/route_surface.rs:289-307`). The error panel renders whenever the word is `Error` (`client-gui/src/app.rs:2618-2647`).
- `action_for` ignores the error for the label: while `boosting` it is `STOP OPTIMIZING` (`route_surface.rs:404-449`). So a stale registration error on a live session produces: word ERROR + the sentence "Registration failed" + a STOP OPTIMIZING button. That is the reported screenshot, and it is the reason a user's only available action is to stop optimization.
- Error copy keys: `state.error` tag, `error.registration` sub-line (mapped from `AppErrorKind::Route`), detail behind `Details` (`route_surface.rs:57-79, 821-832`; i18n `state.error`). The registration error constructor is `AppError::registration` (`client/src/app/dto.rs:503-506`).

### 3.7 WinDivert teardown and the `FWP_E_IN_USE` failure

- 1.12.5 design: one non-diverting pin handle for the app lifetime; Stop closes only diverting handles; a later Optimize reuses the pin; `FWP_E_IN_USE` is retried only when **no** pin is held (`client/src/interceptor/windivert_handle.rs:299-329, 614-656`). With a pin held the same error is treated as external stale state and fails immediately with the "close stale process / restart Windows" remedy (`:498-504, 911-921`).
- Trace: the Stop teardown is clean (all three closes `ok`, pin still active; `trace:1346-1349`). The next open fails `FWP_E_IN_USE`, attempt=1, no retry line, because `pin_active=true` (`trace:1363-1365`). The second attempt 2.8 s later fails identically (`trace:1382-1384`). The remedy card is wrong for this state: no stale process is running (the only process is this one), and the filter belongs to this process's just-closed handles.
- Counter-evidence to a pure "time since close" theory: in the same trace, four Stop→Optimize cycles with 0.65 s gaps succeeded while the capture had seen no game packets (`trace:617-785`), and the 1.12.5 validation matrix claims 30/30 traffic-bearing cycles. The 06:49 difference is that a real, starved, high-rate session had just been torn down. So the trigger is likely filter-object release under live traffic or an external (anti-cheat/EDR) WFP reference — **[H]**; the log has no close-age, handle-stats or WFP object evidence to decide. What is not hypothesis: the policy turns the condition into a terminal session with a misleading remedy.
- The legacy redirect backend has the same re-detect wording in a second loop (`client/src/capture/windivert_redirect.rs:764`); any lifecycle fix must cover both or unify them.

---

## 4. Why the current design tears down capture and keeps dead errors (causal summary)

1. **Capture and session are not separated.** The GUI's start command is restart semantics (`engine.rs:1579-1593`): starting while a run exists closes it first. So any UI action that starts (including one taken while the UI believes the run is idle) tears down the interceptor. There is no "reconfigure in place" command.
2. **Server changes are routed through the control plane, not the capture plane, but their side effects are written into shared status.** The destination report replaces the supervisor (correct for relaying), yet the retired generation's first-attempt result is written as a session-wide registration failure (F1). Nothing distinguishes "superseded" from "failed".
3. **Presentation cannot tell a route/registration warning from a capture fault.** Error is a single `Option<AppError>` with no source epoch; any error outranks the live state (F4), and command errors are never cleared (F5). The 1.12.5 fix cleared one narrow command string, not the engine field or the general staleness.
4. **A relay decision that cannot be applied mid-session has no in-place path in the GUI**, even though the engine has `switch_relay`; the only way to "apply next session" is Stop→Optimize, which is the operation that is unsafe while the WFP teardown is still settling (F3/F2). The user's path through the product forces the one action the lifecycle handles worst.
5. **Presence is a single boolean from a process scan**; a failed scan and a game exit are the same input (F7), and there is no match/auth phase (F8), so lifecycle decisions rest on the least reliable signal available.

---

## 5. Transition-safe state machine (specification)

The design goal is: **a game-server/region change must never change the capture plane.** Capture stops only on explicit user intent, quit/update, or a fatal local fault. Everything else is a route/control-plane update performed in place.

### 5.1 State planes

| Plane | States | Owns |
|---|---|---|
| **P1 Presence** | `ABSENT`, `SCAN_UNKNOWN`, `PRESENT(game_key, pid)` | whether a game process exists; *not* whether a match is live |
| **P2 Capture** | `OFF`, `PINNED_IDLE`, `STARTING`, `CAPTURING`, `FAULT_LATCHED` | WinDivert pin + diverting handles, hold gate |
| **P3 Destination** | `NO_DESTINATION`, `CANDIDATES`, `LOCKED(server, provider, epoch)`, `STALE(epoch)` | the locked game server and its epoch |
| **P4 Session/route** | `NO_RELAY`, `REGISTERING(relay, dest, gen)`, `REGISTERED(relay, dest, gen)`, `SWITCHING(old, new, dest)`, `RELAY_FAILED(retry_at)` | current relay, per-path token, control-plane supervisor |
| **P5 Presentation** | per-source error/warning records (`source`, `epoch`, `severity`, `auto_clear`) | what the user sees; derived, never authoritative |

`epoch`/`generation` counters are monotonic per plane and are carried in every event and log line. Every write to shared status is generation-guarded: a writer may only publish if its generation is the current one for that source.

### 5.2 Events and required handling

| Event | Producer | Required handling |
|---|---|---|
| `PresenceScanResult(Present(game,pid) / None / ScanError)` | presence worker | `ScanError` -> `SCAN_UNKNOWN` (no action, log + count). `None` -> absent candidate; require N=3 consecutive `None` or an observed process exit (`pid` no longer in table) before `ABSENT`. Any `Present` resets the counter. |
| `StartRequested(source)` | UI action only | P2 `OFF/PINNED_IDLE -> STARTING`; apply the route hold **before** handles can transmit; never implicit-stop a healthy `CAPTURING` (reject with "already optimizing", or treat as no-op). |
| `StopRequested(source)` | UI/tray/quit/update | P2 `CAPTURING -> OFF` (Quit also releases pin). Log source. |
| `ServerLocked(server, provider, epoch+1)` | tracker | P3 update in place. If `CAPTURING`: (a) publish `session::set_locked_destination`; (b) re-rank; (c) if a different relay is materially better, enqueue `RelaySwitchRequested`; (d) re-register the control plane with the new destination (generation-scoped). **Never** change P2. |
| `ServerStale(epoch)` | tracker | P3 `LOCKED -> STALE`; keep last known server for display with an "updating" qualifier; no error, no stop, no relay change. |
| `DestinationRanked(candidates, gate)` | chooser | Record decision with reason. While `CAPTURING`: only an in-place switch is allowed; the hold-based switch remains for the pre-first-packet window. |
| `RelaySwitchRequested(new, dest)` | ranking or failover | P4 `REGISTERED -> SWITCHING`; call `engine.switch_relay(new, Some(dest))`; publish only on success; on failure stay on the old relay, retry bounded (2), then surface a route banner. P2 untouched. |
| `ControlPlaneRegistered(gen, relay, dest, token)` | supervisor | If `gen` is current: publish `REGISTERED`; clear the *registration* status/warning for the same or older epoch. If stale: discard silently (log `superseded=true`), never write failure status. |
| `ControlPlaneRetired(gen, reason=Superseded\|Stopped\|Failed)` | supervisor | Only `Failed` for a current generation may set a registration error. `Superseded`/`Stopped` are not failures for presentation. |
| `WindowJudged(fault)` / `StarvationConfirmed(server, relay)` | watchdog | Surface warning; if a ranked alternative exists and policy allows, request a bounded in-place switch; never stop capture; never take the route without a successful registration. |
| `RecoveryConfirmed` | watchdog | Clear the starvation warning/streak for that `(server, relay)` only. |
| `CaptureFault(fatal)` | backend | P2 `CAPTURING -> FAULT_LATCHED`; only this (and teardown-latched) may render the ERROR word while a run existed. Offer Retry/Stop, never "restart Windows" for a recoverable condition. |
| `PresenceAbsent(confirmed)` | presence | P2 `CAPTURING -> OFF` after debounce; P3 reset; P4 disconnect; clear per-run errors. |
| `QuitRequested` / `UpdateInstallRequested` | UI/updater | Full `shutdown`: true Stop then pin release; refuse install on unclean teardown. |

### 5.3 Transition table (capture plane, the safety-critical one)

| From | Event | To | Notes |
|---|---|---|---|
| `OFF` | `StartRequested` + admin + relay selected | `STARTING` | preconditions checked before anything opens |
| `OFF` | `StartRequested` when not ready | `OFF` | disabled action; reason line |
| `STARTING` | start success (pin + handles + hold armed) | `CAPTURING` | status success path clears run errors; registration still async |
| `STARTING` | start failure | `PINNED_IDLE` | record typed error with source+epoch; bounded FWP retry policy; no pin release |
| `CAPTURING` | `StopRequested`, `QuitRequested`, `UpdateInstallRequested`, `PresenceAbsent(confirmed)`, `CaptureFault(fatal)` | `OFF`/`FAULT_LATCHED` | the **only** exits |
| `CAPTURING` | `ServerLocked`, `ServerStale`, `DestinationRanked`, `RelaySwitchRequested`, `ControlPlaneRegistered/Retired`, `WindowJudged`, `ScanError` | `CAPTURING` | state does not change; route/presentation updates only |
| `PINNED_IDLE` | `StartRequested` | `STARTING` | pin reused; no driver reload |
| `FAULT_LATCHED` | `StartRequested` after backoff clears | `STARTING` | explicit Retry only; teardown-latched refuses |

### 5.4 Invariants (normative; each is testable)

- **I1 No capture teardown on destination/region change.** `ServerLocked`/`ServerStale` must not call `stop_interceptor`, `stop_boost` or `start_interceptor_request`. Test: transition with a live session produces zero `WinDivert handle close` events.
- **I2 No implicit pre-stop.** A start request while `CAPTURING` is rejected or a no-op; the engine does not stop a healthy run to start another (`engine.rs:1579-1593` is removed/changed for GUI use).
- **I3 Hold before transmit.** No packet may be tunnelled before the start-of-match hold state is installed; the run-phase starts as `HOLD` and the tunnel send path must read it, not a late atomic set after start.
- **I4 Relay changes are in place and registration-gated.** `current_proxy` and the per-path token are published only after the new relay registered with the destination; the interceptor handle set is unchanged; the previous relay's token stays valid during the transition.
- **I5 Registration status is generation-scoped.** Only a current-generation result may write `registration_error`/`control_registered`; `Superseded` never maps to failure; a newer success clears older errors (including the GUI command-error copy of the same condition).
- **I6 Errors carry a source and epoch and clear on that source's recovery.** The error record is `{source: Capture|Route|Registration|Config|Game, epoch, detail, auto_clear}`; presentation clears when the same source reports healthy at the same or newer epoch.
- **I7 Error does not mask a live capture.** While `CAPTURING`, non-fatal route/registration errors render as a banner/warning *in addition to* the live state; the word remains HOLDING/OPTIMIZING/NO RETURN PATH. Only `FAULT_LATCHED` uses the ERROR word.
- **I8 Presence debounce.** `ScanError != Absent`; absence requires N consecutive clean `None` results or an observed process exit; start requires one `Present` (optionally two for cold starts).
- **I9 Safe-switch window.** `server_chosen` may be set only by a completed choice (or by an explicit pinned-relay decision that also arms the gate deliberately); a config change during the pre-first-packet window re-arms the hold before re-ranking.
- **I10 Failover is bounded and honest.** At most K in-place switches per (server, session); each requires a registration success; on exhaustion the session keeps relaying with a warning; no silent direct fallback.
- **I11 Stop is explicit.** Only Start/Stop/Quit/update/fatal can cross the capture boundary; every cross is logged with `source`.
- **I12 One token authority per path.** Per-path token published before `current_proxy` points at it; previous token retained for the rotation window (`client/src/session.rs:144-185, 265-273`).
- **I13 Startup registration is single-generation.** The first destination report during `STARTING` must amend the startup registration rather than replace a supervisor whose first attempt the engine is awaiting (or the engine must await generation-aware completion). Test: exactly one `control-plane registered` for the start's registration in a normal start.
- **I14 Every transition is observable.** Structured log line + counter for every state change and every decision with its cause; user-initiated actions carry `source=button|tray|shortcut|menu`.
- **I15 No reboot advice for a recoverable condition.** "Restart Windows" copy only after bounded retries fail *and* the process's own close epoch can be excluded; otherwise "retrying in Ns" / "retry" is the copy.

### 5.5 Reconnect and failover policy (route plane)

- **Control-plane loss** (QUIC closed): supervisor reconnects with bounded backoff (`quic/mod.rs:1313-1411` already does this). It must not touch capture; a successful reconnect clears only the registration error for its epoch. The data plane keeps sending on the current relay with the per-path token (valid until rotation/reject).
- **Registration failed on a current relay**: keep the session on the existing relay if one is registered; retry with backoff; surface a route banner after the first failure, an error card only after exhaustion. Never stop capture.
- **Starvation confirmed** (2 judged windows; `client/src/app/watchdog.rs:68-75`): if another ranked candidate exists (the ranking already excludes starved pairs for this server), request an in-place switch; on success, new supervisor + token, capture unchanged; repeat up to K=2 candidates; then warn-only. Preserve the owner's 2026-11-20 "never take the route silently" decision by requiring an explicit policy flag for automatic failover; the recommendation is opt-in "auto-failover on confirmed starvation" with the user informed in the banner.
- **All candidates failed**: keep the session, keep the warning, offer a one-click "switch relay" action that uses `switch_relay`, never Stop/Optimize.

### 5.6 User-visible behavior (target)

| Internal state | Word | Card | Primary action | Secondary |
|---|---|---|---|---|
| `CAPTURING` + route error (non-fatal) | OPTIMIZING (or NO RETURN PATH warning) | route banner strip, dismissible; error detail in Details | STOP OPTIMIZING | — |
| `CAPTURING` + starvation confirmed | NO RETURN PATH (warning) | banner names server+relay and the remote condition; "keeping route" until failover policy acts | STOP OPTIMIZING | switch relay (in place) |
| `CAPTURING` + registration re-registering | OPTIMIZING, route row "updating" | no error | STOP OPTIMIZING | — |
| `PINNED_IDLE` + failed registration | NOT CONNECTED/CONNECTING per route | registration banner with Retry | OPTIMIZE / RETRY | — |
| `FAULT_LATCHED` | ERROR | capture fault; retry countdown when retryable; "restart Windows" only when justified | RETRY | open log |
| `OFF` on game exit | NOT CONNECTED | none | OPTIMIZE | — |

Copy rules: never show "Registration failed" while the route row shows traffic returning; registration failure copy appears only when registration is actually the blocking fact for the user's goal. The error panel must name the failing subsystem ("the relay link", "the game capture"), not just the taxonomy sentence.

---

## 6. Observability requirements

### 6.1 Structured events (minimum)

Every line carries: `ts`, `level`, `event`, `session_id` (monotonic per GUI process), `capture_run_id`, `interceptor_epoch`, `destination_epoch`, `relay`, `supervisor_generation`, and the relevant `source`.

| Event | When | Required fields |
|---|---|---|
| `gui.action` | every button/tray/shortcut/menu action | `action`, `source`, `state_before`, `state_after` |
| `presence.scan` | every scan | `result` (`present\|none\|error`), `game_key`, `pid`, `consecutive_absent`, `duration_ms`, `error` |
| `presence.state` | state change | `from`, `to`, `reason` |
| `capture.state` | every P2 change | `from`, `to`, `reason`, `handle_ids`, `pin_active`, `teardown_ms` |
| `capture.start` | start attempt | `trigger=user|presence|retry`, `result`, `phase` (`plan\|pin\|handles\|hold`), `duration_ms` |
| `capture.stop` | stop | `source=user|tray|quit|update|presence-absent|fatal|start-restart`, `teardown_ok`, `handles_closed`, `session_duration_ms`, `traffic_at_stop` |
| `destination.locked` | new lock | `server`, `epoch`, `accept_reason`, `provider`, `same_network`, `held` |
| `destination.changed` | transition | `old`, `new`, `epoch`, `silence_ms`, `same_provider`, `capture_untouched=true` |
| `route.choice` | every destination ranking | `server`, `chosen`, `current`, `ranked[]`, `gate` (`held|live`), `decision` (`apply|keep`), `reason`, `direct_ping_age_ms` |
| `route.switch` | relay switch attempt | `from`, `to`, `dest`, `attempt`, `result`, `wait_ms`, `published` |
| `registration.attempt` / `registration.result` | each supervisor attempt/result | `relay`, `destination`, `generation`, `result`, `token_rotated`, `superseded` |
| `registration.status` | every write to `registration_error/control_registered` | `old`, `new`, `source=engine|supervisor|gui`, `generation`, `cause` |
| `error.set` / `error.clear` | every presentation error change | `source`, `epoch`, `kind`, `detail`, `auto_clear`, `cause` |
| `watchdog.window` / `watchdog.verdict` | each judged window | `server`, `relay`, counters, `share`, `streak`, `decision` |
| `fwp.open` | each open attempt | `id`, `kind`, `attempt`, `pin_active`, `ms_since_last_close`, `result`, `raw_os_error` |
| `ui.word` | word change | `from`, `to`, `driving_fact` |

### 6.2 Counters / metrics

`capture_starts_total{trigger,result}`, `capture_stops_total{source}`, `capture_restart_without_user_total` (must be 0; alarm on any), `registration_failures_total{reason}`, `registration_superseded_total`, `registration_status_writes_total{result}`, `stale_status_errors_cleared_total`, `error_age_seconds{source}` (histogram), `relay_switch_total{result}`, `presence_scan_errors_total`, `presence_false_absent_total`, `fwp_open_failures_total{pin_active, close_age_bucket}`, `wfp_close_to_open_ms` (histogram), `hold_window_ms`, `destination_transitions_total`.

### 6.3 Diagnostics to add to the evidence set

- WindDivert handle stats (`windivert_handle_stats()`, already implemented at `client/src/interceptor/windivert_handle.rs:281-293` but never logged) at start/stop/failure.
- WFP close->open interval and outcome for each Stop→Optimize, with `pin_active`.
- Relay-side logs for the affected server/replay window (for starvation triage).
- GUI screenshots at error/word transitions.
- A captured (redacted) copy of `gui-trace.log` with the action-log lines once F6 is fixed.

### 6.4 Privacy

Tokens, session ids and relay/server IPs already appear in the trace. Keep the existing policy (no secrets in the repo/evidence; IPs are already treated as operational data). Add a note that `token` fields are credentials for the relay session and should not be pasted into public issues.

---

## 7. Test matrix

Levels: **U** unit/pure, **I** integration (engine + mock QUIC/WinDivert), **M** manual live capture on Windows with the real driver. Evidence for each: a named artifact under `wat/evidence/2026-11-23/stability-audit/` (or a later dated directory if the test runs later).

| ID | Scenario | Given / When / Then | Invariants | Level |
|---|---|---|---|---|
| T1 | Settlement/POI switch | live capture; server changes A->B within 30 s. Then: zero handle closes, `destination.changed`, in-place re-registration, relay choice either applied via `switch_relay` or kept with a route banner; no ERROR word, no stale card. | I1,I4,I5,I7 | U+I+M |
| T2 | Login menu -> match (auth region -> game region) | presence only; first lock is an auth/lobby flow; later the match server appears. Then: capture never restarts; provisional destination yields to the match destination; relay re-ranked for the match; no registration error card. | I1,I8,I9 | U+I+M |
| T3 | Region shift across continents | server IP changes region while capturing. Then: relay switch is awaited and registered with the destination, published only on success, capture untouched; old token grace respected. | I4,I12 | U+I+M |
| T4 | Reconnect (control-plane loss) | drop the QUIC connection mid-session. Then: supervisor reconnects with backoff; capture untouched; registration status goes CONNECTING->REGISTERED; no stale card; data plane keeps current relay. | I1,I5,I12 | I+M |
| T5 | Relay failover after starvation | 2 judged windows with no reply; ranking has an alternative. Then: one in-place switch, registration first, capture untouched; if it also starves, bounded second attempt then warn-only. | I4,I10 | U+I+M |
| T6 | Server lock stale on the same server (loading screen) | 5 s silence, same server returns. Then: `destination.stale` -> re-lock same server; no relay re-choice, no restart, no error. | I1,I9 | U+I |
| T7 | Rapid Stop->Optimize after an active session | capture traffic, Stop, then Optimize at 1 s / 4 s / 10 s. Then: either bounded retry succeeds within budget, or a typed capture error with Retry and close-age context; never "restart Windows" for a same-process condition. | I2,I11,I15 | M (Windows) |
| T8 | Presence scan failure | make `tasklist` fail (or inject `ScanError`). Then: `presence.scan result=error`, no stop, no state change, counter increments. | I8 | U+I |
| T9 | Presence flicker | one/two `None` results then `Present`. Then: no stop (debounce), no restart loop; only after N absences does the session stop. | I8 | U+I |
| T10 | Stale registration error clears | supersede a supervisor during start; replacement registers. Then: no ERROR word; `registration.status` shows superseded ignored and success published; observable in trace. | I5,I6,I13 | U+I |
| T11 | Superseded supervisor is not a failure | unit: start supervisor's generation retired before first attempt completes. Then: no `registration_error`, `superseded_total` increments. | I5,I13 | U |
| T12 | Hold ordering | start with an already-connected relay; first packet arrives before the hold is installed. Then: no packet is tunnelled before the hold; no destination-report supervisor churn at start; exactly one start registration. | I3,I9,I13 | U+I |
| T13 | Error presentation while capturing | live capture + forced route error + forced registration error. Then: word stays OPTIMIZING (or warning), banner shows, action stays STOP; clearing the source restores the word without a restart. | I6,I7 | U+I+M |
| T14 | Game exit | process disappears. Then: after debounce, capture stops, state cleared, relay disconnected, no ERROR card. | I8,I11 | I+M |
| T15 | Action audit | every user action (button/tray/shortcut/menu). Then: exactly one `gui.action` line with source and state transition; Stop attribution is possible from the trace alone. | I14 | U+M |
| T16 | Elevation/update handshake | elevation restart or self-update while capturing. Then: true Stop, pin release ordering, no stranded handles, no stale WFP object. | I11,I15 | M |
| T17 | FWP close-age matrix | Stop with (a) no traffic, (b) light traffic, (c) starved burst; reopen at 0.5/1/3/10 s with the pin held. Then: record close->open outcome and correlate; define the retry budget from data. | I15 (supports F2) | M |

Any T1-T6 passing while the *trace* still shows a `WinDivert handle close` during the scenario fails the invariant: the test must assert on the absence of teardown events, not only on the final UI state.

---

## 8. Roadmap (proposals, not implemented here)

**P0 — stop lying to the user and stop the unrecoverable path**
1. **Generation-scoped registration status** (F1): add a supervisor generation to `EngineStatus` writes; never map `Superseded` to `registration_error`; let a current success clear the error; make `poll_health` clear the *engine* error too (not only command text) for registration recovery; log every status write with cause. Files: `client/src/quic/mod.rs:1145-1218, 1300-1411`; `client/src/engine.rs:856-876, 2217-2232`; `client-gui/src/app.rs:1535-1538, 2937-2961`; `client/src/app/commands.rs:61-67`. Tests T10/T11.
2. **Presentation split** (F4/F5): errors carry source+epoch; non-fatal route/registration errors render as a banner and never replace the live word; clear on recovery; remove the dead-end sticky command error (or give it an explicit dismiss/retry that does not stop capture). Files: `client-gui/src/app/route_surface.rs:289-307, 404-449, 821-832`; `client-gui/src/app.rs:2618-2691, 2937-2961`; `client/src/app/dto.rs:403-457`. Tests T13.
3. **Start/Stop semantics** (F2, I2): no implicit pre-stop of a healthy capture from a start request; explicit `restart` only; add a close-age-aware FWP policy (bounded retry even with the pin when this process closed handles within T; distinct error copy; Retry action) and log `windivert_handle_stats`. Files: `client/src/engine.rs:1579-1593, 1998-2027`; `client/src/interceptor/windivert_handle.rs:299-329, 614-656`; `client-gui/src/app.rs:2675-2691`. Tests T7/T17.
4. **Presence scan semantics** (F7): worker returns `Present|None|ScanError`; debounce absence (N=3 or process-exit); log/count scan errors. Files: `client-gui/src/app/choice.rs:57-73`; `client-gui/src/app/presence.rs`; `client-gui/src/app.rs:2063-2163`; `client/src/games/mod.rs:661-691`. Tests T8/T9.

**P1 — make transitions safe by construction**
5. **In-place relay update** (F3): on `ServerLocked`, if capturing, use `commands.switch_relay` (already implemented) with destination; gate publish on registration; keep the hold-based switch only for the pre-first-packet window; fix the hold/`server_chosen` ordering (I9); route the manual relay dropdown through the same command so a user pin does not reset `connected`/`control_registered`. Files: `client-gui/src/app.rs:1091-1263, 1614-1686, 1775-1797, 3229-3233`; `client/src/app/commands.rs:199-212`; `client/src/engine.rs:745-783`. Tests T1/T3/T12.
6. **Bounded in-place failover** on confirmed starvation (F9/F10): opt-in policy, ranked candidate, registration-gated, K=2, warn-only if exhausted. Files: `client/src/app/watchdog.rs:68-75, 190, 227-248`; `client-gui/src/app.rs:1775-1797`; wire `client/src/route/failover.rs` or delete it. Tests T5.
7. **Auth/menu provisional destinations** (F8): classify the first lock as provisional until sustained bidirectional traffic; do not let a provisional destination pin a relay decision; re-rank on the match server. Tests T2.

**P2 — evidence and operations**
8. **UI action audit log** (F6) and `ui.word`/`error.*` transition logging (I14); capture in the trace by default. Tests T15.
9. **Diagnostics surface**: handle stats, WFP close->open, relay switch results, presence counters in the status/DTO and in `--status` output.
10. **Training + runbook**: `docs/training/game-transition-training.md`; on-call triage for FWP/registration/starved-session reports; require traces with action logging.

Every item above is a proposal with a verification plan; none is claimed as fixed or shipped. Each fix needs a failing-before test (T1-T17), a mutation check, and fresh Windows live evidence before it can be called verified.

---

## 9. Open questions / unknowns

0. **Trace-to-build provenance.** No build stamp; the file spans at least two binaries (the 06:01/06:03 retry lines are impossible under committed code, while the 06:49 no-retry shape matches it). All 1.12.5 claims in this document are restricted to the 06:42:38-06:50:15 elevated session and remain line-shape inferences until a build-stamped reproduction is captured. See `docs/audit/stability-verification-2026-11-23.md` §6 and `docs/audit/windivert-lifecycle-2026-11-23.md`.
1. **Attribution of the 06:49:38 Stop and the 06:49:39/42 starts.** No UI action log exists. The reconstruction (Stop followed by two Optimize clicks, not one automatic restart) is the most consistent with the code and timing but is not proven. Needs F6 logging or a reproduction.
2. **FWP root cause.** Same-process WFP filter wind-down under traffic vs. external anti-cheat/EDR WFP references. The trace shows clean closes and two failures at 1.2 s and 4.0 s after close, yet four rapid cycles succeeded when no game traffic was captured. No handle stats/close-age data exists; T17 defines the experiment.
3. **Why the 1.12.5 matrix (30/30 traffic-bearing Stop→Optimize) did not reproduce it.** Host-specific EDR/EAC/elevation or traffic-pattern difference; the release note's "does not claim the pin fixes the original fault" caveat applies.
4. **Whether both control-plane registrations at start are expected.** The engine's start registration and the first destination report race; the current design has no generation-aware "amend in place" path. Instrumenting `registration.result` will show how often a start supervisor is superseded before its first attempt.
5. **Whether relay `45.77.32.236` was actually better for `178.107.14.x`.** The choice had no exact server location ("Unknown") and no direct ping; the ranking is a prior, not evidence. Any failover design must keep the "unknown" honesty.
6. **Why the new server `178.107.13.95:36605` had zero return traffic.** Wrong lock (a non-game flow in the port band), a relay/token problem after re-registration, or a genuinely dead server path. The per-flow counters and relay logs needed to decide are not in the evidence set.
7. **Whether this title's auth/menu traffic actually crosses regions.** The ROB RUN profile is dynamic-server with a 32768-60999 range; the trace shows only the match-like flows. The auth/menu requirement is general (F8) but its live impact for this title is unmeasured.
8. **Presence scan reliability on this host.** The session shows 81 successful scans in the window; the scan-error rate under the user's EDR is unknown (no counter).

---

## Appendix A — Evidence index (selected trace lines)

- Presence/start: `trace:924-926` (only presence transition + auto-start), `trace:986` (second control-plane connection during start), `trace:941-960` (pin + handles), `trace:989-991` (first destination-aware choice keeps the live relay), `trace:994-996` (dual registration; one superseded).
- Transitions without teardown: `trace:1239-1253`, `trace:1278-1288`.
- Starvation: `trace:1307-1309`, `trace:1320-1321`, `trace:1331-1332`.
- Stop and failed restarts: `trace:1340-1349`, `trace:1350-1365`, `trace:1369-1384`.
- Quit/pin release: `trace:1396-1400`.
- Manual Stop/Optimize cycles with no traffic: `trace:617-731`, `trace:670-785`.
- No-game-packets warning: `trace:828`.

## Appendix B — How to re-derive

```powershell
# time convention
# log timestamps are UTC; host is UTC+07:00
$ev = "wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log"
(Get-Content $ev | Measure-Object -Line).Lines          # 1400
Get-FileHash $ev -Algorithm SHA256                      # see header

# key events with original line numbers (ANSI stripped)
# see wat/evidence/2026-11-23/stability-audit/game-transition-key-events.log

# code anchors
Select-String -Path client-gui/src/app.rs -Pattern "Keeping the live session|Pinned relay|Game exited|start interception"
Select-String -Path client/src/interceptor/windows.rs -Pattern "Locked server stale|Shutdown signal received"
Select-String -Path client/src/quic/mod.rs -Pattern "control-plane registered|control-plane registration failed"
Select-String -Path client/src/engine.rs -Pattern "registration_error|Generation|spawn_registration"
```

No command in this audit starts/stops the GUI, touches WinDivert/WFP, or modifies the machine. All reads are of the worktree, the evidence log, and `%LOCALAPPDATA%\Lightspeed\gui-trace.log`.
