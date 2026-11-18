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
- #168 Delta Force - gameplay UDP and the matchmade endpoints are now measured
  (2026-11-17 process-target trace); the remaining blocker is the client's
  path-MTU handling on the reporter's network, not a missing fact.

A built-in profile needs **measured** facts. Custom-process targeting needs none
of them, which is why it is the recommended answer whenever a profile is missing.

## The three facts a profile needs

| Fact | Why it matters | How to get it |
|---|---|---|
| **Gameplay UDP port range** | The capture filter is scoped to it; a guessed range captures nothing or the wrong traffic | `Get-NetUDPEndpoint -OwningProcess <PID>` (PowerShell) or `netstat -ano -p udp \| findstr <PID>` (cmd) while in a match; report the **remote** ports you see |
| **Process name** | Used for detection and process-scoped capture | Task Manager → Details |
| **Matchmade or fixed servers** | A matchmaker assigns a different host each match; the profile must be marked a dynamic server so each match re-detects its host instead of locking the first one | Ask the reporter; the profile's `dynamic_server` flag follows |

Anti-cheat is not a blocker: LightSpeed does not hook, inject, or modify game
processes. Steam Datagram Relay is a blocker (see below). A game whose gameplay
itself runs over TCP needs the TCP game path - the built-in Minecraft Java
Edition profile is the reference profile for it - and new TCP-only titles are
tracked in #176 (see *TCP games* below).

## When a profile cannot help

| Situation | Answer |
|---|---|
| Gameplay runs over **Steam Datagram Relay (SDR)** - Valve titles such as CS2, Dota 2 and Deadlock can | No profile: the game already routes through Valve's network and we cannot chain into it |
| Gameplay is **TCP-only** | Needs the TCP game path: the built-in Minecraft Java Edition profile is the reference profile, and new TCP-only titles are tracked in #176 (*TCP games* below). Login/patch TCP next to UDP gameplay is fine - only the UDP gameplay is carried |
| Port range unknown and no reporter can measure it | Use **Target a process** in the app (custom process targeting), which relays that process's UDP without a profile |
| Kernel anti-cheat with a strict network policy | Usually fine (see `docs/supported-games.md`), but verify per game; never claim compatibility we have not tested |

## TCP games

TCP game traffic has a **shipped path**: protocol v5 carries a TCP stream through
the tunnel with a client-side terminator and a relay-side splice, so the game
server sees one ordinary connection. The built-in **Minecraft Java Edition**
profile (TCP 25565) is the reference profile for it. The design/implementation
record is [`docs/tcp-tunnel-design.md`](tcp-tunnel-design.md).

It is not yet a general "any TCP game" capability: the request and profile flow
is built around UDP gameplay, user/custom profiles reject `tcp_ports`, and
TCP-primary auto-detection is not exposed in the GUI. Built-in TCP profiles can
be added by the maintainers, and **Tibia** now uses that path (login/world TCP
`7171`/`7172`; select the game profile, not **Target a process**, which relays
UDP only). A new TCP-only title still needs the maintainer path tracked in
[#176](https://github.com/ShibbityShwab/lightspeed/issues/176).

## Open requests (2026-11-17)

| Game | Issue / discussion | State | Missing |
|---|---|---|---|
| Aion 2 | #148, discussion #149 | blocked (TCP gameplay) | the reporter's 2026-10/11 screenshots show the world connection is **TCP** (`ESTABLISHED` to `193.202.112.233:13328` plus TCP 443) and the only UDP socket is loopback `127.0.0.1:37300`, so there is no remote UDP path to profile; tracked by the TCP work in #176 |
| Black Desert Online | #153 | blocked | measured remote gameplay UDP port range; the form's range arrived as region names (`Nz` to `LAX`), and whether `BlackDesert64.exe` or a launcher owns the gameplay sockets is unconfirmed |
| Delta Force | #168, discussion #63 | measured, blocked by the client MTU bug | UDP gameplay confirmed by process targeting on 2026-11-17 (`43.130.90.129:28000`, `94.74.76.3:28000`, `43.164.65.4:28000`, `43.153.142.239:17869`); matchmade servers, so the profile must be dynamic. The reporter's `WSAEMSGSIZE` (10040) failures are the client path-MTU issue tracked in the 2026-11-18 audit (F-01), not a missing measurement. |
| EA Sports FC 27 | #166 | profile added (release pending; #166 closes with the release) | - |
| Battlefield 6 | discussion #172, #63 | under evaluation | measured gameplay UDP port range and matchmade-vs-fixed servers |
| Warface | discussion #63 | under evaluation | measured gameplay UDP port range and matchmade-vs-fixed servers |
| Deadlock | discussion #142 (#169 closed as a duplicate) | blocked | whether gameplay runs over Steam Datagram Relay; if it does, a relay profile cannot help |
| Tibia | #176 (UDP profile shipped in 1.10.0 from #171) | TCP profile shipped (release pending) | - ; select the Tibia game profile (TCP `7171`/`7172`), not Target a process |
| Roblox | #167 | not a profile request | a bug report about relayed Roblox traffic, tracked in #167 |

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
