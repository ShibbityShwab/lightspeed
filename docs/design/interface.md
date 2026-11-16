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
   real window: narrow (~435 px) and wide (~1270 px), boosting and idle.
5. **Record the decision** next to the code or in `wat/state/decisions.md` so the next
   session does not relitigate it.

## State contract

**Eleven route states**, plus the four server-leg states below. Each row has exactly one driving fact and the sentence the window must
show in that state. A state the client cannot observe must not be shown (rules below); a
state the client can observe but the window has no room to name is still listed here, with
the trace line that carries it.

| State | What the window must say | Driving fact (single source) |
| --- | --- | --- |
| Idle | Not optimizing, relay connected or not | engine status |
| Connecting | Relay being reached | control plane |
| Holding | Traffic stays direct until a relay is chosen for the locked server | `direct_fallback` + `sent == 0` (`is_holding`) |
| Choosing | A relay is being ranked for the server | destination-aware chooser |
| Relaying | Route = selected relay; `Relayed` shows sent and returned in both directions | interceptor counters |
| Direct fallback | `DIRECT`, with `Direct fallback` as the route line and the ledger's `Path`; the relay was abandoned, and why | watchdog verdict: the same fault held for `RELAY_REPLY_STREAK` = 2 consecutive judged windows, each with >= `RELAY_REPLY_MIN_INTERCEPTED` = 20 intercepted and a return share below one in `RETURN_SHARE_DENOMINATOR` = 8 - unless the server's own flow pair shows a working share. `relay_reply_fault` + `ReplyFaultStreak` are the only place this is derived |
| Re-test pending | `DIRECT`, route line `Direct fallback · retry in {secs}s`; the relay row still names the abandoned relay | `relay_retry_at`, the watchdog's `ReplyFallback.due`, published only while `retry_after_fallback` is on; at zero the hold lifts and the **same** relay is judged again - no re-rank, no interception restart |
| Server rejects relay | `DIRECT`, route line and ledger's `Path` = `Server rejects the relay` | `relay_rejection`: the re-tests are spent (`RELAY_RETRY_BACKOFF` exhausted, or retries switched off); only a chain that returned nothing at all on every window is also remembered as a relay failure |
| Relay recovered | The normal `Relaying` sentence: the chain is closed and nothing is remembered as a failure. The recovery itself is trace-only - the window must not invent a "recovered" banner | a judged window with a working return share (>= 1 reply in 8) closes the fallback chain; trace line `The relay is returning this server's traffic again; the fallback chain is closed` |
| Needs elevation | The action is to restart as administrator | platform check |
| Error | What failed, in the engine's own words | engine error |

The fallback surface reads top to bottom as one story: traffic goes direct
(`Direct fallback`), a countdown says when the same relay is re-tested (`Re-test pending`),
a healthy window ends the story (`Relay recovered`), and only a chain that never returned
anything at all and spent its retries becomes `Server rejects the relay`.

### Server-leg states (added 2026-11-16, task-14)

The relayed path has two halves and the client only measures one of them. The
`Server leg` ledger row (between `Server ping` and `Relay RTT`) names the
relay-to-game-server leg and, in the same breath, how the relay choice obtained
it. There is exactly one driving fact: the ranking row for **the relay in use**
(never the staged winner), plus the v1 estimator's published measurement when one
exists. The window does not derive the leg from the geo table or from the
choice's total a second time.

| State | What the window must say | Driving fact (single source) |
| --- | --- | --- |
| Server leg measured | `measured {ms} ms · n={n}` (with `· last {secs}s` once the estimator reports a sample age) | the v1 quiet-flow estimator published an estimate for `(relay in use, game server)`: `destination_estimator().estimate()` - the same "publishable or None" gate the ranking's `Measured` source uses |
| Server leg bounded | `at least {ms} ms · triangle bound` | the ranking row for the relay in use has `dest_source = GeometricFloor`: `direct - client leg` is a proof of a lower bound, not a measurement |
| Server leg not measured | `not measured · prior ~{ms} ms ±{unc} ms` | the ranking row for the relay in use has `dest_source = RegionPrior`; `{unc}` is `PRIOR_UNCERTAINTY_US` (25 ms), the same charge the ranking added to the whole-path total |
| Server leg unknown | `not measured · no region prior` | the ranking row is `Unknown`, or the estimator says `Measured` but publishes nothing the surface can show |
| Relay cannot beat direct | caption: `This relay cannot be faster than direct for this server` | `L1 >= L3d`: the keepalive's one-RTT client leg against a fresh ICMP direct ping. A triangle-inequality proof from two measurements the client owns |

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
  the header's metric, the ranking's triangle floor and this verdict. A stale
  ping may not floor a candidate, decide a verdict, or be displayed as current.

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
  leg to the relay, not the game's own ping.
- Counters show both directions. A sent-only counter reads as health when the return path
  is dead.
- The window may claim only what the client measures. `Relayed` counts datagrams the client
  intercepted and sent; the client cannot see what the relay did with them afterwards, so
  no sentence may claim the relay's forwarding is healthy. The relay verdict line reports
  the outbound leg only ("the outbound leg is clean"), never the relay's delivery.
- A state the engine cannot distinguish must not be invented by the UI. `Relay recovered`
  is trace-only for exactly this reason; the window falls back to the `Relaying` sentence.

## Layout spec (decided 2026-11-14)

The window must earn its pixels at every size, and no element may be pushed out of
view by a size change.

**Breakpoints**

| Width | Layout |
| --- | --- |
| < 380 px | single column, labels above values, card full width |
| 380-619 px | single column, label/value rows side by side |
| >= 620 px | two columns: status left (~58%), route/config right (~42%) |
| height < 560 px | short mode: drop the sparkline, collapse the globe to 76 px, keep the ledger and the primary action in the top band |

**Bands, not fillers.** The state rail (state word + metric) is a fixed 34 px band
and never allocates the remaining height. The primary action is a fixed 48 px
control directly under it. Only the config region scrolls, and it scrolls
*below* those two bands, so the action can never be pushed off-screen.

**Minimums.** The window refuses to go below 360x420 (the viewport already
carries 320x400; raise it), and the content column refuses to go below 320 px
wide. If the space cannot hold a control, the control moves to the next row
instead of being clipped.

**Never hidden.** A control that does not fit is a layout bug: it moves, or the
window enforces a larger minimum. Clipping is not an acceptable answer.

**Proof.** Two captures per change: 435x720 and 720x610. The second one is the
case that exposed the rail stretching to fill the window.

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
- The `Server leg` row's measured branch (`measured {ms} ms · n={n} · last
  {secs}s`) is wired to the v1 estimator's `estimate()` and `measurement_age()`
  (task-13). Whether it is reachable in a live run depends on the estimator's
  production feed: `Measured` needs ≥8 validated whole-path samples *and* ≥8
  client-leg samples for the same relay. If the feed produces nothing on a live
  session the row correctly shows the prior state, not an empty measurement.
- The seven new `route.server_leg*` / `route.cannot_beat_direct` catalog strings
  ship as English in all nine locales (documented untranslated fallback, decided
  2026-11-16). Translation debt, not a design decision.
- The whole-path ranking key is canonical in the engine since WF-065 task-17
  (`sort_scored`: total, jitter tie-break, address last); the GUI consumes that
  order and has no comparator of its own.

## Verification protocol

- Capture the real window at the widths the shipped build allows. Today
  `main.rs` pins the viewport to 320 px (min = max), so **the 320 px capture is
  the real one**; the 435/1270 px captures in the layout spec need a resizable
  build and are not reachable in this one. State that in the report rather than
  claiming a wide capture.
- Read the screenshot, not the code: the question is what a user sees, not what the
  bindings say.
- For status changes, force the state (start a boost, let the watchdog fall back) and
  capture it rather than trusting a unit test alone.
- A visual change ships with the state it produces listed in the pull request or commit.
- For the server-leg row the honest captures are: relayed **without** a measurement
  (the reachable state at this HEAD) and relayed **with** one only if the v1
  estimator has a production feed in the build being captured. Do not fake a
  measurement by injecting samples into the estimator: a screenshot of a state the
  shipping engine cannot reach is not evidence for that state.
