# GitHub backlog triage — 2026-11-17

| | |
|---|---|
| **Scope** | All open issues (7) and discussions (10) on `ShibbityShwab/lightspeed`; the documentation half of the game-request backlog; maintainer replies, one label change, one discussion closed |
| **Method** | `gh issue list/view`, GraphQL `discussions` read, full thread reads; every reply checked against `docs/` before posting; `gh issue comment`, `gh issue edit`, GraphQL `addDiscussionComment`/`closeDiscussion` |
| **Baseline** | `master` at `ba35558`; latest release **1.11.0** (2026-11-16); all eight relays `control_alpn = required` (`docs/quic-alpn-rollout.md` status) |
| **Outcome** | 10 comments posted (3 issues, 7 discussion threads), #168 moved to `blocked`, #69 closed as resolved, docs/game-requests.md + docs/supported-games.md corrected to shipped reality, this record |

Raw state capture: `wat/evidence/2026-11-17/github-state-after.json`,
`wat/evidence/2026-11-17/discussions-after.json`; reply texts as posted:
`wat/evidence/2026-11-17/replies/`.

---

## 1. Issues (7 open)

| # | Title | Kind | State after | Action taken | Blocker / next fact |
|---|---|---|---|---|---|
| [#140](https://github.com/ShibbityShwab/lightspeed/issues/140) | QUIC control-plane registration fails with error 120 | bug | open, `bug` | **No new reply** — waiting on the reporter's evidence (Wireshark capture of UDP 4433, a clean-machine run, or a current-release `--test-control` output). The public reply remains an owner decision, and 1.11.0 changes the retest conditions (pre-1.11.0 clients now legitimately get alert 120 from `required` relays) | Reporter: path/responder evidence. Owner: sign-off on any further public reply |
| [#148](https://github.com/ShibbityShwab/lightspeed/issues/148) | Aion 2 | game profile | open, `blocked`, `game-profile` | **No new reply** — the blocking fact was named in the thread on 2026-11-16 with the exact command; the reporter's latest evidence (loopback `127.0.0.1:37300`, a `-p tcp` listing) still does not contain it | Measured **remote** gameplay UDP endpoint in the world |
| [#153](https://github.com/ShibbityShwab/lightspeed/issues/153) | Black Desert Online | game profile | open, `blocked`, `game-profile` | **No new reply** — blocking fact and command already posted 2026-11-16 | Measured gameplay UDP port range (form arrived as `Nz`/`LAX` region names); whether `BlackDesert64.exe` owns the gameplay sockets |
| [#166](https://github.com/ShibbityShwab/lightspeed/issues/166) | EA Sports FC 27 | game profile | open, `game-profile` | **No new reply** — the two confirmations were requested 2026-11-13 and are still unanswered | (1) that `21006-21039` is gameplay, not EA-app control traffic; (2) matchmade vs fixed servers (dynamic marker) |
| [#167](https://github.com/ShibbityShwab/lightspeed/issues/167) | Roblox relay return path starving | bug | open, `bug` | [Comment](https://github.com/ShibbityShwab/lightspeed/issues/167#issuecomment-6934094785): retest on 1.11.0 (the line pasted is from 1.8.4), naming the two log lines and the discriminator | Reporter retest: the `Relay return path is starving … errors=… last_error=…` line and the `WinDivert recv failed …` / `WinDivert receive handle is invalid (os error 6); interception stopped` line, plus whether the **Relayed** down count rises while falling back |
| [#168](https://github.com/ShibbityShwab/lightspeed/issues/168) | Delta Force | game profile | open, `blocked` (label added), `game-profile` | [Comment](https://github.com/ShibbityShwab/lightspeed/issues/168#issuecomment-6934095946): corrected the `netstat` interpretation (unconnected UDP sockets do not show a remote endpoint) and named the app-trace measurement; [label](https://github.com/ShibbityShwab/lightspeed/issues/168) `blocked` added | Measured gameplay endpoint from the app trace (`Locked game server: <ip>:<port>` in `gui-trace.log`); if the trace resolves no server while packets are captured, the gameplay is TCP and #176 is the tracker |
| [#176](https://github.com/ShibbityShwab/lightspeed/issues/176) | TCP support for Tibia | feature | open, `enhancement` | [Comment](https://github.com/ShibbityShwab/lightspeed/issues/176#issuecomment-6934095397): the TCP tunnel path is shipped (protocol v5, client terminator, relay splice; Minecraft Java Edition is the built-in TCP profile); the shipped Tibia profile is UDP `7171-7172` so it cannot accelerate Tibia; the gap is a general TCP-only profile/exposure path (custom profiles reject `tcp_ports`, TCP-primary auto-detection is not exposed in the GUI); no dates | Product work: general TCP-only profile path; this issue is the tracker |

Closed issues verified in this pass: **#147** closed as duplicate of #140 (2026-11-16), **#59** closed completed (2026-11-16), **#171** Tibia closed completed (2026-11-13) and superseded by #176's TCP correction.

---

## 2. Discussions (10)

| # | Title | State after | Action taken | Blocker / next fact |
|---|---|---|---|---|
| [#1](https://github.com/ShibbityShwab/lightspeed/discussions/1) | Welcome to LightSpeed — v0.3.0 Released | open (announcement) | **No action** — historical announcement | None |
| [#2](https://github.com/ShibbityShwab/lightspeed/discussions/2) | Introduce yourself + where are you playing from? | open | **No action** — community intro thread, one member reply; not a support question | None |
| [#49](https://github.com/ShibbityShwab/lightspeed/discussions/49) | please help | open | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/49#discussioncomment-19666686): links to install, user guide, FAQ and troubleshooting docs; asked for OS/version/log path if those do not cover it. Left open (no reporter follow-up yet) | Reporter: whether the docs answered it |
| [#63](https://github.com/ShibbityShwab/lightspeed/discussions/63) | suggested games to support | open | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/63#discussioncomment-19666683): per-game status (Battlefield 6, Warface, Delta Force, Aion 2), what each is blocked on, pointer to `docs/game-requests.md`; nothing promised | Per game: measured gameplay UDP port range + matchmaking |
| [#69](https://github.com/ShibbityShwab/lightspeed/discussions/69) | Help | **closed (resolved)** | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/69#discussioncomment-19666687) + [closed as resolved](https://github.com/ShibbityShwab/lightspeed/discussions/69): v1.4.2 replaced the hardcoded loopback placeholders with signed-registry discovery; docs/community-network.md linked | None — reopen if a current build still shows two relays |
| [#142](https://github.com/ShibbityShwab/lightspeed/discussions/142) | Please add Deadlock | open | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/142#discussioncomment-19666680): SDR status, per-game status doc link, the one fact that decides it | Whether Deadlock gameplay runs over Steam Datagram Relay |
| [#146](https://github.com/ShibbityShwab/lightspeed/discussions/146) | Add Custom Game (Server) be Saved | closed (answered) | **No action** — both parts were answered and closed on 2026-11-12 (saved custom server entries + process targeting shipped) | None |
| [#149](https://github.com/ShibbityShwab/lightspeed/discussions/149) | This fixes a common global login issue in Aion 2 ! | open | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/149#discussioncomment-19666685): Aion 2 not in the profile list, tracked by #148 (blocked on the measured endpoint), external Reddit link marked unverified, login-side TCP is outside the UDP gameplay path | Same fact as #148 |
| [#169](https://github.com/ShibbityShwab/lightspeed/discussions/169) | PLEASE ADD DEADLOCK | closed (duplicate) | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/169#discussioncomment-19666681): confirms the duplicate closure and points at #142's status; added because the original request stated the user would follow the canonical thread | Same fact as #142 |
| [#172](https://github.com/ShibbityShwab/lightspeed/discussions/172) | Pls add the game BF6 | open | [Comment](https://github.com/ShibbityShwab/lightspeed/discussions/172#discussioncomment-19666682): the two measured facts a profile needs, Target-a-process fallback, pointers to #63 and the docs | Measured gameplay UDP range + matchmade-vs-fixed |

---

## 3. Documentation changes

| File | Change |
|---|---|
| [`docs/game-requests.md`](game-requests.md) | Open-request table refreshed to 2026-11-17: one status line per request with the exact missing fact, plus Roblox (#167) marked as not a profile request; new **TCP games** section describing the shipped TCP path (protocol v5, client terminator, relay splice; Minecraft Java Edition is the built-in reference profile), the remaining gap (custom profiles reject `tcp_ports`, TCP-primary auto-detection not exposed in the GUI) and #176 as the tracker with `docs/tcp-tunnel-design.md` as the design/implementation record; Tibia called out as a shipped **UDP** profile (7171-7172) that cannot carry Tibia's TCP game connection; the "cannot help" table and anti-cheat paragraph corrected to match |
| [`docs/supported-games.md`](supported-games.md) | The stale **"TCP-only games (not supported)"** section is replaced by **"TCP game traffic (partial support)"**: the shipped TCP path and the reference profile are described, the gap (TCP-only title intake, TCP-primary auto-detection, user-profile `tcp_ports`) is named and tracked in #176, and the design/implementation record is linked; Tibia and Minecraft Java Edition notes added; the community-requests section now gives a per-game status line pointing at `docs/game-requests.md` |

No numbers or claims were added that are not backed by the tree or an existing doc:
`1.11.0` is the current release (`CHANGELOG.md`), the fleet is 8/8
`control_alpn = required` (`docs/quic-alpn-rollout.md`), and the TCP path
(protocol v5, `TcpExtHeader`, client terminator, relay splice, built-in
Minecraft Java profile) exists in the tree.

---

## 4. Reply-consistency checks

- **No dates, no commitments.** No comment promises a release, a version or a date.
  "1.11.0 (the current release)" is the only version named, and it is a fact.
- **No contradiction with the docs.** The claims used in replies (TCP path shipped
  and its gap, Tibia profile is UDP-only, Target-a-process relays UDP only,
  process-targeting does not cover TCP, the 1.11.0 log lines and fault variants,
  eight discovered relays, custom server entries saved) are each backed by
  `docs/`, `CHANGELOG.md` or the tree.
- **No guessed facts.** Requests still missing their measured port/mode fact were
  not given one; #168's `netstat` reading was corrected rather than accepted.
- **Labels.** After this pass: #148 `blocked`+`game-profile`, #153
  `blocked`+`game-profile`, #168 `blocked`+`game-profile`, #166
  `game-profile`, #176 `enhancement`, #167/#140 `bug`.
- **No game request closed.** #168 stays open under `blocked`; #166/#172/#63/#142
  stay open with an actionable reporter fact. Only the Q&A thread #69 was closed,
  and it already had an accepted answer.

---

## 5. Not actioned, and why

- **#140 / #147 (QUIC alert 120).** #147 is closed as a duplicate. #140 waits on
  the reporter's path evidence; a further public reply is an owner decision, and
  after the 1.11.0 rollout the retest conditions changed (a pre-1.11.0 client
  against a `required` relay now gets alert 120 by design), so any reply needs
  the current client named.
- **#148, #153, #166.** The blocking fact was already named in the thread with the
  exact command; repeating it would be noise. They move when the reporter posts
  the measurement.
- **#146, #2, #1.** Already answered / not support threads. No reply added.
- **Recording.** Every action above has its URL in this file; replies are stored
  verbatim under `wat/evidence/2026-11-17/replies/`.

---

## 6. Verification

- All actions were confirmed by GitHub API read-back, not by the command's exit
  code alone: the open-issue list shows #168 with `blocked`+`game-profile`, the
  discussions query shows the new comments as the latest reply on each thread and
  #69 `closed: true`. Raw captures:
  `wat/evidence/2026-11-17/github-state-after.json`,
  `wat/evidence/2026-11-17/discussions-after.json`.
- Documentation guard: `cargo test -p lightspeed-client
  the_supported_games_page_lists_every_registry_entry` -> **1 passed, 0 failed**
  in the target that contains it (624 filtered), 1 passed, 0 failed in the second
  target that also links the page check (614 filtered), 0 tests elsewhere.
- Game-registry suite: `cargo test -p lightspeed-client games` -> **54 passed,
  0 failed** (571 filtered). The first run failed two temp-directory tests with
  `Os { code: 5, kind: PermissionDenied }` because this host's sandbox denies the
  default `%TEMP%`; rerunning with `TEMP`/`TMP` pointed at a writable directory
  passed all 54, so that is environmental, not a regression from this change.
- No code, workflow or Rust file was changed by this task, so the cargo
  fmt/clippy/test gates over the workspace are not applicable to this pass. The
  working tree also carried concurrent teammate changes (release workflow, infra
  scripts, `client/src/route/destination.rs`, `docs/architecture.md`) that this
  record does not cover.
- **Unverified here:** reporter-side claims (network captures, in-game ports),
  the owner's decision on any further #140/#147 reply, and the exact behaviour of
  a TCP-only game on a live Windows host (the TCP path's purpose and gap were
  taken from the tree and docs; no live TCP-only game test was run in this pass).
