# LightSpeed desktop UX audit — WF-081 W1 (revision r10, 2026-11-20 delta)

**Status:** W1 design input, **delta revision r10**. Docs only; no GUI code, config,
engine or fleet change was made by this revision. The W1 set is being re-baselined
after the program branch was rebased onto `origin/master` (`76aba5d`); this
document is re-baselined against that tree and the owner's **mobile-only
viewport decision**.
**Tree:** worktree `.agents/worktrees/dsh-b74d064d-6fe5`, HEAD `0df57ce` (the W1
docs commit) directly on `origin/master` `76aba5d` — 16 commits newer than the
tree the first version audited (`62c6cf6`).
**Owner of this document:** `ux-lead` (task-18). **Sibling W1 docs:**
`gui-core-boundary.md`, `design-system-2026.md`, `cross-platform-2026.md`,
`gui-refresh-spec.md` (synthesis, itself being re-baselined in this delta wave).
**Binding facts:** `docs/design/interface.md`. **It is now stale against shipped
behaviour** — it was not changed in the delta while four of its route states were
deleted from the product and two new fact families were added. This document says
where, in §3; the contract amendments belong to the spec/W2 wave (the delta plan
already records them as W2-2 work).
**Historical inputs, never approved:** `ui-audit.md`, `ui-design-critique.md`
(Oct 2026, verdict **REVISE**, 16 required changes). §8 re-checks them.

### Revision log

| Rev | Date | What changed |
|---|---|---|
| r5 | 2026-11-19 | QA R16/R17: one definition for the 5 literal colour sites; A2 shipped-word note (§3.1). |
| **r10** | **2026-11-20** | **Delta re-baseline on `76aba5d`.** Old F5 (Direct fallback / Re-test pending / Server rejects relay) marked **obsolete** and replaced by the warn-only starvation flow; new flows F11 (location ladder), F12 (route trace), F13 (update-check disclosure); state critique rewritten — `interface.md` declared stale; microcopy inventory extended with the 23 new keys; usability cases added; findings re-ranked (**36**: 4 Blocker, 8 Critical, 16 Major, 8 Minor), with the master-fixed items named; owner mobile-only decision applied (all copy judged at 320 px). |
| **r16** | **2026-11-20** | **Delta-fix (QA r4):** finding count corrected to 36 (F17); English-key framing corrected to **45 = the 26 pre-delta keys + 19 added by the delta** (F19, aligns the audit with the brief); the 435/1270 capture citation corrected to `interface.md:18`/`:249-250` (F22); **30 px state-word Q15 answered**: RU `state.not_returning` / `action.restart_admin_optimize` shortened in `client-gui/locales/ru.toml`, ES/PT-BR proposals recorded, chrome cost stated (§4.4a, T21). |

**How to read this document.** A finding that is not backed by a `file:line` or a
screenshot in `wat/evidence/2026-11-19/team/ux-lead/` (first pass) or
`wat/evidence/2026-11-20/team/ux-lead/` (delta) does not exist. Proposed copy is
written as shipped behaviour: present tense, no dates, no promises, no unmeasured
"faster". There is no user research in this repository; §1 is derived from the
product's own state contract and shipped copy, not presented as research data.

---

## 0. Verdict and leverage

**Verdict: master made the honesty surfaces materially better and introduced two
new honesty risks. The four Blockers from W1 all still reproduce.**

What changed for the better (details and evidence in §0.1): starved relays no
longer silently take the route from the player — the window and the tray say
**NO RETURN PATH** and keep relaying; the route card no longer invents a country
for a hosting server (cloud region → operator geofeed with attribution and
as-of → hosting "unknown" + caption → country estimate → unknown, with no dot
where the data cannot support one); the client shows a relay-measured server leg,
an `L + S` floor and both traceroutes; the tray never says "optimizing" while the
return path fails; the false "20/45/90 s re-test" promise is gone; and the
update check now has a written privacy disclosure.

What did not change: the product still cannot tell the user that a boost failed
(the engine error exists, only the tray dot sees it), a disconnected window still
has no Connect control, the primary action is still an unnamed custom widget, and
control-plane registration failure is still invisible. Two **new** honesty risks
came with the delta: the starvation banner attributes the fault to the **relay**
while the client can only observe that no replies arrived (its own fault enum
says the game server may be the cause), and the new honesty copy ships **English
in ja/ko/zh-Hans** together with the rest of the state vocabulary (**45 keys:
the 26 pre-delta set plus 19 added by the delta**), while the recorded
translation-debt decision covers seven.

Counts by the `lightspeed-gui-design` severity ladder: **4 Blocker, 8 Critical,
16 Major, 8 Minor** (36 findings, §5). The owner chose the **mobile-only
viewport** (320 px pinned, single column, height 420–1400, initial 760), so every
copy judgement below is made at 320 px; the old "pinned width is a defect"
finding is retired as a decision (§0.2).

Top three leverage fixes, in order:

1. **Close the failure/recovery half of the product (B1–B4, C6).** One visible
   error surface fed by the engine's own `*_error` / `registration_error` fields;
   a Connect control beside Disconnect; an accessible name and visible focus on
   the one action. This remains mostly wiring — the facts already exist in
   `EngineStatus` and are read only for the tray dot (`app.rs:4090-4104`).
2. **Make the starvation warning provable and actionable (C1, C3, M16).** The
   banner must state what the client measured ("the relay is not returning this
   server's traffic yet" / "your game's replies are not arriving"), not a
   fault verdict the watchdog cannot establish; the `Relayed` row must use the
   same counters as the verdict instead of the relay-wide diagnostic counter
   (today the two can contradict); and the remedy should say that picking
   another relay restarts interception.
3. **Make the mobile-only surface honest and readable at 320 (C5, C7, M5, M15).**
   Fix the two false/jargon copy items, finish the i18n debt the honesty
   surfaces actually have, and stop the route card from growing into a wall of
   wrapped mono rows on the one width the product ships.

### 0.1 What master changed, and what it fixed

Delta evidence: `wat/evidence/2026-11-20/rebase-delta/delta.txt`;
`wat/state/current-phase.md:15-26` (rebase entry), `:65-75` (warn-only entry),
`:96-104` (flap fix), `:106-125` (geofeed/locking), `:127-136` (disclosure),
`:138-146` (traceroutes). Code read in this revision; GUI copy captured live at
320 px (evidence §10).

| Delta | User-visible effect | W1 finding it closes / changes |
|---|---|---|
| **Warn-only starvation** (`76aba5d`): no automatic direct fallback, no re-test chain, no "server rejects the relay" state; confirmed starvation raises a warning and keeps relaying (`app/watchdog.rs:66-95`, `225-260`; `app.rs:2079-2090`, `2261-2277`; tray `TrayState::RelayStarved` `app/window.rs:142-159`, `platform/windows.rs:136-158`) | rail `NO RETURN PATH` (danger), danger banner in the route card naming the remedy, amber tray state; the warning never changes the route itself; the relay is demoted for future rankings (`relay_history.rs:218-240`, `app.rs:1706-1728`) | **Retires** the old F5 flows and old finding "Direct fallback omits why"; **fixes** the relay↔direct flap (the `ff7de13` flap fix is superseded by removing the machine) and the live "tray said optimizing while direct" defect (`tests.rs:383-425`) |
| **Location ladder** (`f988e96`, `5954a0a`, `bbc96cc`, `0084f4e`): cloud region → operator geofeed (display-only, attributed, as-of) → hosting "unknown" + caption → country estimate → unknown; no dot for hosting/geofeed (`server_place.rs:1-31`, `47-94`, `171-240`; card `app.rs:2224-2246`, `2289-2331`) | the card says *where the server is and how sure it is*; a G-Core Virginia server shows `Server location: operator geofeed — Manassas, US-VA` / `(G-Core, as-of 2026-11-19)` / `Shown as-is…` instead of a false country | **Adds** the honest location surface the old card lacked; **fixes** the map's wrong-country dot for hosting ranges (live label in `current-phase.md:113`) |
| **Flow-aware server locking** (`order.rs` `FlowOwner` + `ProviderClass`; GUI `detection_provider.rs:1-30`, `42-60`) | the client refuses to re-lock a stale game-server pin onto an unrelated high-port flow, so the shown server is the one the game actually uses | **Fixes** the "fake Brazil"/LAX misroute symptom (state entry `:117-125`, `:159-167`) and the relay-history mis-attribution class |
| **Relay-measured server leg + `L + S`** (`f988e96`; `app/choice.rs:183-302`; card `app.rs:2416-2426`, `2490-2501`) | `Server leg` row is the relay's own ICMP measurement with sample count and age (`measured` / `measured, unvalidated`), plus `Expected path ~N ms (L + S)` | **Improves** the old "server leg measured is unreachable" gap; the estimator rows remain only as fallback (`app.rs:2552+`) |
| **Route trace**: relay TTL trace (`proxy`, `MSG_SERVER_TRACE`) and client traceroute (`client_trace.rs:1-30`, caption `:617-643`) | `Route` ledger row (`hops · last router RTT` / `hops · no router answered`) and a caption under the map (`Route to server: N hops · nearest responding hop X ms` + provider name when known) — placement anchors, never a city | **Adds** the placement surface an unknown hosting server lacked |
| **Update-check disclosure** (`bbc96cc`): `privacy.md:186-204`, `data-dictionary.md:25-37` | the user-initiated GitHub release check is now written down (request, User-Agent, no startup/background check) | **Fixes** the pre-existing "privacy policy missing the update call" gap; the *in-app* disclosure is still absent (M13) |
| **Truthful settings copy**: the old re-test checkbox and its 20/45/90 s tooltip removed; `starvation_policy` states warn-only (`en.toml:37`, rendered `app.rs:3416-3421`; retired key still loads `config.rs:388-412`) | Settings tells the truth about what happens when a relay starves | **Fixes** half of old C5 (the retest half); **keeps** the hold-copy half (C5) |
| **Relay-leg health/data-address pairing fix** (delta entry `current-phase.md:20`) | a relay's measured client leg is attributed to its data-plane address, not its `/health` port | **Fixes** a silent wrong-relay measurement class behind the old LAX misroute |
| **Owner decision: mobile-only viewport** (`gui-refresh-brief.md:4-6`, `:323`) | 320 px pinned, single column, height 420–1400 | **Retires** old M4's "pinned width is a defect" claim; residue is §5 M4 |

### 0.2 The mobile-only decision and what it does to the previous audit

The owner chose to **keep the shipped 320 px pinned single-column window** and
make it the product: width pinned, height user-resizable 420–1400, initial 760
(`main.rs:110-115`, `app/window.rs:35-39`); the 435/620/1120 px layouts in
`interface.md:150-157` are no longer product requirements. Consequences judged in
this revision:

- **Retired:** the W1 finding that a pinned width is a defect, and the "wide/
  short breakpoints are unreachable" contract-drift item — they are now a
  recorded decision, not a bug (`gui-refresh-brief.md:4-6, 323`). The
  `interface.md:150-157` width layouts are superseded by the owner's decision;
  the real `435/1270` capture references are `interface.md:18` and `:249-250`
  (`:172` is the `435x720 / 720x610` proof, not a 1270 px capture).
- **Kept as a real finding:** the window still does not remember its geometry
  (height resets to content each launch, position is never persisted) and it
  still auto-resizes between states, moving the action bar under the pointer
  (§5 M4). Mobile-first does not require a jumpy window.
- **New at 320:** the route card's content grew (warning banner, location block,
  trace caption, up to six new ledger rows). This width cannot absorb that the
  way a wider window would (§5 M5, §4.4).
- Everything in §4 is judged at 320 px: longest wrapped strings were measured in
  the live window (`window-settings-routing-320-delta.png`,
  `window-settings-routing-320-de-delta.png`) or computed from the catalog and
  the 17 px mono value style.

### 0.3 Status of the W1 (r5) findings

| W1 finding | Status after the delta | Evidence |
|---|---|---|
| B1 action has no accessible name / no visible focus | **Reproduces** | `ledger.rs:71-137`; live UIA `Custom` unnamed (2026-11-19 capture, `window-connected-320-delta.png` today) |
| B2 `Disconnect` is one-way | **Reproduces** | `app.rs:2783-2802` (link only while connected), action disabled `4199-4235`; no Connect control in the window |
| B3 failed boost is silent | **Reproduces** | `app.rs:1569-1581` (trace only), `4090-4104` (errors only recolour the tray) |
| B4 control-plane registration invisible | **Reproduces** | `engine.rs:78-87`, `1495-1507`; no GUI read (grep 0 matches) |
| C1 direct fallback omits its cause | **Obsolete — fixed by design** | the fallback state no longer exists (`en.toml` keys removed; `watchdog.rs:188-194`); replaced by new C1 (starvation attribution) |
| C2 `OPTIMIZING` while traffic held direct | **Reproduces** (pre-server hold) | `app.rs:1845-1850` `holding` needs a locked server; rail `2079-2085` |
| C3 `Relayed` can show impossible/mismatched direction pair | **Reproduces, and now can contradict the warning** | `app.rs:2733-2765` uses `interceptor_from_proxy`; verdict uses `packets_reply_data` (`interceptor/traits.rs:271-285`) |
| C4 headline metric unlabelled / dual meaning | **Reproduces** | `app.rs:2117-2148`; contract `interface.md:134-136` unchanged |
| C5 false settings facts (hold 3 s, retest 20/45/90) | **Half fixed:** retest copy deleted with the machine; hold copy still says 3 s | `en.toml:36`; `app.rs:3471-3478` vs `app.rs:97`, `1226-1238` |
| C6 internal strings as copy | **Reproduces; one new instance** | `update.rs:234-240` → `app.rs:3938`; `engine.rs:1217-1221` → `app.rs:2704-2716`; new English hover `app.rs:2350-2354` |
| C7 9-locale claim untrue on honesty surfaces | **Worse:** 45 English-identical keys in ja/ko/zh (the 26 pre-delta set + 19 delta-added) incl. all new honesty copy | §4.1 #7; locale comparison script |
| C8 elevation action exits before UAC resolves | **Reproduces** | `platform/windows.rs:266-280` |
| M2 `pick your game` vs Auto hint | **Reproduces** | `app.rs:2104-2111`, `2838-2844`; live today |
| M3 game shown as raw registry key | **Reproduces** | `choice.rs:524-540`; `app.rs:2106`, `2825-2837`, `2999` |
| M4 pinned width / wide breakpoints | **Retired as a decision**; residue is new M4 | §0.2 |
| M5–M16 (disabled reason, discovery, hover-only, centring, targets, destructive, tray, update, density, placeholder leak, disclosure) | M5–M16 mostly **reproduce** with renumbering (§5); the W1 sparkline and literal-token findings are downgraded to **m5/m7** because W2 owns them | §5 |
| M13 five literal colour sites | **Reproduces** (globe unchanged); downgraded to minor because W2-2 scope now includes `globe.rs` | `globe.rs:71-73,125,133`; QA R1 |

---

## 1. Jobs-to-be-done and the one-second question

There is no user research in this repository. These jobs are derived from the
product's state contract, the shipped copy and the engine's own vocabulary; the
usability tests in §6 are how they should be validated.

### 1.1 The one-second question

> *Is my traffic being boosted, through which relay, and is the link good?*
> — `lightspeed-gui-design` §0

The window answers the three parts in three places: the rail word (what), the
rail sub-line (where: game → relay, or the hold state), and the rail metric +
route card (how good). A player alt-tabs mid-match and reads the top ~250 px
only; everything below the route card is secondary in that second.

**Verdict on the one-second read:** the *state* half improved a lot — the rail now
cannot say `OPTIMIZING` while a confirmed return-path fault is active (warning
outranks it, `app.rs:2079-2090`), and the tray cannot either
(`app/window.rs:142-159`). The *proof* half is where the delta added weight: at
320 px the route card can now carry a warning banner, four location lines, a
trace caption, and up to six more ledger rows (§5 M5), all below the fold of a
one-second glance. And the headline metric still has no visible label and still
changes meaning between the relay leg and a direct ICMP ping (§5 C4).

### 1.2 Jobs

| # | Job (context, not persona) | Trigger | Done looks like | Where the window answers | Status |
|---|---|---|---|---|---|
| J1 | *Tell me I am or am not being boosted, right now* | alt-tab mid-match, tray glance | one word, correct for the route actually in use, and a warning when the route is failing | rail `app.rs:2072-2090`; banner `2261-2277`; tray `window.rs:142-159` | **improved, still at risk** in the hold-before-server case (§5 C2) |
| J2 | *Tell me the game is being helped, not just "connected"* | match start | game + relay in one line; counters that only move when the game moves | rail `app.rs:2092-2111`; ledger `2394-2473` | partially: `Relayed` counter sources disagree with the verdict source (§5 C3) |
| J3 | *Start it for me* | game appears | no click; the app detects and starts | presence `app.rs:1944-2043` | works until it fails silently (§5 B3) |
| J4 | *Start/stop it myself* | manual choice, troubleshooting | one outcome-named control, keyboard reachable, reversible | action slot `ledger.rs:39-137`; click `app.rs:4222-4234` | **not for keyboard/SR users** as an outcome (§5 B1); elevation can kill the app (§5 C8) |
| J5 | *Pick the right relay (or trust Auto)* | first run; new region | the relay best for the *game server*, with the number it ranked on, and a way to override | `boost_server_combo` `app.rs:2879-2966` | implemented; **picking another relay during play restarts interception**, and the starvation banner does not say so (§5 M16) |
| J6 | *Pick the game, or a program the registry does not know* | non-registry game | registry picker, custom profile, process target | `app.rs:2822-2853`, `3595-3707`, `3709-3793` | implemented; 31-row popup at 320 unverified; game names read as raw keys (§5 M3) |
| J7 | *When it breaks, tell me what broke and what to do* | boost fails, relay starves, update fails | the engine's own words + a next action, in the user's language | warning banner is the **only** failure surface that works | **partial:** starvation is visible; boost-start/registration/fatal errors are not (§5 B3, B4, C6) |
| J8 | *Leave it running in the tray and get it back* | close window, relaunch | X hides; tray restores; a second launch explains itself | `window.rs:161-177`; tray `platform/windows.rs:101-165`; notice `single_instance.rs:76-89` | works; tray is English-only and `error` has no detail (§5 M12) |
| J9 | *Keep it current* | new release | a visible, actionable update path with a stated privacy cost | `app.rs:3905-3988`; docs `privacy.md:186-204` | **incomplete:** dead ends and no in-app disclosure (§5 M13) |
| J10 | *Know where the game server is and how sure the client is* | hosting/cloud server, odd geography | a labelled placement with its source, or an honest "unknown" | location ladder `app.rs:2224-2246`, `2289-2331`; `server_place.rs` | **newly answered well**; the prose is centred and English in CJK (§5 C7, M9) |
| J11 | *Know what this program sends over my network* | privacy-sensitive player, sysadmin | every connection listed, including probes | `privacy.md:178-184` lists relay/registry/GitHub; traceroutes are missing | **gap** (§5 M14) |

---

## 2. Flow inventory

Format per flow: **Entry** → **Decisions** → **Failure** → **Recovery** → **Copy**
(verbatim, `file:line`) → **Friction** / **320 px**. "Reachable today" notes what a
stranger can actually do in the shipped window. All copy is judged at the pinned
320 px (`§0.2`).

### F1 — First run, no relay (and no saved config)

- **Entry:** launch with no `config.toml`; defaults load, discovery starts, and
  the app auto-connects when discovery lands (`app.rs:656-671`, `app.rs:1460-1489`).
- **Decisions:** none the window asks for. The relay row shows a spinner while
  discovery runs (`app.rs:2888-2904`); the Game row says
  `No game detected - Auto will follow one when you start it` (`en.toml:68`,
  rendered `app.rs:2838-2844`).
- **Failure:** discovery fails with no saved relays → `No relays discovered`
  (warn) with the reason only in a hover and a `Retry` link
  (`app.rs:2896-2904`; the hover fallback is hardcoded English `2899-2901` while
  the catalogued `relay.registry_empty` `en.toml:56` is unused). Discovery runs
  once at start and only on a click (`app.rs:767-773`, `3437-3454`).
- **Recovery:** `Retry`; Settings → `Refresh relays`; Settings → `Relays` → add a
  relay manually. Nothing in the no-relay state points at any of these.
- **Copy:** `Discovering relays…`; `No relays discovered`; `Retry`;
  `Auto (detect)`; the action label is `RESTART AS ADMINISTRATOR TO OPTIMIZE`
  (unelevated) or a disabled `OPTIMIZE MY ROUTE` (elevated) (`ledger.rs:39-57`).
- **320 px:** the empty relay row + Retry fits one wrapped line; the dead-end is
  the same as before — the loudest control is either disabled without a reason
  or an elevation invite that cannot produce the outcome (§5 M1, M6).
- **Evidence:** not reachable on this host (discovery succeeds); code-cited.

### F2 — Relay discovery failure with a saved list

- **Entry:** discovery fails while the relay list is non-empty
  (`app.rs:2804-2818`).
- **Decisions:** keep using the saved list (the app does), retry, or fix in
  Settings.
- **Failure:** `Relay refresh failed - using the saved list` (warn,
  `en.toml:55`, `app.rs:2804-2818`) with the raw reason on hover.
- **Recovery:** `Retry`; Settings → `Refresh relays`.
- **Copy:** the sentence above; `Retry`.
- **320 px:** the warn line wraps once; the hover-only cause is not reachable by
  keyboard or screen reader (§5 M8).
- **Evidence:** code-cited (not reproducible without killing the registry).

### F3 — Connected / idle (the most common state)

- **Entry:** a saved relay auto-connects at launch (`app.rs:674-676`), or
  discovery connects (`app.rs:1460-1482`).
- **Decisions:** start the boost, pick a game, pick a relay, open Settings.
- **Failure:** none in this state by definition; the failure is that the window
  looks identical when the boost start or the control-plane registration has
  failed (§5 B3, B4).
- **Copy:** rail `CONNECTED` + metric + `Relay ready, pick your game` /
  `Relay ready, {name} selected` (`app.rs:2086-2111`); ledger `Relay`,
  `Relay RTT`, `Game server -- start optimizing to place it`,
  `Relayed -- start optimizing to count` (`app.rs:2394-2473`, `en.toml:219-220`);
  Game hint `No game detected - Auto will follow one when you start it`;
  footer `Hide to tray` / `Settings` / `Quit`.
- **320 px:** captured live on the delta build:
  `window-connected-320-delta.png` (320×866).
- **Friction:** the rail tells the user to *pick* a game while the Game row (Auto
  mode) says Auto will do it — two instructions for one fact (§5 M2); the Game
  row names nothing until a game is found, and the game key is shown raw when it
  is (§5 M3).
- **Evidence:** live today + `window-connected-320.png` (W1).

### F4 — Boosting (healthy)

- **Entry:** a game appears and the relay is connected → auto-start
  (`app.rs:1944-2043`); or press the action (`app.rs:4222-4234`). Requires
  elevation; unelevated is F6.
- **Decisions:** stop; leave it; change game/relay (changing either restarts
  interception, `app.rs:716-769`).
- **Failure:** relay starvation → F5 (warning, no route change). Interceptor
  start failure → **window unchanged** (§5 B3). Fatal data-plane error later →
  tray dot only (§5 B4).
- **Recovery:** `STOP OPTIMIZING` (custom-painted button, keyboard-focusable but
  with no focus indication and no accessible name, §5 B1).
- **Copy:** rail `OPTIMIZING` + metric + `{game display} → {relay}`
  (`app.rs:2084-2111`); ledger gains `Server ping`, relay-measured `Server leg` +
  `Expected path ~N ms (L + S)`, `Relay→server (app)`, `Route`, `Relay RTT`,
  and `Relayed {sent} up · {returned} down`; action `STOP OPTIMIZING`.
- **320 px:** this is the densest state — see the row inventory in §5 M5. Long
  mono values (`Relay→server 231 ms · measured n=12 · 8s ago`, 48–60 chars at the
  17 px mono value style) wrap to two or three lines each in the narrow ledger
  (`app.rs:2652-2672`).
- **Evidence:** live W1 boosting capture
  `window-boosting-hold-320.png` (same shell, pre-delta counter semantics);
  healthy-with-new-rows state not captured (needs a live game, §9).

### F5 — Starvation warning (warn-only) — **replaces the old Direct fallback / Re-test pending / Server rejects relay flows**

**The old flows are obsolete, by evidence, not by preference.** The keys
`state.direct`, `route.direct_fallback`, `route.retry_in` and
`route.server_rejects_relay` were deleted from all nine catalogs in `76aba5d`
(`git show 76aba5d -- client-gui/locales/en.toml`, §4.2); the watchdog no longer
has a retry budget or a switch (`app/watchdog.rs:180-194, 225-260`); the config
key `retry_after_fallback` is retired and ignored on load
(`config.rs:388-412`); the state machine's own test asserts
`switch_to_direct == false` (`app/tests.rs:322-359`).

- **Entry:** a relay is carrying the game server, and the watchdog judges two
  consecutive 8 s windows (`RELAY_REPLY_TIMEOUT`, `RELAY_REPLY_STREAK`)
  with at least 20 intercepted datagrams, a return share below one in eight,
  and the server's own flow not healthy (`app/watchdog.rs:3-39, 291-330`;
  the server-flow corroboration is what stopped one bad wide window accusing a
  working relay, `watchdog.rs:306-318`).
- **Decisions:** keep playing; press `STOP OPTIMIZING`; pick another relay in the
  Relay combo (this is the only route-change mechanism the product now has). The
  Settings caption states the policy: LightSpeed warns and keeps relaying, never
  switches by itself (`en.toml:37`).
- **What the user sees:** rail word `NO RETURN PATH` in danger
  (`app.rs:2079-2081`); a danger banner above the map in the route card with
  `Relay is not returning your game traffic - stop optimizing or pick another
  relay` (`en.toml:184`; banner `app.rs:2261-2277`) or, for the local-injection
  fault, `The relay's replies are not reaching your game - stop optimizing and
  check the log` (`en.toml:185`; key choice `app/watchdog.rs:508-519`); the tray
  goes amber with `relay is not returning your game traffic`
  (`platform/windows.rs:136-158`); the rail's 3 px edge goes danger instead of
  green (`app.rs:4113-4143`); the route and counters stay live (the session is
  still relaying).
- **Failure:** the warning is a claim about *no replies arriving*. The client
  cannot distinguish "the relay dropped the return path" from "the game server
  never replied" — its own fault enum says so (`watchdog.rs:97-102`: "the first is
  the relay's or the game server's fault") — but the banner blames the relay
  (§5 C1). The warning is also undismissable (by design) and has no "since"
  information.
- **Recovery / clearing:** a fully healthy judged window clears it
  (`watchdog.rs:149-158, 225-260`; first fault window of a new streak does **not**
  clear it — that was the 2026-11-20 flap, `tests.rs:155-185`); a new game
  server, a relay change, stopping the boost, or losing the interceptor also
  clear it (`watchdog.rs:489-506`, `540-580`). There is no "recovered" banner,
  which is correct: the warning simply disappears (`interface.md:39` still says
  recovery is trace-only).
- **Copy (320 px):** the English banner is 80 characters at BODY 13 semibold in
  a ~270 px card → ~3 lines (the German translation, 103 characters, ~4 lines).
  Rail word + banner + tray state repeat the same fact in three phrasings
  (`NO RETURN PATH` / `Relay is not returning your game traffic` / `relay is not
  returning your game traffic`).
- **What is verified:** unit tests pin the raise/clear/never-switch/demote rules
  (`app/tests.rs:155-185, 188-216, 265-291, 322-380`, tray `383-425`); the live
  banner/tray state was **not** observed in this revision (§9) — no safe
  starvation fixture on this host.

### F6 — Elevation required

- **Entry:** launch unelevated; `is_admin == false` (`app.rs:503`,
  `platform/windows.rs:184-231`).
- **Decisions:** restart as administrator or not.
- **Failure:** the action becomes `RESTART AS ADMINISTRATOR TO OPTIMIZE` even
  when no relay is selected or the app is disconnected (`ledger.rs:47-51`,
  `tests.rs:1986-2030`, `2939-2959`); pressing it spawns a hidden PowerShell
  `Start-Process -Verb RunAs` and calls `std::process::exit(0)` immediately
  (`platform/windows.rs:266-280`). Declining UAC leaves nothing running and no
  message; there is no "restarting…" feedback.
- **Recovery:** relaunch manually; there is no in-app path back.
- **Copy:** `RESTART AS ADMINISTRATOR TO OPTIMIZE` (`en.toml:166`);
  Components `Elevation: administrator / standard user` (hardcoded English,
  `app.rs:3502-3543`).
- **320 px:** the German label is 45 characters (`de.toml:13`), Russian 50; the
  action button shrinks type to fit with a 9 px floor (`ledger.rs:77-88`). The
  rendered size in those locales is **not** visually verified (§9).
- **Evidence:** not reachable in this session (process elevated); code-cited.

### F7 — Game not detected (and detected)

- **Entry:** Auto mode, presence worker scans every 5 s off-thread
  (`app/presence.rs:26-45`, `app.rs:1944-2043`).
- **Decisions:** wait for Auto, pick a game, create a custom game, or target a
  process.
- **Failure:** none — "no game" is normal; but the rail and Game row contradict
  each other (§5 M2), and a game the registry does not know is only reachable
  through `Manual / Custom…`, `New custom game…` or `Target a process` — three
  affordances for one need.
- **Copy / 320 px:** `No game detected - Auto will follow one when you start it`
  (`en.toml:68`, one wrapped line at 320); when detected:
  `Game found:` + the raw key (`app.rs:2825-2837`) and rail
  `Relay ready, {raw key} selected` (`app.rs:2106`) — live today, the delta build
  still shows the registry key, lower-cased, on three surfaces (§5 M3).
- **Evidence:** live today (`window-connected-320-delta.png` shows the no-game
  hint; W1 `window-game-detected-start-failed-silent.png` shows the raw key).

### F8 — Update available

- **Entry:** Settings → `Check for updates` (`app.rs:3437-3444`), or the test
  env var `LIGHTSPEED_OPEN_UPDATE` (`app.rs:661-662`). No proactive check, no
  badge, no tray notice.
- **Decisions:** install, close, ignore.
- **Failure:** install failures show `install failed: <raw error>`
  (`app.rs:4084-4087`); a check failure shows the raw `Err` text as the status
  line (`update.rs:234-240`, rendered `app.rs:3938`); when `status.installer` is
  `None` — the shell-installer receipt path and every non-Windows build
  (`update.rs:186-213`) — `Update available` is shown with **no install action**
  and no instruction (`app.rs:3940-3961`).
- **Recovery:** close and re-check; no retry control beyond the Close button.
- **Copy:** `Current version: 1.11.4`; `Latest version: v1.11.4` (two spellings,
  §5 m1); `You're up to date` / `Update available`; `Install update`; the
  hardcoded English UAC line in `Installing` (`app.rs:3964-3972`). **No in-app
  sentence says the check contacts GitHub** — that disclosure lives only in
  `privacy.md:186-204` / `data-dictionary.md:25-37` (§5 M13, F13).
- **320 px:** the sheet body is one short stack; the dead-end branch is a version
  line, a status line and `Close`.
- **Evidence:** W1 `window-update-sheet.png` (up-to-date branch; the delta
  build's sheet is unchanged by code read).

### F9 — Quit-to-tray, restore, second launch

- **Entry:** window X (`app.rs:4014-4041`, `window.rs:161-177`); footer
  `Hide to tray` (`app.rs:4161-4197`); tray `Show window`
  (`platform/windows.rs:52-68`, `101-128`).
- **Decisions:** hide vs quit; tray Quit vs window Quit.
- **Failure:** X hides; with no tray it exits (correct). If tray creation fails,
  closing exits without a word (`platform/windows.rs:170-176`).
- **Recovery:** tray `Show window` or double-click. A second launch shows
  `LightSpeed is already running. / Look for the ⚡ icon in the system tray, or
  the open LightSpeed window.` (`single_instance.rs:76-89`) — verified in the W1
  session.
- **Copy:** `Hide to tray`, `Quit`, `Show window`, `Connect`, `Disconnect`,
  `Quit` (`en.toml:136-166`; tray hardcoded `platform/windows.rs:52-55`);
  tray tooltips (`platform/windows.rs:146-158`) — now including
  `⚡ LightSpeed — relay is not returning your game traffic` for the new
  `RelayStarved` state, still English-only (§5 M12).
- **320 px:** the footer wraps to two rows in German (`window-connected-320-de-delta.png`),
  which is acceptable; `Beenden` stays inside the frame.
- **Evidence:** W1 `window-connected-320-de.png`; delta German capture today.

### F10 — Error states

**There is still no Error state in the window.** The contract requires one
(`interface.md:41`), the catalog carries `state.error` (`en.toml:154`), and it is
never rendered (unused key, §4.1 #17). What actually happens:

| Failure | Fact exists in | Reaches the window? | Reaches the tray? | Evidence |
|---|---|---|---|---|
| Interceptor/boost start refused (driver missing, permissions, platform) | `tracing::error!` only (`app.rs:1569-1581`) | no | no — `interceptor_error` is set from the interceptor's health, not from a start refusal (`engine.rs:1461-1466`) | live W1 evidence: `window-game-detected-start-failed-silent.png`; trace `ERROR start_interceptor failed: WinDivert.dll not found …` |
| Fatal interceptor error while running | `EngineStatus::interceptor_error` | no | yes (red dot, tooltip `error`) | `engine.rs:1461-1466`; `app.rs:4090-4104` |
| Control-plane registration refused | `EngineStatus::registration_error` / `control_registered` (`engine.rs:78-87`, `1495-1507`) | no — the GUI never reads these fields (grep 0 matches) | no | code only |
| Relay starvation | `RelayStarvationWarning` | **yes** — rail word, banner, tray (F5) | yes (amber) | `app.rs:2079-2090`, `2261-2277` |
| Update check/install failure | `Result<_, String>` | raw string (`app.rs:3938`, `4084-4087`) | no | code |
| Crash | `crash.rs` writes a record + `MessageBoxW` | yes (modal box, English only) | n/a | `crash.rs:26-158` |

**Friction:** the most common field failure (boost refuses to start) remains
invisible in both window and tray; the new warning proves the product *can*
surface a failure, which makes the silence on the start path more conspicuous.
Copy for the missing state already exists (`state.error`) and the tray
`error` branch has no detail (`platform/windows.rs:158`).

### F11 — Server-location ladder (new)

- **Entry:** a game server is locked; the route card draws the map and, under it,
  the location lines (`app.rs:2224-2246`, `2289-2331`).
- **Decisions:** none — display only. The user decides how much to trust the
  relay placement, and the card exists so that decision is informed.
- **The five states** (one `Place` variant each, `server_place.rs:47-94`;
  resolver order `server_place.rs:171-204`):

| State | When | Exact copy (`en.toml`) | Dot? | Detail line |
|---|---|---|---|---|
| Cloud region | address in the AWS/GCP published ranges | `Server location: cloud provider region` (`:204`) | yes, region point | `{PROVIDER} · {region}` (`app.rs:442`) |
| Operator geofeed | address in one of 7 operator RFC 8805 feeds | `Server location: operator geofeed — {place}` (`:208`) | **no** | `({provider}, as-of {date})` (`:209`); `Shown as-is from the operator's published geofeed.` (`:210`) with `Source: {source}` on hover (`:211`, `app.rs:2313-2317`) |
| Hosting network | curated hosting/CDN ASN ranges | `Server location: hosting network — unknown` (`:205`) | **no** | `AS{asn} · {name}` (`app.rs:469`); caption `Free IP databases are unreliable for hosting ranges.` (`:207`) |
| Country estimate | falls through to the DB-IP country table | `Server location: country-level estimate from IP` (`:203`) | yes, country centroid | none |
| Unknown | nothing matched | `Server location: unknown` (`:206`) | **no** | same hosting caption (`:207`) |

- **Failure:** none of these is an error; the "unknown" states are the honest
  answer. The geofeed path is display-only and never steers ranking
  (`server_place.rs:222-240`, `chooser_point` returns `None` for geofeed).
- **Recovery:** n/a (a different server may place differently).
- **Copy / 320 px:** the block is drawn inside `ui.vertical_centered`
  (`app.rs:2278`), so all four lines are **centre-aligned** under the map and the
  longer captions wrap to 2 lines each; a hosting server shows caption + ASN +
  long hint = three centred lines. That is the one place the ladder's own honesty
  is least readable (§5 M9); “Server location:” is repeated on all seven keys
  (§5 m4).
- **Verified:** the geofeed state was live-verified by the delta session
  (`current-phase.md:113`: UIA shows
  `Server location: operator geofeed — Manassas, US-VA` / `(G-Core, as-of
  2026-11-19)` / `Shown as-is…`); the unit tests pin the ladder
  (`server_place.rs:252-351`, hosting hint `app/tests.rs:983-1076`). Cloud,
  country and unknown states were not re-captured here (§9).

### F12 — Route trace (new)

- **Entry:** two independent traces.
  1. **Client traceroute** (location probe): runs when a server is locked and a
     relay session exists (`app.rs:1275-1293` → `client_trace.rs:1-30`); the
     result is a caption under the map: `Route to server: {hops} hops`
     (`en.toml:223`) plus `nearest responding hop {ms} ms` (`:224`) and a
     provider/hosting suffix when the last hop's prefix is in the shipped
     indexes (`client_trace.rs:617-643`). Bounded to 20 hops / 30 s / one run /
     30-minute cache; private and reserved targets refused
     (`client_trace.rs:39-51`, `98-130`).
  2. **Relay TTL trace** (placement anchor for a hosting server): the relay's
     report lands in a `Route` ledger row: `{hops} hops · last router RTT {ms} ms`
     or `{hops} hops · no router answered` (`en.toml:200-202`; row
     `app.rs:2446-2448`, text `app/choice.rs:265-284`), fresh for 10 minutes and
     bound to this `(relay, server)` pair (`app/choice.rs:249-263`).
- **Decisions:** none the user makes from it; it is evidence for trust in the
  placement (F11) and for a hosting server whose geoIP is unknown.
- **Failure:** a trace that cannot run simply yields no caption/row (no error
  surface); an incomplete client trace still reports the last hop that answered
  (`client_trace.rs:858-877`). Neither trace claims a city, and the client
  caption's explanation is a **hardcoded English hover**
  (`app.rs:2350-2354`) — the only explanation of what the number is.
- **Recovery:** n/a.
- **Copy / 320 px:** the caption is one or two wrapped CAPTION lines; the ledger
  `Route` row's mono value (`{hops} hops · last router RTT {ms} ms`, ~38 chars)
  wraps to two lines at 320. Label `Route` collides with the section word
  “Route” used elsewhere (`$5 m3`).
- **Verified:** unit tests (`client_trace.rs:769-1029`,
  `app/tests.rs:3045-3080`); live sample in `current-phase.md:143`
  (`CLIENT_TRACE … hops=14 last_hop=195.22.195.35 rtt=356ms complete=false`).
  The rendered row/caption was not captured in this revision (§9).

### F13 — Update-check disclosure (new)

- **Entry:** Settings → Maintenance → `Check for updates` (`app.rs:3437-3444`).
  No startup/background check runs in normal operation (the
  `LIGHTSPEED_OPEN_UPDATE` env hook is a test switch, `app.rs:661-662`).
- **What actually happens (documented):** one user-initiated HTTPS `GET
  https://api.github.com/repos/ShibbityShwab/lightspeed/releases/latest`
  (`update.rs:145-182`), carrying only a
  `lightspeed-gui-updater/<version>` User-Agent; GitHub sees the IP as for any
  HTTPS request; the release zip + `.sha256` download only after a second click
  (`privacy.md:186-204`, `data-dictionary.md:25-37`).
- **Decisions:** check or not; install or not.
- **Failure:** the disclosure is **not in the app**. The sheet shows versions, a
  status line and buttons; nothing tells the user that pressing the button
  contacts GitHub, what is sent, or that the download follows a second click
  (§5 M13). Documented nuance: closing the dialog sets the UI back to idle
  (`app.rs:3977-3985`) but does **not** cancel the in-flight request — the docs
  phrase “Closing the update dialog stops it” (`privacy.md:202`) is stronger than
  the code (§5 m8).
- **Recovery:** re-open and check again.
- **Copy:** the sheet keys (`en.toml:126-134`) hold no disclosure sentence; the
  disclosure lives only in docs. The flow should either add one short in-sheet
  line (e.g. “Checks github.com for a newer release. Sends only this app's
  version.”) or link the privacy section — a W2 copy decision.
- **Verified:** docs read in this revision; W1 live capture shows the
  up-to-date sheet (`window-update-sheet.png`); no network capture was attempted.

---

## 3. State-inventory critique against `interface.md`

`docs/design/interface.md` was **not changed** in the delta (`git diff
62c6cf6..HEAD -- docs/design/interface.md` is empty; recorded in
`current-phase.md:20`, `:75`). It is now materially stale in both directions:
four of its eleven route states no longer exist, and the shipped window has two
new fact families with no contract row. The spec/W2 wave owns the amendments
(delta plan: `current-phase.md:26`); this section is the audit's source for them.

### 3.0 What is stale in the contract (explicit)

Amendment slots below are **proposed** (A7-A13; the spec already carries approved
A1-A6), so the spec can renumber them as it re-baselines.

| Contract location | Says | Shipped reality | Action for the spec |
|---|---|---|---|
| `interface.md:33` Holding | driving fact `direct_fallback + sent == 0` | `direct_fallback` no longer exists; `holding` = `tunnelling_enabled == Some(false)` **and** a locked server (`app.rs:1845-1850`) | A7: replace the driving fact |
| `interface.md:36` Direct fallback | whole state row | **state does not exist** (keys deleted; no route switch) | A8: delete the row, or declare it retired |
| `interface.md:37` Re-test pending | countdown row | **state does not exist** (no retry budget) | A8: delete |
| `interface.md:38` Server rejects relay | row | **state does not exist** (no exhaustion verdict; starvation is a warning) | A8: delete |
| `interface.md:39` Relay recovered | "trace-only; window falls back to Relaying" | the warning **clears visibly** when a healthy window proves the return path; no banner is invented (correct in spirit) | A9: reword for the warn-only model |
| `interface.md:43-46` fallback story paragraph | "traffic goes direct → countdown → recovered → server rejects" | no longer describes the product | A8: replace with the warn-only story |
| `interface.md:61-67` server-leg table | four estimator states | shipped adds the relay-measured rows (`measured` / `measured, unvalidated`), `Relay→server (app)`, `Expected path (L+S)`, relay `Route`; the four old rows are only the fallback path (`app.rs:2552+`) | A10: add the new rows and their sources |
| `interface.md:24` "Eleven route states" | no "no return path" row | shipped has `NO RETURN PATH` (danger) with its own driving facts | A11: add the row (warning source, streak, clear rule) |
| `interface.md:145-174` layout/breakpoints | 380/620 px layouts (`:150-157`); 435/1270 capture references at `:18` and `:249-250` (`:172` proves 435x720/720x610) | owner decision: mobile-only 320 px (`gui-refresh-brief.md:4-6, 323`) | A12: replace with the mobile-only geometry |
| `interface.md:134-136` headline rule | headline `ms` is the relay leg | shipped switches to the direct ICMP ping when fresh (`app.rs:2117-2148`) — **still** a contract/code mismatch | A13: decide the metric and label it visibly (§5 C4) |

The contract's other honesty rules are still the right rules and are not
relitigated here: one source per fact (`interface.md:13-16`), no claim the client
cannot measure (`:138-141`), unreachable states must not be faked (`:142-143`).
The new warning is judged against them in §5 C1.

### 3.1 Route states — shipped vs contract

| Contract state | Implemented? | Reachable? | Named? | Honest? |
|---|---|---|---|---|
| Idle (`:31`) | split: `NOT OPTIMIZING` (disconnected) / `CONNECTED` (`app.rs:2086-2090`) | yes | two words for one contract row | yes in spirit; contract stale. `state.disconnected` still unused (`en.toml:149`, §4.1 #17; A2 note below) |
| Connecting (`:32`) | **not implemented**; `EngineStatus.connected` is the keepalive loop (`engine.rs:60-62`) | no | no | **dishonest by omission**: during the first seconds the rail says `NOT OPTIMIZING` / `No relay yet`, identical to “cannot connect at all” |
| Holding (`:33`) | word `HOLDING` from `self.holding` (`app.rs:2082-2083`); sub-line `Choosing a relay for this server` (`2092-2093`) | only after a server locks; before that the same hold is invisible | contract sentence (traffic stays direct) never shown | **partially dishonest** — §5 C2 |
| Choosing (`:34`) | merged into Holding's sub-line (`en.toml:218`) | yes | merge undocumented in `interface.md` | acceptable, contract stale |
| Relaying (`:35`) | `OPTIMIZING` + `{game} → {relay}` + two-direction counter (`app.rs:2084-2111`, `2733-2765`) | yes | yes | counters can be impossible / contradict the warning (§5 C3) |
| Direct fallback (`:36`) | **removed** | — | — | row obsolete (see §3.0) |
| Re-test pending (`:37`) | **removed** | — | — | row obsolete |
| Server rejects relay (`:38`) | **removed** | — | — | row obsolete |
| Relay recovered (`:39`) | warning clears on a fully healthy window (`watchdog.rs:149-158`) | yes | correctly no “recovered” banner | yes, but the row's wording is stale |
| **No return path (new, no contract row)** | `NO RETURN PATH` danger + banner + tray (`app.rs:2079-2090`, `2261-2277`, `window.rs:142-159`) | needs a live starving relay | yes, one word + one sentence | **claim outruns measurement** (§5 C1) |
| Needs elevation (`:40`) | not a rail state; the action slot changes label (`ledger.rs:47-51`) | yes | contract calls it a state, window a button | partial; reason hidden in a sheet, UAC exit risk (§5 C8) |
| Error (`:41`) | **not implemented in the window** | no | copy exists (`en.toml:154`) and is unused | **no** (§2 F10, §5 B3/B4) |

**A2 note (retained from r5).** The two words the shipped window shows across the
Idle row are **`NOT OPTIMIZING`** (`en.toml:155`, disconnected branch
`app.rs:2086-2090`) and **`CONNECTED`** (`en.toml:150`). `NOT CONNECTED` is new
copy proposed by the spec's amendment A2, not one of the shipped words; the
catalog still carries an unused `state.disconnected = "Disconnected"`
(`en.toml:149`, dead-key list §4.1 #17), which A2 should wire or delete.

### 3.2 Server-leg states

| Contract state | Implemented? | Reachable? | Honest? |
|---|---|---|---|
| Server leg measured (`:63`) | estimator rows still present as fallback (`app.rs:2552+`) | “measured” by the v1 estimator is documented as possibly unreachable (`interface.md:232-237`) | falls back to prior/bound correctly |
| **Relay-measured leg (new)** | `Relay→server {ms} ms · measured n={n} · {secs}s ago` / `… measured, unvalidated …` (`en.toml:195-196`; row `app.rs:2422-2426`) | when the relay reports for this pair and the client validated the triangle floor, or fail-open (`app/choice.rs:183-216`) | **yes** — “unvalidated” is qualified, and the source is the relay's own measurement |
| **Expected path (new)** | `Expected path ~{ms} ms (L + S)` (`en.toml:199`) | both measured terms present | **no as rendered**: the same sentence is used as the row *label* too, so the label shows the literal `{ms}` template (§5 M15); and `(L + S)` is undefined jargon |
| **Relay→server (app) (new)** | passive app-RTT row (`en.toml:197-198`, `app.rs:2439-2441`) | fresh passive sample | yes — display-only secondary, no ranking |
| **Relay route trace (new)** | `Route` row (`en.toml:200-202`, `app.rs:2446-2448`) | fresh relay trace for this pair | yes — hops and last router RTT only, no city |
| Server leg bounded / not measured / unknown (`:64-66`) | present as fallback (`choice.rs` server-leg state) | pending/stale ping | yes |
| Relay cannot beat direct (`:67`) | caption + hover arithmetic (`app.rs:2364-2377`) | fresh ping + client leg | yes; no next action (W2 design decision) |

### 3.3 States the window shows that the contract does not name

| Shown | Where | Problem |
|---|---|---|
| `NO RETURN PATH` + banner + tray | `app.rs:2079-2090`, `2261-2277`; `window.rs:142-159` | no contract row; cause claim issue (§5 C1) |
| Location ladder (5 states + hosting caption) | `app.rs:2224-2246`, `2289-2331`; `server_place.rs` | no contract row; source rules live only in code docs |
| Client-trace caption + relay `Route` row | `app.rs:2339-2355`, `2446-2448`; `client_trace.rs` | no contract row; the explanation is a hardcoded hover |
| `Discovering relays…` / `No relays discovered` | `app.rs:2888-2904` | no contract row; cause hover-only |
| `Relay ready, pick your game` / `Relay ready, {raw key} selected` | `app.rs:2104-2111` | contradicts Auto mode (§5 M2); raw key (§5 M3) |
| `Game server` value = engine scan sentence `Auto-detecting (1024-65535)…` | `engine.rs:1217-1221` → `app.rs:2704-2716` | a status sentence in a measured-address slot; untranslated (§5 C6) |
| Disabled action with no reason | `app.rs:4199-4235`, `ledger.rs:89-113` | a control that cannot show why it is disabled (§5 M6) |

### 3.4 Contract drift to resolve (summary for the spec)

1. **Retire the fallback story** (rows 36-38, paragraph 43-46) and **add the
   starvation warning** (A7-A9, A11), including its driving facts and clear rule.
2. **Add the location ladder, route trace and relay-measured server-leg rows**
   (A10) with their sources and their “no dot / display-only / no city” rules.
3. **Replace the layout section** (A12) with the owner's mobile-only geometry;
   delete the 380/620 px layouts and the wide screenshots from the contract.
4. **Resolve the headline metric** (A13): either the contract's “headline is the
   relay leg” is restored and labelled, or the contract accepts the shipped
   server-ping-when-fresh behaviour and the label becomes visible (§5 C4).
5. **Keep the honesty rules**; the new warning must satisfy
   `interface.md:138-141` (claim only what the client measures).

---

## 4. Microcopy inventory

Rules applied (`lightspeed-gui-design` §1.6, §7; `interface.md:13-16`):
one fact one source; active voice; one job per sentence; an outcome-named button
that keeps its name through the flow; no invented precision; honest failure;
empty states invite the next action; 9 locales.

The catalog has **182 keys** (was 163); the delta added 23 and deleted 4.

### 4.1 Violations shipped today

| # | Current copy / behaviour | Where | Rule broken | Proposed |
|---|---|---|---|---|
| 1 | Starvation banner: `Relay is not returning your game traffic - stop optimizing or pick another relay` while the signal is “no replies arrived” | `en.toml:184`; banner `app.rs:2261-2277`; fault docs `watchdog.rs:97-102` say the cause may be “the relay's or the game server's” | honesty: claim beyond measurement | `No replies from the relay for this game server yet - stop optimizing or pick another relay`; keep the relay-specific wording only for a future measured relay-side signal |
| 2 | The same warning repeats the fault as three wordings: `NO RETURN PATH` (rail), `Relay is not returning your game traffic` (banner), `relay is not returning your game traffic` (tray) | `app.rs:2079-2081`, `2261-2277`; `platform/windows.rs:155-157` | one voice, one fact | one sentence, reused by all three surfaces |
| 3 | Remedy “pick another relay” does not say that picking restarts interception | `en.toml:184`; switch path `app.rs:716-769` | no promised action without its cost | `…or pick another relay (the game's connection restarts)` |
| 4 | `The relay's replies are not reaching your game - stop optimizing and check the log` | `en.toml:185` | active voice, but “check the log” is not an in-app path | `…stop optimizing, then open the log in Settings → Maintenance` |
| 5 | `Expected path ~{ms} ms (L + S)` is used **as both the row label and the value** | label `app.rs:2425`; value `app/choice.rs:295-302`; `en.toml:199` | one fact one source; no placeholder leak | label `Expected path`; value `~{ms} ms (relay leg + server leg)`; add a `route.expected_path_label` key |
| 6 | `Server leg` relay-measured values are long mono strings: `Relay→server {ms} ms · measured n={n} · {secs}s ago` and the `measured, unvalidated` variant | `en.toml:195-196`; rendered 17 px mono `app.rs:2668-2672`, narrow ledger `2652-2665` | 320-px readability; no placeholder/unit leak but 2-3 wrapped lines | split: `Server leg 231 ms` + a caption `measured from the relay · n=12 · 8s ago` |
| 7 | **ja / ko / zh-Hans carry 45 English-identical catalog keys** — the 26 keys that already shipped English before the delta plus the 19 added with it — including every new honesty string: `state.not_returning`, `route.relay_not_returning`, `route.injection_failed_warning`, `settings.starvation_policy`, all `route.location_*`, `route.expected_path`, `route.server_leg_relay_measured*`, `route.route_trace*`, `route.trace_*`, plus the older state/route vocabulary | locale catalogs (comparison script, delta evidence); `interface.md:238-240` records a **7-key** English fallback decision | 9 locales; the documented decision understates the shipped set 6x and covers exactly the honesty surface | translate the state/route vocabulary; or re-record the decision with the real **45-key** list (26 pre-delta + 19 delta). The stated “CJK keep English for the bundled font subset” rationale (`current-phase.md:122`) is weak: the GUI ships Noto JP/KR/SC faces (`app/theme.rs:35-57`) |
| 8 | Hold copy: `Waits up to {secs}s for a relay before using the direct path.` rendered with **3 s** (`LOCK_SETTLE`) | `en.toml:36`; `app.rs:3471-3478`; real hold deadline `HOLD_FALLBACK = 120 s` (`app.rs:97`, `1226-1238`) | false data (40x off) | `Keeps traffic direct until it identifies the game server, for up to 2 minutes.` from `HOLD_FALLBACK` |
| 9 | `starvation_policy` is 191 characters (229 in German) of CAPTION; the only place the warn-only policy is stated | `en.toml:37`; `app.rs:3416-3421`; live: 5 wrapped lines (`window-settings-routing-320-delta.png`, `…-de-delta.png`) | one job per sentence at 320; discoverability | shorten to two sentences (warn + manual options) and let the banner carry the short form |
| 10 | Client-trace caption explanation is a **hardcoded English** hover: “The last hop that answered the client's own traceroute…” | `app.rs:2350-2354` | 9 locales; the only explanation of the trace is hover-only | catalog it; put the one-line version in the caption or a focusable disclosure |
| 11 | Location prose is centre-aligned under the map, with the `Server location:` prefix repeated on 7 keys and the hosting hint as a full centred sentence | `app.rs:2278`, `2289-2331`; `en.toml:203-211` | one gutter (`interface.md:199-207`); 320-px readability | left-aligned label/value block; `Location:`/`Uncertain:` two-word forms; hint as caption under the value |
| 12 | Provider/region and ASN detail lines are hardcoded formats: `format!("{} · {region}")`, `format!("AS{asn} · {name}")` | `app.rs:442`, `469` | catalog owns user copy; separators/order are locale-specific | keys with named placeholders (`route.location_provider_detail`, `route.location_hosting_detail`) |
| 13 | `Relay` label in the ledger vs `Route` label for the traceroute row vs `Route` section word; three meanings of route/relay | `en.toml:171` (`route.title`), `:200` (`route.route_trace`), `:182` (`route.game_server`) | one word one meaning | label the trace row `Path trace`; keep `Route` for the card/section |
| 14 | Update sheet: `Current version: 1.11.4` vs `Latest version: v1.11.4`; raw error text as status; no sentence about contacting GitHub | `app.rs:3924-3938`; `update.rs:234-240`; disclosure only in `privacy.md:186-204` | one fact one spelling; honest failure; disclosure in the surface that acts | normalise the tag; map errors to a sentence; add one disclosure line in the sheet |
| 15 | `state.error` is catalogued and never rendered (error state absent) | `en.toml:154`; §2 F10 | dead copy masking a missing state | wire it (B3/B4) or delete it |
| 16 | 18 of 182 keys unused: `app.title`, `settings.capture_*` (4), `proxies.title`, `proxies.manage_hint`, `relay.registry_empty`, `game.none_detected`, `state.disconnected`, `action.connect`, `action.connecting`, `action.manage`, `route.server`, `route.status`, `route.path_rtt`, `route.path_hint`, `route.start_manual` | `client-gui/locales/en.toml`; unused scan in delta evidence | dead copy (translators maintain 9 locales for it); three are the reconnect vocabulary the window lacks (`action.connect`, `action.connecting`, `state.disconnected`) | delete, or wire the Connect pair (B2) |
| 17 | `-- start optimizing to place it` / `-- start optimizing to count` | `en.toml:219-220`; `app.rs:2714`, routed row | action name differs from the button (`Optimize my route`) | `Not measured yet` / `No traffic relayed yet` |
| 18 | Hardcoded English remains: `Routing` section, Components block (`Components`, `Version`, `Elevation`, `Capture`, `Npcap`, `Relays` + values), telemetry hover, `Fetch the latest…`, `Probe every relay…`, `Enter the server's IP and port.`, `No relays yet. Discovery runs automatically.`, the UAC line, the relay-picker/vanish hovers, the cannot-beat hover, the port-range hover; tray menu/tooltips; second-instance notice | `app.rs:3125`, `3390-3394`, `3416`, `3450`, `3457-3458`, `3486-3543`, `3842`, `3970`, `2899-2901`, `2947-2966`, `2364-2377`, `3192-3201`; `platform/windows.rs:52-55`, `146-158`; `single_instance.rs:78-80` | 9 locales (`interface.md:231` exempts only the Components labels) | catalog keys; the Components block stays the one documented English-only exception |
| 19 | `Relayed` counts `up` from tunnel sends and `down` from **all** proxy datagrams incl. keepalive echoes | `app.rs:2733-2765`; `engine.rs:161-166`; the verdict uses `packets_reply_data` (`interceptor/traits.rs:271-285`) | one fact one source; counters must agree with the warning at all times | use the server-flow pair for both directions, or label the down half `relay replies` and exclude keepalives |
| 20 | `Game server` slot shows the engine's scanning sentence `Auto-detecting (1024-65535)…` | `engine.rs:1217-1221` → `app.rs:2704-2716` | measured-address slot holding status prose; untranslated | keep `-- start optimizing to place it`; localize a `Finding the game server…` line for the rail |

### 4.2 Removed by master (fixed copy — do not re-add)

- `state.direct`, `route.direct_fallback`, `route.retry_in`,
  `route.server_rejects_relay` are gone from all nine catalogs and from the code
  (delta diff; §2 F5). The old violations “fallback without a cause”, “retest
  promises 20/45/90 s” and “server rejects relay has no user action” no longer
  exist.
- The re-test checkbox and its tooltip were removed; the Settings `Routing`
  section now states the warn-only policy instead (`app.rs:3404-3421`).
- The `retry_after_fallback` config key is retired and ignored on load, and old
  config files still load (`config.rs:388-412`).
- The tray can no longer read `optimizing` while the return path fails
  (`app/window.rs:142-159`, `tests.rs:383-425`).
- The false country for hosting servers is no longer drawn or named
  (`server_place.rs:206-220`).

### 4.3 Copy that is correct and should not be relitigated

- Rail state words are single uppercase words (`CONNECTED`, `OPTIMIZING`,
  `HOLDING`, `NO RETURN PATH`, `DIRECT` is retired) — canon, not an eyebrow.
- Action labels are outcome-named and stable (`en.toml:164-166`,
  `tests.rs:1986-2030`).
- The location ladder's honesty qualified words are right in kind:
  “cloud provider region”, “operator geofeed”, “hosting network — unknown”,
  “country-level estimate”, “unknown”, and `Shown as-is from the operator's
  published geofeed.` with source/as-of.
- The `measured, unvalidated` qualifier on a fail-open relay report is exactly
  the honesty rule (`en.toml:196`; `app/choice.rs:198-216`).
- The trace rows claim hops/RTT only, never a city
  (`client_trace.rs:1-16`, `app/choice.rs:265-284`).
- `settings.starvation_policy` (once translated) states the warn-only model in
  plain language; the second-instance notice remains the best recovery copy in
  the product (`single_instance.rs:76-89`).

### 4.4 The 320 px copy judgement (measured/computed)

| String | Chars (en / de) | Where it renders | At 320 px |
|---|---|---|---|
| `route.relay_not_returning` banner | 80 / 103 | route card, BODY 13 semibold | ~3 / ~4 wrapped lines inside a danger frame above the map |
| `settings.starvation_policy` | 191 / 229 | Settings sheet, CAPTION 11 | **5 wrapped lines**, measured live in both locales (`window-settings-routing-320-delta.png`, `…-de-delta.png`) — acceptable in a sheet, but it is the only statement of the policy |
| `settings.hold_policy` (3 s fact) | 61 / 73 | Settings, CAPTION 11 | 2 lines; wrong number (C5) |
| `route.server_leg_relay_measured` / `…_unvalidated` | 55 / 60 | ledger value, **VALUE 17 mono** | ~2 / ~3 wrapped lines in the narrow ledger |
| `route.route_trace_hops` | 38 | ledger value, 17 mono | ~2 wrapped lines |
| `route.expected_path` | 30 | ledger row label **and** value (M15) | label leaks `{ms}`; value ~2 lines |
| `route.location_*` captions | 24-52 / 25-68 | under map, CAPTION 11, centred | 1-2 centred lines each; hosting shows 3 centred lines |
| `route.trace_route` + `trace_nearest` | 28 + 30 | under map, CAPTION 11 | 1-2 centred lines |
| footer (German) | `In den Infobereich ausblenden` | footer buttons, `horizontal_wrapped` | wraps to two rows; `Beenden` stays visible — verified live |

The route-card row inventory in F4/F5 plus this table is the input for §5 M5:
the card can reach ~14 text rows under the rail at 320 px, several wrapping to
2-3 lines, on the one width the product ships.

#### 4.4a State words at the required 30 px, and the restart action label (r16)

The required `STATE` 30 px (spec §1.2/§6.4; the shipped `STATE * 0.82` 24.6 px
is retired) changes the rail fit. Measured with the design-system's own method
(`fonts.py` via `delta-copy.py`; my calibration reproduces `delta-copy.md` §5
@24.6 exactly) as raw advances, uppercase, installed chain, against the 288 px
rail, word-only:

| locale | word | width @30 px | verdict |
|---|---|---|---|
| RU | `НЕТ ОБРАТНОГО ПУТИ` | 342.8 | **overflows** (−54.8) |
| RU | `НЕ ОПТИМИЗИРУЕТСЯ` (idle; retired by A2) | 326.8 | **overflows** (−38.8) |
| ES | `SIN RUTA DE VUELTA` | 309.0 | **overflows** (−21.0) |
| PT-BR | `SEM CAMINHO DE VOLTA` | 328.4 | **overflows** (−40.4) |
| DE | `OPTIMIERUNG LÄUFT` | 275.1 | fits, 12.9 px slack (thin) |
| ja | `state.idle` (retired by A2) | 270.0 | fits 288 by 18.0; over the 276 cap — **retires with A2** |
| all other W2-set locale/keys | ≤ 249.0 | fit (EN worst 221.1) |

These widths are the **HEAD catalogs** — the basis the QA r4 F1 finding
measured. After this revision's `client-gui/locales/ru.toml` edit the current
tree measures RU `state.not_returning` at **222.9 px** and the restart label at
**2 lines: landed, not an open overflow**. ES (309.0) and PT-BR (328.4) remain
open until a copy owner applies their shortened strings; RU `idle` retires with
A2.

RU is worst because Cyrillic falls through to the built-in `Ubuntu-Light` face —
the bundled Inter subset and the CJK faces carry no Cyrillic — so RU's width and
weight come from a different face than EN/DE. The long-term fix is font
coverage (spec Q19); the v1 fix is copy.

**Recommendation — shorten, do not wrap** (applied to
`client-gui/locales/ru.toml` under this task; script and output in the evidence):
`state.not_returning` = **“Нет возврата”** (222.9 px @30 = one line, 65.1 px
slack); `action.restart_admin_optimize` = **“Перезапустить от имени
администратора”** (356.9 px @15 = **2 lines** in the 264 px action column,
i.e. `ACTION_H_WRAPPED` 56). Proposed for the locale owners (outside this
task's write scope): ES `SIN RETORNO` 176.8, PT-BR `SEM RETORNO` 178.7; if the
idle word survives A2, RU `НЕ АКТИВНО` 193.9; DE may shorten to `OPTIMIERUNG`
167.2 for margin. **Acceptance:** every locale state word ≤ **276 px** at 30 px
(12 px / ~4 % safety under 288, above the ±2 px measurement tolerance), proved
in a real 320 px capture (T21).

**Chrome cost (spec §1.3, unchanged):** a one-line state word costs **0 px**
(rail stays 128 normal / 108 short); the two-line restart label's 72 px action
band is already the priced worst case (275/255/239), so **0 px chrome change**
and the 320×420 worst-case scroll stays **181 ≥ 180**. The alternatives cost
more: a two-line state-word rule needs +36 px rail (compact chrome 239→275,
scroll **145 < 180**), and the three-line restart slot needs an 89 px band
(compact 239→256, scroll **164 < 180**) — both would need an owner-approved
scroll-floor change. **Q15 answer: shorten RU to two lines; do not adopt the
73 px three-line slot.**

Coordinated and closed with `design-system`: its **`delta-r14`**
(`wat/evidence/2026-11-20/team/design-system/delta-r14.md`, 2026-11-20)
independently re-ran the same measurement and confirms these numbers. It
adopts the **≤276 px** acceptance cap and drops the 24.6 px basis from the fit
claim; lists ES/PT-BR beside RU (RU landed at 222.9; `SIN RETORNO` 176.8 /
`SEM RETORNO` 178.7 pending the copy owner); records `НЕ АКТИВНО` 193.9 /
`НЕ ПОДКЛЮЧЕНО` 268.0 / `OPTIMIERUNG` 167.2 as contingencies; states the RU
width as a Cyrillic-fallback symptom with Q19 re-subsetting as the long-term
fix; accepts the two-line RU restart label (Q15) with the 73 px option priced
out (256 chrome → 164 scroll < 180 floor) and names the shortened strings in its
capture ask; and flags the dropped “to optimize” purpose for the spec/copy owner
to carry elsewhere. No disagreement.

---

## 5. Severity-ranked findings

Ladder: `lightspeed-gui-design` §6. Format per the skill:
`[SEVERITY] surface: problem / Where / Rule / Impact / Fix / Effort`.
Severity is carried by the section heading and the ID prefix (`B`/`C`/`M`/`m`);
effort S ≤ half a day, M ≈ a day, L > a day for one implementer including
screenshot verification.

### Blocker

**B1 — The primary action has no accessible name and no visible focus.**
*Where:* `app.rs:4199-4235` draws the action; `app/ledger.rs:71-137` paints it
(`allocate_exact_size(..., Sense::click())`, no `widget_info`, no focus painting,
`hovered` is the only interaction state at `:93`). Live UIA on the delta build
exposes it as `Custom` with an empty name (`window-connected-320-delta.png`).
*Rule:* accessibility floor, keyboard-complete core path, visible focus
(`lightspeed-gui-design` §1.3); a core flow a screen-reader user cannot complete
is the ladder's Blocker class.
*Impact:* the one control that starts or stops the product is unnamed to
assistive technology; a keyboard user can Tab to it but sees no focus ring.
Automation cannot press it by coordinate (the elevated window drops injected
input; W1 evidence).
*Fix:* `response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true,
&label))`, paint a ring from `response.has_focus()`, keep `primary_action()`
(`ledger.rs:39-57`) as the single label source.
*Effort:* S.

**B2 — `Disconnect` is one-way from the window; the state after it is a dead end.**
*Where:* the only relay action in the config card is the `Disconnect` link shown
while connected (`app.rs:2783-2802`); `toggle_relay_connection`
(`app.rs:2051-2060`) is reachable only through it; the action after disconnecting
is a disabled `OPTIMIZE MY ROUTE` with no reason (`ledger.rs:52-56`,
`app.rs:4199-4235`). The tray menu is the only other path (`app.rs:4045-4056`).
*Rule:* no dead ends; a control that cannot show its state is a defect.
*Impact:* verified live in W1 (`window-disconnected-320.png`) and unchanged:
a player who disconnects to compare direct vs relayed has no control to get back.
*Fix:* make the slot a toggle (`Disconnect` ↔ `Connect`), wire the unused
`action.connect`/`action.connecting` keys (`en.toml:158-160`), show the reason
when the action is disabled.
*Effort:* S.

**B3 — A failed boost is silent: the window looks healthy while the product is not running.**
*Where:* `start_boost` logs and returns on error (`app.rs:1569-1581`); the
auto-start latch holds regardless (`app.rs:1997-2043`); engine errors are read
only for the tray colour (`app.rs:4090-4104`). Live W1 evidence:
`ERROR start_interceptor failed: WinDivert.dll not found …` while the window
showed `CONNECTED` with an enabled action
(`window-game-detected-start-failed-silent.png`).
*Rule:* state honesty (No. 1), all states including error, "errors say what
happened and how to fix it".
*Impact:* the user presses or waits for a boost, nothing happens, and the window
offers no evidence anything was attempted. The engine's own comment expected a
visible prompt (`engine.rs:1461-1466`).
*Fix:* an error surface (rail sentence + detail) fed by the start result and the
four `*_error` fields; do not latch auto-start over a failure the user has not
seen.
*Effort:* M.

**B4 — Control-plane registration failure is invisible; `CONNECTED` can be false.**
*Where:* `EngineStatus.control_registered` / `registration_error`
(`engine.rs:78-87`, `1495-1507`) are never read by `client-gui/` (grep: zero
matches). The rail's `CONNECTED` is the keepalive loop (`engine.rs:60-62`), which
can run while the relay refused the session registration — the refusal class that
dominates the fleet's M1 counters (`current-phase.md`, 2026-11-18 F-02).
*Rule:* state honesty; "connected" must mean what the user thinks it means.
*Impact:* a rejected client sees a healthy window that will never relay.
*Fix:* distinguish `CONNECTING` / `CONNECTED` / `RELAY REFUSED` from
`connected && control_registered`, with the reason from `registration_error`;
this also gives the contract's `Connecting` row its home.
*Effort:* M.

### Critical

**C1 — The starvation banner blames the relay for a fault the client cannot establish.**
*Where:* banner copy `en.toml:184` rendered at `app.rs:2270-2274`; key choice
`app/watchdog.rs:508-519`; the fault's own doc says the cause is "the relay's or
the game server's" (`watchdog.rs:97-102`); the test that pins the banner states
"the route-card banner blames the relay only when the relay is at fault"
(`tests.rs:354-358`) — but `NoReplyFromRelay` means “no datagram came back”, which
a silent game server produces identically.
*Rule:* claim only what the client measures (`interface.md:138-141`);
`lightspeed-gui-design` No. 1.
*Impact:* the new flagship warning can accuse a working relay (or a quiet game
server) with a red, unmissable banner; a player who switches relays on it wastes
a match — the same failure mode the old fallback had, in words instead of a route
change.
*Fix:* state the measurement, not the verdict:
`No game traffic is coming back from this relay yet` (and keep
`Local injection is failing` for that measured fault); add a relay-side
starvation counter only when it is measured (the delta added those counters —
use them before the sentence blames the network).
*Effort:* S (copy) + S (counter gate).

**C2 — `OPTIMIZING` is shown while traffic is still held on the direct path.**
*Where:* rail decision `app.rs:2079-2090`; `holding` requires
`tunnelling_enabled == Some(false)` **and** a detected server
(`app.rs:1845-1850`); before a server locks, the hold is real
(`app.rs:1593-1618`; trace: `Holding traffic on the direct path until the game
server is known`) but the rail shows `OPTIMIZING`.
*Rule:* `interface.md:131` "`OPTIMIZING` is only true while traffic is actually
tunnelled" — the exact class of bug the contract exists for.
*Impact:* the one word answering the one-second question is wrong at the most
common boost-start moment (game just launched, server not yet known).
*Fix:* derive the rail from the engine gate (`tunnelling_enabled`) rather than
from the `holding` proxy, and show `HOLDING` + the contract's sentence for both
the pre-server and post-server hold.
*Effort:* S.

**C3 — `Relayed` can show an impossible direction pair and contradict the warning.**
*Where:* `app.rs:2733-2765` combines `interceptor_tunnel_sent` (up) with
`interceptor_from_proxy` (down, which includes keepalive echoes and undecodable
frames, `engine.rs:161-166`), while the starvation verdict reads
`packets_reply_data` (`interceptor/traits.rs:271-285`). Live W1 evidence:
`0 up · 3 down` and `0 up · 6 down`
(`window-boosting-hold-320.png`, `window-stop-attempt-noop.png`).
*Rule:* "counters show both directions" from one story (`interface.md:137-139`);
one fact one source.
*Impact:* the ledger can read “N down” while the banner says no return path —
two surfaces disagreeing about the same measurement, which is exactly the
dishonesty class the new warning was meant to end.
*Fix:* count both directions from the same server-flow pair (already exposed as
`interceptor_flows`, used by `record_relay_history`), or rename the down half
`relay replies` and exclude keepalives; make the warning and the row read the
same source.
*Effort:* S.

**C4 — The headline metric is unlabelled and changes meaning.**
*Where:* `app.rs:2117-2148` — the headline is the direct ICMP server ping when
fresh, otherwise the relay leg; the only label is a hover text. The contract says
the headline is the relay leg (`interface.md:134-136`).
*Rule:* "Any number shown as ping is labelled with what it measures"; no
fabricated precision.
*Impact:* the first number the eye lands on can be either of two measurements;
a screen reader gets the number with no label at all.
*Fix:* render the leg in the rail (`242 ms relay` / `38 ms direct`) and reconcile
the contract; keep the hover as detail.
*Effort:* S.

**C5 — Settings states the hold duration wrong by 40x.**
*Where:* `settings.hold_policy` rendered with `LOCK_SETTLE` = 3 s
(`en.toml:36`, `app.rs:3471-3478`) while the hold deadline is
`HOLD_FALLBACK` = 120 s (`app.rs:97`, `1226-1238`). The retest half of this
finding is fixed (the checkbox and its false tooltip are gone, §4.2).
*Rule:* no invented precision; one fact one source; copy from the constant that
owns the fact.
*Impact:* live on the delta build (UIA: `Waits up to 3s for a relay before using
the direct path.`); the user is told a 3-second wait when it can be two minutes
of unexplained direct traffic.
*Fix:* pass `HOLD_FALLBACK` into the copy, add a test that renders the constant.
*Effort:* S.

**C6 — Internal strings are shown as user copy.**
*Where:* `update_status_line` returns the raw `Err` string
(`update.rs:234-240` → `app.rs:3938`); install failure `install failed: …`
(`app.rs:4084-4087`); the `Game server` row shows
`Auto-detecting (1024-65535)…` (`engine.rs:1217-1221` → `app.rs:2704-2716`); the
new client-trace explanation is hardcoded English (`app.rs:2350-2354`).
*Rule:* one voice, user language, 9 locales; errors say what happened and how to
fix it.
*Impact:* English diagnostics inside translated windows and a status sentence in
a measured-address slot.
*Fix:* map every engine/user error to a localized sentence at the boundary (one
error taxonomy — `architect`'s DTO work is the home) and keep the raw text behind
"Open log file"/details.
*Effort:* M.

**C7 — The nine-locale claim is not true on the honesty surfaces.**
*Where:* hardcoded English UI (§4.1 #18 full list, e.g. `Routing`
`app.rs:3416`, Components `3486-3543`, tray `platform/windows.rs:52-55`); and
ja/ko/zh-Hans carry **45 English-identical keys** (the 26 pre-delta set plus 19
added by the delta), including every new
starvation/location/trace string and the older state vocabulary (§4.1 #7). The
recorded decision (`interface.md:238-240`) covers **7** keys. Live: German
Settings still shows `Routing` (`window-settings-routing-320-de-delta.png`).
*Rule:* nine locales is a product rule (`lightspeed-gui-design` §3).
*Impact:* a Japanese/Korean/Chinese player reads the warning and the location
ladder in a language they did not choose — the exact surfaces that explain
failure and uncertainty. The German evidence shows the surrounding copy is
translated, so the debt is not a pipeline artefact.
*Fix:* catalog the hardcoded strings; translate the state/route/location
vocabulary; re-record the English-fallback decision with the real key list (or
shrink it). `scripts/Localization.ps1` exists — the gap is copy work, not
tooling. The “CJK font subset” rationale (`current-phase.md:122`) does not hold:
the GUI ships Noto JP/KR/SC.
*Effort:* M.

**C8 — The elevation action exits the process before UAC resolves.**
*Where:* `platform/windows.rs:266-280`: spawn hidden PowerShell `Start-Process
-Verb RunAs`, then `std::process::exit(0)` unconditionally. A declined or failed
UAC prompt leaves nothing running; the spawn result is never read.
*Rule:* no dead ends; a user-triggered state must be recoverable; "never draw an
action that cannot act".
*Impact:* the client is lost mid-session with no message; the user must relaunch
from the Start menu.
*Fix:* keep the current process alive until the elevated instance confirms
startup (handshake via the existing single-instance guard,
`single_instance.rs:97-120`), show "Restarting as administrator…", and surface a
refused/failed spawn.
*Effort:* M.

### Major

**M1 — The action invites elevation even when elevation cannot produce the outcome.**
*Where:* `primary_action` returns the elevation label whenever `!is_admin`
regardless of relay/connection (`ledger.rs:47-51`; pinned by
`tests.rs:1986-2030`, `2939-2959`).
*Rule:* never draw an action that cannot act; outcome-named button.
*Impact:* on a fresh unelevated install with no relays the loudest control
promises "optimize" and delivers a relaunch into the same empty state.
*Fix:* gate the elevation label on a relay existing and a game being startable,
or show the missing precondition beside it.
*Effort:* S.

**M2 — The rail and the Game row give opposite instructions.**
*Where:* `Relay ready, pick your game` (`en.toml:180`, `app.rs:2104-2111`) vs
`No game detected - Auto will follow one when you start it` (`en.toml:68`,
`app.rs:2838-2844`); both visible at once (live today,
`window-connected-320-delta.png`).
*Rule:* one fact one source; the mode the user chose owns the sentence.
*Impact:* the user is told to act and not to act on the same screen.
*Fix:* derive the rail sentence from `game_auto` / `game_manual` /
`selected_custom` (extend the `game_caption` helper, `ledger.rs:220-230`).
*Effort:* S.

**M3 — The detected game is shown as the internal registry key.**
*Where:* `auto_detected_game` holds the key (`choice.rs:524-540`); shown raw at
`app.rs:2106` (`Relay ready, fortnite selected`), `2825-2837`
(`Game found: fortnite`), `2999` (`Auto — fortnite`), while the boosting line
uses the display name (`Fortnite → …`, `app.rs:2103`). Live in W1
(`window-game-detected-start-failed-silent.png`, `window-boosting-hold-320.png`)
and unchanged.
*Rule:* one fact one spelling; user-facing names come from the display name.
*Impact:* the product's second-most-important fact reads as an unfinished
feature; the same game is two names on one screen.
*Fix:* resolve `(key, display)` once (the `pick_game_entry` helper already
returns `display`, `app/picker.rs:28-55`) and use `display` everywhere
user-facing.
*Effort:* S.

**M4 — The mobile-only window does not remember its geometry and resizes itself between states.**
*Where:* width pinned 320, height 420-1400 (`main.rs:110-115`,
`app/window.rs:35-39`); content height is auto-computed and pushed as
`ViewportCommand::InnerSize` whenever it changes by >8 px (`app.rs:4278-4289`);
no size/position persistence anywhere. Measured heights for one state family
(W1): 866 (EN connected), 803 (disconnected), 988 (EN boosting), 914 (DE
connected; 914 today again).
*Rule:* 2026 window floor — remembers size/position; a mobile-only window still
should not jump (`lightspeed-gui-design` §2).
*Impact:* the action bar and footer move vertically between states, under a
pointer that is aiming at them; the user cannot say where the window will be next
launch. The pinned width itself is now a recorded product decision and is not a
finding.
*Fix:* persist height (and position where the platform allows) and stop
auto-resizing on transient state changes; choose one height per launch.
*Effort:* M.

**M5 — The route card's content no longer fits the one width the product ships.**
*Where:* under the rail the card can draw: the warning banner (`app.rs:2261-2277`,
~3-4 lines), the map (132 px), up to 4 location lines (`2289-2331`), the trace
caption (`2339-2355`), a ledger of up to 11 rows (`2394-2473`: Relay, Server
ping, Server leg, Expected path, Relay→server (app), Route, Relay RTT, Path,
Target, Game server, Relayed), the cannot-beat caption (`2364-2377`) and the
sparkline (`2382-2386`). Several values are 17 px mono strings that wrap to 2-3
lines (§4.4). At 320 px the card alone can approach the whole window, pushing the
config region (the only scrolling part) to a sliver on a 1080p monitor whose
height limit is ~968 pt (`window.rs:43`, `105-113`).
*Rule:* "one glance answers the one-second question"; the card earns its pixels
at every size (`interface.md:145-148` has the general rule even though its
breakpoints are retired).
*Impact:* on the only shipped width, the instrument becomes a wall of text and
the inputs the user came for are below the fold.
*Fix:* a prioritisation pass in W2 — at most 5-6 ledger rows by default, details
behind one disclosure row, location/trace as one compact block; shorten the mono
values (§4.1 #6) and remove the sparkline in v1 (owner decision,
`current-phase.md:46`).
*Effort:* M.

**M6 — A disabled primary action never explains itself; first run has no guidance.**
*Where:* disabled fill `ledger.rs:89-113` (no tooltip/reason); readiness computed
at `app.rs:4199-4221` and nothing renders *why*.
*Rule:* a control that cannot show its state is a defect; empty states invite the
next action.
*Impact:* the first-run user sees a dead bright slot and no next step.
*Fix:* render the unmet precondition under the action ("Connect to a relay to
start") or keep the slot live and open what is missing.
*Effort:* S.

**M7 — Discovery-failure recovery is hover-only, untranslated and has no retry loop.**
*Where:* `app.rs:2888-2904` (reason in hover only; hardcoded fallback sentence
`2899-2901`); relay-refresh failure `2804-2818`;
`settings.hold_policy`/`Refresh relays` not signposted; discovery only re-runs on
a click (`app.rs:767-773`, `3437-3454`).
*Rule:* errors say what happened and how to fix it; content on hover must be
reachable without hover.
*Impact:* a transient failure at launch leaves a permanent no-relay state unless
the user finds the small Retry link or Settings.
*Fix:* body-text error line with a cause and a `Try again` button; bounded
auto-retry with backoff on startup failure.
*Effort:* S.

**M8 — Facts that only exist on hover.**
*Where:* relay-picker measured legs ("Median of five probes…",
`app.rs:2947-2966`), failure reasons (`2804-2818`, `2896-2904`), the cannot-beat
arithmetic (`2364-2377`), the port-range explanation (`3192-3201`), the
telemetry hover (`3390-3394`), the **new** geofeed provenance
(`app.rs:2313-2317`, `location_geofeed_source`) and the **new** client-trace
explanation (`2350-2354`).
*Rule:* keyboard-complete; no hover-only affordance; assistive tech must get the
fact.
*Impact:* the facts that make the new honesty surfaces meaningful (what the trace
is, where the geofeed came from) are mouse-only; `on_hover_text` is not focusable
in egui.
*Fix:* reserve hovers for extra detail; put decision-changing facts in a visible
caption or a focusable disclosure.
*Effort:* M.

**M9 — Prose and controls break the one-gutter rule at 320 px.**
*Where:* the route card wraps the map **and** the whole location/trace block in
`ui.vertical_centered` (`app.rs:2278`), so location captions, the hosting hint,
the geofeed note and the trace caption all render centre-aligned and ragged while
the ledger below is left-aligned; the Settings relay rows are centred
(`app.rs:3798-3839`); the config card is centred by `ui.vertical_centered`
(`app.rs:4255-4270`), so `Relay` / `Game` labels and the game hint are centred
(live: `window-connected-320-delta.png`,
`window-settings-routing-320-delta.png`, both German captures).
*Rule:* one gutter; nothing centres except the map (`interface.md:199-207`).
*Impact:* the screen reads as several layouts stacked; the ladder's careful
wording looks like captions under a picture rather than a labelled fact.
*Fix:* left-align the location/trace block under the map (keep only the map
centred) and left-align the config card and relay rows.
*Effort:* S.

**M10 — Several controls are below the 24 px pointer-target floor.**
*Where:* `Disconnect` and `Retry` are `ui.link` labels at BODY 13
(`app.rs:2793-2800`, `2813`, `2902`) — ~19 px tall; the relay delete `×` is a
14 px `Button` (`app.rs:3817-3821`).
*Rule:* minimum 24 px pointer target (`lightspeed-gui-design` §1.3); <24 px is
Major on the ladder.
*Impact:* motor-impaired and touch users mis-click; the delete `×` is
destructive and tiny.
*Fix:* give links a 24-32 px hit area and confirm before delete.
*Effort:* S.

**M11 — Destructive actions have no confirmation or undo.**
*Where:* relay `×` removes and persists immediately (`app.rs:3817-3821`,
`3873-3879`); `Reset to defaults` deletes `config.toml` and reconnects
(`app.rs:3865-3902`, `config.rs:289-300`).
*Rule:* safety floor for destructive actions; state honesty.
*Impact:* one stray click loses the custom relay list or every setting.
*Fix:* confirm `Reset`; offer an undo/re-add for a deleted relay; keep the
stable-firewall-rule discipline as the model.
*Effort:* S-M.

**M12 — The tray is English-only, its menu lies about its state, and its error says nothing.**
*Where:* menu copy hardcoded and `Connect`/`Disconnect` always enabled
(`platform/windows.rs:52-68`, `102-128`); tooltips hardcoded
(`146-158`); the error tooltip is `— error` with no detail (`158`); `RTT {n}ms`
uses a value that is 0.0 before the first sample (`app.rs:4101-4103`);
the new starved tooltip is correct in fact but English-only.
*Rule:* nine locales; state honesty; tray actions mirror the window.
*Impact:* the tray is the only surface for a hidden window; it cannot say what is
wrong, and its menu offers no-ops.
*Fix:* localize tray copy via `i18n::t`, enable items from `TrayState`, and let
the starved/error states open/point at the window with a detail line.
*Effort:* S-M.

**M13 — The update flow dead-ends and states nothing about what it contacts.**
*Where:* `app.rs:3905-3988`; installer only for the receipt-less Windows path
(`update.rs:186-213`); raw errors (`update.rs:234-240`); tag spelling
`v1.11.4` vs `1.11.4` (`app.rs:3924-3936`); the GitHub disclosure lives only in
docs (`privacy.md:186-204`, `data-dictionary.md:25-37`), never in the sheet.
*Rule:* 2026 integration floor — "updater shows progress, version and failure";
honest disclosure in the surface that acts.
*Impact:* `Update available` without an install path (shell installer and every
non-Windows install) and no instruction; the user cannot tell what pressing the
button sends.
*Fix:* always offer an actionable path (installer or `Open releases page`),
normalise versions, map errors to sentences, add one disclosure line to the
sheet; a check that finds an update may mark the footer.
*Effort:* M.

**M14 — The traceroute probes are not in the privacy policy's network list.**
*Where:* `privacy.md:178-184` says "The only network connections are:" four
items; `privacy.md:83-99` lists the *measurements* as ICMP + shadow packets only.
The delta added a **client-side `tracert`/`traceroute` to the game server**
(`client-gui/src/client_trace.rs:1-30`, spawned at `app.rs:1275-1293`) and a
**relay-side TTL trace** (`proxy` `MSG_SERVER_TRACE`). Neither appears in the
policy or the data dictionary (`data-dictionary.md:25-37` covers only the update
check). The commit that added the client trace touched no privacy doc
(`git show --stat 7f80701`).
*Rule:* `[PRIVACY_STUB]` — the privacy policy stays public and accurate;
`[TRANSPARENCY_STUB]`.
*Impact:* the policy claims completeness while two probe families are missing;
the product's credibility rests on being the transparent alternative.
*Fix:* add both traces to the network list and the local-measurement section
(destination, cadence, payload, retention), and record them in the data
dictionary.
*Effort:* S.

**M15 — The `Expected path` row uses the same sentence as its label and its value, leaking `{ms}`.**
*Where:* label `i18n::t("route.expected_path")` at `app.rs:2425` (the raw
template with `{ms}`), value from `expected_path_text`
(`app/choice.rs:295-302`) which is the same key with `t_with`. The row renders
as `Expected path ~{ms} ms (L + S)` next to `Expected path ~480 ms (L + S)`.
*Rule:* one fact one source; no placeholder leak; one row one number.
*Impact:* a visible malformed string on the primary surface, and the same fact
printed twice.
*Fix:* add a `route.expected_path_label` key (or a plain `Expected path` label)
and shorten the value (`~480 ms (relay leg + server leg)`); add a render test for
the row's label/value pair.
*Effort:* S.

**M16 — The starvation remedy copy omits what the remedy costs, and points at a log without a path.**
*Where:* `en.toml:184-185`; picking a relay during play calls
`connect_selected` → `restart_interceptor_on_selected_relay`
(`app.rs:716-769`), i.e. the game's connection restarts — the banner says only
"pick another relay"; `check the log` has no in-app path (the control is
Settings → Maintenance → `Open log file`, `app.rs:3445-3447`).
*Rule:* no promised action without its cost; outcome-named next step.
*Impact:* a player on a starving relay may switch mid-match and lose the join,
having been told it was free; the injection-fault path sends them hunting.
*Fix:* `…pick another relay (the game's connection restarts)` and
`…open the log in Settings`; alternatively a `Stop / Switch relay` pair in the
banner itself.
*Effort:* S.

### Minor

- **m1 — Two spellings of one version:** `Current version: 1.11.4` vs
  `Latest version: v1.11.4` (`app.rs:3924-3936`). Normalise once.
- **m2 — Separator/format drift:** hardcoded `{provider} · {region}` and
  `AS{asn} · {name}` (`app.rs:442`, `469`), `~` floor marker, `-` in catalog
  sentences vs `—` in others (`en.toml:55, 66, 208`). Pick one separator per role.
- **m3 — “Route” means two things:** the card/section (`en.toml:171`) and the
  traceroute ledger row (`en.toml:200`) — `Path trace` for the row.
- **m4 — `Server location:` prefix repeated on 7 keys** (`en.toml:203-209`),
  which at 320 px spends a third of each line on the prefix; a label + value
  block fixes it with M9.
- **m5 — The RTT sparkline still has no axis, unit or label** and is drawn from
  the first keepalive samples (`widgets.rs:437-470`, `app.rs:2382-2386`). The
  owner decision removes it in W2 v1 (`current-phase.md:46`); if that changes,
  it needs a fixed scale and a label.
- **m6 — The port-range hover references a label that no longer exists:**
  "Use this if Packets Sent stays at 0 after 15 s" (`app.rs:3192-3201`); the
  shipped row is `Relayed {sent} up · {returned} down` and the 15 s threshold
  exists nowhere in code.
- **m7 — Five literal colour sites remain outside the token module**
  (`globe.rs:71-73,125` `from_rgb` ×4 + `:133` `Color32::WHITE` ×1; definition
  from r5 §3.1), so the map cannot theme with `design.rs`. W2-2's scope now
  includes `globe.rs` (QA R1), which is why this is Minor here.
- **m8 — A disclosure sentence is stronger than the code:** `privacy.md:202`
  says “Closing the update dialog stops it”; closing sets the UI to idle
  (`app.rs:3977-3985`) but the in-flight request is not cancelled (the worker
  thread completes and the result is parked). Tighten the sentence or the code.

### Counts

| Severity | IDs | Count |
|---|---|---|
| Blocker | B1-B4 | 4 |
| Critical | C1-C8 | 8 |
| Major | M1-M16 | 16 |
| Minor | m1-m8 | 8 |
| **Total** | | **36** |

Two W1 findings are retired (C1 fallback-cause, and the width half of M4), one
was half-fixed (C5 retest copy), and four are new (C1 starvation attribution,
M5 density, M14 trace disclosure, M15 placeholder leak).

---

## 6. Usability test plan (executable by `qa-engineer`)

### 6.0 Method and automation constraints (updated for the delta)

- **Build/run:** `$env:CARGO_TARGET_DIR=%TEMP%\qa-target; cargo build -p
  lightspeed-gui`. For boost tests copy `WinDivert.dll` + `WinDivert64.sys`
  beside the exe. The released 1.11.4 GUI at
  `<repo>\target\release\lightspeed-gui.exe` is also usable for copy/layout
  cases (this revision captured it).
- **Elevated windows:** if the QA session is elevated and the automation daemon
  is not, `SendInput`/`PostMessage` are dropped by UIPI. Drive **real buttons**
  through UI Automation (`InvokePattern`) and read state from the UIA tree +
  window screenshot. **The primary action is a custom-painted widget with no
  accessible name** (B1): coordinate injection cannot press it and UIA exposes
  it as `Custom` — record that as the expected failure of T3/T5/T16 until B1 is
  fixed.
- **Popups:** egui popups are invisible to UIA; only a screenshot proves an open
  combo. A driver click cannot open a combo under UIPI — use a non-elevated
  session for T6/T7.
- **States that need a live game or a hostile relay** are marked *fixture
  required*; if no fixture is available, report **unverified** — never fabricate
  a state (`lightspeed-gui-design` §8, `interface.md:245-261`).
- **Screenshots:** save to `wat/evidence/<date>/qa/`; capture at **320 px** (the
  only width) plus the actual height, and note the height.
- **Run each case twice:** EN, then DE via top-level `language = "de"` in
  `%APPDATA%\light-speed\config.toml` (back up first), plus ja once C7 is fixed.

### 6.1 Cases

| ID | Case | Precondition | Steps | Expected observation | Pass/Fail |
|---|---|---|---|---|---|
| T1 | One-second read | connected, no game | open the window; read rail word, sub-line, metric, card in <2 s | word describes the route; metric has a visible unit + what it measures | FAIL today (C4) |
| T2 | First run, no relay | fresh config dir; network | launch; observe 10 s | a next step is visible; no disabled bright action without reason | FAIL today (M1/M6) |
| T3 | Boost start failure is visible | exe folder without WinDivert files; detectable process | launch; wait for auto-start | the window states the failure in the user's language with a next action | FAIL today (B3) |
| T4 | Reconnect | connected | press `Disconnect`; reconnect **from the window only** | a Connect control exists and works; disabled action explains itself | FAIL today (B2) |
| T5 | Boost/stop, keyboard only | elevated build with WinDivert; detectable process | Tab through the window; Space/Enter; check focus visibility | every control reachable, focus ring visible, action starts/stops | FAIL today (B1) |
| T6 | Relay picker content | connected | open Relay combo (non-elevated session), screenshot | rows show label + measured leg; chosen-for-server marked; fits/scrolls at 320 | screenshot-only |
| T7 | Game picker size | connected, 28 built-ins + ≥1 custom | open Game combo, screenshot | usable at 320 (scrolls, selection visible, `New custom game…` reachable) | unverified today |
| T8 | Elevation path, declined | **non-elevated** session | press action; decline UAC; then accept once | declining leaves the app running with an explanation | FAIL today (C8) |
| T9 | Tray round-trip | tray present | X (hide), restore via tray/double-click; tray Quit | hide/restore/quit work; tooltip names the real state; no no-op items | partial (M12) |
| T10 | Second launch | first instance running | launch again | notice names the tray as the way back and exits | PASS today |
| T11 | Update flow + disclosure | network | Settings → `Check for updates`; close during check; reopen | a result or clear retry; no stuck state; **the sheet says what it contacts** | FAIL today (M13: no disclosure; raw-error branch) |
| T12 | Error surface | any engine `*_error`/registration-error fixture | trigger; read window and tray | window names what failed in user language; tray and window agree | FAIL today (B3/B4) |
| T13 | **Starvation warning** *fixture required*: a live game whose relay forwards but returns <1 reply in 8 for two 8 s windows | elevated build + game + hostile relay (e.g. a controlled relay that drops returns) | start match; wait for the banner; then STOP OPTIMIZING; then repeat and switch relays | rail `NO RETURN PATH` (danger) + banner with a cause the client can prove; tray amber; counters and banner agree (C3); switching relays restarts interception, as the copy says (M16); a healthy window clears the warning with no “recovered” banner | NOT RUN in this revision (no fixture, §9); see tests `app/tests.rs:155-380` for the logic |
| T14 | **Location labels** *fixture required*: a cloud-range server, a hosting-range server, a geofeed server, an ordinary server | elevated build + a controlled server address per class | lock a server; screenshot the card | the five ladder states say the right source; hosting/geofeed draw **no** dot; geofeed shows place + provider/as-of + shown-as-is + source on hover; no false country | partial: geofeed was live-verified by the delta session (`current-phase.md:113`); run all five |
| T15 | **Route trace** *fixture required*: server lock + a relay; a filtered path | elevated build + game | lock, wait for the trace; inspect caption + `Route` row | caption `Route to server: N hops · …` and row `N hops · last router RTT M ms` (or `no router answered`); no city is claimed; the explanation is reachable (today only by hover — M8) | unverified today; `CLIENT_TRACE` live sample in `current-phase.md:143` |
| T16 | **320 px full-density card** | force a state with server + server-leg + trace + warning where possible (or construct with the documented rows) | screenshot the window at 320; measure the card height, the **remaining config-region height**, and whether the pinned action bar and footer are still on-screen | PASS only if the config region keeps at least ~120 px usable and every pinned band stays visible; no ledger row wraps to more than 2 lines at 17 px mono | **estimated to fail** (M5; arithmetic in §4.4) — measure first |
| T17 | Location prose alignment | any server lock | screenshot; check the location lines sit on the same left gutter as the ledger | left-aligned; only the map is centred | FAIL today (M9) |
| T18 | Stale/duplicate counters | boosting with a quiet game | compare `Relayed` up/down with the warning state; watch for `0 up · N down` | both directions from the same source; no impossible pair; banner and row agree | FAIL today (C3) |
| T19 | i18n sweep (new copy) | EN/DE/ja/ko/zh | per locale: warning banner, location labels, expected path, trace caption, settings policy | no English left on a translated surface (documented exemptions only) | FAIL today (C7: 45 keys in ja/ko/zh = 26 pre-delta + 19 delta; hardcoded `Routing` in DE) |
| T20 | Destructive actions | custom relay added; settings changed | `×` delete a relay; `Reset to defaults` | confirmation or undo; recoverable | FAIL today (M11) |
| T21 | **30 px state-word + RU restart-label fit** (r16) | 320 px window in EN, DE, RU (plus ES/PT-BR once shortened) | force each state word (connecting/connected/not-connected/holding/optimizing/no-return-path/error) and the unelevated restart action; screenshot the rail and action bar | every state word is one line at 30 px with ≥12 px slack (≤276 px measured); RU restart label wraps to exactly **2 lines** and the action bar stays on-screen; no 9 px shrink | **at HEAD** RU overflowed 342.8 px and the restart was 3 lines; **RU is landed** on the current tree (222.9 px / 2 lines via `ru.toml` r16) and the capture proves it — **ES 309.0 / PT-BR 328.4 remain open** until their copy lands |

Pass/fail is per the acceptance line, not "it looked fine". A case that cannot be
set up is **unverified** with the missing fixture named.

---

## 7. Handoff to the delta spec / W2

The `gui-refresh-spec` synthesis (being re-baselined in this delta wave) should
consume:

1. **Contract amendments A7-A13** from §3.0, including the new warning row with
   its driving facts and clear rule, and the deletion of the three retired
   states.
2. **The warning's claim discipline** (C1): banner copy states the measurement,
   and the relay counter family is used only once it is measured; the same
   counters feed `Relayed` (C3).
3. **The copy deck** in §4.1, one row per shipped string, each number generated
   from the constant that owns it (C5, M15).
4. **Mobile-only shell decisions:** one height per launch, persisted geometry,
   a route card that fits 320 px with a prioritised row set (§5 M4, M5).
5. **The i18n plan** (C7): catalog the hardcoded strings and translate the
   state/location/trace vocabulary in ja/ko/zh (45 keys = the 26 pre-delta keys +
   19 added by the delta), or record the real fallback decision.
6. **Disclosure**: the in-app update disclosure line (M13) and both traceroutes
   in `privacy.md`/`data-dictionary.md` (M14).
7. **The failure surface** (B3/B4/C6) — the boundary DTO work is its natural
   home; the W1 spec already names `AppStatus.error`.
8. **The 30 px state-word fit** (Q15, r16): one line ≤276 px per locale in the
   installed chain; shortened RU/ES/PT words; the RU restart label at two lines
   (`ACTION_H_WRAPPED` 56). The spec **retired the old 24.6 px fit number in r18**
   (`gui-refresh-spec.md:63`), so the “281.1 px of 288 fits” figure is no longer
   a live claim — do not reinstate it (§4.4a).

---

## 8. October 2026 re-check, and what master fixed

The Oct-2026 set (`ui-audit.md`, `ui-design-critique.md`) was never accepted and
remains historical. Checked against the delta tree:

| Oct item | Status now | Evidence |
|---|---|---|
| Top-10 #1 one card for every content type | No longer reproduces | route card + config card + sheets (`app.rs:2188`, `4255-4270`) |
| #2 ALL-CAPS eyebrow | No longer reproduces | `section_label` sentence case (`widgets.rs:391-399`); only state word/action caps |
| #3 `WORD - fragment` / pipe-joined counters | Mostly fixed; dashed copy remains in two catalog strings | `en.toml:55`, `:66`, `:216` |
| #4 mono as costume / unused HEADING | No longer reproduces | `theme.rs:191-201`; ledger mono is data only |
| #5 default egui chrome | Partially — OS title bar still the light system bar; popups unverified | every capture |
| #6 eight inline literals / palette hole | app.rs fixed; 5 literal colour sites in `globe.rs` (m7) | `grep` |
| #7 loudest control not the product action | No longer reproduces in structure; elevation remains loudest by design and is named | `ledger.rs:39-57` |
| #8 disconnected card claims live data | No longer reproduces | W1 `window-disconnected-320.png` |
| #9 controls buried in `More` | No longer reproduces | Settings sheet |
| #10 footer never renders / 320 clipping | No longer reproduces for the footer; 320 is now the only width and the *density* is the new risk (M5) | live captures |
| Critique req. 1-16 | The r5 status table still holds except: req. 11 literal migration (5 sites, m7), and the reduced-motion requirement (req. 13 — the shell has no animation, so there is nothing to gate; keep it that way) | r5 §8 + §0.2 |
| `interface.md` open issue: window absent from OS window list while a game has focus | Not reproduced with a synthetic process; remains open | `interface.md:226-229` |
| `interface.md` open issue: sparkline has no axis/label | Still open (m5; W2 removes it) | `widgets.rs:437-470` |
| `interface.md` open issue: Components labels English-only | Still true; and the exception does not cover the new hardcoded English (C7) | live delta capture |
| `interface.md` open issue: 7 route.server_leg* strings English | **Worse**: 45 keys in ja/ko/zh (26 pre-delta + 19 delta) (C7) | locale comparison |
| `interface.md` open issue: measured server leg reachable? | Improved by the relay-measured rows (F11/F12); the *estimator* measured branch remains as documented | `app/choice.rs:183-302` |

---

## 9. Unverified, blocked, and assumptions

Named so no reader mistakes a plan for evidence:

1. **Starvation warning/banner/tray live state** — not observed in this
   revision: it needs a live game and a relay that forwards but returns nothing;
   no safe fixture exists on this host (the W1 live runs that produced the
   pre-delta fallback are evidence of the old behaviour, not the warning).
   Code + unit tests cited (`app/tests.rs:155-425`).
2. **Location ladder live states** — cloud, hosting, country and unknown were
   not re-captured; only the operator-geofeed state was live-verified by the
   delta session (`current-phase.md:113`). Ladder rules and unit tests cited
   (`server_place.rs:252-351`, `app/tests.rs:983-1076`).
3. **Route trace rows/caption** — not rendered in this revision (needs a locked
   server with traffic); logic tests and the `CLIENT_TRACE` live sample cited
   (`client_trace.rs:769-1029`, `current-phase.md:143`).
4. **Relay-measured server-leg rows** — not captured (needs live relay reports);
   unit tests cited (`app/tests.rs:2961-3080`).
5. **320 px worst-case card density** — estimated from the row inventory and the
   measured wrapped strings (§4.4), not measured in a fully populated state.
6. **Elevation-required state and UAC decline** — this session is elevated; the
   W1 capture limitation stands. Code-cited (`platform/windows.rs:266-280`).
7. **egui popups** (relay/game pickers) — not openable under injected input on
   the elevated window; UIA cannot see them. Content cited from source.
8. **Screen reader announcements** — the unnamed `Custom` action is a live UIA
   fact (captured today), but no NVDA/Narrator session was run.
9. **ja/ko/zh rendered layout** — only catalog data was compared (45
   English-identical keys: the 26 pre-delta set + 19 delta-added); German was
   captured live. No CJK screenshot. RU 30 px copy is measured but not yet
   rendered in the shell (T21).
10. **Update-available / installer branches** — only the up-to-date sheet is
    captured (W1) and the code read; no such release/install receipt here.
11. **200 % DPI, Linux, macOS** — untouched; `desktop-platform` owns the parity
    plan; the mobile-only decision does not change those tiers.
12. **Relay-side starvation counters** (added in `76aba5d`) were not exercised
    live (needs the post-1.11.4 proxy deployed to a relay); the banner must not
    claim relay-side certainty until they are (C1).

---

## 10. Evidence index

First pass (W1, 2026-11-19) — `wat/evidence/2026-11-19/team/ux-lead/`:
`report.md`; `window-connected-320.png`; `window-disconnected-320.png`;
`window-settings-sheet.png`; `window-settings-sheet-de.png`;
`window-update-sheet.png`; `window-connected-320-de.png`;
`window-game-detected-start-failed-silent.png`; `window-boosting-hold-320.png`;
`window-stop-attempt-noop.png`; `config-backup-before-ux-audit.toml`;
`revision-r5.md`.

Delta revision (2026-11-20) — `wat/evidence/2026-11-20/team/ux-lead/`:

| Path | Contents |
|---|---|
| `delta-r10.md` | working notes: method, what was verified/skipped, side effects and cleanup, old→new finding mapping |
| `window-connected-320-delta.png` | live shipped 1.11.4 GUI (started by the release verification), connected/idle, 320×866 — shows `CONNECTED`, the unchanged M2/M3/B1 facts |
| `window-settings-routing-320-delta.png` | Settings → `Routing` at 320: English `Routing` label, the 5-line starvation policy, the still-wrong `Waits up to 3s…` hold copy, English Components |
| `window-connected-320-de-delta.png` | same window in German, 320×914 |
| `window-settings-routing-320-de-delta.png` | German Settings at 320: `Routing` still English, 5-line translated policy (229 chars) |
| `config-backup-before-delta.txt` | GUI config used for the captures (restored afterwards) |
| `delta-r16.md` | r16 working notes: RU/ES/PT 30 px measurements, Q15 answer, chrome cost, audit-count fixes, coordination with `design-system` |
| `ru_state_fit.py` / `ru-state-fit-output.txt` | the measurement script and its output (imports the design-system's `fonts.py`/`delta-copy.py`; §0 calibration reproduces the @24.6 table exactly) |

Also changed by r16 (copy only): `client-gui/locales/ru.toml` —
`state.not_returning = "Нет возврата"` (222.9 px @30) and
`action.restart_admin_optimize = "Перезапустить от имени администратора"`
(356.9 px @15 = 2 lines); key set unchanged, so the locale-parity test is
unaffected.

Canonical text evidence referenced throughout: `docs/design/interface.md`
(unchanged, now stale — §3.0), `docs/design/gui-refresh-brief.md` (delta version,
mobile-only at `:4-6`, `:323`), `docs/privacy.md:83-99, 178-204`,
`docs/data-dictionary.md:25-37`, `wat/state/current-phase.md:15-26, 65-75,
96-104, 106-125, 127-136, 138-146`.
