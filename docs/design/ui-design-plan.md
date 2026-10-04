# LightSpeed GUI — UI Design Plan (Pass 1)

Status: **design phase, pass 1 of 2.** No code is written from this document until it is
accepted. Grounding sources read in full before writing: `client-gui/src/app.rs`
(`mod theme`, `LightSpeedApp::ui`, `config_body`), `web/styles.css` (`:root`), and the existing
`client-gui/DESIGN.md`. Canonical visual language is the website; where the app and the site
disagree, the site wins unless a noted app constraint overrides it.

## Subject & job

LightSpeed is a competitive-gaming network optimizer: it measures the relays it knows about and
reroutes a chosen game's traffic through the one with the shortest path. The desktop window
(`client-gui`, egui/eframe, Windows-first) is therefore not a page and not a control panel — it is
a **status instrument**. A player alt-tabs to it mid-match and must answer three questions in about
one second: *is my traffic being boosted, through which relay, and is the link good or bad.*
Starting and stopping the boost with confidence, choosing the game and the relay, and reading the
proof (round-trip milliseconds, packets carried, sessions) are the secondary jobs. This reframes
every styling decision below: legibility outranks expression, measurements outrank prose, and the
one thing the window must never do is compete with the game beside it for attention.

## Design principles

1. **One glance, one answer.** The state rail names the state and the live round trip above
   everything else; nothing on screen is louder, and nothing above it competes.
2. **The path is the product.** Show the two endpoints that exist — your relay and the game
   server — and the one round trip we actually measure. LightSpeed's claim is a *shorter path*,
   not anonymity, so the window shows a route, never a "protected" badge.
3. **Numbers are proof.** Every measured value (ms, packets, sessions, ports, addresses) is set in
   JetBrains Mono, tabular, right-aligned where it forms a column; prose never outranks a number.
4. **One bright control; hairlines everywhere else.** Solid accent fill is spent on exactly one
   affirmative action. Every other boundary is a 1px `border-1` hairline on a near-background
   surface — no second bright fill, no shadows.
5. **Silence unless the state changes.** No entrance animation, no idle motion, no hover motion on
   things that are not controls. The globe is the single accepted flourish, and it only moves while
   a boost is live and the window has focus.

## Tokens

The target is **one token module** carrying the site's full `:root` scale (the standing goal is
extraction to `client-gui/src/design.rs`, with `mod theme` reduced to re-exports). The app adopts
the site's names verbatim so the two files diff cleanly. Alpha tints stay `rgba` (egui builds them
with `Color32::from_rgba_unmultiplied`); the alpha channel is never baked into a hex value.

### Target token set

| Token | Value | Role |
|---|---|---|
| `bg-0` | `#07070f` | deepest plane: sunken fields, plots, window backdrop behind sheets |
| `bg-1` | `#0a0a1a` | window fill (the app's default background) |
| `bg-2` | `#101024` | raised surface: cards, the state rail, sheets |
| `bg-3` | `#16162e` | hovered / active rows, quiet filled controls |
| `bg-4` | `#1d1d3a` | pressed rows, globe graticule band, highest surface |
| `border-1` | `#23233f` | every hairline: card edges, dividers, plot axes |
| `border-2` | `#2f2f52` | hover / focus hairline, scrollbar thumb |
| `text-1` | `#f2f2fa` | primary text, labels, values |
| `text-2` | `#b4b4cc` | prose: help text, second line of the rail |
| `text-3` | `#8a8aa8` | tertiary: units, captions, footnotes, disabled ink |
| `accent` | `#6c5ce7` | action fill (the single bright control), globe route arc |
| `accent-strong` | `#7d6ff0` | hover fill of the action, focus glow |
| `accent-deep` | `#5a4bd4` | pressed fill of the action |
| `accent-text` | `#a29bfe` | accent used as *ink*: links, selected labels, focus ring |
| `accent-soft` | `rgba(108, 92, 231, 0.12)` | selected-row tint, rail edge on CONNECTED |
| `accent-line` | `rgba(108, 92, 231, 0.28)` | accent hairline: focused field, active tab |
| `signal` | `#00d68f` | good / healthy: BOOSTING state, quality RTT, live counters |
| `signal-soft` | `rgba(0, 214, 143, 0.12)` | BOOSTING chip/edge tint |
| `info` | `#00cec9` | neutral fact: discovery in flight, "measuring" |
| `info-soft` | `rgba(0, 206, 201, 0.12)` | info tint |
| `warn` | `#fdcb6e` | attention: degraded RTT, relay-refresh failure, game server marker |
| `warn-soft` | `rgba(253, 203, 110, 0.12)` | warn tint |
| `danger` | `#ff6b6b` | bad / destructive: errors, STOP BOOST ink |
| `danger-soft` | `rgba(255, 107, 107, 0.12)` | error tint, STOP hover fill |
| `on-accent` | `#ffffff` | ink placed on `accent` (new; replaces the raw `Color32::WHITE`) |
| `scrim` | `rgba(7, 7, 15, 0.72)` | new: backdrop behind in-window sheets (Proxy Manager, Update) |
| `grid` | `rgba(242, 242, 250, 0.06)` | new: globe graticule and plot gridlines |

Semantic grammar, fixed: **signal = good, warn = attention, danger = bad, accent = action and
identity, info = neutral fact.** `accent` is never used to mean "connected"; it means "this is the
thing you can do" or "this is LightSpeed's route".

### Mapping from the existing app tokens

| Existing (`mod theme`) | Value today | Target token(s) | Change |
|---|---|---|---|
| `BG` | `#0a0a1a` | `bg-1` | rename only — same value, now named for its plane |
| `BG` (reused as input/plot fill) | `#0a0a1a` | `bg-0` | **role split:** sunken fields drop one plane deeper |
| `SURFACE` | `#101024` | `bg-2` | rename only |
| `SURFACE_RAISED` | `#16162e` | `bg-3` | rename only |
| — (missing) | — | `bg-4` | **new:** pressed rows + highest surface |
| `BORDER` | `#23233f` | `border-1` | rename only |
| — (missing) | — | `border-2` | **new:** hover/focus hairline (replaces `ACCENT * 0.6` strokes) |
| `TEXT` | `#f2f2fa` | `text-1` | rename only |
| — (missing) | — | `text-2` | **new:** the prose level between primary and dim |
| `TEXT_DIM` | `#8a8aa8` | `text-3` | rename only |
| `ACCENT` | `#6c5ce7` | `accent` | rename only |
| — (missing) | — | `accent-strong`, `accent-deep`, `accent-text`, `accent-soft`, `accent-line` | **new:** hover/pressed fills, accent ink, tints (all already exist on the site) |
| `OK` | `#00d68f` | `signal` + `signal-soft` | rename; soft tint added |
| `WARN` | `#fdcb6e` | `warn` + `warn-soft` | rename; soft tint added |
| `BAD` | `#ff6b6b` | `danger` + `danger-soft` | rename; soft tint added |
| — (missing) | — | `info` + `info-soft` | **new:** the state the app currently fakes with `TEXT_DIM` ("measuring…") |

Inline-literal debt stays debt: the ~20 `Color32::from_rgb` calls at app.rs 749, 856, 898, 903,
914, 916, 936, 1470, … migrate to the nearest target token **when the code around them is touched**
(749 `(255,190,60)` → `warn`; 856 `(25,25,35)` → `bg-0`; 898/903 `(220,90,90)` → `danger`;
914 `(40,90,55)` → `signal-soft` over `bg-0`; 916 `(60,60,60)` → `bg-3`; 936 `(220,130,50)` → `warn`;
1470 `(220,80,80)` → `danger`). No new inline literal is ever added.

## Type

Faces are **fixed by the bundle**: Inter Regular, Inter SemiBold, JetBrains Mono Regular. There is
no mono bold and no Inter Light, so weight is a binary choice inside Inter and emphasis in data is
carried by **size, color, and alignment — never by a fake weight.** Sizes are px, matching the
site's rem scale at a 16px root (2xs→11, xs→12, sm→13, md→17, lg→20, 2xl→30), so a token has the
same size on the landing page and in the window.

| Role | Family | Weight | px / line | Where |
|---|---|---|---|---|
| `caption` | Inter | Regular | 11 / 16 | units (`ms`), footnotes, tooltips, disabled ink |
| `label` | Inter | SemiBold | 12 / 16 | field labels and section labels, sentence case |
| `body` | Inter | Regular | 13 / 19 | prose: help text, the rail's second line |
| `value` | JetBrains Mono | Regular | 17 / 24 | addresses, ports, node ids, counters — tabular by nature |
| `title` | Inter | SemiBold | 20 / 26 | sheet titles ("Proxy Manager", "Update") |
| `state` | Inter | SemiBold | 30 / 34 | the state word in the rail |
| `metric` | JetBrains Mono | Regular | 30 / 34 | the live round trip in the rail, same optical size as `state` |

Rules: the state word and the metric are the only 30px type in the window and they sit on the same
baseline; mono at 11px is forbidden (that is mono-as-costume — captions are Inter); tracking is
`-0.01em` on the 30px pair and `0` everywhere else; **no wide-tracked all-caps eyebrows**; help
text wraps under 80 characters. Numbers never jitter: JetBrains Mono is tabular, and the RTT figure
is padded to three characters so 9 ms and 124 ms do not shift the rail.

## Layout

The window is 440×800 by default, 320×400 minimum. Regions top to bottom: **state rail**
(full-bleed, fixed), **action bar** (fixed), **route card** (fixed), **config** (the only scrolling
region), **footer** (fixed). This is the repo's `scroll-body-shell`: the middle owns the scroll so
the footer is never pushed off-screen. Card radius is 8 and there is exactly one card style; the
rail has **no radius and no side margin**, so the shell never reads as a stack of identical rounded
cards.

### State: not boosted (no relay)

```
┌────────────────────────────────────────────────────┐
│▌NOT BOOSTED                                        │
│ No boost server                                    │
│ ─────────────────────────────────────────────────  │
│  [        BOOST MY GAME  (waiting on relay)      ] │
│ ─────────────────────────────────────────────────  │
│  ROUTE                                             │
│   ┌────────────────────────────────────────────┐    │
│   │      ( globe at rest, no arc, graticule )   │    │
│   └────────────────────────────────────────────┘    │
│   RELAY          -- not connected                  │
│   GAME SERVER    --                                │
│   RELAYED        --                                │
│  ─────────────────────────────────────────────────  │
│  Boost server        [ Discovering relays…     ▾ ]  │
│  Game                [ VALORANT                ▾ ]  │
│  ▸ Advanced                                         │
│ ─────────────────────────────────────────────────  │
│  Hide to tray   Open log                    Quit   │
└────────────────────────────────────────────────────┘
```

*Intent: the player learns there is no path yet and that the only useful action is to wait or retry
discovery.* Alignment: the rail's 3px left edge and the card's 12px inner gutter share one left
axis; the action bar and both config rows span the full content width, so the eye reads top-down as
one column.

### State: connected (relay up, not routing)

```
┌────────────────────────────────────────────────────┐
│▌CONNECTED                              24 ms       │
│ Relay ready, VALORANT selected                     │
│ ─────────────────────────────────────────────────  │
│  [              BOOST MY GAME                    ]  │  solid accent = only bright control
│ ─────────────────────────────────────────────────  │
│  ROUTE                                             │
│   ┌────────────────────────────────────────────┐    │
│   │   ( globe: relay marker, no server yet )    │    │
│   └────────────────────────────────────────────┘    │
│   RELAY          fra1.de                           │
│   CITY           Frankfurt                         │
│   GAME SERVER    -- start a boost to place it      │
│  ─────────────────────────────────────────────────  │
│  Boost server        [ Auto (fastest)          ▾ ]  │
│  Game                [ VALORANT                ▾ ]  │
│    Game found: VALORANT                            │
│  ▸ Advanced                                         │
│ ─────────────────────────────────────────────────  │
│  Hide to tray   Open log                    Quit   │
└────────────────────────────────────────────────────┘
```

*Intent: the relay is chosen and measured; one press starts the boost.* Alignment: the 30px state
word is left-flush to the rail gutter and the 30px metric is right-flush to it, so the two hero
values frame the rail like the dial marks of an instrument; the ledger's label column is fixed at 14
characters and every value starts on the same x.

### State: boosting

```
┌────────────────────────────────────────────────────┐
│▌BOOSTING                               24 ms       │
│ VALORANT  →  fra1.de                               │
│ ─────────────────────────────────────────────────  │
│  [  ■  STOP BOOST                                ]  │  quiet: bg-3, danger hairline + ink
│ ─────────────────────────────────────────────────  │
│  ROUTE                                             │
│   ┌────────────────────────────────────────────┐    │
│   │  ( globe: home •, relay ◆, server ▲, arc )  │    │
│   └────────────────────────────────────────────┘    │
│   RELAY          fra1.de                           │
│   CITY           Frankfurt                         │
│   GAME SERVER    185.40.12.9:8400                  │
│   RELAYED        12,412 packets   41 sessions      │
│   [ RTT sparkline, 64 px, signal ]                 │
│  ─────────────────────────────────────────────────  │
│  Boost server        [ Auto (fastest)          ▾ ]  │
│  Game                [ VALORANT                ▾ ]  │
│  ▸ Advanced                                         │
│ ─────────────────────────────────────────────────  │
│  Hide to tray   Open log                    Quit   │
└────────────────────────────────────────────────────┘
```

*Intent: the path is live and working, and the only destructive control is available but does not
attract a mis-click.* Alignment: the rail swaps its edge color to `signal`, the state word, the
edge, and the sparkline all carry the same green so "good" is one color at three depths; the
counters align on the ledger's value column, not on a centered row.

### State: game pick (nothing detected)

```
┌────────────────────────────────────────────────────┐
│▌CONNECTED                              24 ms       │
│ Relay ready, pick your game                        │
│ ─────────────────────────────────────────────────  │
│  [              BOOST MY GAME                    ]  │  disabled until a game is chosen
│ ─────────────────────────────────────────────────  │
│  GAME                       choose what to boost   │
│   ┌─────────────────┐   ┌─────────────────┐         │
│   │ VALORANT        │   │ Counter-Strike 2│         │
│   └─────────────────┘   └─────────────────┘         │
│   ┌─────────────────┐   ┌─────────────────┐         │
│   │ Apex Legends    │   │ Dota 2          │         │
│   └─────────────────┘   └─────────────────┘         │
│   ┌─────────────────┐   ┌─────────────────┐         │
│   │ Fortnite        │   │ … full registry │         │
│   └─────────────────┘   └─────────────────┘         │
│   Start your game and connect to a server, then     │
│   LightSpeed picks it up — or choose it here.       │
├────────────────────────────────────────────────────┤
│  Hide to tray   Open log                    Quit   │
└────────────────────────────────────────────────────┘
```

*Intent: when detection finds nothing, the closed registry becomes a visible set of named choices
instead of a hidden dropdown, so the window never dead-ends.* Alignment: a strict 2-column grid on
the 440 window, one column at 320; every tile has the same height so the grid stays rectangular and
the wrapped help text sits on the card's left gutter, not centered.

### State: narrow 320

```
┌──────────────────────────┐
│▌BOOSTING                 │
│ 24 ms                    │
│ VALORANT → fra1.de       │
│ ────────────────────────  │
│ [  ■  STOP BOOST       ]  │
│ ────────────────────────  │
│ ROUTE                    │
│  ┌────────────────────┐   │
│  │  ( globe 72 px )   │   │
│  └────────────────────┘   │
│  RELAY                   │
│   fra1.de                │
│  CITY                    │
│   Frankfurt              │
│  GAME SERVER             │
│   185.40.12.9:8400       │
│  RELAYED                 │
│   12,412 pkts 41 sess    │
│ ────────────────────────  │
│ Boost server             │
│  [ Auto (fastest)     ▾ ] │
│ Game                     │
│  [ VALORANT           ▾ ] │
│ ▸ Advanced               │
│ ────────────────────────  │
│ Hide tray    Log         │
│                   Quit   │
└──────────────────────────┘
```

*Intent: nothing clips and the state is still the first thing read when the window is snapped
beside a game.* Alignment: the metric wraps to a second line under the state word rather than
shrinking either one; every label/value ledger row becomes label-above-value; all controls go
full-width so the tap targets survive.

## Anti-slop self-critique

Walked trait by trait against the skill's checklist. "Revised" means the first draft of this plan
had the tell and the plan above is the corrected version.

1. **Identical rounded cards for every content type, one radius for all hierarchy, one soft shadow
   everywhere — PRESENT, REVISED.** The first draft kept the current shape: a banner card, a
   connection card, and two config cards, all the same `bg-2 / 1px / radius 10` block. *Changed:*
   the state banner became a **full-bleed rail with zero radius and no side margin**, the route
   became the only true card (radius 8), and the config rows stopped being cards at all — they are
   hairline-divided rows on `bg-1`. Three treatments for three content roles, not one. *Why:*
   the state is the instrument bezel, the route is the one piece of content, the settings are a
   list; giving them one shape erased that hierarchy, which is exactly the generic-utility look.
2. **Decorative gradients or wash backgrounds to fill space — ABSENT.** No gradient exists in the
   target. The only colored fill that is not ink is the globe's route arc, and that arc encodes
   *where traffic actually went*.
3. **ALL-CAPS eyebrows over every section; meta strings joined with middle dots; "WORD — fragment"
   titles — PRESENT, REVISED.** The current code ships all three: `theme::card` uppercases its
   title into an 11px dim eyebrow over every card, and the state detail line is
   `"{game} - {rtt} ms"`. *Changed:* card titles are gone; sections are distinguished by whitespace
   and hairline, and their labels are sentence-case `label` type ("Boost server", "Game").
   Dot/dash-joined meta strings are replaced by the **label/value ledger**, which also gives
   addresses a real column. The state word stays uppercase because it is the state — the one
   instance per screen, not a label above content. *Why:* an eyebrow on every card is the single
   loudest "generated dashboard" tell, and it was spending 11px type on a word that carried no
   information the content did not.
4. **Numbered markers 01/02/03 where content is not a sequence — ABSENT.** Nothing is numbered. The
   only ordering device is the fixed top-to-bottom reading order of rail → action → route → config.
5. **Accent sprayed onto a single word for no informational reason — CONSTRAINED, REVISED.** The
   first draft inherited the current behavior of painting CONNECTED in `accent`. *Changed:*
   `accent` now has exactly two jobs (the one affirmative action, and the globe's route arc);
   CONNECTED is `text-1`, and only BOOSTING carries a color (`signal`). *Why:* colouring a state
   with the *action* colour makes "connected", "click me" and "LightSpeed's route" indistinguishable
   — and it left the green "good" signal with nowhere to be special.
6. **Fade/slide entrances on every element, hover transitions on everything, motion without a
   triggering action — ABSENT.** No entrance animation, no cross-fades. Motion is limited to
   control hover (needed feedback) and the globe (only while boosting and focused, and disabled
   under reduced-motion). Value changes in the counters hard-cut; they do not animate.
7. **Monospace as a costume — ABSENT by rule.** Mono is confined to `value` and `metric`: relay
   host, city code, game-server `ip:port`, packet/session counters, and the round trip. Units and
   captions are Inter 11. The first draft used mono for the ledger's label column; *changed:* labels
   are Inter `label`, mono starts at the value column, so the mono run reads as a data block.
8. **egui default chrome left in place wherever custom chrome exists — PRESENT, REVISED.** Today
   the Proxy Manager and Update dialogs are stock `egui::Window`s (default title bar, default
   rounding) and the Advanced section uses the stock collapsing header arrow. *Changed:* both
   dialogs become **in-window sheets** — `bg-0` panel, `border-1` hairline, `scrim` backdrop,
   `title` type — and the disclosure marker becomes the theme's own `▸`/`▾` glyph with a hairline
   row, not an egui arrow. The combo boxes get our box and hairline rather than default chrome.
   *Why:* the rule is that the moment any part of the window is custom, every remaining default
   widget makes the whole thing read as "default egui with colors changed" — which is the failure
   the last revision fixed for the panels and left in the dialogs.
9. **Neutral test — "is this what I would build for any network utility?" — ONE PART REVISED.** The
   generic answer is a big toggle, a green "Protected" badge, and a world map. *Changed:* the rail
   answers *boosted / not boosted*, the route card shows the actual relay→game-server endpoints
   that our data supports, and the empty states say "No boost server" / "start a boost to place it"
   instead of showing a success badge over unmeasured data. The `STOP BOOST` control is deliberately
   not the brightest thing on screen (see principle 4 and the boosting wireframe) — a big red
   button next to a game is a mis-click waiting to happen. What remains that *is* general — a
   label/value ledger, a dark palette, a centered hero number — is justified because for this
   product the ledger *is* the instrument readout.

Residual risk to watch in pass 2: the label/value ledger is the most template-able element here; it
must stay short (four rows), left-aligned to the card gutter, and keep the RTT out of it so the rail
remains the single place the round trip lives.

## Done checklist

Definition of done for the implementation phase, straight from the skill, expressed as gates:

- [ ] **Tokens only.** Every color in code touched by the change comes from the token module; no
      new `Color32::from_rgb`/`from_rgba` literal is introduced, and each legacy literal met during
      the change is migrated (app.rs 749, 856, 898, 903, 914, 916, 936, 1470, …).
- [ ] **One token module.** The site's full scale lives in one place (`client-gui/src/design.rs`
      target); `client-gui/DESIGN.md` is updated to match the new names in the same change.
- [ ] **Type scale respected.** Only the six roles above; no ad-hoc `font-size`; no mono below 17px
      outside genuine data; no new font family.
- [ ] **Screenshots captured and LOOKED AT** at **default width (440)** *and* **320px** for each of
      the four states (not boosted, connected, boosting, game pick), before the next change.
- [ ] **Evidence saved** under `.omo/evidence/gui-ux/` with descriptive names, paths named in the
      report and in the commit message.
- [ ] **320px check:** no clipped text, no horizontal scroll, ledger rows stacked, controls
      full-width.
- [ ] **Accessibility floor:** `text-3` on `bg-1` and `bg-2` checked for contrast; no state carried
      by color alone (every state also has its word); reduced-motion honored by stopping the globe.
- [ ] `cargo fmt --all --check` — clean.
- [ ] `cargo check -p lightspeed-gui` — clean.
- [ ] `cargo clippy -p lightspeed-gui --all-targets` — 0 warnings.
- [ ] `cargo test -p lightspeed-gui` — green.
- [ ] **Committed separately** from unrelated work, with the screenshot paths named in the message.
