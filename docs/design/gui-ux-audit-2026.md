# LightSpeed desktop UX audit — WF-081 W1 (2026-11-19)

**Status:** W1 design input. Docs only; no GUI, engine, config or fleet change was
made. Owner acceptance of the W1 spec is the gate before W2.
**Tree:** worktree `.agents/worktrees/dsh-b74d064d-6fe5`, HEAD `62c6cf6`
(`client-gui/` clean; `cargo build -p lightspeed-gui` from these sources).
**Owner of this document:** `ux-lead` (task-2). **Sibling W1 docs:**
`gui-core-boundary.md`, `design-system-2026.md`, `cross-platform-2026.md`,
`gui-refresh-spec.md` (synthesis).
**Binding facts:** `docs/design/interface.md` (state contract). This audit tests
the window against it; where the two disagree the contract wins and the code is
the defect, unless the contract itself is stale — those cases are named as
contract drift, not silently resolved.
**Historical inputs, never approved:** `docs/design/ui-audit.md` and
`docs/design/ui-design-critique.md` (Oct 2026, verdict **REVISE**, 16 required
changes). §8 states which of their findings still reproduce here.

**How to read this document.** A finding that is not backed by a `file:line` or a
screenshot in `wat/evidence/2026-11-19/team/ux-lead/` does not exist. Proposed
copy is written as shipped behaviour: present tense, no dates, no promises, no
unmeasured "faster". No user research exists for this product; nothing in §1 is
presented as research data — the jobs are derived from the product's own state
contract and shipped copy, and are labelled as such.

---

## 0. Verdict and leverage

**Verdict: the window has become an honest instrument in structure and remains
dishonest at the edges — exactly where the user is in trouble.** The Oct-2026
complaints about the shell are fixed: one action slot with three gate-driven
labels, a fixed rail/action/footer shell, no stale data on disconnect, Privacy/
Maintenance/About reachable in Settings, tokens in `design.rs`. What the refresh
did not fix is the **failure and recovery half of the product**: a failed boost
leaves a normal-looking window, a control-plane refusal is invisible, a
disconnected window cannot reconnect, the primary action is unnamed to
assistive technology, and several shipped numbers are simply not true
(§5 C5).

Counts by the `lightspeed-gui-design` severity ladder: **4 Blocker, 8 Critical,
15 Major, 6 Minor** (33 findings, §5).

Top three leverage fixes, in order:

1. **Make failure visible and recoverable (B3, B4, C1, C2).** One error surface
   fed by the engine's own `*_error` fields and `registration_error`; the rail
   never says `OPTIMIZING` while the tunnel holds traffic direct; `Direct
   fallback` names its cause. This is mostly wiring — the facts already exist in
   `EngineStatus` and are read only for the tray dot today
   (`app.rs:3851-3874`).
2. **Make the one action usable by everyone (B1, B2).** Give the custom action
   button an accessible name, a visible focus ring, and a Connect action in the
   window so `Disconnect` is reversible. The catalog already contains the unused
   `action.connect` / `action.connecting` keys (`locales/en.toml:157-159`).
3. **Make the copy true (C5, C6, C7).** Wire the two settings facts to the code
   constants that own them (3 s vs 120 s; 20/45/90 vs 30/120), replace internal
   strings shown as user copy, and finish the i18n debt the state surface
   actually has (26 English keys in ja/ko/zh, not the documented 7).

Owner decisions embedded in those fixes (named, not assumed): where the error
surface lives (rail sentence vs a sheet), whether the window gets a `Connect`
control or the address bar for reconnect, and whether the 320 px pinned viewport
is reopened in W2 (`gui-refresh-brief.md` §11 lists it as a spec decision).

---

## 1. Jobs-to-be-done and the one-second question

There is no user research in this repository. These jobs are derived from the
product's state contract (`interface.md:29-46`), the shipped copy and the
engine's own vocabulary; they should be validated in the usability tests in §6
before any of it is treated as fact.

### 1.1 The one-second question

> *Is my traffic being boosted, through which relay, and is the link good?*
> — `lightspeed-gui-design` §0

The window answers the three parts in three places: the rail word (what),
the rail sub-line (where: game → relay, or the fallback reason), and the rail
metric + route card (how good). A player alt-tabs mid-match and reads the top
~250 px only; everything below the route card is secondary in that second.

**Verdict on the one-second read:** structurally correct and now calm (one
bright control, no entrance motion, dark-first). It fails at one case: the
headline metric has no visible label and changes meaning between the relay leg
and the direct ICMP ping depending on freshness (§5 C4), so the second question
"how good" is answered by a number the user must hover to identify.

### 1.2 Jobs

| # | Job (context, not persona) | Trigger | Done looks like | Where the window answers | Status |
|---|---|---|---|---|---|
| J1 | *Tell me I am or am not being boosted, right now* | alt-tab mid-match, glance from a second monitor, tray hover | one word, correct for the route actually in use | rail word `app.rs:1979-1990`; tray tooltip `platform/windows.rs:142-152` | **at risk** in hold / silent-failure states (§5 B3, C2) |
| J2 | *Tell me the game is being helped, not just "connected"* | match start; game launch | game name + relay in one line, counters that only move when the game moves | rail sub-line `app.rs:1992-2013`; ledger `Relayed` `app.rs:2485-2520` | partially: counters can show `0 up · 6 down` and the game name is shown as a raw key (§5 C3, M3) |
| J3 | *Start it for me* | game appears | no click; the app detects and starts | presence worker `app::presence.rs`; auto-start `app.rs:1893-1935` | works until it fails silently (§5 B3) |
| J4 | *Start/stop it myself* | manual choice, troubleshooting | one obvious outcome-named control, keyboard reachable, reversible | action slot `ledger.rs:39-137`; click path `app.rs:3960-3999` | **not for keyboard/SR users** as an outcome; elevation errors kill the process (§5 B1, C8) |
| J5 | *Pick the right relay (or trust Auto)* | first run; new region | the relay that is best for the *game server*, with the number it was chosen on, and a way to override | route picker / `boost_server_combo` `app.rs:2631-2740` | implemented; popup not visually verified (§9) |
| J6 | *Pick the game, or a program the registry does not know* | non-registry game; a launcher | registry picker, custom profile, process target | `game_combo` `app.rs:2744-2846`; editor `app.rs:3357-3462`; target picker `app.rs:3471-3535` | implemented; 31-row popup in 320 px unverified; one-step claim confuses Manual vs Auto copy (§5 M2) |
| J7 | *When it breaks, tell me what broke and what to do* | boost fails, relay starves, relay rejects, update fails | the engine's own words + a next action, in the user's language | **absent** — errors only recolour the tray dot | **fail** (§5 B3, B4, C1, C6) |
| J8 | *Leave it running in the tray and get it back* | close window, relaunch | X hides; tray restores; a second launch explains itself | `close_decision` `app/window.rs:133-150`; tray `platform/windows.rs:101-158`; notice `single_instance.rs:76-89` | works; tray copy English-only (§5 M11) |
| J9 | *Keep it current* | new release | a visible, actionable update path | update sheet `app.rs:3667-3749`, only via Settings | **incomplete** (§5 M12) |
| J10 | *Earn trust: no hidden claims* | reading any number | every shown number names what it measures and comes from one source | ledger/server-leg rows; `interface.md` | mostly; four violations (§5 C4, C5, C6, C3) |

---

## 2. Flow inventory

Format per flow: **Entry** → **Decisions** → **Failure** → **Recovery** → **Copy**
(verbatim, `file:line`) → **Friction**. "Reachable today" notes what a stranger
can actually do in the shipped window.

### F1 — First run, no relay (and no saved config)

- **Entry:** launch with no `config.toml` (fresh install). `LightSpeedApp::new`
  loads defaults, starts discovery, and auto-connects when discovery lands
  (`app.rs:580-596`, `app.rs:1335-1364`).
- **Decisions:** none the window asks for. The relay row shows a spinner while
  discovery runs (`app.rs:2641-2647`); the Game row says
  `No game detected - Auto will follow one when you start it`
  (`locales/en.toml:67`, rendered `app.rs:2590-2596`).
- **Failure:** discovery fails and no saved relays exist →
  `No relays discovered` (warn) with the reason only in a hover and a `Retry`
  link (`app.rs:2648-2658`; hover fallback text is hardcoded English
  `app.rs:2651-2653` while the catalogued `relay.registry_empty` sentence is
  unused, `locales/en.toml:55`). No retry loop exists: discovery runs once at
  start and only on `Retry` / Settings → `Refresh relays`
  (`app.rs:691-697`, `app.rs:3210-3216`).
- **Recovery:** `Retry`; Settings → `Refresh relays`; Settings → `Relays` →
  add a proxy manually (a three-field form inside a sheet, `app.rs:3610-3633`).
  Nothing in the no-relay state points at any of these.
- **Copy:** `Discovering relays…`; `No relays discovered`; `Retry`;
  `Auto (detect)`; `No game detected - Auto will follow one when you start it`;
  action label depends on elevation: `RESTART AS ADMINISTRATOR TO OPTIMIZE`
  (unelevated) or a **disabled** `OPTIMIZE MY ROUTE` (elevated)
  (`ledger.rs:39-57`).
- **Friction:** with no relay the action slot is either a disabled button with
  no reason (elevated) or an elevation invite that cannot produce the outcome
  (unelevated) — see §5 M1/M5. First run has no "what this app does" surface and
  no path from the empty relay row to Settings → Relays. **Not reachable for
  screenshot on this host** (discovery succeeds here); code-cited only.

### F2 — Relay discovery failure with a saved list

- **Entry:** discovery fails while `proxies` is non-empty (saved or env proxies,
  `app.rs:461-466`).
- **Decisions:** keep using the saved list (the app does), retry, or fix in
  Settings.
- **Failure:** a single caption in the relay row:
  `Relay refresh failed - using the saved list` (warn, `locales/en.toml:54`,
  rendered `app.rs:2556-2569`), with the raw reason on hover and a `Retry`
  link. The app stays connected to the saved relay.
- **Recovery:** `Retry`; Settings → `Refresh relays`.
- **Copy:** the sentence above; `Retry`; the hover carries the engine's raw
  reason (`app.rs:2559-2564`).
- **Friction:** "saved list" is jargon for the previous session's relay list; the
  reason is hover-only (not focusable, not read by assistive tech in any
  equivalent form); the caption is *below* the relay picker it refers to and
  above the Game row, so it can be read as a Game warning. See §5 M6, M7.

### F3 — Connected / idle (the most common state)

- **Entry:** launch with a saved relay (auto-connects), or after discovery
  (`app.rs:591-596`, `app.rs:1353-1357`).
- **Decisions:** start the boost (action), pick a game, pick a relay, or open
  Settings.
- **Failure:** none in this state by definition; the failure *is* that the
  window looks identical when the boost start or the control-plane registration
  has failed (§5 B3, B4).
- **Recovery:** n/a.
- **Copy:** rail `CONNECTED` + metric + `Relay ready, pick your game`
  (or `Relay ready, {name} selected`) (`app.rs:1986-2013`); ledger `Relay`,
  `Relay RTT`, `Game server -- start optimizing to place it`,
  `Relayed -- start optimizing to count` (`app.rs:2191-2246`,
  `locales/en.toml:202-203`); Game row hint `No game detected - Auto will
  follow one when you start it`; footer `Hide to tray` / `Settings` / `Quit`.
- **Friction:** the rail tells the user to *pick* a game while the Game row
  (in the default `Auto (detect)` mode) says Auto will do it — two instructions
  for one fact (§5 M2). The Game row and the rail also name a detected game two
  ways: `Relay ready, fortnite selected` vs `Game found: fortnite` vs the rail's
  boosting line `Fortnite → …` (§5 M3).
- **Evidence:** `window-connected-320.png`; German variant
  `window-connected-320-de.png`.

### F4 — Boosting (auto-start or manual)

- **Entry:** a game appears and the relay is connected → auto-start
  (`app.rs:1893-1935`); or press the action (`app.rs:3986-3999`). Requires
  elevation; the unelevated path is F6.
- **Decisions:** stop; leave it; change game/relay (changing either can restart
  interception, `app.rs:669-689`).
- **Failure:** interceptor start failure → **window unchanged** (§5 B3). A fatal
  data-plane failure later (`interceptor_error`) → tray dot turns red, window
  unchanged (§5 B4). Loss of the relay return path → watchdog falls back
  (F5).
- **Recovery:** `STOP OPTIMIZING` (danger) — a custom-painted button that the
  keyboard can reach by Tab but which paints **no focus indication**
  (`ledger.rs:89-137`), and which automation cannot press by coordinate because
  the elevated window drops injected input (§6.0).
- **Copy:** rail `OPTIMIZING` + metric + `{game display} → {relay}`
  (`app.rs:1984-2005`); ledger gains `Relay RTT` (when measured), optionally
  `Server ping`, `Server leg`, `Path`; `Relayed` reads
  `{sent} up · {returned} down` (`locales/en.toml:198`); action
  `STOP OPTIMIZING`; hover hints on the metric and on the cannot-beat caption.
- **Friction:** while the engine holds traffic direct (first seconds after a
  start, until a game server is locked), the rail already says `OPTIMIZING` —
  the one word the contract reserves for traffic that is actually tunnelled
  (§5 C2; live trace + `window-boosting-hold-320.png`). The `Game server` row
  can display the engine's scanning sentence `Auto-detecting (1024-65535)…`
  instead of an address (§5 C6). `Relayed` can read `0 up · 3 down` (§5 C3).
- **Evidence:** `window-boosting-hold-320.png` (real WinDivert interception with
  a synthetic process standing in for a game; no game traffic, so counters are
  relay keepalives).

### F5 — Direct fallback + re-test

- **Entry:** the no-reply watchdog judged two consecutive 8 s windows with no
  working return path (`app::watchdog.rs:1-39`), sets `direct_fallback`, and
  schedules a re-test 30 s then 120 s (`watchdog.rs:121-122`).
- **Decisions:** wait, keep playing direct, or turn re-testing off in
  Settings → `Re-test a relay after falling back` (`app.rs:3164-3183`).
- **Failure:** `Direct fallback` with no cause shown; after both retries the
  state becomes `Server rejects the relay`
  (`app.rs:2229-2233`, `app.rs:2431-2447`).
- **Recovery:** automatic re-test while `retry_after_fallback` is on; a healthy
  window closes the chain silently (`interface.md:39`); no UI action, and none
  is expected (contract: `Relay recovered` is trace-only).
- **Copy:** rail `DIRECT` + `Direct fallback` / `Direct fallback · retry in {n}s`
  / `Server rejects the relay` (`app.rs:1982-1998`, `app.rs:2431-2447`);
  ledger `Path` row holds the same words (`app.rs:2226-2235`).
- **Friction:** the contract requires the window to say *why* the relay was
  abandoned (`interface.md:36` "the relay was abandoned, and why"); the window
  says only what happened (§5 C1). **Not reachable on demand** (needs a relay
  that forwards but does not return, and a live game); code/contract-cited only.
  The countdown itself is implemented and unit-tested
  (`app/tests.rs:1620-1671`).

### F6 — Elevation required

- **Entry:** launch unelevated; `is_admin == false` (`app.rs:420`,
  `platform/windows.rs:184-231`).
- **Decisions:** restart as administrator or not.
- **Failure:** the action slot becomes
  `RESTART AS ADMINISTRATOR TO OPTIMIZE` *even when no relay is selected or the
  app is not connected* (`ledger.rs:47-51`, pinned by
  `app/tests.rs:1493-1519` and `app/tests.rs:2478-2492`); pressing it spawns a
  hidden PowerShell `Start-Process -Verb RunAs` and calls
  `std::process::exit(0)` immediately (`platform/windows.rs:259-273`). If the
  user declines UAC, nothing relaunches and the app is already gone — the tray
  icon disappears and the session is lost. There is no "restarting…" feedback.
- **Recovery:** relaunch manually; there is no in-app path back.
- **Copy:** `RESTART AS ADMINISTRATOR TO OPTIMIZE` (`locales/en.toml:165`);
  Components block shows `Elevation: administrator / standard user`
  (hardcoded English, `app.rs:3264-3288`).
- **Friction:** an action whose stated outcome is impossible in the no-relay
  state (§5 M1); process exit on UAC decline (§5 C8); long labels are shrunk to
  fit 320 px with a floor of 9 px (`ledger.rs:77-88`) — the German label is 45
  characters (`locales/de.toml:13`), Russian 50 (`locales/ru.toml:13`);
  rendered size in those locales is **not visually verified** (§9).
- **Evidence:** not reachable in this session (process is elevated).

### F7 — Game not detected (and detected)

- **Entry:** Auto mode with no known game process; presence worker scans every
  5 s off-thread (`app/presence.rs:26-45`, `app.rs:1843-1943`).
- **Decisions:** wait for Auto, pick a game, create a custom game, or target a
  process.
- **Failure:** none — "no game" is a normal state; but the two descriptions
  disagree (rail `pick your game` vs Game row `Auto will follow one`) and there
  is no hint of *which* games are detected (the rail says "your game" without
  saying what is watched). A game the registry does not know is only reachable
  through `Manual / Custom…`, `New custom game…` or `Target a process` — three
  different affordances for "the game I play is not listed".
- **Recovery:** pick/create/target.
- **Copy:** `No game detected - Auto will follow one when you start it`
  (`locales/en.toml:67`); `Auto (detect)`; `Manual / Custom…`;
  `New custom game…`; `Target a process`; when detected:
  `Game found:` + the raw key (`app.rs:2577-2589`) and rail
  `Relay ready, {raw key} selected` (`app.rs:2008`).
- **Friction:** the detected game is shown as the registry key `fortnite`
  (lowercase) on three surfaces while the boosting line shows `Fortnite`
  (§5 M3). Game identity is the product's second most important fact and it
  reads as a bug.
- **Evidence:** `window-game-detected-start-failed-silent.png` (live detection
  of a synthetic `FortniteClient-Win64-Shipping.exe`).

### F8 — Update available

- **Entry:** Settings → `Check for updates` (`app.rs:3200-3206`), or the test
  env var `LIGHTSPEED_OPEN_UPDATE` (`app.rs:585-587`). There is **no**
  proactive check, no badge and no tray notice.
- **Decisions:** install, close, or ignore.
- **Failure:** (a) install failures show `install failed: <raw error>`
  (`app.rs:3845-3849`); (b) a check failure shows the raw
  `Result<String>` error text as the status line (`update.rs:229-240`,
  rendered `app.rs:3700`); (c) when `status.installer` is `None` — the
  shell-installer receipt path, and every non-Windows build
  (`update.rs:186-213`) — "Update available" is shown with **no install
  action** and no instruction (`app.rs:3702-3724`).
- **Recovery:** close and re-check; there is no retry control in the sheet
  (only `Close`).
- **Copy:** `Check for updates`; `Current version: 1.11.3`; `Latest version:
  v1.11.3`; `You're up to date` / `Update available`; `Install update` +
  `Download, verify, and install the latest version`; `Installing update…` +
  `Approve the UAC prompt if one appears; LightSpeed will restart.` (hardcoded
  English, `app.rs:3732`).
- **Friction:** the user is told an update exists but is not always given a way
  to get it, and version strings mix `1.11.3` with `v1.11.3` (one fact, two
  spellings) (§5 M12). The copy is translated except the UAC line, so the
  German sheet is mixed (§5 C7).
- **Evidence:** `window-update-sheet.png` (up-to-date branch only).

### F9 — Quit-to-tray, restore, second launch

- **Entry:** window X (`app.rs:3780-3804`, `close_decision`
  `app/window.rs:133-150`); footer `Hide to tray` (`app.rs:3944-3949`); tray
  menu `Show window` (`platform/windows.rs:52-68`, `101-128`).
- **Decisions:** hide vs quit; tray Quit vs window Quit.
- **Failure:** X hides; if `has_system_tray()` is false the same X exits
  (`app/window.rs:143-146`) — correct. If the tray icon failed to create, the
  app logs and closing exits; the window does not warn the user.
- **Recovery:** tray `Show window` or double-click the icon. A second launch
  shows a message box: `LightSpeed is already running. / Look for the ⚡ icon in
  the system tray, or the open LightSpeed window.` (`single_instance.rs:76-89`)
  — verified in this session (UIA read of the modal).
- **Copy:** `Hide to tray`, `Quit`, `Show window`, `Connect`, `Disconnect`,
  `Quit`; the tray tooltip
  `⚡ LightSpeed — disconnected / connected · RTT {n}ms / optimizing / error`
  (`platform/windows.rs:142-152`).
- **Friction:** all tray/menu copy is hardcoded English and bypasses the 9-locale
  catalogs (§5 M11); `Connect`/`Disconnect` are always enabled regardless of
  state; the `error` tooltip says only "error" and the window says nothing —
  the two surfaces disagree about the most important failure (§5 B3).
  The footer `Hide to tray` button appears only while the tray exists
  (`app.rs:3944-3946`), so on a tray-less platform the footer loses a row
  silently.

### F10 — Error states

There is no Error state in the window. The state contract requires one:
`Error | What failed, in the engine's own words | engine error`
(`interface.md:41`). What actually happens:

| Failure | Fact exists in | Reaches the window? | Reaches the tray? | Evidence |
|---|---|---|---|---|
| Interceptor/boost start refused (driver missing, permissions, platform) | `tracing::error!` only (`app.rs:1452-1454`) | no | probably not: `interceptor_error` is set from the interceptor's health, not from a start refusal (`client/src/engine.rs:1461-1466`) | live: `window-game-detected-start-failed-silent.png`; trace `ERROR start_interceptor failed: WinDivert.dll not found …` |
| Fatal interceptor error while running | `EngineStatus::interceptor_error` (`engine.rs:1461-1466`) | no | yes (red dot, tooltip `error`) | code only |
| Control-plane registration refused | `EngineStatus::registration_error` + `control_registered` (`engine.rs:78-87`, `1495-1507`) | no — the GUI never reads these fields (grep: no matches in `client-gui/`) | no | code only |
| Redirect/capture/WinDivert start errors | `*_error` fields (`engine.rs:105-151`) | no | yes (red dot) | code only |
| Update check/install failure | `Result<_, String>` | raw string shown (`app.rs:3700`, `3845-3849`) | no | code only |
| Crash | `crash.rs` writes a record + `MessageBoxW` — but consumers must call `record_fatal`; `run()` does (`main.rs:35-38`), panics hit the hook | yes (modal box, English only) | n/a | code only |

**Friction:** the single most common failure class in the field (boost refuses
to start) is invisible in the window and, for the start-refusal path, in the
tray too. The engine's own comment says the field exists so "the tray and the
restart prompt can only see it through this field" (`engine.rs:1461-1466`) —
the restart prompt does not exist. Copy for the missing state already exists in
the catalog: `state.error = "Error"` (`locales/en.toml:153`, currently unused).

---

## 3. State-inventory critique against `interface.md`

Every row of the binding state contract, checked against the shipped code and
the live window. "Honest?" asks the user-level question: can a stranger read the
state word and be right.

### 3.1 The eleven route states

| Contract state (`interface.md`) | Implemented? | Reachable? | Named as contract says? | Honest? |
|---|---|---|---|---|
| Idle — "Not optimizing, relay connected or not" (`:31`) | split in two: `NOT OPTIMIZING` when disconnected, `CONNECTED` when the relay is up (`app.rs:1986-1990`) | yes | no — the contract has one row, the window has two words (`NOT OPTIMIZING` / `CONNECTED`; see the A2 note below) | **yes in spirit**; the contract is stale, not the window. `state.disconnected` exists in the catalog and is unused (`locales/en.toml:148`) |
| Connecting — "Relay being reached" (`:32`) | **not implemented**. `EngineStatus.connected` is the keepalive loop (`client/src/engine.rs:60-62`); there is no visible connecting state | no | no | **dishonest by omission**: during the first seconds of a launch the rail says `NOT OPTIMIZING` / `No relay yet`, identical to "cannot connect at all" |
| Holding — "Traffic stays direct until a relay is chosen for the locked server" (`:33`) | word `HOLDING` is selected when `is_holding` (`app.rs:1979-1983`); sub-line is `Choosing a relay for this server` (`app.rs:1992-1993`) | only after a server is detected **and** the engine reports the hold; before a server is detected the same hold is invisible (see below) | no: the contract's sentence (traffic stays direct) is never shown; `HOLDING` is named but unexplained | **partially dishonest**: see C2 — while holding with no server locked, the rail says `OPTIMIZING` |
| Choosing — "A relay is being ranked for the server" (`:34`) | used as the *Holding* sub-line (`route.choosing`, `locales/en.toml:200`) | yes | merged into Holding | acceptable merge, but undocumented: two contract rows now share one sentence |
| Relaying — "Route = selected relay; Relayed shows sent and returned" (`:35`) | `OPTIMIZING` + `{game} → {relay}` + two-direction counter (`app.rs:1984-2005`, `2485-2520`) | yes | yes | **counters can be impossible** (§C3); game name is the raw key in two of three surfaces (§M3) |
| Direct fallback — "`DIRECT` … the relay was abandoned, and why" (`:36`) | `DIRECT` + `Direct fallback` / `Server rejects the relay` (`app.rs:1982-1998`, `2431-2447`) | needs the watchdog, live game, returning relay | **why is missing** | **no** — the user cannot tell relay fault from server fault from local fault |
| Re-test pending — route line `Direct fallback · retry in {secs}s` (`:37`) | yes (`app.rs:2436-2447`; `app/tests.rs:1620-1637`) | as above | yes | yes, except one countdown source says 20/45/90 s in Settings while the code retries at 30/120 s (§C5) |
| Server rejects relay (`:38`) | `relay_rejected_now` → rail sub-line + ledger `Path` (`app.rs:2229-2233`, `2451-2467`) | after both retries spent | yes | yes, once reachable |
| Relay recovered (`:39`) | trace only (`interface.md:39-40`, `142-143`) | yes | **must not** be shown — correctly not shown | yes |
| Needs elevation (`:40`) | not a rail state; the action slot changes label (`ledger.rs:47-51`) | yes | no: the contract calls it a state, the window makes it a button | **partially**: the rail can simultaneously say `CONNECTED` and the action say "restart as administrator", which is readable, but the *reason* is not stated anywhere else and the Components block hides it in a sheet |
| Error — "What failed, in the engine's own words" (`:41`) | **not implemented in the window** | no | no | **no** — see §2 F10, §5 B3/B4 |

**Note on the Idle row and amendment A2 (revision r5, 2026-11-19).** The two
words the shipped window actually shows across this contract row are
**`NOT OPTIMIZING`** (drawn from `state.idle`, `locales/en.toml:154`, in the
disconnected branch at `app.rs:1986-1990`) and **`CONNECTED`**
(`locales/en.toml:149`, `app.rs:1986-1987`). `NOT CONNECTED` is **new copy**
proposed by the spec's amendment A2 — it is not one of the shipped words, and
the spec's rationale should read "shipped `NOT OPTIMIZING`; new
`NOT CONNECTED`" rather than "two shipped words". The catalog already carries an
unused `state.disconnected = "Disconnected"` (`locales/en.toml:148`, listed as
dead in §4.1 #20), so A2 should either wire that existing key or delete it and
state plainly that `NOT CONNECTED` replaces it.

### 3.2 Server-leg states (`interface.md:61-67`)

| Contract state | Implemented? | Reachable? | Honest? |
|---|---|---|---|
| Server leg measured (`:63`) | yes (`app.rs:2259-2318`, `choice.rs:170-230`) | documented as possibly unreachable: needs ≥8 validated whole-path samples **and** ≥8 client-leg samples for the same relay (`interface.md:232-237`) | correctly falls back to prior/bound, verified by `app/tests.rs` server-leg tests |
| Server leg bounded (`:64`) | yes, including the early triangle floor (`app.rs:2271-2305`) | after a fresh direct ping | yes — states "at least", never "measured" |
| Server leg not measured (`:65`) | yes, with `PRIOR_UNCERTAINTY_US` (`app.rs:2306-2316`) | yes | yes |
| Server leg unknown (`:66`) | yes (`app.rs:2317`) | yes | yes |
| Relay cannot beat direct (`:67`) | yes, caption + hover arithmetic (`app.rs:2161-2174`) | needs a fresh direct ping and client leg | yes, but the caption gives no next action (the user is told the relay cannot help while the client keeps relaying); design decision for W2 |

### 3.3 States the window shows that the contract does not name

| Shown | Where | Problem |
|---|---|---|
| `Discovering relays…` / `No relays discovered` | relay row `app.rs:2641-2658` | not in the contract; no driving-fact row; the failure reason lives only in a hover |
| `Relay ready, pick your game` / `Relay ready, {name} selected` | rail `app.rs:2006-2013` | contradicts Auto mode (`:67` vs `:179`), and "should be a key when a game is detected" |
| `Game found: {raw key}` | Game row `app.rs:2577-2589` | key, not display name |
| `Auto-detecting (1024-65535)…` in the **Game server** value slot | ledger `app.rs:2411-2423` ← `client/src/engine.rs:1217-1221` | a status sentence where a measured address belongs; untranslated engine text |
| `-- start optimizing to place it` / `count` | ledger hints `locales/en.toml:202-203` | names an action that is not named that anywhere else (`Optimize my route`) |
| Disabled action with no reason | `app.rs:3963-3972` + `ledger.rs:52-56` | a control that cannot show why it is disabled |
| `error` tray tooltip with no detail | `platform/windows.rs:151` | a state word with no fact |

### 3.4 Contract drift that W2 must resolve (contract vs code)

1. **Headline metric meaning.** `interface.md:134-136`: "Any number shown as
   'ping' is labelled with what it measures. The headline `ms` is the leg to the
   relay, not the game's own ping." The code changed the headline to the direct
   game-server ping when a fresh ICMP sample exists and the relay leg otherwise
   (`app.rs:2019-2050`). The window therefore contradicts the binding contract,
   and the label ("which leg is this?") is hover-only. Either the contract is
   amended (and the metric gets a visible label) or the code reverts; the audit
   does not get to choose — but it can say the current state is the worst of
   both: a headline whose meaning changes and no visible unit of reference.
2. **`Connecting` and `Error` rows exist in the contract and not in the window**;
   `HealthHolding`/`Choosing` are merged. The contract's state set and the
   window's state set must be reconciled in `gui-refresh-spec.md`.
3. **Layout spec breakpoints are unreachable.** The spec has 380-619 px and
   ≥620 px layouts and 435×720 / 720×610 captures (`interface.md:150-157`,
   `170-174`); the shipped window pins width to 320 px min=max
   (`main.rs:91-100`), so `narrow` is always true (`app.rs:3760`) and the
   side-by-side ledger / label-left config rows never render. The verification
   protocol acknowledges this (`interface.md:247-251`) — the *spec* still
   contains layouts no build can reach.
4. **"The early (pre-ranking) bound is not a second ranking rule"** etc. —
   implemented correctly (`choice.rs:243-251`); no action.
5. **`Relayed` row** contract says counters show both directions
   (`interface.md:137-139`); the implementation mixes a data-plane send counter
   with a relay-wide receive counter that includes keepalive echoes
   (`engine.rs:161-166`; `app.rs:2476-2520`) — see C3.

---

## 4. Microcopy inventory

Rules applied (`lightspeed-gui-design` §1.6, §7; `interface.md:13-16`):
one fact one source; active voice; one job per sentence; outcome-named button
that keeps its name through the flow; no invented precision; honest failure;
empty states invite the next action; 9 locales.

### 4.1 Violations shipped today

| # | Current copy | Where (file:line) | Rule broken | Proposed |
|---|---|---|---|---|
| 1 | `Waits up to {secs}s for a relay before using the direct path.` rendered with 3 s (`LOCK_SETTLE`) | `locales/en.toml:36`; `app.rs:3233-3240` (passes `LOCK_SETTLE`), `app.rs:100`; the real hold deadline is `HOLD_FALLBACK = 120 s`, `app.rs:94`, used at `app.rs:1136-1151` | one fact one source **and** false data: the wait is up to 120 s, not 3 s | `LightSpeed keeps traffic direct until it identifies the game server, for up to 2 minutes.` (constant: `HOLD_FALLBACK`) |
| 2 | hover `… re-tested after 20 s, then 45 s, then 90 s. Off leaves the session on the direct path until the next match.` | `app.rs:3171-3174` vs `watchdog.rs:121-122` (`RELAY_RETRY_BACKOFF = [30 s, 120 s]`, two retries) | false data; wrong count (says three, code has two) | `Retests the relay after 30 seconds, then after 2 minutes. After the last retest the session stays on the direct path until the server changes.` (generated from the constant) |
| 3 | `Direct fallback` (rail and ledger) with no cause | `app.rs:1982-1998`, `2229-2233`, `2431-2447`; contract requires "and why" `interface.md:36` | incomplete state; "one job per sentence" | `Direct fallback: the relay is not returning this server's traffic` / `Direct fallback: your local send path failed` (the watchdog already tells these apart — `watchdog.rs:144-153`) |
| 4 | `OPTIMIZING` while the engine holds traffic direct | `app.rs:1979-1990` (rail decision ignores the hold/`tunnelling_enabled` until `direct_fallback` is derived, `app.rs:1749-1756`) | false state claim (contract: `OPTIMIZING` only while traffic is tunnelled, `interface.md:131`) | keep `HOLDING` for the hold, with `Traffic stays direct until a relay is chosen for the game server` (`interface.md:33`) |
| 5 | `Relayed` = `{sent} up · {returned} down` | `locales/en.toml:198`; `app.rs:2485-2520`; sources `engine.rs:161-166` | one fact one source: `up` counts tunnel sends, `down` counts all datagrams from the proxy including keepalive echoes → `0 up · 6 down` | `Relayed {sent} up · {returned} down` with both from the same server-flow counters, or label the down counter as `Relay replies` |
| 6 | `Game server` value = `Auto-detecting (1024-65535)…` | engine `client/src/engine.rs:1217-1221` → `app.rs:2411-2423` | a status sentence in a measured-address slot; untranslated engine text in a 9-locale UI | keep `-- start optimizing to place it` in the slot; move the scan sentence to the rail sub-line, localized (`route.scanning = "Finding the game server…"`) |
| 7 | `Relay ready, pick your game` (rail, while Game is `Auto (detect)`) | `locales/en.toml:179`, `app.rs:2008-2010` vs `locales/en.toml:67`, `app.rs:2590-2596` | one fact two sources; contradictory instruction | `Relay ready · Auto waits for your game` (Auto) / `Relay ready · {display name} follows` (detected) / `Relay ready · {display name} pinned` |
| 8 | `Relay ready, fortnite selected` / `Game found: fortnite` / `Auto — fortnite` | `app.rs:2008`, `2577-2589`, `2751`; key from `choice.rs:284-300` | internal identifier shown as a name; same game two spellings in one window (`Fortnite` in the rail's boosting line `app.rs:2005`) | always `display_name`, never the registry key |
| 9 | `Relay refresh failed - using the saved list` | `locales/en.toml:54`, `app.rs:2556-2569` | jargon ("saved list"); `WORD - fragment`; cause hover-only | `Relay list not updated. Using the relays saved on this PC.` + visible cause line or a link to Settings |
| 10 | `The relay registry returned no usable relays.` duplicated: hardcoded fallback hover vs catalogued key | `app.rs:2651-2653` (hardcoded) and `locales/en.toml:55` (`relay.registry_empty`, unused) | one fact two sources; dead translation | use the catalog key in one place; show the reason as body text, not a hover |
| 11 | `Latest version: v1.11.3` vs `Current version: 1.11.3` | `app.rs:3686-3698`; `update.rs:163-167` (tag) vs `env!("CARGO_PKG_VERSION")` | one fact two spellings | normalize the tag once: `Current version: 1.11.3` / `Latest version: 1.11.3` |
| 12 | `install failed: …` / `no install receipt found` / `release lookup failed: HTTP 403` shown as the status line | `update.rs:229-240`; `app.rs:3700`, `3845-3849` | internal/diagnostic text as user copy; untranslated | `The update could not be installed. Check your connection and try again.` + `Open log file` |
| 13 | `-- start optimizing to place it` / `-- start optimizing to count` | `locales/en.toml:202-203`, `app.rs:2421`, `2518` | action name differs from the button (`Optimize my route`); passive-leaning hint | `Not measured yet` / `No traffic relayed yet` |
| 14 | `Routing` section, `Re-test a relay after falling back`, `Components`, `Version/Elevation/Capture/Npcap/Relays` values, `Enter the server's IP and port.`, `No relays yet. Discovery runs automatically.`, `Approve the UAC prompt if one appears; LightSpeed will restart.`, `Fetch the latest community relay list…`, the telemetry hover, the relay-picker hovers, the cannot-beat hover | `app.rs:2877`, `2940-2954`, `3142-3146`, `3163`, `3167-3175`, `3212-3221`, `3250-3291`, `3604`, `3732`, `2168-2173`, `2699-2714`; tray copy `platform/windows.rs:52-55`, `142-152`; second-instance notice `single_instance.rs:78-80` | 9-locale rule (`lightspeed-gui-design` §3, §5); `interface.md:231` exempts only the Components labels | catalog keys for all of them; the Components block is the one documented English-only exception and should be marked as diagnostic in the sheet |
| 15 | ja / ko / zh-Hans: **26** English-identical keys including `state.direct`, `state.holding`, `route.direct_fallback`, `route.server_rejects_relay`, `route.choosing`, `route.retry_in`, `route.sent_returned`, `route.relay_rtt`, `settings.hold_policy`, `settings.remeasure` | `client-gui/locales/{ja,ko,zh-Hans}.toml` (detected by comparing values to `en.toml`); documented as a 7-key exception at `interface.md:238-240` | the documented translation-debt decision understates the shipped set by 3-4x, and it covers exactly the honesty surface (fallback story) | either translate the state/route vocabulary or re-record the decision with the real key list; W2 must not claim "9 locales" while the fallback story is English |
| 16 | `Holding`, `Path`, `Server leg`, `Relayed`, `Relay RTT`, `Game path` — product jargon on the primary surface | `locales/en.toml:182-200` | plain-language rule for the one-second read; jargon words with no explanation on-screen | keep the terms only if the rail sentence explains them once; `Holding` → `Holding (traffic stays direct)` |
| 17 | `Use this if Packets Sent stays at 0 after 15 s.` | `app.rs:2944-2948` hover | ghost reference: the shipped UI has no `Packets Sent` label (the row is `Relayed`, `Sent/returned`); "15 s" is not a number anywhere in code | `Set this when the Relayed row shows 0 up after 15 seconds.` and generate the threshold from a constant, or delete the hover |
| 18 | `Add Proxy`, `Open config folder`, `Reset to defaults` | `locales/en.toml:46,48,49` | button naming/voice mix ("Add Proxy" title case while every other button is sentence case); `config folder` jargon | `Add relay`, `Open settings folder`, `Reset settings` |
| 19 | `⚡ LightSpeed — error` / `— disconnected` / `— connected · RTT {n} ms` | `platform/windows.rs:142-152`, `app.rs:3871-3873` | state word without fact; `RTT 0ms` fake precision before the first sample (`app.rs:3872` passes `latest_rtt_ms`, initially 0.0); English-only | `LightSpeed — not connected`, `LightSpeed — connected, relay 42 ms` (omit the leg until measured), `LightSpeed — needs attention: open the window` (from the `*_error` field) |
| 20 | 18 of 163 catalog keys are never referenced: `app.title`, `settings.capture_status/available/missing/active`, `proxies.title`, `proxies.manage_hint`, `relay.registry_empty`, `game.none_detected`, `state.disconnected`, `action.connect`, `action.connecting`, `action.manage`, `route.server`, `route.status`, `route.path_rtt`, `route.path_hint`, `route.start_manual` | `client-gui/locales/en.toml` (script count in evidence report §1/§3); none referenced by `client-gui/src` | dead copy that translators maintain for 9 locales; three of them (`action.connect`, `action.connecting`, `state.disconnected`) are exactly the reconnect vocabulary the window lacks | delete or wire them; the Connect pair is the fix for B2 |

### 4.2 Copy that is correct and should not be relitigated

- Rail state words are single uppercase words (`CONNECTED`, `OPTIMIZING`,
  `DIRECT`, `HOLDING`) — canon, not an eyebrow (`lightspeed-gui-design` §3).
- Action labels are outcome-named and stable: `Optimize my route` /
  `Restart as Administrator to optimize` / `Stop optimizing`
  (`locales/en.toml:163-165`), pinned by tests (`app/tests.rs:1493-1535`).
- `Direct fallback · retry in {n}s` matches the contract exactly
  (`interface.md:37`).
- `Cannot be faster than direct` is the one whole-path claim and is allowed by
  the contract (`interface.md:67, 82-86`).
- `Current version` / `You're up to date` / `Update available` are honest and
  short (`locales/en.toml:128-133`).
- The second-instance notice is the best recovery copy in the product: it names
  the reason and the two ways back (`single_instance.rs:78-81`).

---

## 5. Severity-ranked findings

Ladder: `lightspeed-gui-design` §6. Format per the skill:
`[SEVERITY] surface: problem / Where / Rule / Impact / Fix / Effort`.
Severity is carried by the section heading and the ID prefix
(`B` Blocker, `C` Critical, `M` Major, `m` Minor) and each finding keeps the
skill's five fields.
Effort is relative (S ≤ half a day, M ≈ a day, L > a day for one implementer,
including screenshot verification).

### Blocker

**B1 — The primary action has no accessible name and no visible focus.**
*Where:* `app.rs:3960-3999` draws the action; `app/ledger.rs:71-137` paints it
(`allocate_exact_size(..., Sense::click())`, no `widget_info`, no focus
painting; `hovered` is the only interaction state at `ledger.rs:93`). Live UIA
tree exposes it as `Custom` with an empty name (every capture, e.g.
`window-connected-320.png` UIA: `… Text "Quit" / Custom / Text "Relay" …`).
*Rule:* accessibility floor, keyboard-complete core path, visible focus
(`lightspeed-gui-design` §1.3); ladder defines a screen-reader-completable core
flow as Blocker.
*Impact:* a screen-reader user cannot find or hear the one control that starts
or stops the product; a keyboard user can Tab to it (egui 0.37 fakes a click on
Space/Enter for focused click-sensing widgets, `egui/src/context.rs:1755-1766`)
but sees no focus ring, so the core path has no visible focus anywhere on the
action. Automation also cannot press it by coordinate (verified:
`window-stop-attempt-noop.png`).
*Fix:* report a widget info with the current label
(`response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label))`),
paint a focus ring from `response.has_focus()`, and keep the pure
`primary_action()` gate as the single source of the label.
*Effort:* S.

**B2 — `Disconnect` is one-way from the window; the state after it is a dead end.**
*Where:* the only direct relay action in the config card is the `Disconnect`
link shown while connected (`app.rs:2545-2552`); the reconnect path
`toggle_relay_connection` exists (`app.rs:1956-1965`) but is only reachable
through that link. The action slot after disconnecting is
`OPTIMIZE MY ROUTE` disabled because `connected` is false
(`ledger.rs:52-56`, `app.rs:3963-3972`) with no reason. The only other ways back are the tray menu
(`app.rs:3811-3817`), restarting the app, or a hidden side effect of
Settings → `Refresh relays`, which reconnects after a successful re-discovery
(`app.rs:1349-1357`) — none of them is discoverable from the disconnected state.
*Rule:* "a control that cannot show its own state is a defect"; no dead ends;
keyboard-complete core path.
*Impact:* verified live — after pressing `Disconnect` the window offers no way
to reconnect (`window-disconnected-320.png`). A player who disconnects to compare direct
versus relayed has no obvious way back in the window.
*Fix:* make the same slot a toggle (`Disconnect` ↔ `Connect`), wire the unused
`action.connect`/`action.connecting` keys (`locales/en.toml:157-159`), and put
the reason on the disabled action ("Connect to a relay first").
*Effort:* S.

**B3 — A failed boost is silent: the window looks healthy while the product is not running.**
*Where:* `start_boost` logs and returns on error (`app.rs:1432-1500`,
`1452-1454`); the auto-start path latches regardless of the result
(`app.rs:1924-1935`); `EngineStatus::interceptor_error` is read only for the tray
colour (`app.rs:3851-3874`). Live trace:
`ERROR start_interceptor failed: WinDivert.dll not found …` while the window
showed `CONNECTED / Relay ready, fortnite selected` with an enabled
`OPTIMIZE MY ROUTE` (`window-game-detected-start-failed-silent.png`).
*Rule:* state honesty (No. 1), all states including error, "errors say what
happened and how to fix it" (`lightspeed-gui-design` §1).
*Impact:* the user presses or waits for a boost, nothing happens, and the window
gives no evidence anything was attempted; the failure is only discoverable in a
log file. The engine's own comment expected a visible prompt
(`client/src/engine.rs:1461-1466`).
*Fix:* add an error surface (rail sentence + `settings.error` region or a
sheet), fed by the start result, `interceptor_error`, `capture_error`,
`windivert_error`, `redirect_error`; make the auto-start latch retry only after
the user dismisses or the error changes.
*Effort:* M.

**B4 — Control-plane registration failure is invisible; `CONNECTED` can be false.**
*Where:* `EngineStatus.control_registered` / `registration_error`
(`client/src/engine.rs:78-87`, `1495-1507`) are never read by `client-gui/`
(grep: zero matches). The rail's `CONNECTED` is `status.connected`, i.e. the
keepalive loop (`engine.rs:60-62`), which can run while the relay refused the
session registration — the same refusal that dominates the fleet's
`M1` counters (99.4% alert-120 rejects on the pre-1.11 population,
`wat/state/current-phase.md` 2026-11-18 F-02).
*Rule:* state honesty; "connected" must mean what the user thinks it means.
*Impact:* a user on a rejected session sees a healthy window that will never
relay; the natural conclusion is that the game/relay is at fault.
*Fix:* make the rail distinguish `CONNECTING` / `CONNECTED` / `RELAY REFUSED`
from `connected && control_registered`, with the reason from
`registration_error`; this also gives the contract's `Connecting` row a home
(§3.1).
*Effort:* M.

### Critical

**C1 — `Direct fallback` does not say why the relay was abandoned.**
*Where:* `app.rs:2431-2447`; contract `interface.md:36` requires the cause.
*Rule:* state honesty; one job per sentence (the state word is the cause).
*Impact:* "my relay is broken" and "the game server rejects the relay" and "my
local send path is broken" are three different user actions; the window collapses
them into one word. The watchdog already computes the distinction
(`watchdog.rs:144-153`).
*Fix:* put the cause in the rail sub-line and the `Path` row (copy deck in §4.1
#3); the `Path` row already has room.
*Effort:* S.

**C2 — `OPTIMIZING` is shown while traffic is still held on the direct path.**
*Where:* rail decision `app.rs:1979-1990`; the hold is only visible through
`direct_fallback`, which requires `tunnelling_enabled == Some(false)` **and** a
detected server (`app.rs:1749-1756`). Before a server locks, the hold is real
(trace: `Holding traffic on the direct path until the game server is known`) but
the rail shows `OPTIMIZING` (live: `window-boosting-hold-320.png`,
`0 up · 3 down`).
*Rule:* `interface.md:131` "`OPTIMIZING` is only true while traffic is actually
tunnelled"; this is the exact class of bug the contract was written for.
*Impact:* the one word that answers the one-second question is wrong in the
most common boost-start moment (game just launched, server not yet known).
*Fix:* derive the rail from the engine's gate (`tunnelling_enabled`) rather than
from the GUI's fallback proxy; show `HOLDING` + the contract's sentence
(`interface.md:33`) for both the pre-server and the post-server hold.
*Effort:* S.

**C3 — `Relayed` can show an impossible direction pair.**
*Where:* `app.rs:2485-2520` combines `interceptor_tunnel_sent` (up) with
`interceptor_from_proxy` (down, which "includes keepalive echoes and frames that
failed to decode", `client/src/engine.rs:161-166`; `relayed_base` baselines the
pair at session start, `app.rs:1763-1781`). Live: `0 up · 3 down`, then
`0 up · 6 down` (`window-boosting-hold-320.png`, `window-stop-attempt-noop.png`).
*Rule:* "counters show both directions" from one story (`interface.md:137-139`);
one fact one source.
*Impact:* a reader who knows what packets are sees a counter that cannot be
true; a reader who does not sees a healthy-looking return path on a session that
has not sent anything.
*Fix:* count both directions from the same server-flow counters (the per-server
`interceptor_flows` pair already exists and is used for the watchdog,
`app.rs:1665-1678`) or rename the down half to `relay replies` and exclude
keepalives.
*Effort:* S (GUI-only if the engine already exposes the pair; M if a new field
is needed).

**C4 — The headline metric is unlabelled and changes meaning.**
*Where:* `app.rs:2019-2050` — the headline is the direct ICMP server ping when
fresh, otherwise the relay leg; the only label is a hover text
(`route.server_ping_hint` / `route.rtt_hint`). Contract says the headline is the
relay leg (`interface.md:134-136`).
*Rule:* "Any number shown as ping is labelled with what it measures"; no
fabricated precision.
*Impact:* the first number the eye lands on can be either of two measurements
with different meanings; the user cannot tell without hovering, and screen
readers get the number with no label at all.
*Fix:* render the unit and the leg in the rail (`242 ms relay` / `38 ms direct`)
and reconcile `interface.md` with the decision; keep the hover as detail.
*Effort:* S.

**C5 — Two shipped settings facts are false (hold duration and retest schedule).**
*Where:* `settings.hold_policy` rendered with `LOCK_SETTLE` = 3 s
(`locales/en.toml:36`, `app.rs:3233-3240`) while the hold deadline is
`HOLD_FALLBACK` = 120 s (`app.rs:94`, `1136-1151`); the retest hover says
"20 s, 45 s, 90 s" (`app.rs:3171-3174`) while `RELAY_RETRY_BACKOFF` is
`[30 s, 120 s]` (`watchdog.rs:121-122`, two retries, `watchdog.rs:133-136`).
*Rule:* no invented precision; one fact one source; copy generated from the
constant that owns the fact.
*Impact:* the user is told a 3-second wait before the direct path; it can be two
minutes of unexplained direct traffic. Verified live in the English Settings
sheet (UIA: `Waits up to 3s for a relay before using the direct path.`).
*Fix:* pass the constants into the copy (`HOLD_FALLBACK`,
`RELAY_RETRY_BACKOFF`), state the count correctly, and add a unit test that the
copy renders the constants.
*Effort:* S.

**C6 — Internal strings are shown as user copy.**
*Where:* `update_status_line` returns the raw `Err` string
(`update.rs:229-240` → `app.rs:3700`); install failure shows
`install failed: …` (`app.rs:3845-3849`); the `Game server` row shows the
engine's scanning sentence `Auto-detecting (1024-65535)…`
(`client/src/engine.rs:1217-1221` → `app.rs:2411-2423`, live screenshot).
*Rule:* one voice, user language, 9 locales; "errors say what happened and how
to fix it".
*Impact:* English diagnostics inside translated windows, and a status sentence
in a field that should hold a measured address.
*Fix:* map every engine/user error to a localized sentence at the boundary
(one error taxonomy — `architect`'s DTO work is the natural home) and keep the
raw string behind "Open log file"/details.
*Effort:* M.

**C7 — The 9-locale claim is not true on the surface that matters.**
*Where:* hardcoded English UI (`app.rs:3163`, `3167-3175`,
`3250-3291`, `3604`, `3732`, `2877`, several hovers — full list §4.1 #14);
ja/ko/zh-Hans carry **26** English-identical keys including the whole fallback
story (`state.direct`, `state.holding`, `route.direct_fallback`,
`route.server_rejects_relay`, `route.retry_in`, `route.sent_returned`), while
`interface.md:238-240` records a **7-key** English fallback decision; German
Settings shows `Routing` and `Re-test a relay after falling back` in English
(`window-settings-sheet-de.png`).
*Rule:* nine locales is a product rule (`lightspeed-gui-design` §3).
*Impact:* a German/Japanese/Chinese/Korean player reads the exact states that
explain failure in a language they did not choose; the recorded decision covers
less than half of what ships.
*Fix:* catalog the hardcoded strings, translate the state/route vocabulary, and
re-record the fallback decision with the real key list (or shrink it by
translating). `scripts/Localization.ps1` already exists — this is copy work, not
new tooling.
*Effort:* M.

**C8 — The elevation action exits the process before UAC resolves; declining UAC loses the app.**
*Where:* `platform/windows.rs:259-273`: `relaunch_as_admin` spawns
`powershell -WindowStyle Hidden -Command "Start-Process '<exe>' -Verb RunAs"`
and then calls `std::process::exit(0)` unconditionally. If the user declines
UAC (a normal, sensible response to an unexpected prompt) nothing relaunches and
the process is already gone; the tray icon disappears. The spawn's own failure
is never read (the handle from `silent_command(...).spawn()` is dropped).
*Rule:* no dead ends; a user-triggered state must be recoverable; "never draw an
action that cannot act".
*Impact:* the client is lost mid-session with no message, and the user must find
and relaunch the app from the Start menu.
*Fix:* keep the current process alive until the elevated instance confirms
startup (wait on the spawn result / a parent-child handshake with a timeout),
show a "Restarting as administrator…" state, and surface a refused or failed
spawn instead of exiting into nothing. The single-instance guard already exists
(`single_instance.rs:97-120`) and is the natural handshake.
*Effort:* M.

### Major

**M1 — The action invites elevation even when elevation cannot produce the outcome.**
*Where:* `primary_action` returns the elevation label whenever `!is_admin`,
regardless of relay/connection (`ledger.rs:47-51`), pinned by
`app/tests.rs:1493-1519` and `app/tests.rs:2478-2492`.
*Rule:* never draw an action that cannot act (`lightspeed-gui-design` §3
elevation rule); outcome-named button.
*Impact:* on a fresh unelevated install with no relays, the loudest control
promises "optimize" and delivers a relaunch into the same empty state; the user
learns the product is broken.
*Fix:* gate the elevation label on "a relay exists and a game can start" (or
show the elevation label *and* the missing precondition beneath it); keep the
three-label slot.
*Effort:* S.

**M2 — The rail and the Game row give opposite instructions.**
*Where:* `Relay ready, pick your game` (`locales/en.toml:179`, `app.rs:2008`)
vs `No game detected - Auto will follow one when you start it`
(`locales/en.toml:67`, `app.rs:2590-2596`); both visible at once in
`window-connected-320.png`.
*Rule:* one fact one source; the mode the user chose should own the sentence.
*Impact:* the user is told to act and not to act in the same screen.
*Fix:* make the display derive from `game_auto` / `game_manual` /
`selected_custom` in one place (the `game_caption` helper already does this for
the Game row — `ledger.rs:226-236`; extend it to the rail).
*Effort:* S.

**M3 — The detected game is shown as the internal registry key.**
*Where:* `auto_detected_game` holds the key (`choice.rs:284-300`); displayed raw
at `app.rs:2008` (`Relay ready, fortnite selected`), `app.rs:2577-2589`
(`Game found: fortnite`), `app.rs:2751` (`Auto — fortnite`); the rail's boosting
line uses the display name (`Fortnite → …`, `app.rs:2005`). Live in
`window-game-detected-start-failed-silent.png` and `window-boosting-hold-320.png`.
*Rule:* one fact, one spelling; user-facing names come from the display name.
*Impact:* the product's second-most-important fact reads as an unfinished
feature; the same game is two names on one screen.
*Fix:* resolve `auto_detected_game` to `(key, display)` once (the
`pick_game_entry` helper already returns `display`, `app/picker.rs:28-55`) and
use `display` everywhere user-facing.
*Effort:* S.

**M4 — The window is a pinned 320 px column that resizes itself as state changes.**
*Where:* width pinned min=max=320 (`main.rs:91-100`); content height is
auto-computed and pushed as `ViewportCommand::InnerSize` every frame the needed
height changes by >8 px (`app.rs:4037-4053`); bounds 420-1400
(`app/window.rs:35-39`); no size/position persistence anywhere in the GUI.
*Rule:* 2026 window floor — resizable within stated bounds, remembers
size/position, never clips (`lightspeed-gui-design` §2).
*Impact:* measured live heights for one logical state family: **866** (EN
connected), **803** (EN disconnected), **988** (EN boosting), **914** (DE
connected). The action bar and footer move vertically between states — under a
mouse that is aiming at them — and the user cannot widen the window to see the
map/ledger at a glance on a desktop monitor. The wide breakpoints in
`interface.md:150-157` are unreachable (§3.4.3).
*Fix:* W1 decisions in `gui-refresh-brief.md` §11 / `desktop-platform`'s doc:
resizable window + remembered geometry; keep a content-driven minimum, not a
fixed width. If 320 px stays, remove the unreachable breakpoints from
`interface.md` and stop implying them.
*Effort:* M-L.

**M5 — A disabled primary action never explains itself; first run has no guidance.**
*Where:* disabled fill in `ledger.rs:89-113` (no tooltip/no reason text);
`app.rs:3963-3972` computes readiness but nothing renders *why*.
*Rule:* a control that cannot show its state is a defect; empty states invite the
next action.
*Impact:* the first-run elevated user sees a dead bright slot and no next step.
*Fix:* render the unmet precondition under the action ("Connect to a relay to
start") or keep the slot live and open what is missing.
*Effort:* S.

**M6 — Discovery-failure recovery is hover-only, untranslated and has no retry loop.**
*Where:* `app.rs:2639-2660`; reason in hover only; hardcoded fallback sentence
`app.rs:2651-2653`; discovery only re-runs on a click (`app.rs:691-697`);
catalog `relay.refresh_failed` (`locales/en.toml:54`) uses "saved list" jargon.
*Rule:* errors say what happened and how to fix it; content on hover must be
reachable without hover (WCAG 2.2 / keyboard).
*Impact:* a transient network failure at launch leaves a permanent no-relay
state unless the user finds the small `Retry` link or Settings.
*Fix:* body-text error line with a cause and a "Try again" button; auto-retry
with backoff on startup failure (bounded, visible).
*Effort:* S.

**M7 — Facts that only exist on hover.**
*Where:* relay-picker measured legs ("Median of five probes…", `app.rs:2699-2702`,
`2711-2714`), failure reasons (`app.rs:2560-2564`, `2650-2653`), the cannot-beat
arithmetic (`app.rs:2164-2173`), the port-range explanation
(`app.rs:2944-2948`), the telemetry hover (`app.rs:3142-3146`).
*Rule:* keyboard-complete; no hover-only affordance; assistant tech must get the
fact.
*Impact:* the same class of fact (what this number means, why it failed) is
available to mouse users only; `on_hover_text` is not focusable in egui.
*Fix:* reserve hovers for extra detail; put the fact that changes the user's
decision in a visible caption or a focusable disclosure.
*Effort:* M.

**M8 — Config card content and Settings relay rows are horizontally centred.**
*Where:* `app.rs:4018-4034` wraps the config card in `ui.vertical_centered`;
that makes labels (`Relay`, `Game`), the game hint and placeholders centred while
the route card above is left-aligned (`app.rs:2148` only centres the map). The
Settings relay manager rows centre their label/address (`app.rs:3569-3601`);
live in `window-connected-320.png`, `window-settings-sheet.png`,
`window-connected-320-de.png`, `window-settings-sheet-de.png`.
*Rule:* one gutter, nothing centres except the map (`interface.md:199-207`);
anti-slop tell "nothing lines up on a left edge except by accident".
*Impact:* the screen reads as two different layouts stacked; the labels do not
align with the ledger's label column above them.
*Fix:* drop `vertical_centered` for the config card body (centre the column with
`ui.set_width` + explicit left margin instead) and left-align the relay rows.
*Effort:* S.

**M9 — Several controls are below the 24 px pointer-target floor.**
*Where:* `Disconnect` and `Retry` are `ui.link` labels at BODY 13 px
(`app.rs:2545-2552`, `2565`, `2654`) — ~19 px tall; the relay delete `×` is a
14 px `Button` (`app.rs:3579-3581`); the game/save links in the picker
(`app.rs:2825`) likewise.
*Rule:* accessibility floor — minimum 24 px pointer target (`lightspeed-gui-design`
§1.3); ladder calls <24 px targets Major.
*Impact:* motor-impaired users and anyone on a touch/pen device mis-click; the
delete `×` is destructive and tiny.
*Fix:* give links a 24-32 px hit area (`ui.add(egui::Button::new(...).frame(false))`
with padding, or wrap in `ui.allocate_ui` of 28 px) and confirm before delete.
*Effort:* S.

**M10 — Destructive actions have no confirmation or undo.**
*Where:* relay `×` removes the entry and persists immediately
(`app.rs:3579-3583`, `app.rs:3635-3641`); `Reset to defaults` deletes
`config.toml` and reconnects (`app.rs:3627-3664`, `config.rs:302-312`).
*Rule:* change discipline "prefer deleting a control over hiding it"; safety
floor for destructive actions; state honesty.
*Impact:* one stray click loses the custom relay list or every setting; the
product's recovery is to re-run discovery.
*Fix:* confirm or undo for `Reset`, and for a relay delete offer Un hide
(re-add) in the same row for a few seconds; keep the stable firewall-rule
discipline as the model.
*Effort:* S-M.

**M11 — Tray and window disagree, and the tray is English-only with a fact-free error word.**
*Where:* `platform/windows.rs:52-55` menu copy, `142-152` tooltips;
`Connect`/`Disconnect` always enabled regardless of state (`windows.rs:53-54`);
the error tooltip is `— error` with no detail (`windows.rs:151`); `RTT {n}ms`
is formatted from a value that is 0.0 before the first sample
(`app.rs:3872`, `windows.rs:144-149`).
*Rule:* nine locales; state honesty; "the tray actions mirror the window".
*Impact:* the tray is the only surface for a hidden window; it cannot say what
is wrong and its menu offers actions that do nothing in that state.
*Fix:* localize tray copy via `i18n::t` (the tray is created after i18n is
initialised), enable menu items from the same `TrayState`, and let the error
state open/point at the window error surface.
*Effort:* S-M.

**M12 — The update flow dead-ends and exposes raw internals.**
*Where:* `app.rs:3678-3734`; `update.rs:186-213` (installer only for the
receipt-less Windows path), `229-240` (raw errors), `163-167` (tag `v…`);
no proactive check (`start_update_check` only from Settings or an env var).
*Rule:* 2026 integration floor — "updater shows progress, version and failure"
(`lightspeed-gui-design` §2); no dead ends.
*Impact:* "Update available" without an install path (shell-installer and every
non-Windows install) and no instruction; failures show `.`-prefixed rust error
text; the user cannot tell whether the update happened.
*Fix:* always offer an actionable path (installer or "Open releases page"),
normalize versions, map errors to sentences, add a one-line "build" marker in
the footer when a check found an update.
*Effort:* M.

**M13 — Five literal colour sites remain outside the token module, and the token doc lies about it.**
*Where:* `client-gui/src/globe.rs` — four `Color32::from_rgb` constructors at
`:71-73` (map background, grid, land) and `:125` (route arc), plus one
`Color32::WHITE` constant literal at `:133` (map-marker core) = **five literal
colour sites**. Definition used here, in `design-system-2026.md` and in the spec:
a literal colour site is any colour value written at a use site instead of being
imported from `design.rs`, whether *constructed* (`from_rgb`/`from_rgba`) or
*named* (`WHITE`). Under the narrower "`from_rgb`/`from_rgba` only" definition
the count is 4; under the wider "any colour value at a use site" definition it
is 5 — the W1 set standardises on **5**, and both numbers are recorded so the
definition cannot drift again. `app.rs:9-10` correctly claims none in `app.rs`,
which has been read as "none in the GUI". `design.rs:1-8` still describes itself
as "for now unreferenced" and the layout migration as future work, which is
stale (it is the live token module).
*Rule:* tokens only (`lightspeed-gui-design` §1.4); state honesty applies to
code comments too.
*Impact:* the map's five literal colours cannot theme with the tokens (light
mode, contrast fixes in W2 would miss them); a reader of the source is told the
opposite of the truth.
*Fix:* add map tokens to `design.rs` for the five sites (`bg_0`/`grid`/
`text_3`/`accent`/`on_accent` are the nearest existing roles; the marker core
needs a map token) and correct the two doc comments.
*Effort:* S.

**M14 — The RTT sparkline still has no axis, units or label, and can appear immediately as a shape.**
*Where:* `app/widgets.rs:437-470` (hand-painted line, `PLOT_H` 64, last point
coloured by `rtt_colour`); drawn as soon as `rtt_history` has ≥2 samples and the
session is relaying (`app.rs:2179-2183`). Live: the boosting screenshot draws it
from the first keepalive samples, normalised to min/max so its vertical scale is
meaningless (`window-boosting-hold-320.png`).
*Rule:* "numbers are proof", no fabricated precision, every number labelled.
*Impact:* the chart reads as data but cannot be read; a min-max normalised line
makes a 1 ms wobble look like a cliff. Listed as open in `interface.md:230`.
*Fix:* either label it (unit, window, last value) and use a fixed scale, or drop
it at narrow width / short height — `design-system` decides which. W2 needs an
explicit decision; the October "drop it at short height" recommendation was
never implemented (the short-height branch only folds the route card into the
scroll region, `app.rs:2528-2532`).
*Effort:* S-M.

**M15 — The port-range hint references a label that no longer exists.**
*Where:* `app.rs:2940-2954` (`Use this if Packets Sent stays at 0 after 15 s.`);
the shipped rows are `Relayed` with `{sent} up · {returned} down`
(`locales/en.toml:198`), and the 15 s threshold exists nowhere in code.
*Rule:* copy points at facts the screen shows; no invented thresholds.
*Impact:* a troubleshooting hint that cannot be followed; the user looks for a
field that is not there.
*Fix:* rewrite against the current row and generate the threshold from a
constant, or link the troubleshooting wiki without the number.
*Effort:* S.

### Minor

- **m1 — Two spellings of one version:** `Current version: 1.11.3` vs
  `Latest version: v1.11.3` (`app.rs:3686-3698`; live
  `window-update-sheet.png`). Normalise once.
- **m2 — Mixed separators and dash styles:** `-` in catalog sentences
  (`locales/en.toml:54`, `67`), `—` in `game.auto_named` (`:64`), `·` in data
  pairs (`:198`), `→` painted as text in the rail (`app.rs:2005`). Pick one
  separator per role (`design-system` owns the glyph set).
- **m3 — `Show window` is the only tray action that is state-correct;** the
  menu order puts a no-op action first in two of four states (see M11).
- **m4 — Button naming case is inconsistent:** `Add Proxy` (`locales/en.toml:46`)
  vs `Save server` / `Open log file` sentence case. One voice.
- **m5 — The window has no Connect vocabulary** while the catalog has
  `action.connect` and `action.connecting` unused (`locales/en.toml:157-159`) —
  evidence that the missing control (B2) was once intended.
- **m6 — The OS title bar stays in the system light theme over the dark app**
  (visible in every capture; e.g. the white strip above `CONNECTED` in
  `window-connected-320.png`). Windows 11 dark-mode adaptation is a
  `desktop-platform` gap, not a pixel choice, but it is the first thing a user
  sees.

---

## 6. Usability test plan (executable by `qa-engineer`)

### 6.0 Method and automation constraints (learned in this audit)

- **Build:** `$env:CARGO_TARGET_DIR=%TEMP%\qa-target; cargo build -p lightspeed-gui`.
  For boost tests copy `WinDivert.dll` + `WinDivert64.sys` beside the exe
  (from the installed app or the main checkout's `target\debug`).
- **Launch:** the GUI must run in the user's session. The window's X hides to the
  tray; use the footer `Quit` button (or tray Quit) to exit between cases.
- **Elevated windows:** if the QA session is elevated and the automation daemon
  is not, `SendInput`/`PostMessage` are dropped by UIPI. Drive **real buttons**
  through UI Automation (`InvokePattern`) and read state from the UIA tree +
  window screenshot. **The primary action is a custom-painted widget with no
  accessible name** today (B1): coordinate injection cannot press it and UIA
  exposes it as `Custom` with no label — record that as the expected failure of
  T3/T5 until B1 is fixed, then re-run with UIA `Click` on the labelled element.
- **Popups:** egui popups are invisible to UIA; only a screenshot proves an open
  combo. A driver click cannot open a combo under UIPI — use a non-elevated
  build/session for T6 or accept screenshot-only evidence when it is open.
- **States that need a live game or a hostile relay** are marked *fixture
  required*; if no fixture is available, report **unverified** — never fabricate
  a state (`lightspeed-gui-design` §8, `interface.md:245-261`).
- **Screenshots:** save to `wat/evidence/<date>/qa/` and name them in the result.
  Capture at the widths the shipped build allows (320 px today) plus the window's
  actual height, and note the height (it changes per locale/state, M4).
- **Run each case twice:** EN, then DE via top-level `language = "de"` in
  `%APPDATA%\light-speed\config.toml` (back up first). ja/ko/zh run once the
  catalog is translated (C7).

### 6.1 Cases

| ID | Case | Precondition | Steps | Expected observation | Pass/Fail |
|---|---|---|---|---|---|
| T1 | One-second read | connected, no game | open the window; note rail word, sub-line, metric, route card in <2 s | word describes the route; sub-line names game→relay; metric has a visible unit + what it measures (C4); no stale data | PASS only if the metric is identifiable without hover; FAIL today (C4) |
| T2 | First run, no relay | fresh config dir (back up/rename `%APPDATA%\light-speed`); network available | launch; observe first 10 s | a next step is visible; no disabled bright action without reason; discovery failure shows a cause and a retry | PASS only if both discovery-success and discovery-failure variants give a next step; FAIL today (M5/M6) |
| T3 | Boost start failure is visible | exe folder **without** `WinDivert.dll`/`WinDivert64.sys`; a detectable process running (a renamed `PING.EXE -t 127.0.0.1` as `FortniteClient-Win64-Shipping.exe` is enough) | launch; wait for auto-start | the window states the failure in the user's language and offers a next action (open log, retry, install guide) | FAIL today (B3; live evidence `window-game-detected-start-failed-silent.png`) |
| T4 | Reconnect | connected | press `Disconnect`; try to reconnect **from the window only** | a Connect control exists and works; the disabled action explains itself | FAIL today (B2; `window-disconnected-320.png`) |
| T5 | Boost / stop, keyboard only | elevated build with WinDivert; detectable process | Tab through the window (count stops), activate with Space/Enter; verify focus is visible on every stop; press the action to start and to stop | every control reachable, focus ring visible on each, action starts/stops the tunnel | FAIL today (B1: no visible focus, unnamed action); key-activation itself needs confirming in the same run |
| T6 | Relay picker content | connected | open the Relay combo (non-elevated QA session) and screenshot | each row shows label + measured client leg; the relay chosen for the current server is marked; the list fits/scrolls at 320 px; "best" marker has a text explanation (not colour only) | screenshot-only; verify copy against `app.rs:2684-2709` |
| T7 | Game picker size | connected, 28 built-ins + ≥1 custom | open the Game combo and screenshot | the list is usable at 320 px (scrolls, selection visible, `New custom game…` reachable) | unverified today (popup not captured); record pass/fail on first run |
| T8 | Elevation path | **non-elevated** session | press the action; observe UAC; accept once, then decline once | declining leaves the app running and explains what is needed; accepting relaunches with no session loss beyond the restart | FAIL today (C8: process exits before UAC resolves) |
| T9 | Tray round-trip | tray present | X (hide), restore via tray `Show window` and double-click; tray Quit | hide/restore/quit all work; tooltip names the real state; menu items only enabled where meaningful | partial today (M11: always-enabled items, English-only, fact-free error) |
| T10 | Second launch | first instance running | launch the exe again | the notice names the tray as the way back and exits 0 | PASS today (`single_instance.rs:77-89`, observed) |
| T11 | Update flow | network | Settings → `Check for updates`; close during the check; reopen | a result or a clear retry appears; closing does not leave the app stuck; version strings match format | PASS for up-to-date; FAIL/blocked for available branch (no receipt-less install here) and for raw-error branch (C6/M12) |
| T12 | Error surface | any engine `*_error` fixture reachable (e.g. T3) | trigger; read window and tray | window names what failed in user language; tray and window agree | FAIL today (B3/B4) |
| T13 | Locale sweep | EN/DE/ja/ko/zh | per locale: rail word, sub-line, action label, ledger hints, error copy; measure the action label's rendered height | no English left in a translated surface (documented exemptions only); action label ≥11 px and never clipped | DE: FAIL (C7: `Routing`, `Re-test a relay…`; English fallback in ja/ko/zh per §4.1 #15) |
| T14 | 200 % DPI | Windows display scale 200 % | launch; screenshot each state | content not clipped; action/footer reachable; hit targets scale | unverified today |
| T15 | Destructive actions | a custom relay added; settings changed | press `×` on the relay; press `Reset to defaults` | confirmation or undo; state remains recoverable | FAIL today (M10) |

Pass/fail is per the acceptance line, not per "it looked fine". A case that
cannot be set up is reported as **unverified** with the missing fixture named.

---

## 7. What W2 should build from this audit (handoff)

The `gui-refresh-spec` synthesis should consume:

1. **A state→surface map** with the corrections in §3: add `CONNECTING`,
   `RELAY REFUSED` (registration), `ERROR`; make `HOLDING` the pre-tunnel word;
   give `DIRECT FALLBACK` its cause; remove the `OPTIMIZING`-while-held claim.
2. **One error surface and one error taxonomy** (B3, B4, C6): the engine fields
   already exist; the boundary crate (`gui-core-boundary.md`) owns the mapping,
   the GUI renders.
3. **The action slot contract**: accessible name, visible focus, three labels
   (plus disabled reason), Connect/Disconnect as one reversible slot.
4. **The copy deck** in §4.1, one row per shipped string, with the constant that
   owns each number.
5. **Window decisions** from `desktop-platform` (§M4): resizable + remembered
   geometry, or a documented pinned-width exception; the breakpoints in
   `interface.md` must match whichever ships.
6. **The sparkline decision** (M14) from `design-system`.
7. **The i18n plan** (C7): catalog the hardcoded strings, translate the state
   vocabulary, re-record the English-fallback decision with the real key list.

---

## 8. October 2026 checklist — what reproduces on `62c6cf6`

The Oct-2026 set (`ui-audit.md` top 10, `ui-design-critique.md` 16 required
changes, verdict REVISE) was never accepted. Checked against the current tree and
live window:

| Oct item | Status now | Evidence |
|---|---|---|
| Top 10 #1 every content type is the same card | **No longer reproduces** — one route card + one config card + sheets; settings are a sheet, not section cards | `app.rs:2141-2184`, `4025-4033`; `window-connected-320.png` |
| Top 10 #2 ALL-CAPS eyebrow above every section | **No longer reproduces** — `section_label` is sentence case `LABEL` (`app.rs/widgets.rs:391-399`); only the state word and action label are uppercase (canon) | `window-settings-sheet.png` |
| Top 10 #3 `WORD - fragment` strings and pipe-joined counters | **Mostly fixed**; residue in `relay.refresh_failed` and `game.none_detected_hint` (dash), and the counters now use `·` | `locales/en.toml:54,67,198` |
| Top 10 #4 monospace as costume / unused HEADING | **No longer reproduces** — mono only for data (`VALUE`), ledger labels are Inter CAPTION | `app/theme.rs:191-201`, `app/ledger.rs:269-278` |
| Top 10 #5 default egui chrome where custom chrome exists | **Partially** — combos/buttons/checkbox chrome themed (`app/theme.rs:131-204`); the OS title bar is still the light system bar (M6), and popups are unverified | every screenshot |
| Top 10 #6 eight inline literals / 11-variable palette hole | **app.rs fixed** (zero literal colour sites, full token set in `design.rs`); **5 literal colour sites remain in `globe.rs`** — four `from_rgb` (`:71-73,125`) + one `Color32::WHITE` (`:133`) (M13) | `grep Color32::from_rgb` + `Color32::WHITE` |
| Top 10 #7 loudest control identical in every state, not the product action | **No longer reproduces in structure** — one slot, three gate labels (`ledger.rs:39-57`); the elevation state is still the loudest when unelevated, by design and now named | `app/tests.rs:1493-1519` |
| Top 10 #8 disconnected card claims live data | **No longer reproduces** — markers gated on `connected` (`app.rs:2093-2116`), sparkline gated on relaying (`:2179-2183`), `Relay --` (`:2391-2409`); verified live | `window-disconnected-320.png` |
| Top 10 #9 half the controls buried in one disclosure | **No longer reproduces** — Settings sheet with Privacy/Maintenance/About, footer pinned | `window-settings-sheet.png` |
| Top 10 #10 footer never renders; 320 px clips text | **No longer reproduces for the footer** (pinned, verified); wrapping improved; the window still cannot be wider than 320 px (M4) and the German footer wraps to two rows (accepted) | `window-connected-320-de.png` |
| Critique req. 1 keep an elevation path | **Done** — one slot, three labels | `ledger.rs:39-57` |
| Critique req. 2 rule making the disconnected card honest | **Done** (code + live) | see #8 |
| Critique req. 3 destination for Privacy/Maintenance/About/Disconnect | **Done** (sheet) — but Disconnect became one-way (B2) | `window-settings-sheet.png` |
| Critique req. 4 retire dashed-meta copy | **Mostly** — Reliability Shield/Advanced copy gone; two catalog strings remain | `locales/en.toml:54,67` |
| Critique req. 5 caps contradiction / `ROUTE` eyebrow | **Done** — sentence-case section labels, no `ROUTE` | `app.rs:2529` |
| Critique req. 6 complete the token scale | **Done** — `design.rs` (S1-S5, radii, sizes, type) | `design.rs:94-164` |
| Critique req. 7 height breakpoint | **Implemented** (`short = height < 560`, `app.rs:3761`) but not exercised: the window's content-driven height rarely drops below 560 and the minimum is 420; needs a live case in W2 | `app.rs:3761`, `2527-2532` |
| Critique req. 8 game picker as a deliberate panel | **Partly** — one-step combo + editor sheet; the popup list is 31 rows at 320 px and unverified (T7) | `app.rs:2744-2846` |
| Critique req. 9 relay identity from real data | **Done** — `friendly_label` or raw address, no invented cities | `discovery.rs:135-157`, `app.rs:2391-2409`; live `LAX - Los Angeles` |
| Critique req. 10 rail/sparkline colour rule | **Done** — sparkline `text_3` with `rtt_colour` last point; rail edge signal only while boosting | `widgets.rs:437-470`, `app.rs:3883-3890` |
| Critique req. 11 migrate all literals | **Partially** — app.rs zero; `globe.rs` five literal colour sites — 4 `from_rgb` + 1 `Color32::WHITE` (M13) | `globe.rs:71-73,125,133` |
| Critique req. 12 accent contrast / accent never ink | **Done** — accent fill only, `accent_text` for ink (`theme.rs:146`), `on_accent` replaces white | `design.rs:52-64,88` |
| Critique req. 13 no unclaimed reduced motion | **Resolved by removal** — the shell has no motion at all (static map, no rotation); the "reduced motion" claim must not reappear in the spec unless a real OS source is wired | `app.rs:2149-2153`, `globe.rs` |
| Critique req. 14 shadows off | **Done** (`theme.rs:141-142`) | code |
| Critique req. 15 clamp and scroll sheets | **Done** — every sheet clamps width/height and scrolls; verified visually | `widgets.rs:414-432`, `app.rs:3066-3095` |
| Critique req. 16 ledger mechanics in pixels | **Done** — `ROW_H`, `LEDGER_LABEL_W`, fixed allocations | `design.rs:137-140`, `app.rs:2359-2373` |
| Critique "additional finding" fabricated relay data | **Not reproducible** (labels are real) — but the *game* identity is now shown as a raw key (M3) | live screenshots |
| Critique additional finding 22 games / six tiles | **Not applicable** (no tile grid shipped); registry now 28 games and the popup size is the open question (T7) | `client/src/games/mod.rs:396-425` |
| Critique additional finding bold-weight claim | **Done** — `Inter-SemiBold` is registered as the `semibold` family | `app/theme.rs:17-93` |
| Critique additional finding combo-chrome misdiagnosis | **Done** (global themed visuals) | `theme.rs:131-204` |
| Critique additional finding status-vocabulary drift | **Still reproduces, differently** — window says `NOT OPTIMIZING`/`CONNECTED`, tray says `disconnected`/`connected`, catalog has an unused `state.disconnected`, and the manual hints say `start optimizing` while the button says `Optimize my route` (§4.1 #13) | `locales/en.toml:148,163,202-203`; `windows.rs:142-152` |
| Critique additional finding `text-3` at 11 px is the floor | **Still the floor** — contrast not re-measured here; `design-system` owns the measured table | `design.rs:49` |
| Critique "route card carries the most chrome" | **Still true** — rail + map + five potential ledger rows + sparkline; `design-system`/`interface-designer` decision for W2 | `window-boosting-hold-320.png` |
| `interface.md` open issue: window absent from OS window list while a game has focus | **Not reproduced** in this session (synthetic process, not a fullscreen game); remains unverified/open | `interface.md:226-229` |
| `interface.md` open issue: sparkline has no axis/units/label | **Still open** (M14) | `widgets.rs:437-470` |
| `interface.md` open issue: Components labels English-only by decision | **Still true**, and the exception does not cover `Routing`/`Re-test…` (C7) | `window-settings-sheet-de.png` |
| `interface.md` open issue: 7 route.server_leg* strings English | **Still true — and it is 26 keys in ja/ko/zh** (C7) | locale comparison |
| `interface.md` open issue: measured server leg reachability | **Unchanged/unverified** | `interface.md:232-237` |

---

## 9. Unverified, blocked, and assumptions

Named so no reader mistakes a plan for evidence:

1. **Elevation-required state** — this session's process is elevated
   (`Settings → Components → Elevation: administrator`); the unelevated window,
   the UAC decline path and the elevated-relaunch flow were not driven. Code-cited
   only (§2 F6).
2. **Direct fallback / re-test / `Server rejects the relay`** — needs a live
   game and a relay that forwards but does not return; not reproducible on
   demand without touching the fleet (owner gate). Code + contract cited.
3. **A real boosting session with game traffic** — the live boost used a
   synthetic process and produced no game traffic (`0 up · n down` are relay
   keepalives), so the `Game server`, `Server ping`, `Server leg` and measured
   `Relaying` branches were not exercised. No screenshots are claimed for them.
4. **egui popups** (relay picker, game picker) — could not be opened under
   injected input on the elevated window; UIA cannot see them. Content cited from
   source; the 31-row game list at 320 px is a W2 verification item (T7).
5. **Screen reader** — the unnamed `Custom` action is a live UIA fact, but no
   NVDA/Narrator session was run; announcements are inferred from the tree.
6. **200 % DPI, Linux, macOS** — untouched here; `desktop-platform` owns the
   parity plan.
7. **ja / ko / zh-Hans rendered layout** — only catalog data was inspected; the
   German layout was captured. The action-label shrink at 9 px
   (`ledger.rs:77-88`) is arithmetic from the code and the 45-50 character de/ru
   labels; it was **not** measured in those locales.
8. **Update-available and update-failure branches** — no such release and no
   receipt-less install in this session; only `You're up to date` was observed.
9. **"Window often absent from the OS window list while a game has focus"** —
   not reproduced with a synthetic process; remains open (`interface.md:226-229`).
10. **The synthetic game** was a renamed `PING.EXE`; detection, auto-start,
    hold and teardown are real code paths, but no game packets were relayed.
11. **This audit did not re-measure contrast or type metrics** — `design-system`
    owns the measured table; this document cites sizes/roles only.

---

## 10. Evidence index

All under `wat/evidence/2026-11-19/team/ux-lead/`:

| Path | Contents |
|---|---|
| `report.md` | working notes: method, captures, trace excerpts, side effects/cleanup, unverified list, reproduction steps |
| `window-connected-320.png` | connected / idle, EN, elevated, 320×866 |
| `window-disconnected-320.png` | after `Disconnect`, no reconnect control, 320×803 |
| `window-settings-sheet.png` | Settings sheet, EN (relay rows centred; scroll) |
| `window-settings-sheet-de.png` | Settings sheet, **DE** — `Routing`, `Re-test a relay after falling back` still English |
| `window-update-sheet.png` | update check, up to date (`1.11.3` vs `v1.11.3`) |
| `window-connected-320-de.png` | connected / idle, **DE**, 320×914 (window grew for the locale) |
| `window-game-detected-start-failed-silent.png` | game detected, auto-start **failed**, window still `CONNECTED` with no error |
| `window-boosting-hold-320.png` | real WinDivert interception holding traffic direct while the rail says `OPTIMIZING`; `Game server` shows the engine scanning sentence; `0 up · 3 down`; sparkline; `STOP OPTIMIZING` |
| `window-stop-attempt-noop.png` | the same state after a coordinate click on the action — injected input does not reach the elevated window's custom button |
| `config-backup-before-ux-audit.toml` | the GUI config used during captures (restored afterwards) |

