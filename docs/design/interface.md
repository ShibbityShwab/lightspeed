# Interface design: LightSpeed desktop client

This is the working method and the state contract for `client-gui`. It exists because the
GUI's worst bugs have been **state bugs**, not pixel bugs: a window that said `OPTIMIZING`
while traffic was on the direct path, `No game detected` under a pinned game, and a
`Relayed` counter that only counted one direction.

## Method

1. **Inventory the states before touching layout.** Every state the client can be in, and
   what the user must be able to read in it (see below). A surface that cannot name its
   state is not ready to be designed.
2. **One source of truth per fact.** The window must never derive the same fact two ways.
   If the header and the ledger can disagree, one of them is lying.
3. **Hierarchy pass, in this order:** what is happening now, on what route, with which
   server, then the action, then history. Everything else is subordinate.
4. **Implement in egui, then look at it.** A change is not done until it has been seen in a
   real window: the pinned 320 px form at 420, 760 and 200 % scale, boosting and idle.
5. **Record the decision** next to the code or in `wat/state/decisions.md` so the next
   session does not relitigate it.

## State contract

_Amended 2026-11-20 by A1–A14; `docs/design/gui-refresh-spec.md` §2 is the
build contract of record for W2._

**Nine route rows** (amended 2026-11-20, A1–A7: three fallback rows retired, the `NO RETURN PATH` warning row added), plus the server-leg states below. Each row has exactly one driving fact and the sentence the window must
show in that state. A state the client cannot observe must not be shown (rules below); a
state the client can observe but the window has no room to name is still listed here, with
the trace line that carries it.

| State | What the window must say | Driving fact (single source) |
| --- | --- | --- |
| Idle | `NOT CONNECTED` when the engine is disconnected, `CONNECTED` when the relay link is up | `engine.connected`; the words are `state.disconnected` / `state.connected` (A2) |
| Connecting | Relay being reached while a start command is in flight | control plane: `session.in_flight` = `StartBoost` (A2) |
| Holding | Traffic is held while the route is chosen for the locked server | `EngineStatus.tunnelling_enabled == Some(false)` while a session runs; the `direct_fallback` field and `is_holding` are deleted on master (A1) |
| Choosing | A relay is being ranked for the server - a sub-state of the HOLDING word, not its own word | destination-aware chooser (`relay.choosing`; A4) |
| Relaying | Route = selected relay; `Relayed` shows sent and returned in both directions | the `run_boosting()` gate plus **one** counter-source pair (`RelayedPair`), never the interceptor counters alone (A5) |
| No return path | `NO RETURN PATH` warning while relaying; the route keeps being named and the session never changes route | `RelayStarvationWarning {server, relay, fault}` raised after `RELAY_REPLY_STREAK` = 2 consecutive same-fault judged windows and cleared only by a fully healthy window; per (server, relay) (A7) |
| Needs elevation | The action is to restart as administrator - an **overlay**, never a word | `Capabilities.elevated` (A6) |
| Error | A localized taxonomy sentence; the engine's verbatim message is one `Details` disclosure / log away | engine error (`error.<kind>` plus its detail; A3) |

The surface is **warn-only** (owner decision 2026-11-20, A7): while a session relays, a
confirmed no-return window raises the `NO RETURN PATH` warning, the route is never changed,
the relay is demoted for future ranking, and a fully healthy window clears the warning.
There is no fallback story, no countdown, no "recovered" banner and no rail word for it.

### Server-leg states (added 2026-11-16, task-14)

The relayed path has two halves and the client only measures one of them. The
`Server leg` ledger row (between `Server ping` and `Relay RTT`) names the
relay-to-game-server leg and, in the same breath, how the relay choice obtained
it. After A9 the **relay's own report** is the primary measured source and the v1
estimator is the fallback; the row always describes **the relay in use** (never the
staged winner). The bounded state has a second, earlier driver: the same triangle floor,
stated from a fresh direct ping once it lands rather than waiting for the next
ranking (`RELAY_RECHECK_INTERVAL` = 600 s, longer than most matches). The window
does not derive the leg from the geo table or from the choice's total a second
time, and it never re-ranks from the surface.

| State | What the window must say | Driving fact (single source) |
| --- | --- | --- |
| Server leg measured | `measured {ms} ms · n={n}` (with `· last {secs}s` once the estimator reports a sample age) | the v1 quiet-flow estimator published an estimate for `(relay in use, game server)`: `destination_estimator().estimate()` - the same "publishable or None" gate the ranking's `Measured` source uses |
| Server leg bounded | `at least {ms} ms · triangle bound` | **(i)** the ranking row for the relay in use has `dest_source = GeometricFloor` - the ranking itself proved `direct - client leg`; **or (ii)** before a re-ranking, a fresh direct ping (≤ `SERVER_PING_FRESH_SECS`) plus the relay's measured one-RTT client leg make the same triangle floor `L3d − L1` provable and stronger than the prior (and when `L1 ≥ L3d`, prove the relay cannot beat direct at all). Both drivers are the same provable verdict, never a measurement; (ii) re-ranks nothing and changes no relay |
| Server leg not measured | `not measured · prior ~{ms} ms ±{unc} ms` | the ranking row for the relay in use has `dest_source = RegionPrior`; `{unc}` is `PRIOR_UNCERTAINTY_US` (25 ms), the same charge the ranking added to the whole-path total |
| Server leg unknown | `not measured · no region prior` | the ranking row is `Unknown` (no provable floor either), or the estimator says `Measured` but publishes nothing the surface can show |
| Relay cannot beat direct | caption: `This relay cannot be faster than direct for this server` | `L1 >= L3d`: the keepalive's one-RTT client leg against a fresh ICMP direct ping. A triangle-inequality proof from two measurements the client owns |
| Server leg relay-measured (A9) | `Relay→server {ms} ms · measured n={n} · {secs}s ago`; when fail-open, `measured, unvalidated` in the same shape | the relay's own ICMP report for `(relay in use, game server)`; primary measured source, display-only |
| Relay→server (app) (A9) | `{ms} ms · n={n}` | the relay's passive app-RTT sample; secondary evidence, never blended into the ICMP figure |
| Route trace (relay) (A9) | `{hops} hops · last router RTT {ms} ms`, or `{hops} hops · no router answered` | the relay's TTL trace; a placement anchor, not a ping |
| Expected path (A9) | `~{ms} ms (L + S)` | a **lower bound** from the client leg plus the relay leg; never a whole-path verdict |

Rules for these states:

- The row and the caption appear only while a tunnel backend is carrying traffic
  **and** the route is the relay. On a direct fallback the `Path` row already says
  the relay is not in use, so a server-leg row there would describe a path the
  client is not on.
- The row describes the relay carrying traffic. When a better relay for this
  server is staged for the next session, the row still describes the current one.
- `Measured` is only ever the estimator's published value. `n` is shown because
  the estimator reports it; the sample age comes from `measurement_age()` and is
  rendered as `· last {secs}s` **only** when the estimator has a sample (the age
  is measured, not inferred - an invented `last N s` is exactly the kind of
  precision this contract exists to prevent).
- The window makes **no claim that relaying is better than direct**. The only
  whole-path claim it may state is the provable "cannot be faster than direct"
  verdict above. The choice trace line carries the arithmetic
  (`better than direct by N ms` / `not better than direct`) so the claim is
  auditable in the log, not asserted on an estimate.
- Direct-ping freshness is one rule (`SERVER_PING_FRESH_SECS`, 60 s) shared by
  the header's metric, the ranking's triangle floor, the early bound and this
  verdict. A stale ping may not floor a candidate, decide a verdict, or be
  displayed as current.
- The location ladder is display-only (A8): five mutually exclusive rungs Cloud →
  Geofeed → Hosting → Country → Unknown, each naming its provenance; hosting and
  unknown carry the unreliable-database hint, geofeed is shown as-is with source and
  as-of date. Only `chooser_point` (Cloud-exact) may feed the ranking; geofeed, hosting
  and unknown draw no dot, and no city is claimed that a source cannot produce.
- The client route trace is display-only (A10): the client's own OS traceroute to the
  game server names hops and the last responding hop's RTT only - never a city, never
  the game's ping. It runs once per (server, relay session) with a bounded deadline and
  never targets private or reserved addresses; it is an undisclosed network probe
  until the privacy documentation says otherwise (Q13).
- The update check is user-initiated only (A11): no startup or background check; it
  contacts the GitHub release API, sends nothing but the request, and the sheet states
  this. `UpdateStatus` is produced only by a user action.
- A direct ping is evidence about **exactly one server**. The ranking floor, the
  choice trace's direct comparison, the `Server leg` bound, the header metric and
  the `Server ping` row all use the ping only when it was measured against the
  server being described; a record for the previous server is treated as
  "direct ping not measured" and never reused (task-25: `follow_game_server` runs
  before `poll_relay_health` refreshes the store, so for one frame it can still
  hold the previous server's sample - observed live as a choice for
  `91.199.82.221` carrying `direct 281` from `91.199.82.162`). A ping whose
  server does not match may not floor a candidate, support a comparison, prove a
  bound, or be displayed as the current server's ping.
- The early (pre-ranking) bound is **not a second ranking rule**: it is
  `L3d − L1` from the same two inputs the ranking's `apply_floor` consumes, it
  only renders when it is stronger than the row it replaces, it re-ranks
  nothing, moves no relay, and it disappears back to the prior state if the ping
  goes stale. The relay choice is unchanged by it.

### Relay-choice inputs (changed 2026-11-16, task-14)

- **Client leg is one round trip.** `discovery.rs` probes each relay five times
  with a TCP connect (the handshake is one RTT) and reports the median and the
  tail spread, instead of timing a whole `GET /health` (handshake +
  request/response ≈ two RTTs). Measured against the same relays the HTTP wall
  time read 1.92-2.03x the TCP figure (`docs/server-leg-signal.md` §4.3), so
  every relay was charged twice for its own distance and the direct-ping floor
  could not bind. The picker's `N ms` and its hover ("median of five probes")
  now mean the one-RTT figure.
- **Ranking key is the whole-path total.** Candidates are compared on
  `client_leg + dest_leg + prior uncertainty` (the total the ranking already
  builds), with jitter only as a tie-break and the address as the final key.
  The destination leg is therefore a validity/uncertainty term, not an overriding
  50 ms bucket. Since WF-065 task-17 this key lives in the engine
  (`client/src/route/relay_choice.rs::sort_scored`) and the GUI consumes that
  order directly; the interim GUI re-rank is gone.
- **Trace line carries the provenance.** `Destination-aware relay choice for
  {server}: {relay} ({state}, total {n} ms{claim}) - {k} of {n} candidates raised
  to the direct-ping floor`, where `{state}` is `Measured(n=..)`,
  `GeometricFloor`, `RegionPrior(±25 ms)` or `Unknown`, and `{claim}` is the
  direct comparison above or `direct ping not measured`.

Rules that follow from the contract:

- The headline word describes the **route**, never the intent. `OPTIMIZING` is only true
  while traffic is actually tunnelled.
- Any number shown as "ping" is labelled with what it measures. The headline `ms` is the
  leg to the relay, not the game's own ping. It occupies **its own row** below the state
  word (A13); the direct ICMP ping lives in the `Server ping` detail row with its age
  and server key.
- Counters show both directions. A sent-only counter reads as health when the return path
  is dead.
- The window may claim only what the client measures. `Relayed` counts datagrams the client
  intercepted and sent; the client cannot see what the relay did with them afterwards, so
  no sentence may claim the relay's forwarding is healthy. The relay verdict line reports
  the outbound leg only ("the outbound leg is clean"), never the relay's delivery.
- A state the engine cannot distinguish must not be invented by the UI. The warning
  **clearing** is the observable effect of recovery (A7); there is no recovered banner,
  no recovered word and no fallback story to render.

## Layout spec (decided 2026-11-14; **mobile-only**, 2026-11-20)

The window is one fixed form: **width pinned at 320 px (min = max), one single column**,
height user-resizable **420-1400 px with an initial 760**, clamped to the work area. There
are no width breakpoints, no side-by-side form rows, no two-column layout and no wide
screenshots: the owner accepted the mobile-only product form on 2026-11-20 (A12).

**Structure.** The state rail is a fixed band (word row, then the metric on **its own row**,
then the sub-line) and never allocates the remaining height. The one action is a fixed
control below the scrolling region. At normal height the route card is fixed; below 560 px
the card and the sub-line fold into the single scrolling region, and below 480 px the footer
drops to the 24 px target tier. Only the config region scrolls, and the **scroll floor at
the 420 px minimum is 180 px** (measured worst case 181 with the fixed chrome at 239).

**Minimums.** The window refuses to go below **420 px** height (the pinned 320 width makes
the old 360 minimum moot: the 360x420 raise is withdrawn). If the space cannot hold a
control, the control moves to the next row instead of being clipped.

**Never hidden.** A control that does not fit is a layout bug: it moves, or the window
enforces a larger minimum. Clipping is not an acceptable answer.

**Proof.** Captures are the real pinned form: **320x420**, **320x760** and **320 pt at
200 % scale**, in EN, DE and RU (RU is the binding long-word case). The old 435x720 /
720x610 pair and the 435/1270 px capture set are permanently withdrawn.

## Ordering and one style system (proposed 2026-11-14)

Today the window stacks: state rail, primary action, map card with the ledger,
then Relay, Game, Target, then the footer. Two problems with that reading:

1. **The action comes before the choice it acts on.** You are invited to press
   OPTIMIZE before being told which relay and game it would use. The pickers are
   the inputs; the button is the outcome.
2. **Mixed containers.** The card, the pickers, the target row and the footer each
   compute their own margin and radius, so nothing lines up on a left edge except
   by accident.

Proposed order (top to bottom):

| # | Band | Why |
| --- | --- | --- |
| 1 | State rail (word, metric, sub-line) | What is happening, first |
| 2 | Map + ledger card | Where it is happening |
| 3 | Relay picker (with measured leg) | The first input |
| 4 | Game picker | The second input |
| 5 | Target a process (secondary, de-emphasised) | Optional path |
| 6 | Primary action | The outcome of 3-5 |
| 7 | Footer (Hide, Settings, Quit) | Never moves |

Proposed style system - one value per role, no local overrides:

- **Card**: one surface colour, R_CARD radius, S3 padding, 1 px border. The map,
  the pickers and the ledger all live on it.
- **Input**: one control height (CONTROL_H), one radius (R_CONTROL), one fill,
  one border. A dropdown and a button are the same object until pressed.
- **Margins**: only S1/S2/S3/S4, left edge aligned to a single gutter; nothing
  centres except the map.
- **Type**: three sizes here - STATE, BODY/LABEL, CAPTION. No ad-hoc sizes.

## Known issues (audited live, 2026-11-13/14)

Closed on 2026-11-14:

- The globe took a third of the card for one marker - now 92 px (76 narrow).
- The relay picker listed names only - it now shows each relay's measured client
  leg, marks the relay the destination-aware chooser picked, and measures on open.
- Settings held two items - it now carries Re-measure relays, the hold policy,
  and a Components block (version, elevation, capture, Npcap, relays).
- Capture was described twice, once as "pcap mode: not detected" - one statement
  remains, and Npcap says what it is for.
- The Proxy Manager row wrapped its delete button - every row is one line now.
- Nothing let a player opt out of relay re-tests - Settings has the switch, and
  the watchdog reads it live.

Open:

- **The window is often absent from the OS window list while a game has focus.**
  Automated clicks fail with a stale handle and a user cannot find the client
  either. `PrintWindow` captures it for evidence, which is a workaround, not a
  fix.
- The sparkline still has no axis, units or label.
- The Components labels are English-only by decision; revisit when the rows settle.
- The `Server leg` row's measured branch has two sources after A9: the **relay's own
  report** is primary (with `measured, unvalidated` when fail-open) and the v1
  estimator's `estimate()` / `measurement_age()` is the fallback (task-13). Reachability
  of the estimator branch still depends on its production feed (`Measured` needs ≥8
  validated whole-path samples *and* ≥8 client-leg samples for the same relay);
  with no report and no feed the row correctly shows the prior state.
- The delta's `route.server_leg*` / `route.cannot_beat_direct` catalog strings and the
  W2-2 shell keys ship as English in all nine locales until the Q5 translation
  pipeline lands (documented fallback; measured composition and the exact key list in
  `wat/evidence/2026-11-20/w2-2/locale-fallback.md`). Translation debt, not a design
  decision.
- The whole-path ranking key is canonical in the engine since WF-065 task-17
  (`sort_scored`: total, jitter tie-break, address last); the GUI consumes that
  order and has no comparator of its own.

## Verification protocol

- Capture the real window in its pinned form: **320x420, 320x760 and 320 pt @
  200 %**, in EN, DE and RU. The 435/1270 px widths are permanently withdrawn
(A12); there is no wide capture to claim.
- Read the screenshot, not the code: the question is what a user sees, not what the
  bindings say.
- For status changes, force the state and capture it rather than trusting a unit test
  alone. The starvation warning is warn-only: force the **warning** only with a
  starving fixture, never by inventing a fallback; otherwise mark the capture **U**.
- A visual change ships with the state it produces listed in the pull request or commit.
- For the server-leg row the honest captures are: relayed **without** a measurement,
  and relayed **with** the relay's own report where the fleet supplies one (A9; the
  v1 estimator is the fallback). Do not fake a measurement by injecting samples into
  the estimator: a screenshot of a state the shipping engine cannot reach is not
  evidence for that state.
