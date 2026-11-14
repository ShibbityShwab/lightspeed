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

| State | What the window must say | Where it comes from |
| --- | --- | --- |
| Idle | Not optimizing, relay connected or not | engine status |
| Connecting | Relay being reached | control plane |
| Holding | Traffic stays direct until a relay is chosen for the locked server | `direct_fallback` + `sent == 0` |
| Choosing | A relay is being ranked for the server | destination-aware chooser |
| Relaying | Route = selected relay; sent and returned both visible | interceptor counters |
| Direct fallback | The relay was abandoned; why | watchdog verdict |
| Server rejects relay | Three spaced re-tests failed for this server | `relay_rejected` |
| Needs elevation | The action is to restart as administrator | platform check |
| Error | What failed, in the engine's own words | engine error |

Rules that follow from the contract:

- The headline word describes the **route**, never the intent. `OPTIMIZING` is only true
  while traffic is actually tunnelled.
- Any number shown as "ping" is labelled with what it measures. The headline `ms` is the
  leg to the relay, not the game's own ping.
- Counters show both directions. A sent-only counter reads as health when the return path
  is dead.
- A state the engine cannot distinguish must not be invented by the UI.

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

## Verification protocol

- Capture the real window with the `computer` tool at ~435 px and ~1270 px.
- Read the screenshot, not the code: the question is what a user sees, not what the
  bindings say.
- For status changes, force the state (start a boost, let the watchdog fall back) and
  capture it rather than trusting a unit test alone.
- A visual change ships with the state it produces listed in the pull request or commit.
