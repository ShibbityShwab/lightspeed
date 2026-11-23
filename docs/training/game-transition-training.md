# Training: game-server transitions, reconnection, and when NOT to restart WinDivert

**Audience:** in-house agents (support, QA, and engineers) triaging LightSpeed's Windows capture lifecycle.
**Companion:** `docs/audit/game-transition-state-machine-2026-11-23.md` (the audit that motivates this brief).
**Source of truth:** `master` `6e30a65` (1.12.5 pinned WinDivert). Paths are repository-relative; trace lines are from `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log` and times are UTC (host is UTC+07:00).

**Learning outcomes.** After this brief you can:
1. Name the five planes a live session spans and which one a change belongs to.
2. Classify a trace line as a server/region transition, a game exit, a presence-scan artifact, or a user action.
3. Say when a WinDivert restart is legitimate, and list the events that must never restart it.
4. Triage the three most common false alarms: the stale "Registration failed" card, "Keeping the live session ...", and a starvation warning.
5. Collect evidence that makes the next incident attributable.

---

## 1. The mental model: five planes, not one "session"

A running LightSpeed session is not one object. It is five layers that change for different reasons:

| Plane | What lives there | Typical events | Where the code is |
|---|---|---|---|
| **Presence** | "is the game process running?" (5 s process scan) | game launched, game closed, scan failed | `client-gui/src/app/presence.rs`; `client-gui/src/app.rs:2063-2163` |
| **Capture** | WinDivert driver pin + per-run intercept/inject/shadow handles + the hold gate | start, stop, fatal handle fault | `client/src/interceptor/windows.rs`; `client/src/engine.rs:1579-2096` |
| **Destination** | the locked game server (the endpoint the game's UDP flow goes to) | lock, stale, re-lock to a new server | `client/src/interceptor/order.rs`; `client/src/interceptor/windows.rs:1492-1559` |
| **Session/route** | current relay, per-relay token, QUIC control connection(s) | relay choice, registration, re-registration, token rotation | `client-gui/src/app.rs:910-1263`; `client/src/quic/mod.rs`; `client/src/engine.rs:704-876` |
| **Presentation** | the word (OPTIMIZING/ERROR/...), card, action button, tray | any of the above, rendered | `client-gui/src/app/route_surface.rs` |

The single most important rule: **a destination change is plane 3 and a route change is plane 4. Neither may move plane 2.** Today the code mostly respects that at the interceptor layer, and fails at the presentation layer (a plane-4 error can repaint the word as plane-2 failure[^1]).

[^1]: `client-gui/src/app/route_surface.rs:289-307` gives `Error` precedence over `Optimizing`; the registration error is a plane-4 fact.

### 1.1 The game's own phases (why a "server" changes at all)

Multiplayer games normally go through: **auth/login service -> main menu/lobby -> matchmaking -> match server -> (optional) server reassignment** (map change, settlement/POI switch, extraction, region rebalance). Different phases can use different hosting providers and regions. Titles with player-hosted or dynamic servers (Dota 2, CS2, ROB RUN) assign a new endpoint, often a new port, per match — the ROB RUN profile documents this at `client/src/games/robrun.rs:1-26, 42-60`.

LightSpeed cannot know "match started". It infers a game server from UDP flows on the profile's port band, attributable to the game process. What it calls a *locked server* is the best current inference, not a game fact (`client/src/interceptor/order.rs:19-54`).

Consequences:
- An auth/menu/lobby flow can be the first lock. A later match server is a *new lock*, not a restart.
- Silence for 5 s marks the lock stale (`order.rs:100-110`) even during a loading screen or a menu pause.
- A game can legitimately move servers several times in a minute (the trace shows three servers in 60 s: `trace:978, 1245, 1280`).

---

## 2. The signals: how to read a transition in the trace

### 2.1 The transition fingerprint (server/region change; capture must NOT restart)

```
🔄 Locked server stale - re-detecting
🔍 Locked game server: <new ip:port>
Destination-aware relay choice for <ip>: <relay> (...)
Keeping the live session on <current relay>; <candidate> is better for this server and applies next session
Connecting QUIC control plane to <relay>:4433
QUIC connection established relay=<relay>:4433 ...
Registered with proxy: session=..., token=..., node=..., region=...
[control-plane registered relay=... token=...]      # only when this generation is current
```

- Produced by: `client/src/interceptor/windows.rs:1549-1559` (stale), `:1492-1500` (new lock), `client-gui/src/app.rs:1055-1061, 1250-1252` (choice/keep), `client/src/quic/mod.rs:1262-1273` (destination re-registration).
- **No `WinDivert handle close` and no `Shutdown signal received` may appear between the stale line and the next second.** If one does, that is a defect (invariant I1 in the audit).
- The relay sentence "...applies next session" means the session deliberately did not move the relay. It is not an error and it is not a failure to log.

### 2.2 Game exit (capture SHOULD stop, after a debounce)

```
Game presence changed: none
Game exited; stopping the relay it was using
🛑 Shutdown signal received
```

- Produced by `client-gui/src/app.rs:2156-2159` then the engine stop path.
- Today a *failed process scan* also produces `none`, because `try_auto_detect_game` maps any error to `None` (`client-gui/src/app/choice.rs:57-73`) and process enumeration returns an empty list on failure (`client/src/games/mod.rs:661-691`). Treat a single `none` with no other evidence as suspect: a scan failure is not an exit (see §4.2).

### 2.3 A user action (the trace used to be silent; it must not be)

- A Stop is only logged as `🛑 Shutdown signal received` (`client/src/interceptor/windows.rs:1321`) followed by handle closes. It has **no source** today. A Quit also logs `Quit requested - stopping engine` (`client-gui/src/app.rs:4389`), an elevation restart logs `elevation handshake:`, an update logs its own lines. If you see the bare teardown with none of those, it came from a UI action (Stop button, the toggle, tray Disconnect, manual-redirect switch) or from a start that pre-stopped a live run.
- Optimize has no GUI log line; the next engine line is `ProcessScanner: found ...` or `Opening WinDivert intercept handle`.
- This is why "the transition stopped my capture" is usually a misreading: look for the actual action evidence before blaming the transition.

### 2.4 Presence-scan artifacts (noise)

`🎮 Auto-detected game: ...` every 5 s is the **scan** heartbeat, not a transition (`client/src/games/mod.rs:548-553`). A presence *transition* logs `Game presence changed:`. In a 60 s window there are ~12 scan heartbeats and one transition; do not count heartbeats as flicker.

### 2.5 Control-plane registration (route plane; capture unaffected)

Two registration sources exist:
1. **Start registration**: the engine awaits the supervisor's first attempt and writes `control_registered`/`registration_error` (`client/src/engine.rs:856-876, 2217-2232`).
2. **Destination-report re-registration**: each new locked server calls `quic::report_destination`, which retires the relay's supervisor and starts a new generation (`client/src/latency.rs:1526-1529`; `client/src/quic/mod.rs:1145-1218`). This one does **not** write `EngineStatus`.

A start can therefore show two `Connecting QUIC` lines ~0.2 s apart and two `Registered with proxy` lines, with only one `control-plane registered`. That is the fingerprint of the stale-card defect: the first (superseded) generation "fails" from the engine waiter's point of view while the second registers successfully and no one clears the status. See §3.1.

### 2.6 Starvation watchdog (route plane warning; capture unaffected)

```
Relay reply window judged: intercepted=N injected=0 from_relay=0 ... verdict=no_reply_from_relay
WARN Relay return path is starving the session; keeping the relay and warning the player (no automatic path switch)
INFO The return-path watchdog found no reply ... the relay is demoted for the next ranking ...
```

- Judged in 8 s windows with a minimum of 20 intercepted datagrams; the warning needs the fault twice in a row (`client/src/app/watchdog.rs:40-75`).
- The relay is **not** switched automatically (owner decision 2026-11-20; `client/src/app/watchdog.rs:190, 227-248`). The demotion only changes a future ranking.
- A starvation warning is not a reason to restart WinDivert. It is a reason to collect relay/server evidence and, when the in-place failover exists, use it.

### 2.7 FWP failures (capture plane; restart questions become real)

```
WinDivert handle open: id=N kind=intercept attempt=1 ... -> FAILED (The object is referenced by other objects ... os error -2144206838)
❌ WinDivert intercept open failed: ... A previous LightSpeed run left its WFP filter/callout registered ...
ERROR start_interceptor failed: AppError { kind: Tunnel, ... }
```

- `-2144206838` is `0x8032000A`, `FWP_E_IN_USE`.
- With the 1.12.5 pin, `FWP_E_IN_USE` is not retried while the pin is held: it is treated as external stale state (`client/src/interceptor/windivert_handle.rs:320-329, 614-656`).
- The remedy text tells the user to close a stale process and restart Windows. That is correct for a genuinely leaked filter from another process and **wrong** when this process has just closed its own handles.

---

## 3. The three false alarms you will be asked about

### 3.1 "Registration failed" card while traffic is being relayed

**What the user sees:** the ERROR word + "Registration failed" + a STOP OPTIMIZING button, while the Relayed row is moving.

**What it means:** a registration error is in the presentation state while the route is actually healthy. Two known mechanisms:
1. A start where the destination report superseded the supervisor before its first attempt completed; the superseded generation's `Done(0)` was written as a registration failure, and the replacement's success was never written (`client/src/quic/mod.rs:1405-1409` vs `:1350-1362`; `client/src/engine.rs:2217-2232`). This is confirmed in the 2026-11-23 trace (`trace:986, 994-996`).
2. A failed command whose text survives later successes (`client-gui/src/app.rs:2937-2941`; `client/src/app/commands.rs:283-304`); the 1.12.5 clear (`client-gui/src/app.rs:1535-1538`) only covers the narrow registration-text case and only while `control_registered` is true.

**What to do:** do not tell the user to restart. Capture the trace section, note whether `control-plane registered` exists for the *surviving* generation, whether two `Connecting QUIC` lines occur around a start, and whether the card predates the last successful registration. File it under the stale-registration defect (F1/F5).

**Do not:** stop/restart the interceptor to "clear" it. A restart is the wrong plane and, after an active session, is the action that can hit §2.7.

### 3.2 "Keeping the live session ... applies next session"

**What it means:** the destination-aware chooser found a relay that is better for the new game server, but the tunnel is live, so it kept the current relay (`client-gui/src/app.rs:1124-1135, 1240-1253`). This is by design today, and it is why a match after a region shift can run on the pre-match relay.

**What to do:** confirm the gate is `Some(true)` (tunnelling enabled) at the time of the choice; note the candidate and whether it differs; this is a product gap (F3), not a fault. The in-place primitive exists (`commands.switch_relay` -> `engine.switch_relay`, `client/src/engine.rs:745-783`) but the GUI does not call it.

**Do not:** suggest Stop -> Optimize as the remedy in triage notes. That is exactly the transition that can fail on this host (F2); it is the reason the user's session ended.

### 3.3 Starvation warning

**What it means:** this client saw relayed datagrams leave and no reply data come back for 8 s windows, twice (`client/src/app/watchdog.rs:40-75`). It can be a bad relay path, a server that does not reply to the relayed flow, or a wrong lock (a flow that is not the match server). It is not by itself a capture fault.

**What to do:** record `(server, relay)`, the counters, whether `injected=0` and `from_relay=0`, and whether the locked server was stable. Check the relay-side logs if available. Note that the watchdog already demoted the pair for future rankings.

**Do not:** stop/restart WinDivert or the app. It changes nothing about the route; the session keeps relaying (warn-only).

---

## 4. When NOT to restart WinDivert

### 4.1 Never restart capture for these

- A new locked game server (settlement, POI, map, region, lobby -> match).
- A stale/re-detect cycle, including one that re-locks the same or a different server.
- A relay choice, a "keeping the live session" decision, a relay demotion, a starvation warning.
- A control-plane reconnect, a registration attempt/failure, a token rotation.
- A presence-scan heartbeat, and **a single** failed scan (treat as unknown; confirm with a second scan or with the process table).
- A stale registration/error card, a word change, a tray state change.
- A game loading screen or menu pause that goes over the 5 s stale timeout.

### 4.2 Legitimate restart triggers

- The user explicitly asks (Stop, then Optimize; Quit).
- The app is about to exit or hand over to an elevated successor or the updater.
- The capture data plane reported a fatal platform failure and `is_active()` is false (`client/src/interceptor/traits.rs:776-784`).
- The teardown latch is set (an unclean Stop refused a later start). The documented remedy is a process restart; do not stack attempts.
- A configuration change that genuinely requires new handles (different game/process target or port policy). Even then, prefer an explicit user action and one command.
- A game process restart: the tracker clears the lock itself (`client/src/interceptor/order.rs:420-453`); the capture handles stay.

### 4.3 The decision matrix

| Observation | Restart capture? | Why |
|---|---|---|
| `Locked server stale - re-detecting` + new lock | **No** | destination plane only; same run continues |
| `Game presence changed: robrun` (game launched) | Only if capture is off and autocapture is on | that is the start path, not a restart |
| one `Game presence changed: none` then `robrun` | **No** | suspect scan artifact; verify (F7) |
| `Keeping the live session ...` | **No** | route decision kept for next session |
| `control-plane registration failed` / stale card | **No** | route/presentation; capture is fine |
| `Relay return path is starving ...` | **No** | route warning; warn-only by design |
| `FWP_E_IN_USE` on start after a Stop | **Only a bounded start retry; never a driver/WFP reset** | see §2.7; restart Windows only when the condition survives the budget and cannot be attributed to this process |
| `teardown did not complete within 1500ms` | Process restart required | fail-closed latch |
| normal WinDivert close/open messages | no | that is the lifecycle itself |

---

## 5. Triage playbook

1. **Get the trace.** `%LOCALAPPDATA%\Lightspeed\gui-trace.log` (rotating; copy it immediately), plus any evidence bundle and the GUI version. Note the host time zone: the log is UTC.
   **Check build identity first.** The trace has no build stamp and can mix binaries; a line shape that committed code cannot emit means that segment is untrustworthy for behavior claims (in the 2026-11-23 slice, `WinDivert handle open retry ... pin_active=true` at 06:01/06:03 cannot come from 1.12.5, whose policy is `!pin_active`). Scope the segment you cite to the process/session that produced it and say so.
2. **Build a timeline with anchors.** Search for `presence changed`, `Locked server stale`, `Locked game server`, `Destination-aware relay choice`, `Keeping the live session`, `Connecting QUIC`, `control-plane registered`, `Relay return path`, `Shutdown signal received`, `WinDivert handle open`, `start_interceptor failed`, `Quit requested`.
3. **Classify each event by plane** (§2). If two different planes appear in one sentence in the user's report, split it.
4. **Check the invariants.** During a destination change: zero handle closes. During a registration success: no ERROR word for that same condition. During a starvation warning: capture still active, relay still the same.
5. **Check the action evidence.** A bare `Shutdown signal received` with no Quit/elevation/update line means a UI action or a start pre-stop; do not attribute it to a preceding transition without evidence. (This is the current trace's lesson: the transitions at 06:47:59 and 06:48:34 closed no handles; the Stop at 06:49:38 was a separate action 64 s later.)
6. **If FWP:** record time since the previous clean close, whether the pin was held, the handle ids, and whether traffic was flowing at Stop. Ask for the same run with `LIGHTSPEED_...` diagnostics if available; do not touch WFP/WinDivert on the user's machine.
7. **Write the evidence note** with trace line numbers and UTC times, state what is confirmed vs. hypothesis, and file against the right finding ID (F1-F11 in the audit).
8. **Do not claim a fix** unless a failing-before test exists and a mutation check was done; for GUI/live work, capture real-window evidence (see `lightspeed-verify`).

### 5.1 Fast trace anchors cheat-sheet

| Question | Search string | File:line of producer |
|---|---|---|
| Did the game exit or was it a scan blip? | `Game presence changed`, `Game exited` | `client-gui/src/app.rs:2095-2103, 2156-2159` |
| Did the server change? | `Locked server stale`, `Locked game server` | `client/src/interceptor/windows.rs:1549, 1499` |
| Did the relay change or not? | `Destination-aware relay choice`, `Keeping the live session`, `Relay chosen` | `client-gui/src/app.rs:1055, 1131/1251, 1261` |
| Did restart happen? | `Shutdown signal received`, `Opening WinDivert intercept handle`, `WinDivert handle close` | `client/src/interceptor/windows.rs:1321, 1172+, 865+` |
| Why did a start fail? | `start_interceptor failed`, `FWP_E_IN_USE`, `-> FAILED` | `client-gui/src/app.rs:1640`; `client/src/interceptor/windivert_handle.rs:498-521` |
| Registration truth? | `Registered with proxy`, `control-plane registered`, `registration failed` | `client/src/quic/mod.rs:486, 1358, 1391` |
| Starvation? | `Relay reply window judged`, `starving the session` | `client/src/app/watchdog.rs:346-380` |

---

## 6. Self-check

1. A user says "switching settlements restarts LightSpeed and then it can't start again". Which part of that is contradicted by the 2026-11-23 trace, and what evidence *is* missing? *(Answer: the transitions at 06:47:59/06:48:34 caused no handle closes; the Stop at 06:49:38 is 64 s later and is unattributable without an action log. Missing: the source of the Stop and of the two start attempts.)*
2. You see two `Connecting QUIC control plane` lines 0.2 s apart during a start and only one `control-plane registered`. What is your first hypothesis, and in which plane is the defect? *(Answer: start registration superseded by the first destination report; route/control plane writes a false failure into presentation — F1.)*
3. The Relayed row is moving, but the word says ERROR / "Registration failed", and the only button says STOP OPTIMIZING. What should the user do, and what should support do? *(Answer: nothing that restarts the capture; it is a presentation/route state bug. Support records the trace; the product fix is generation-scoped status and non-masking errors.)*
4. When is "restart Windows" legitimate? *(Answer: only when an `FWP_E_IN_USE`/stale-WFP condition is proven not to be this process's own recently-closed filter and a bounded in-process retry cannot clear it; the current copy over-claims.)*
5. Which plane must a new game server never touch? *(Answer: capture — no handle close, no start/stop; only destination/route/presentation update in place.)*

---

## 7. Glossary

- **Locked server**: the UDP endpoint the interceptor currently believes is the game's match server.
- **Stale lock**: 5 s without a packet to the locked server, after which the tracker re-detects.
- **Destination-aware choice**: relay ranking by full path (client leg + relay-to-server leg/prior), not by client leg alone.
- **Hold**: the start-of-match gate that withholds tunnelling until a relay is chosen for the locked server.
- **Supervisor / generation**: one per-relay control-plane task; a new destination replaces it with a higher generation. A retired generation must never publish state.
- **Registration**: the QUIC control-plane `Register`/`RegisterAck` exchange that issues the session token; different from the "Registered with proxy" data-plane line only in which layer logs it.
- **Pin**: the process-lifetime non-diverting WinDivert handle that keeps the driver loaded across Stop.
- **FWP_E_IN_USE** (`0x8032000A`): WFP object still referenced; the WinDivert open fails.
- **Plane**: one of presence/capture/destination/route/presentation (§1).
