# Game request triage

How a "please add this game" request becomes a profile, a custom-process answer,
or an honest "this cannot be accelerated". This is the checklist behind
[the game request template](../.github/ISSUE_TEMPLATE/game_request.md).

## Why this exists

Every game request that stalled did so for the same two reasons: the gameplay
UDP port was unknown or guessed from a port-forwarding site, and nobody knew
whether match servers are assigned per match. The thread then becomes rounds of
"what port?" instead of work:

- #148 Aion 2 - fourteen comments, the port was still `37300 ?`.
- #153 Black Desert Online - the range fields arrived as `From: Nz` / `To: LAX`,
  which are regions, not ports.
- #168 Delta Force - the reported ports mix the game port with Steam's query
  block, and it is not clear which carries gameplay.

A built-in profile needs **measured** facts. Custom-process targeting needs none
of them, which is why it is the recommended answer whenever a profile is missing.

## The three facts a profile needs

| Fact | Why it matters | How to get it |
|---|---|---|
| **Gameplay UDP port range** | The capture filter is scoped to it; a guessed range captures nothing or the wrong traffic | `Get-NetUDPEndpoint -OwningProcess <PID>` (PowerShell) or `netstat -ano -p udp \| findstr <PID>` (cmd) while in a match; report the **remote** ports you see |
| **Process name** | Used for detection and process-scoped capture | Task Manager → Details |
| **Matchmade or fixed servers** | A matchmaker assigns a different host each match; the profile must be marked a dynamic server so each match re-detects its host instead of locking the first one | Ask the reporter; the profile's `dynamic_server` flag follows |

Anti-cheat is not a blocker: LightSpeed does not hook, inject, or modify game
processes. TCP-only gameplay *is* a blocker (the tunnel is UDP), as is Steam
Datagram Relay (see below).

## When a profile cannot help

| Situation | Answer |
|---|---|
| Gameplay runs over **Steam Datagram Relay (SDR)** - Valve titles such as CS2, Dota 2 and Deadlock can | No profile: the game already routes through Valve's network and we cannot chain into it |
| Gameplay is **TCP-only** | Out of scope; the tunnel is UDP. Login/patch TCP next to UDP gameplay is fine |
| Port range unknown and no reporter can measure it | Use **Target a process** in the app (custom process targeting), which relays that process's UDP without a profile |
| Kernel anti-cheat with a strict network policy | Usually fine (see `docs/supported-games.md`), but verify per game; never claim compatibility we have not tested |

## Open requests (2026-11-16)

| Game | Issue / discussion | State | Missing |
|---|---|---|---|
| Aion 2 | #148 | blocked | gameplay UDP port measured in a match (the posted `netstat` shows only a loopback UDP row and a TCP listing) |
| Black Desert Online | #153 | blocked | gameplay UDP port; asked twice, the form's range fields were regions |
| EA Sports FC 27 | #166 | near-ready | confirmation that `21006-21039` is gameplay (not EA app control traffic); matchmade servers likely (mark dynamic) |
| Delta Force | #168, discussion #63 | under evaluation | measured remote UDP endpoint; second ask posted 2026-11-16 |
| Battlefield 6 | discussion #63, #172 | under evaluation | port range and matchmaking behaviour |
| Deadlock | discussion #142 | blocked | SDR: if gameplay uses Steam Datagram Relay, a profile cannot help |
| Warface | discussion #63 | under evaluation | port range and matchmaking behaviour |

Update this table when a request moves; it is the public answer to "what is the
status of my game?".

## Turning a request into a profile

1. Confirm the three facts above are in the thread. Do not guess.
2. Add `client/src/games/<game>.rs` beside a similar profile; implement
   `GameProfile` (name, key, ports, process names, `uses_sdr`, `dynamic_server`).
3. Register it in the `GAME_REGISTRY` in `client/src/games/mod.rs`; the count
   guards and `docs/supported-games.md` test will tell you what to update.
4. Mark the issue `game-profile` (the only game label that exists; the template
   used to name a non-existent `game-request`, so old requests arrived
   unlabeled).
5. Ship it in a release note, close the issue, and update the table above.

## Duplicate and closure policy

- One open thread per game. Point duplicates at the canonical thread and close
  them (#147 is the same subject as #140).
- Close a fixed bug with the release that carries the fix and the evidence, even
  if the reporter never replies (#59 was fixed in v1.6.10).
- A request with no usable data after two asks stays open but moves to `blocked`
  with the missing facts named.
