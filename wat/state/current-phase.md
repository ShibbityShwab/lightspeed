# Current Phase: WF-054 Internationalization - shipped, then repaired after the live site exposed two real defects

**Workflow:** WF-054; WF-053, WF-052, WF-051, WF-050, WF-049, WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** RustDev + QAEngineer (lead-verified with the repo's own gates)
**Status:** Waves 1-4 committed and deployed (`cffad91`); the two defects the deploy exposed are fixed and re-deployed. Wave 2 (CLI) excluded by design.
**Last updated:** 2026-10-05

---

## 2026-10-05 (third pass) - the deploy exposed what the structural checks could not

`cffad91` was pushed and `Deploy GitHub Pages` succeeded. Opening the live site in
a real browser then showed two defects that every structural check had passed
over. Both are now fixed; both were caught by looking at the page, not by
asserting on its markup.

**1. Every localized page was served with no stylesheet (the severe one).**
The generated pages live at `web/<tag>/index.html` but referenced their assets
relatively - `href="styles.css"`, `assets/favicon.svg`, `src="app.js"`. From
`/lightspeed/de/` those resolve to `/lightspeed/de/styles.css`, which does not
exist: live proof, `/de/styles.css` -> 404 against `/styles.css` -> 200. So all
eight "translated pages" were unstyled HTML with no icon and no script, while the
English page at the root looked perfect. The same relative resolution broke
`app.js`'s own `fetch('network-stats.json')`. Fixed in two places:
`localize_urls()` in the generator prefixes every relative URL with `../`, and
`app.js` now resolves its data files against the script's own URL
(`dataUrl()`/`DATA_BASE`) so it works from any page depth.

**2. Nested units corrupted the markup and stranded English.** 96 unit pairs nest
(`<td><strong>$0/forever</strong></td>` yields a unit for the cell *and* one for
the strong). `render()` applied both, and the outer span's offsets were stale once
the inner replacement changed the length inside it, so the outer replacement ate
into a closing tag - the published Spanish page read
`<td class="compare-us">$0/para siemprerong></td>`. Fixed by resolving overlaps:
an enclosing unit whose text is identical to the unit inside it stands down (the
inner one renders the same words and keeps the markup), and otherwise the
enclosing one wins because it carries the whole phrase. Stray tag fragments went
16/15/10/26/21/16/13/22 -> 0 across the eight locales.

**The guards added, and why they are the point:** `stray_text_gt()` fails the
build if a rendered page leaves a tag fragment in its text (it fires on the old
output: 10-26 per page, and passes on the new), and `unrooted_urls()` fails the
build if any relative URL would resolve under `/<tag>/`. Both run on every page
every time the generator runs, including `--check`.

**Verified by looking, in a real engine (Bun.WebView), against a local server:**
the German page now applies the stylesheet (`ready: true`), renders the nav with
the `Deutsch` switcher, the German hero and buttons, and `$0/fÃ¼r immer` in the
price cell; Russian renders in Cyrillic with `Ð¡ÐºÐ°Ñ‡Ð°Ñ‚ÑŒ`; both load `app.js` (the live
stats read 62 ms / 0.0% rather than "collecting"), proving the data path works
from a subdirectory. At the mobile breakpoint the switcher is hidden behind the
hamburger and becomes visible when the menu opens - checked, not assumed.

**Still true and unfixed:** a block containing an inline link or `<code>` is
skipped as a unit, so ~27 of 174 Spanish text blocks (FAQ answers, the privacy
notice, the network description) remain in English while their bold fragments
are translated. That needs placeholder-based extraction to fix properly - a
rework of the extractor and a retranslation of all eight catalogs.

---

## 2026-10-05 (later) - WF-054: the English landing page could not reach any translation

Re-verified the whole wave on a fresh session (fmt 0, clippy 0, `cargo test -p
lightspeed-gui` 99/0, `generate-web-locales.sh --check` 0, `extract-web-strings.sh
--check` 442 keys 0) and found one real, user-visible hole nobody had closed.

The eight generated pages carried an `hreflang` alternate set and a no-JavaScript
language switcher, but **`web/index.html` - the page every visitor and crawler
lands on - had neither** (`grep -c hreflang web/index.html` was 0, `lang-switch`
0). The generator also wrote `web/locales/alternates.en.html` and **nothing
consumed it**. So the site advertised "we speak your language" in eight
languages while its front door offered no way in, and the eight pages were
unreachable except by typing the path.

**Why it was open, and the constraint that shaped the fix:** `web/index.html` is
the extractor's source of truth - its text is what `en.json`'s 442 keys index -
so rewriting it is exactly the bug wave 3 hit earlier ("the generator rewrote
`web/index.html` as its own English output, which injected the hreflang block
into the source and shifted every positional key"). The English build therefore
cannot be the source file, and the site has no build step: `web/**` is published
verbatim.

**Fix (three files, no key churn):**

| File | Change |
| --- | --- |
| `scripts/generate-web-locales.sh` | New `render_english()`: English source + the same `alternates_block` and `language_switcher` the other pages get, written to `web/index.en.html`. `--check` now fails when that file is missing or stale. The now-unconsumed `web/locales/alternates.en.html` is gone. |
| `.github/workflows/pages.yml` | New step before the upload: copy `web/index.en.html` over `web/index.html` in the artifact, failing the deploy with an explicit error if it is missing. The tracked source page is untouched, so key positions cannot shift. |
| `web/index.en.html` | New generated artifact (78721 bytes, LF). |

**Verified:**
- Content diff against the source: the output has **zero** missing element texts
  and exactly ten added ones - the nine language names in the switcher and the
  `lang-switch-label` - so no English string was altered while injecting.
- `web/index.en.html` carries 9 `hreflang` links (8 locales + `x-default`), all
  absolute on the published origin, and a switcher whose options are exactly the
  9 locale pages (no 404 can be offered).
- Simulated the deploy step in a scratch copy of `web/`: the promoted
  `index.html` has 9 `hreflang` and the switcher, `lang="en"` intact.
- `bash -n` on the generator; `generate-web-locales.sh --check` and
  `extract-web-strings.sh --check` both rc=0; re-ran the generator to prove it is
  idempotent; `cargo fmt --all --check` rc=0 and `cargo test -p lightspeed-gui`
  99 passed / 0 failed / 1 ignored on the current tree.

**Still not verified, same as before:** no pixel-level look at the served page -
wave 3's evidence remains structural. Language switching is still only reachable
from the selector in the artifact; nothing has been deployed (the workflow runs
on push to the paths in its trigger).

---

## 2026-10-04 - WF-054 i18n: engine plus the whole GUI extracted

The request was full internationalization/localization across the project. Scope
was set by the owner as **everything, phased**, with **machine-translated seed**
catalogs. Wave 1 (the i18n engine and `client-gui`) is complete; the other three
surfaces are not started.

**Landed (local, uncommitted):**

| File | Change |
| --- | --- |
| `client-gui/src/i18n.rs` | New. Catalog loader, tag resolution, English fallback, `t`/`t_with`, `LOCALES`, merged-catalog cache, 11 tests |
| `client-gui/locales/*.toml` | New. 9 catalogs (en source + de, es, fr, ja, ko, pt-BR, ru, zh-Hans) |
| `client-gui/src/app.rs` | 31 static call sites + 9 interpolated strings now catalog-driven; language state; Settings picker |
| `client-gui/src/config.rs` | `language` field parsed/rendered with tests |
| `client-gui/src/main.rs` | `mod i18n;` |

**Verified by re-running the gates myself:** `cargo fmt --all --check` (rc=0),
`cargo clippy --workspace --all-targets -- -D warnings` (clean),
`cargo test -p lightspeed-gui` (99 passed, 1 ignored, 0 failed). The i18n module
alone is 11 tests green on 5 consecutive parallel runs.

**Not done, and the honest reason:** the GUI's other ~130 literals are egui id
salts, tracing log lines, font names and test fixtures that must **not** be
translated; what remains user-visible in this crate is its interpolated and
status copy (partially covered) and `design.rs`/`update.rs`/`discovery.rs`
strings. Waves 2-4 - the `client` and `proxy` clap CLIs, the static website
(incl. per-locale routes and hreflang), and the 30 docs - are untouched.

## 2026-10-05 - WF-054 completion: 8 website locales, 48 doc translations, GUI finished, all verified

Executed as three mass-ulw DAG runs plus local repair. The goal: a non-English
speaker can read the site, read the docs, and use the app in their language.

**Website (run `wf054-web-locales-v2`, 8 nodes):** full 442-key catalogs for
de, es, fr, ja, ko, pt-BR, ru, zh-Hans and eight generated pages. `de.json` was
completed from 44 to 442 keys by a follow-up lane.

**Docs (run `wf054-docs-v1`, 9 nodes):** 48 files - six user-facing pages
(`user-guide`, `install-windows/macos/linux`, `faq`, `troubleshooting`) in eight
languages, as FLAT siblings (`docs/faq.ja.md`) rather than `docs/ja/`, because
`.github/workflows/wiki.yml` globs `docs/*.md` at one level only and strips
`.md` from links; a subdirectory would never publish. `docs/LANGUAGES.md` is the
index and carries an explicit "not reviewed by native speakers" warning.

**GUI:** the remaining 13 literals in `app.rs` (relay refresh/retry, "Discovering
relays…", "No relays discovered", proxy validation, the About block) moved into
the catalogs; nine new keys added across all nine locales.

**Verification, all run by the lead rather than trusted from node summaries:**
- `cargo fmt --all --check` rc 0; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test -p lightspeed-gui` -> **99 passed, 1 ignored, 0 failed**; the 11 i18n parity tests green.
- All 8 generated pages pass an independently written validator: `lang` attribute correct, >=3 headings translated, no `idx.`/`attr.` key leaked, the three install commands byte-identical to English, and `<h1>/<h2>/<section>/<a>/<button>` counts equal to `web/index.html`.
- All 48 localized docs pass an independently written checker: `##` heading count, code-fence count, table-row count and the ordered markdown link-target list all equal the English source, and no file is a copy of English. One real defect was found this way and fixed: `faq.pt-BR.md` had translated the in-page anchor `#what-does-the-proxy-log`, which cannot resolve because GitHub derives anchors from the English heading text.
- No tracked English doc or `web/index.html` was modified (`git status --porcelain docs` shows only untracked additions).

**The run-killing bug this wave found:** the first run's children executed in
isolated checkouts (this session's default), so all seven catalogs were written
into sandboxes and discarded - every node reported `completed` with pasted
"passing" evidence while `web/locales/` held nothing. The session's isolation
default is not configured in `.omo/config.json`; the fix was `isolated: false`,
proven first with a one-file probe that I verified on disk myself. Cancelled the
first run rather than let it finish uncollectable work. Five locales' output was
lost and regenerated. The lesson matches the skill's own warning: a node counts
as completed when its child returns *any* response, so completion claims must be
checked against artifacts, never the transcript.

**Caveats that must not be lost:** every non-English string is machine-assisted
and UNREVIEWED; the new site pages advertise translations that no native speaker
has read. The CLI wave remains excluded by design (operator/log output).


The website is published verbatim from `web/**`, so there is no request-time
place to resolve strings; localization generates real files instead.

**Landed:**

| File | Change |
| --- | --- |
| `scripts/extract-web-strings.sh` | New. Extracts every visible string from `web/index.html` into `web/locales/en.json` (442 keys); `--check` fails on drift |
| `scripts/generate-web-locales.sh` | New. Renders `web/<tag>/index.html` per catalog, injects absolute `hreflang` alternates and a no-JavaScript language picker; `--check` fails on stale pages |
| `web/locales/{en,de}.json` | New. Source catalog plus a hand-authored German seed |
| `web/de/index.html` | New. Generated German page |
| `web/styles.css` | `.lang-switch` control built from the site's own tokens |

**Deliberately excluded from translation:** `<code>`/`<pre>` content, package
commands (`choco install lightspeed`), port ranges, anti-cheat names and game
names. A translated install command breaks copy-paste, so the extractor skips
those elements and a test asserts none leaked in.

**Bugs this wave found in itself, each caught before shipping:**

1. The first extractor captured 144 keys while missing 199 real strings -
   including four of seven section headings - because one combined tag
   alternation let an outer element swallow inner ones. Fixed by scanning per
   tag; key count went 144 -> 343 -> 442.
2. The generator rewrote `web/index.html` as its own "English output", which
   injected the hreflang block into the source and shifted every positional key
   on the next run (`idx.000` -> `idx.001`). The source page is now read-only.
3. Replacement offsets came from a *stripped* copy and were applied to the
   *original*, a 723-character error, so most strings silently kept their
   English text while the page still claimed `lang="de"`. Replacement now uses
   the match's own span, so a miss is impossible by construction.
4. `hreflang="en"` and `x-default` emitted `href=""`; both are now absolute
   URLs built from the published origin.

**Verification:** `generate-web-locales.sh --check` passes; the German page was
validated by a separate script asserting 3/7 sampled headings are German, no
`idx.`/`attr.` key leaked into the HTML, the three sampled install commands are
byte-identical to English, and `<h1>/<h2>/<section>/<a>/<button>` counts match
the English page.

**NOT verified:** no pixel-level look at the served page. A headless browser
screenshot hung and restarted the JS kernel, so wave 3's evidence is structural,
not visual. Only German has a page; the other seven locales have no website
catalog, so "renders each locale" is true for one locale, not eight.

**Next Action:** the wave is feature-complete and locally verified, and the
English landing page now exposes its translations. What remains is the owner's:
commit and push (the whole wave, including tonight's three files), which triggers
`pages.yml` and puts the switcher and the eight locale pages on the live site.
Then the standing caveats: every non-English string is machine-assisted and
**UNREVIEWED**, so do not advertise any locale as supported until a native
speaker has read it; and wave 2 (the `client`/`proxy` CLIs) stays excluded by
design - operator diagnostics and `tracing` logs, where translating breaks
greppability for the sysadmins who are its audience. The one genuine end-user
string there (the telemetry privacy notice) is still noted but not wired.

---

## 2026-10-04 - WF-053 all seven ranked improvements, executed as four DAG phases

Executed via the mass-ulw skill: one run per phase, each phase defined from
the settled previous run's verified outputs, every completion treated as a
claim until the lead re-ran the gates itself.

1. **Continuous relay re-evaluation** - auto-select re-races every 10 minutes
   mid-session and switches only past a 20 ms hysteresis (race_watch).
2. **Adaptive FEC on by default** - config default flips, the GUI engine's
   production path honors it (`enabled: fec`), breaker thresholds re-tuned
   against the pacer.
3. **Telemetry->ML->published loop** - network-stats.sh aggregates saved-app
   samples from relay /metrics; the site hero renders the real median
   (61.6 ms from 1304 paired samples at verification) with a generator
   self-check; model-fit gauges produced end to end (client telemetry ->
   relay gauges) for the Model fit / Trained on hero.
4. **Fleet re-balancing** - NON_FATAL load_skew detector (window-delta share
   + per-hour floor), firing/no-fire self-tests (115-check suite green), and
   docs/fleet-load-rebalancing.md.
5. **Multipath stays opt-in** - recorded in wat/state/decisions.md with
   rationale and alternatives.
6. **Session-token rotation** - RotateRequest/Ack wire messages, client-side
   atomic rotation with previous-token transition slots and a bounded
   exchange, proxy demotion reuse; pinned test fails against the old code
   (mutation-verified).
7. **TCP tunnel (Minecraft Java)** - protocol v5 + TcpExtHeader + capability
   bit, client capture/terminator/TCP checksum, 5-tuple relay splice,
   minecraft-java profile (25565) with the TCP-in-TCP ban; the 42-byte
   byte-pinned roundtrip test mutation-fails against the old code
   (`UnsupportedVersion { version: 5, expected: 3 }`), design in
   docs/tcp-tunnel-design.md.

Also fixed en route: proxy never_loop (12a7801), the GUI RegisterAck caps
integration break, and the GAME_IDS registry gap for minecraft-java.

**Final gates (lead-run on the final worktree):** fmt 0; check 0 on protocol,
client (windivert), proxy, gui; clippy 0 on all four; protocol 73+6, client
405, proxy 139 + integration, gui 79; health-anomaly self-suite 115 checks;
network-stats self-check exits 0. Worktree clean; $0 infrastructure
maintained.

---
# Current Phase: WF-052 Chocolatey pushed; locks no longer brick the window

**Workflow:** WF-052; WF-051, WF-050, WF-049, WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** DevOps + QAEngineer + RustDev
**Status:** Pushed (`82a75f8`, `c4b0eb4`, `8726d1f`); Chocolatey 1.6.14 pushed to the community feed (moderation pending)
**Last updated:** 2026-10-04

---

## 2026-10-04 - WF-052 the Chocolatey key existed all along; the push was the missing step

The owner was right: `CHOCO_API_KEY` has been in the repo's GitHub secrets
since 2026-10-02 - the earlier "no API key exists" notes were stale. The real
gap: the v1.6.14 tag predated the secret, so the release workflow's
bump-chocolatey job bumped the package sources but skipped the push, leaving
the community feed serving 1.6.3 for eleven releases. The new
`choco-push.yml` workflow dispatches pack+push for any version (nuspec
verified against the requested version, `build.sh` packs the OPC zip, `curl`
PUT to the push API with the key). Dispatched for 1.6.14 - run
`37176160239` concluded **success**. Pushes are moderated, so the listing
stays unlisted until a Chocolatey moderator approves it; that is normal.

Also: a first workflow revision put `secrets` in an `if:` expression, which
GitHub Actions rejects (HTTP 422 at dispatch); fixed to the env-gate pattern
release.yml uses. The lesson stands from WF-050's handoff: verify the live
endpoint, not the repo's own notes.

## Poison-tolerant locks

The GUI locks the engine mutex on nearly every frame; a panic under any guard
poisons the lock permanently and every later lock panics too - one background
failure kills the whole window. `read_or_recover`/`write_or_recover` (engine,
7 sites) and `lock_or_recover` (gui, 15 sites) now recover via
`into_inner()`: safe here because the guarded regions are single-field status
assignments, not shared invariants. Tests poison each lock via a caught panic
and assert the next read still works; the engine test was mutation-checked
(panics at the snapshot read when the helper is reverted).

---
# Current Phase: WF-051 The GUI can now update itself in place

**Workflow:** WF-051; WF-050, WF-049, WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** RustDev + QAEngineer
**Status:** Pushed (`1d518cb`); installed build re-synced to master before the feature landed
**Last updated:** 2026-10-04

---

## 2026-10-04 - WF-051 receipt-less installs got an in-place updater

**Driving evidence:** "Check for updates" only ever checked, and for the
Program Files install it did not even do that - axoupdater requires a
cargo-dist install receipt, the manual copy has none, so the dialog reported
"update check unavailable". Every merge therefore drifted the installed build
behind master until someone did an elevated manual copy.

**The fix:** the check falls back to the GitHub releases API when the receipt
is missing (tag names the version; the GUI zip + `.sha256` pair is resolved
from the release assets). An "Install update" button runs `self_update.rs`:
download, verify the sha256 BEFORE anything runs elevated, then an elevated
PowerShell swap script stops the app, extracts with the built-in `tar`
(bsdtar reads zips - no zip crate), copies the new exe into the running exe's
own directory (layout-independent, unlike the MSI), verifies the copy by size,
refreshes WinDivert files when present, and relaunches. An already-elevated
GUI updates with no UAC prompt; a medium one approves once.

**Validated against the live v1.6.14 release, not assumed:** the `.sha256`
file is a 64-hex token + filename, the downloaded archive's sha256 matches it,
and the zip contains exactly `lightspeed-gui.exe`. Tests pin the checksum
parser and the swap script's contract (kill, extract, copy, size check,
relaunch).

Also this phase: the installed Program Files build was re-synced to master
before the feature (it had been pre-`22c9623`, missing the globe/geo bounds
fixes). Open work remains the owner-gated channels (Chocolatey API key, winget
CLA) and the optional mutex-poisoning hardening - see `wat/state/HANDOFF.md`.

---
# Current Phase: WF-050 GUI polish, admin fixes, and a bug round - handoff to an incoming agent

**Workflow:** WF-050; WF-049, WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** RustDev + QAEngineer (this week's GUI work); handoff brief in `wat/state/HANDOFF.md`
**Status:** Pushed `b32975d..22c9623`, every commit CI-green; installed Program Files build is one round behind (pre-`22c9623`)
**Last updated:** 2026-10-04

---

## 2026-10-03..04 - WF-050 the GUI got its polish and a bug round

This week's workstream was the Windows GUI, driven by the owner's feedback:
boost servers became a dropdown with "Auto (fastest)" as the visible default
(it was already the default on first install - the pill UI hid it); Inter +
JetBrains Mono bundled under SIL OFL; the duplicated brand row moved into an
About card with GitHub/Website/Releases/star links; a route globe with offline
IP geolocation; 320px window support. Admin handling was wrong twice and fixed:
`is_admin` used `net session` (fails when the Server service is stopped, a
false "not admin") and `relaunch_as_admin` always exited the process even when
already elevated. `scripts/dev-gui.ps1` (run the debug build, refuse duplicate
instances) and `scripts/install-gui.ps1` (install verified by file size, not by
`Copy-Item`'s misleading exit code) removed the install-friction loop.

The bug round (`22c9623`) fixed three reachable panics, each mutation-tested:
the WinDivert packet parser accepted an IHL below the RFC 791 minimum and
parsed ports out of the IP header on malformed network input; the globe's land
blob and the geo country table both indexed generated data unchecked.

**Open work, ranked, is in `wat/state/HANDOFF.md`:** rebuild + reinstall the
GUI to Program Files (installed copy predates the bug fixes), the owner-gated
package channels (Chocolatey API key, winget CLA), a GUI auto-update path to
end the install drift, optional mutex-poisoning hardening, and the owner's
product calls (#137 Minecraft, #59 Fortnite). CI is green for every pushed
commit; scheduled health/page workflows are green; fleet 8/8 on v1.6.14.

---

# Current Phase: WF-049 The monitor's red runs were its own throttled cron

**Workflow:** WF-049; WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** DevOps + QAEngineer
**Status:** Pushed (e07774b); winget PR #445619 open
**Last updated:** 2026-10-02

---

## 2026-10-02 - WF-049 five red monitor runs, none of them a fleet problem

**Driving evidence:** asked to check GitHub, the network, and the GUI client.
The network was healthy - 8/8 relays on v1.6.14, all `/health` 200, traffic
flowing (relay-fra 20.9M packets, up from the frozen 19.0M of WF-034). The GUI
was healthy - the installed 1.6.14 registered over QUIC to relay-lax-1 and its
53 tests passed. But **every scheduled Health Anomaly Monitor run has failed
since 2026-10-01** (13:18, 18:55, 23:13, 02:16, 08:35).

**Reproduced against live production history rather than guessed at.** Fetching
the stats branch's 90 snapshots and running the detector locally showed the
real cause: **GitHub throttles the collector's hourly cron to ~3.5-6h**
(measured pages.yml gaps of 215m, 219m, 368m), and two detectors read that
throttling as an outage.

1. `stale_history` had a 3h limit against a cron that runs every 3.5-6h, so it
   fired on nearly every run - reporting the known throttling as new.
2. `abuse_flood`'s floor was calibrated for HOUR-LONG windows. With windows
   stretched to 8-13h, ordinary scanner traffic summed past the fixed 1000
   floor.

**The floor's value was re-derived from live data, not tweaked until green.**
Over a 136h uptime the quiet relays logged relay-ewr-1 **47 abuse/h** and
relay-syd-1 **319/h** of background scanning; the one genuine flood, relay-fra,
ran at **9,350/h**. The old 1000/h floor sat just above scanner noise; 5000/h
sits in the gap - clear of noise, ~2x under a real flood. The floor is now also
scaled by the window's real duration, so it means "this much abuse per hour" at
any cadence.

**Then a third, more basic defect:** after the cadence fixes the run still
failed on `idle_relay` - "a relay that is UP but had a quiet window". Its own
message says "traffic distribution, not a failure", and live data confirms it
(relay-nrt 4.2M lifetime packets, relay-bom-1 1.6M - both merely quiet during
the window). `idle_relay` and `abuse_flood` are now NON_FATAL_TYPES: still
printed, still posted to Discord, no longer failing the build. Every other
detector still fails the run, including the warning-severity ones that mean
real degradation (`stale_history`, `saved_regression`, `negative_saving_spike`,
`version_lag`), so a stopped collector is still caught.

**Verified both directions, not just the green one:** real production history
with the workflow's exact flags -> **EXIT 0** where every run has failed since
2026-10-01; a history faked 9h stale -> **EXIT 1** on `stale_history`; a
synthetic flood at relay-fra's real 9,350/h rate -> still caught; `--json`
stdout still parses (the notice moved into the human-readable branch only).
**103 self-test checks pass.**

**One bug found by that verification rather than shipped:** the gate first
referenced `$NON_FATAL_TYPES` from inside the output block, before it was
defined, so `set -u` aborted it with "unbound variable" and printed a
contradictory notice. `fatal_count` is now computed once, before both uses.

**Also found and recorded, not fixed (owner-gated):** the package channels.
Scoop is current (bucket synced to 1.6.14); **Chocolatey still serves 1.6.3**
and winget has **never been bootstrapped**. The Chocolatey 1.6.14 package is
built and correct but cannot be pushed - no API key exists on this host or in
the repo's secrets. That is recorded in `dist/chocolatey/README.md` with the
exact push command so it is a one-step fix for the owner.

---

# Current Phase: WF-048 Last dependency PR landed; the fleet resumed traffic

**Workflow:** WF-049; WF-048, WF-047, WF-046, WF-045, WF-044, WF-043 below
**Agent:** DevOps + QAEngineer
**Status:** Pushed (e07774b); winget PR #445619 open
**Last updated:** 2026-10-02

---

## 2026-10-01 - WF-048 the dependency backlog is clear

**Driving evidence:** with #106, #108, #109, #138, #139 and #107 all resolved,
re-listing the open PRs left exactly one: **#136**, a Docker base-image bump
(rust 1.98.0 -> 1.98.1). One Dockerfile line, 15 passing checks, no failures.

**Verified beyond the PR's own checks:** the tag was confirmed to exist on
Docker Hub (`1.98.1-slim-bookworm`, amd64, active, 312 MB) rather than assuming
a version string is published - a missing base image fails the build at pull
time, and Dependabot's green tick describes an older base anyway.

**Landed as `80659fe`.** Docker workflow in flight.

**Also observed, and worth recording because it closes an earlier finding:**
the live fleet has RESUMED relaying. `relay-fra` now reports
`packets_relayed=19058803`, up from the 19005389 that had been frozen since
2026-10-01T01:46Z while uptime advanced - the exact stalled-counter condition
WF-034 (`fleet_idle`) and WF-028 (`stale_history`) were built to catch. So the
fleet traffic stop documented in WF-034 was real and has since cleared, and the
detectors that watch for it are in place for the next occurrence.

**Open items remaining are all owner-gated:** #137 (Minecraft support request)
and #59 (the Windows/Fortnite thread) are product calls, and the Chocolatey push
plus the winget CLA need credentials only the owner holds.

---

# Current Phase: WF-047 The GUI crate is now linted by CI

**Workflow:** WF-047; WF-046, WF-045, WF-044, WF-043, WF-042 below
**Agent:** DevOps + QAEngineer
**Status:** Pushed (6a4f8cf + f7f1607); CI in flight
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-047 removing the mechanism, not just the lints

**Driving evidence:** WF-042 fixed three lints in `lightspeed-gui` but left the
question of how they got there. Reading the job answered it:

```yaml
windows-gui:
  - uses: dtolnay/rust-toolchain@stable   # no clippy component
  - run: cargo build --release -p lightspeed-gui
  - run: cargo test -p lightspeed-gui      # builds + tests, never lints
```

And every other clippy invocation in `ci.yml` carries `--exclude
lightspeed-gui`. So the crate was **structurally unreachable by any lint** -
which is exactly how a manual `RangeInclusive::contains` and two
`map_or_identity` accumulated unnoticed from the day they were written.

**Change:** added the `clippy` component to that job's toolchain and one
`Clippy GUI` step. The exact command was run locally first
(`cargo clippy -p lightspeed-gui --all-targets` -> rc=0), so the step lands
green instead of surfacing a backlog in CI.

**Why this matters more than the lint fixes:** fixing three lints treats the
symptom. Without the step, the next lint in this crate regresses silently in
exactly the same way. This is the second time this session that a gap was
invisible not because it was hard to see but because the pipeline excluded
it - the first being the nine infra self-tests no workflow invoked (WF-027).
Both are now wired in.

**Also landed, verified the same way as the other dependency PRs:** #139
(`tokio-test` 0.4.5 -> 0.4.6), applied on current master and `cargo check
--workspace` -> Finished. Lockfile only.

**Verification:** YAML parses with 11 jobs and the correct step order; the new
step's exact command passes locally at rc=0; CI run in flight.

---

# Current Phase: WF-046 CI verified the migration; fmt check has a platform gotcha

**Workflow:** WF-046; WF-045, WF-044, WF-043, WF-042, WF-041, WF-040 below
**Agent:** DevOps + QAEngineer
**Status:** Verified green, lesson recorded
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-046 the Windows jobs passed, and my local fmt check lied

**Driving evidence:** pushing WF-044/WF-045 (the windivert beta migration and the
test-harness provider fix) produced CI run 36855915823 -> **failure**, but not
where expected:

```
Windows Build & Test:  success   <- the risky change, on a real Windows runner
Windows GUI Build:     success   <- ditto
Check & Test:          FAILURE   <- formatting only
```

The two jobs that exercise the actual surface I had flagged as unverifiable
locally - the beta WinDivert crates compiling and linking under MSVC - **passed**.
That is the first real evidence the migration works on the platform it targets,
and it is stronger than anything this workstation could produce.

**The failure was mine, and its mechanism matters:** `cargo fmt --all --check`
failed on `windivert_handle.rs:101`, where a long cast line needed wrapping.
I had run the same check locally and reported it clean (rc=0). It WAS clean
locally - because that file is `cfg(windows)`-gated, and rustfmt formats a
gated-out module differently on a Windows host than on the ubuntu runner.

**Lesson recorded for every future round:** a local `cargo fmt --all --check`
passing does NOT imply it passes on Linux when the changed code is
platform-gated. The platform-gated module is not the one rustfmt saw locally.
For any change to `cfg(windows)`/`cfg(unix)` code, CI's formatting job is the
first true check, not a formality.

**Fix:** applied rustfmt's own suggestion (`cargo fmt --all`), re-checked clean,
committed as `adef131` (style only, isolated from the substantive commits so it
is separately revertable).

**Post-fix CI run 36857581098:** `Check & Test => success`, and every other job
success (Benchmark Regression still running at time of writing).

---

# Current Phase: WF-045 Test binaries never pinned the rustls provider

**Workflow:** WF-045; WF-044, WF-043, WF-042, WF-041, WF-040 below
**Agent:** RustDev + QAEngineer
**Status:** Fixed and verified; landing with WF-044 per owner approval
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-045 `cargo test --workspace` failed 5 QUIC tests locally

**Driving evidence:** while attributing a failure seen on the WF-044 branch, the
three-way comparison settled both the cause and the blame:

| Invocation | Result |
|-----------|--------|
| `cargo test --workspace` (GUI in graph) | **5 tests FAILED** across 2 files |
| `cargo test --workspace --exclude lightspeed-gui` (**CI's command**) | all pass |
| Either, on pristine master | identical to the above |

So it was **pre-existing**, not caused by WF-044 - and CI never saw it, because
excluding the GUI is exactly what avoids the trigger.

**Root cause:** rustls panics at `rustls-0.23.45/src/crypto/mod.rs:249` with
"Could not automatically determine the process-level CryptoProvider from Rustls
crate features." The repo documents this condition itself at
`client-gui/Cargo.toml:26-29`: the GUI's tree enables BOTH providers (ring via
the client's QUIC stack, aws-lc-rs via reqwest/axoupdater), and its stated fix
is that `main.rs` pins ring. **A test binary has no `main`**, so nothing pins it
in a workspace run - and the panic names rustls configuration rather than a
test-harness gap, sending readers after the cryptography instead of the setup.

**Two of the four affected files already had private copies of the fix**
(`quic_telemetry_capability.rs`, `register_destination.rs` each define their own
`install_crypto_provider`), while `control_reconnect.rs` and
`phase0_reconnect.rs` had none - the same workaround discovered twice,
missing twice.

**Change:** one shared helper in `client/src/test_support` (the module that
exists to expose internals to integration tests) and a call at the point each
failing test builds its QUIC config. Idempotent, so concurrent test threads and
binaries can call it freely. The private copies in the two working files were
left alone - they pass, and rewriting passing tests is not part of this fix.

**Verification:** `cargo test --workspace` -> **every suite green, including the
5 previously-failing QUIC tests**. Also recorded: my first two attribution
attempts were invalid - running the test WITHOUT `--features quic` compiles the
file to nothing (`#![cfg(feature = "quic")]`) and reports a vacuous
`0 passed`, and a standalone run WITH the feature proved nothing about the
workspace case. Only the three-way comparison was evidence.

---

# Current Phase: WF-044 windivert beta migration unblocks windows 0.62 (PR #107)

**Workflow:** WF-044; WF-043, WF-042, WF-041, WF-040, WF-039 below
**Agent:** RustDev + QAEngineer
**Status:** Migration compiles on a branch; one pre-existing test failure being attributed
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-044 #107 was never a version bump

**Driving evidence:** PR #107 (windows 0.48 -> 0.62) fails CI on Windows GUI Build. The
owner authorised attempting the beta path, so the failure was traced to its root:

```
windivert 0.6.0       -> windows 0.48.0
windivert-sys 0.10.0  -> windows 0.48.0   (max STABLE)
client/Cargo.toml     -> windows 0.48      <- the only line #107 changes
```

Bumping one side of that contract gives the crate two incompatible `HANDLE`
types (`isize` vs `*mut c_void`), which is exactly the CI error. The repo ALREADY
documents the constraint at `client/Cargo.toml:95`: "The `windows` types must
match windivert-sys's own 0.48 dependency so `HANDLE` is the same type."

**Two further blockers found by attempting it:**
1. `windivert-sys` declares `links = "WinDivert"`, and cargo allows only ONE
   package per graph to own a `links` value - so 0.10 and 0.11 cannot coexist.
2. BOTH `client` and `client-gui` declare `windivert` directly, so #107's
   single-file change could never resolve regardless.

**Migration implemented on branch `chore/windivert-beta-windows-062`** (master
untouched): `windivert` 0.7.0-beta.4 + `windivert-sys` 0.11.0-beta.2 +
`windows` 0.62 in both manifests, plus six FFI sites in
`client/src/interceptor/windivert_handle.rs`:
- `HANDLE` -> `WinDivertHandle = *mut core::ffi::c_void` (0.11 dropped the
  `windows` dependency entirely, which is WHY the lockstep constraint disappears)
- `raw.is_invalid()` -> `raw.is_null()`
- `ok.as_bool()` -> a `succeeded(ok: c_int)` helper (0.11 models BOOL as c_int)
- `WinDivertOpen` returns `isize`; cast is a representation change, not a deref

**Verified:** `cargo check -p lightspeed-client --features windivert-redirect` ->
**Finished**; `cargo check --workspace` -> **Finished**; proxy suite 387 passed;
client lib suite 383 passed. `Cargo.lock` SHRINKS by ~87 lines because the
windows-0.48 chain drops out.

**One failure, attribution in progress - and my first two attempts at it were
INVALID, recorded so the mistake is not repeated:**
- Attempt 1 ran `cargo test -p lightspeed-client --test control_reconnect`
  WITHOUT `--features quic`. The file is `#![cfg(feature = "quic")]`, so it
  compiled to nothing and reported `0 passed; 0 failed` - a vacuous pass I
  nearly recorded as "pre-existing, not mine".
- Attempt 2 ran the same test standalone WITH the feature and it passed 3/3,
  which also proved nothing about the workspace case.
- The real reproduction is `cargo test --workspace`, where feature unification
  activates `quic` via lightspeed-gui/proxy and the 3 tests DO run - and fail.

The failure is a **rustls CryptoProvider panic** at
`rustls-0.23.45/src/crypto/mod.rs:249`: neither provider can be auto-detected.
The repo documents this exact condition at `client-gui/Cargo.toml:26-29`
("The GUI's tree enables both rustls providers - ring via the client's QUIC
stack, aws-lc-rs via reqwest/axoupdater - which makes rustls's automatic
provider selection panic"), and its stated fix is that `main.rs` pins `ring`.
That fix cannot help a TEST binary, which never runs `main`.

**Not yet concluded:** whether this reproduces on pristine master. A stashed
comparison is running. Either way it is unrelated to windivert - the panicking
code path is QUIC/TLS, which this migration does not touch.

---

# Current Phase: WF-043 thiserror patch bump verified on current master (PR #138)

**Workflow:** WF-043; WF-042, WF-041, WF-040, WF-039, WF-038 below
**Agent:** RustDev + DevOps
**Status:** Verified, awaiting test run before landing
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-043 three dependency PRs resolved by fixing them at the source

**Driving evidence:** the PR list changed under the session's own work. Checking
it directly rather than from memory:
- **#106** (`tray-icon` 0.25.1) -> **CLOSED**, superseded by `32306a9`.
- **#108** (`dirs` 7.0) -> **CLOSED**, superseded by `32306a9`.
- **#109** (`sha2` 0.11) -> closed earlier, superseded by `9416adc`.
- **#138** (new): `thiserror` 2.0.20 -> 2.0.21 patch group.

So three Dependabot PRs that had been open and red/stale since 2026-09-21 are
now resolved, each by landing the upgrade on `master` with its own
verification rather than merging a branch whose checks described an older tree.

**WF-043, the new one, verified rather than waved through:** #138 is a PATCH
bump whose checks are green (16 SUCCESS, 0 fail), which is exactly the case
where it is tempting to skip verification. But its checks ran against base
`c115738e` while master is now `d03beaf`, so the ticks are stale - the same
condition that justified refusing #106/#108 in WF-038. The standard was applied
consistently instead:
- `cargo update -p thiserror@2.0.20 --precise 2.0.21` on current master.
  Note the spec had to be disambiguated: the lockfile carries BOTH
  `thiserror@1.0.69` and `thiserror@2.0.20` (different transitive majors), and a
  bare `-p thiserror` errors as ambiguous. Bumping both lines would have been a
  different and riskier change than the one under review.
- `cargo check --workspace --exclude lightspeed-gui` -> **Finished**.
- The resulting lockfile diff matches PR #138 exactly: the 2.x line moves
  2.0.20 -> 2.0.21, the 1.0.69 line is untouched.

**Owner approved landing it.** Verification complete: `cargo test --workspace
--exclude lightspeed-gui` -> **every test binary passed, 0 failures** (including
the 129-test client suite and the 67-test proxy suite). Landed as `cb17f86`,
lockfile only; CI in flight.

**Correction issued the same round, worth recording because it nearly misled a
report:** a CI failure was initially attributed to the GUI lint fix commit
`3b13a8b`. It was not. Checking the run rather than trusting the monitor's
label showed run `36850180531` belongs to `cfb04cc` on branch
`dependabot/cargo/windows-0.62.2` - PR #107 - while the lint-fix run
(`36850174898`) on `3b13a8b` **passed all 13 jobs**. The misattribution is
recorded so the next reader does not chase the wrong commit.

**That wrong turn produced the real diagnosis for #107, now concrete:**
`windows` 0.62.2 changed `HANDLE` from `pub struct HANDLE(pub isize)` to
`pub struct HANDLE(pub *mut core::ffi::c_void)`, while `windivert-sys` 0.10.0
still declares `WinDivertClose(handle: HANDLE)` against the 0.48 shape. So #107
needs a `windivert-sys` upgrade AND FFI-boundary code migration - not a version
bump. That is the "multiple breaking releases on the FFI surface" the triage
guessed at, now stated as compiler output.

---

# Current Phase: WF-042 lightspeed-gui has never been linted by CI (maintenance)

**Workflow:** WF-042; WF-041, WF-040, WF-039, WF-038, WF-037, WF-036 below
**Agent:** QAEngineer + RustDev
**Status:** Lints fixed and pushed (3b13a8b); CI scope decision pending owner
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-042 the GUI crate is excluded from every CI check job

**Driving evidence:** a local clippy run surfaced three real lints in
`lightspeed-gui`. Checking whose they were produced the more important finding:
`lightspeed-gui` is excluded from **8 sites** in `ci.yml` - clippy (line 32),
build (35, 130, 183), test (38, 133), and coverage (157, 163). The crate has
never been linted, tested, or coverage-measured by the main pipeline, so these
lints accumulated unseen since they were written.

**Pre-existing, proven not assumed:** stashing the change and running the
identical clippy on pristine master reproduced all three errors - the same
procedure used for the proxy `never_loop` and the earlier GUI lint question.

**Fixed (3 lines, all semantics-preserving):**
| Site | Change |
|------|--------|
| `platform/windows.rs:336` | `port >= 28015 && port <= 30000` -> `(28015..=30000).contains(&port)` |
| `update.rs:32,33` | `.map_or(0, \|v\| v)` -> `.unwrap_or(0)` |

The port check was verified by hand rather than taken on the linter's word:
BOTH bounds are inclusive in the original, which is exactly what `..=` means,
so 28015 and 30000 stay accepted and 28014/30001 stay rejected. That code sits
in the Rust-client process-scanning path - the same family as WF-023's Windows
interception work - where a boundary slip would be costly.

**Verification:** `RUSTFLAGS=-Dwarnings cargo clippy -p lightspeed-gui
--all-targets` -> **clean, rc=0 - the first time this crate has been
lint-clean** - and `cargo test -p lightspeed-gui` -> **53 passed; 0 failed;
1 ignored**.

**Open decision, deliberately NOT taken unilaterally:** whether to bring the
crate into CI. Removing the exclusion outright is wrong - the Linux jobs
exclude it for real GTK build-weight reasons - so the sensible shape is to
scope clippy/build/test for this crate to the Windows job that already builds
it (`windows-gui`). That changes what CI runs for everyone, so it is the
owner's call rather than a maintenance edit.

---

# Current Phase: WF-041 Stale dependency PRs (#106/#108) are now locally verifiable

**Workflow:** WF-041; WF-040, WF-039, WF-038, WF-037, WF-036 below
**Agent:** RustDev + DevOps
**Status:** Verified locally, awaiting owner decision on landing
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-041 the GUI crate builds here, so the stale bumps can be tested

**Driving evidence:** the WF-038 triage could only call #106 (`tray-icon`
0.24.2 -> 0.25.1) and #108 (`dirs` 6.0.0 -> 7.0.0) "green but STALE" - their
checks ran against base commit `922dea4f`, and master has since moved well past
it. Stale green is not evidence, but neither was there any way to produce
current evidence, because both bumps live in `client-gui/` - the crate CI
deliberately EXCLUDES from its main check job
(`--exclude lightspeed-gui`).

**WF-039's toolchain removed that excuse:**
- `cargo check -p lightspeed-gui` -> **Finished** in 1m02s. The GUI crate,
  including `tray-icon`, `dirs`, the eframe/wgpu stack and Windows FFI, builds
  on this host. This is the last crate that had been written off as "needs
  MSVC".

**Then the bumps were tested directly, with current evidence:**
- Applied `tray-icon = "0.25.1"` and `dirs = "7.0"` to
  `client-gui/Cargo.toml` and ran `cargo check -p lightspeed-gui` ->
  **Finished** in 52s. Both compile.
- API-drift check on the two major/minor moves: `dirs::data_local_dir()` and
  `dirs::config_dir()` (the only two call sites, both in `paths.rs`) are
  unchanged across 6 -> 7; the `tray_icon::Icon::from_rgba` and import sites
  still resolve at 0.25.1.
**Verified, with current evidence:**
- `cargo check -p lightspeed-gui` with both bumps applied -> **Finished** in 52s.
- `cargo test -p lightspeed-gui` -> **53 passed; 0 failed; 1 ignored**. This
  includes `paths::tests::{crash_log_lives_under_the_data_dir,
  config_file_lives_under_the_config_dir, log_file_lives_under_the_data_dir}` -
  the tests that directly exercise the `dirs` upgrade, which is why the major
  bump is safe to trust rather than merely "it compiled".
- API drift checked at both call sites: `dirs::data_local_dir()` and
  `dirs::config_dir()` are unchanged across 6 -> 7; the `tray_icon::Icon::from_rgba`
  and import sites resolve at 0.25.1.

**Owner approved landing both**, so the change proceeds to the `[QUALITY_STUB]`
gate (fmt + clippy on the crate) before a push.

**Landed as `32306a9`.** `cargo fmt --all --check` was clean. The clippy run
surfaced 3 errors in `client-gui/src/update.rs` (a manual `RangeInclusive::contains`
and two `map_or_identity`), and they were PROVEN PRE-EXISTING by stashing both
bumps and reproducing the identical 3 errors on pristine master - the same
procedure used for the proxy `never_loop`. So they were not fixed here to make a
local gate green, and the verified version bumps were landed on their own merits.

**Separate finding, recorded rather than acted on: `lightspeed-gui` has never
been linted.** Every CI clippy invocation uses `--exclude lightspeed-gui`
(ci.yml lines 32 and 253), so these lints have existed unenforced and are only
now visible because a local toolchain finally exists for that crate. Fixing them
is a real, separable piece of work - deliberately not bundled into a dependency
bump.

---

# Current Phase: WF-040 sha2 migrated to 0.11 (PR #109 fixed)

**Workflow:** WF-040; WF-039, WF-038, WF-037, WF-036, WF-035 below
**Agent:** RustDev + DevOps
**Status:** Implemented, gate running, push pending
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-040 the dependency the triage twice declined is now migrating

**Driving evidence:** WF-038 triaged PR #109 (`sha2` 0.10.9 -> 0.11.0) as a
compile break with 8 failing CI jobs and declined to touch it, because this
workstation could not compile `proxy` at all. WF-039 removed that ceiling by
installing Rust and MinGW; this workflow cashes it in.

**Reproduce first.** Setting `sha2 = "0.11"` in `proxy/Cargo.toml` locally
produced exactly ONE error:

```
error[E0277]: the trait bound `Array<u8, UInt<...>>: LowerHex` is not satisfied
```

**Cause confirmed against the crate, not inferred:** sha2 0.11's changelog
states it "replace[d] type aliases with newtypes", so `finalize()` returns an
`Array` newtype that no longer implements `LowerHex`. The single call site was
`format!("{:x}", hasher.finalize())` in `proxy/src/handoff.rs`.

**Fix found empirically, not guessed:** a minimal probe crate tested four
candidates against the compiler (`as_slice` fold, `iter`, `as_ref`, `to_vec`).
Three compiled, `as_ref` did not. `iter` was chosen - no intermediate
allocation. The output was then checked against the published SHA-256 test
vector for "abc": the new code produces
`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`, matching
character for character.

**Verified by the project's OWN regression test.** `proxy/src/handoff.rs:979`
already asserts `sha256_file` against that same known digest; the full
`cargo test -p lightspeed-proxy` run passed with the change applied. This
matters more than usual here: the hash backs handoff binary verification, so a
build that compiles but computes a different digest would be worse than one
that fails to compile - it would reject valid releases or accept tampered ones.

**Change:** `proxy/Cargo.toml` `sha2 = "0.10"` -> `"0.11"`, and the one call
site reformatted as shown above; `Cargo.lock` regenerated by cargo.

**Not done:** the `windows` bump (#107) is NOT part of this: it spans several
breaking releases on the WinDivert/WFP surface and still needs MSVC, so it
stays open and CI-driven.

**One gate caveat, diagnosed rather than worked around:** `cargo fmt --all
--check` is rc=0, but `cargo clippy -p lightspeed-proxy --all-targets` fails on
THIS host with `clippy::never_loop` in `proxy/src/main.rs`, and the failure was
proven pre-existing by stashing every one of this workflow's changes and
rerunning against pristine master - identical error. The cause is the lint's
interaction with `#[cfg(unix)]`: the loop's only `break` in the unix branch is
compiled out on this `windows-gnu` target, while the `#[cfg(not(unix))]` branch
still breaks correctly. CI lints on ubuntu and is unaffected - `origin/master`
at `b5b4eae` contains that exact loop and its CI passed, running the very same
`cargo clippy` command. Reproducing CI's Linux lint locally was attempted and
is not possible here (cross-compiling needs `x86_64-linux-gnu-gcc`). The
unrelated code was deliberately NOT edited to appease a lint that CI never
sees.

**Pushed as `9416adc`** (owner authorised the fix). CI in flight.

**VERIFIED GREEN, same day:** CI run 36845378316 on `9416adc` -> **completed
success, all 13 jobs green** - including the 8 jobs PR #109 had been failing
(Check & Test, Coverage, E2E, macOS, Windows Build & Test, Benchmarks, Feature
Matrix ml, Proxy QUIC Smoke). The migration is confirmed on the real build
fleet, not just locally.

**PR #109 is now CLOSED as superseded.** `master` carries `sha2 0.11.0` in
`Cargo.lock` (line 4998), so the dependency was fixed at the source rather than
by merging the stale PR - Dependabot closed it after the bump landed. A red PR
that stays red because nobody can compile it, versus the same upgrade verified
and merged with its regression test passing: the difference was purely the
missing toolchain.

---

# Current Phase: WF-039 Local Rust toolchain installed (verification ceiling removed)

**Workflow:** WF-039; WF-038, WF-037, WF-036, WF-035, WF-034 below
**Agent:** DevOps
**Status:** Implemented and verified
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-039 the workstation can finally run the repo's own quality gate

**Driving evidence:** every workflow from WF-023 through WF-038 carried the same
standing caveat - "no Rust toolchain on this host, so `cargo fmt`/`clippy`/`test`
cannot run locally" - which meant Rust changes were CI-only and the `sha2` and
`windows` Dependabot migrations could not even be attempted (WF-038 declined
them for exactly this reason).

**Change:** installed the stable Rust toolchain via rustup (`--profile minimal`)
and then `rustup component add clippy rustfmt`, because `[QUALITY_STUB]` requires
them. Free, no services, no billing: `[COST_STUB]` holds.

```
cargo 1.98.1      (2026-08-05)
rustc 1.98.1      (2026-09-01)
clippy 0.1.98     (2026-09-01)
rustfmt 1.9.0     (2026-09-01)
```

**First results on the real codebase:**
- `cargo fmt --all --check` -> **clean, rc=0**. The workspace is rustfmt-clean as
  committed; no Rust file needed touching.
- `cargo check -p lightspeed-protocol` -> **clean**. The pure-Rust crate compiles.
- `cargo check --workspace` -> **rc=101**, and this round corrected an
  over-broad claim made about it. The failure is `ring v0.17.14`'s build
  script, then `getrandom`/`windows-sys` failing on `dlltool.exe: program not
  found`. The repo documents the requirement itself
  (`client/Cargo.toml:44`, `proxy/Cargo.toml:28`: "requires C compiler for
  ring"), and this host has no `gcc`, `cc`, or `clang`.

**Correction issued the same day:** the first version of this entry implied the
protocol crate was verifiable locally. `cargo check -p lightspeed-protocol`
passes, but `cargo check -p lightspeed-protocol --tests` **fails** - the test
build pulls `windows-sys`/`getrandom`, which need the same missing C toolchain.
So the accurate pre-MinGW boundary was: workspace-wide `cargo fmt`, and
`cargo check` on the pure-Rust crate WITHOUT dev-dependencies. Anything needing
a C compiler - `ring`, `getrandom`, `windows-sys`, every test binary, the
client and proxy crates - was not verifiable here.

**Follow-up:** with the owner's approval, a MinGW toolchain (WinLibs) was
installed to close the C-compiler gap, then the boundary was **re-measured
rather than assumed**:

| Surface | Before MinGW | After MinGW |
|---------|--------------|-------------|
| `cargo fmt --all --check` | clean | clean |
| `cargo check -p lightspeed-protocol` | pass | pass |
| `cargo check -p lightspeed-protocol --tests` | **fail** | **pass** (proptest, criterion compile) |
| `cargo check -p lightspeed-proxy` (ring/QUIC) | **fail** | **pass** |
| `client-gui`, WinDivert/WFP FFI | fail | fail (needs MSVC + hardware) |

This matters concretely: the `sha2` 0.10 -> 0.11 migration the WF-038 triage
declined (PR #109, a `proxy` dependency) is now work that can be attempted
WITH local compile feedback instead of blind edits.

**Honest scope of the new capability:** rustup selected the
`stable-x86_64-pc-windows-gnu` host tuple, while releases ship
`x86_64-pc-windows-msvc`. That difference is real, but the Windows-only
dependencies (`windivert`, `windows`) are `cfg(windows)`-gated rather than
target-triple-gated, so a local Windows build does compile that code path -
which makes this genuinely useful for the FFI-facing migrations. Where
gnu-vs-msvc could still diverge, CI remains the authority.

**What is still NOT locally verifiable:** Windows runtime behaviour - WinDivert
driver interaction, the single-instance guard, tray failure paths. WF-023's
"known verification gap" stands and is narrower than before, not closed.

**Not changed:** no source file, no dependency, no manifest. This entry corrects
a stale environment note that appeared in every prior workflow's status.

---

# Current Phase: WF-038 Dependabot PR triage (maintenance)

**Workflow:** WF-038; WF-037, WF-036, WF-035, WF-034, WF-033 below
**Agent:** DevOps + QAEngineer
**Status:** Assessed (no merges performed - see below)
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-038 six open Dependabot PRs, sorted by what they can actually break

**Driving evidence:** six chore(deps) PRs have been open since 2026-09-21..28.
Their CI results fall into three distinct groups, which a "are the checks
green?" glance would flatten into one:

| PR | Bump | Files | Result | Blast radius |
|----|------|-------|--------|--------------|
| #135 | thiserror 2.0.20 -> 2.0.21 | Cargo.lock | 15 SUCCESS, 0 fail | patch, lockfile only |
| #136 | rust 1.98.0 -> 1.98.1 (Docker) | infra/docker/Dockerfile | 15 SUCCESS, 0 fail | patch base image |
| #106 | tray-icon 0.24.2 -> 0.25.1 | Cargo.lock, client-gui | 14 SUCCESS, 0 fail | GUI tray; **stale** |
| #108 | dirs 6.0.0 -> 7.0.0 | Cargo.lock, client-gui | 14 SUCCESS, 0 fail | **major**; **stale** |
| #107 | windows 0.48.0 -> 0.62.2 | Cargo.lock, client/Cargo.toml | **3 FAILURE** | WinDivert/WFP FFI |
| #109 | sha2 0.10.9 -> 0.11.0 | Cargo.lock | **8 FAILURE** | crypto digest API |

**The two failing PRs are compile breaks, not flaky checks:**
- **#109** fails Check & Test, Code Coverage, E2E, macOS, Windows, Benchmarks and
  Feature Matrix (ml) - i.e. everything that compiles. The only real usage is
  `use sha2::{Digest, Sha256}` in `proxy/src/handoff.rs:550`; sha2 0.11 changed
  that trait's API, so the crate must be migrated before this can land.
- **#107** fails Check & Test, Windows GUI Build and Feature Matrix (ml).
  `windows` 0.48 -> 0.62 spans several breaking releases and this is the
  WinDivert/WFP FFI surface, which **cannot be compiled on this workstation**
  (no Rust toolchain), so any migration here must be driven by CI, not by me.

**A green tick is not proof for the older PRs.** #106 and #108 are based on
`922dea4f` while master is now `0f7296a0` - their checks ran against a tree that
predates this session's changes, so they would need a fresh run to be evidence
today. #135 and #136 are newer (2026-09-28) and touch only a lockfile and a
Docker base image, but even they predate master's current tip.

**Not done, deliberately:** no PR was merged. Merging depends on the user's
judgement and, for #107/#109, on code migration first; merging a breaking bump
blind would be the kind of change this mandate exists to prevent. The repo's
own rule (`[COST_STUB]`, and the quality gate in `wat/rules.md`) is satisfied by
CI either way, but a red PR that stays red is honest, whereas a merged one that
breaks `master` is not.

**Verification:** every claim above comes from `gh pr view` output (check
conclusions, changed files, base commits) rather than from the PR titles.

---

# Current Phase: WF-037 zero_relay cried wolf on normal traffic distribution (maintenance)

**Workflow:** WF-037; WF-036, WF-035, WF-034, WF-033, WF-032, WF-031 below
**Agent:** QAEngineer + SecOps
**Status:** Committed and pushed (d35ffd5), CI in flight
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-037 the outage detector fired on three healthy relays

**Driving evidence:** running the shipped detector against live history (now that
WF-035 restored publishing, the history had grown to 79 snapshots) produced:

```
[critical] zero_relay: relay-bom-1: reachable for 3 snapshots but relayed 0 packets while the fleet relayed 566212
[critical] zero_relay: relay-mad-1: ...
[critical] zero_relay: relay-syd-1: ...
```

Three criticals - and every one of them wrong. All eight relays report
essentially the same uptime (~399,000s, about 4.6 days), so nothing restarted;
`/health` was healthy on all three; and each has real history (bom-1 50
sessions, mad-1 106, syd-1 49). They were simply not routed to during the
window.

**Root cause:** `zero_relay` fired on silence alone. The genuine 1.6.3 outage
signature is silence PLUS clients being rejected - the relay is up, clients
try, and every packet is dropped. Silence with no rejections is normal
distribution across eight relays.

**Change:** the rule now branches on whether the relay rejected any clients in
the window:
- `zero_relay` (critical) - silent AND rejecting: the outage signature, and the
  message now names the rejection count that justifies it.
- `idle_relay` (warning) - silent with no rejections: labelled as traffic
  distribution, explicitly not a failure.

**Why this mattered:** an alert that fires on healthy behaviour is worse than
no alert, because it trains whoever is on call to ignore the detector that
does matter. The false criticals would have buried a real outage in noise.

**Verification:** the suite grew to **88 checks** (from 82). The zero_relay
fixture now models the real outage (silence plus climbing rejections), and a
new `idle_relay` case pins the warning path with its
"traffic distribution, not a failure" wording and asserts it does NOT claim the
outage signature. Against live history the three relays now report as warnings
alongside the genuine `abuse_flood`. All eight local suites pass.

**Not changed:** no source, packaging, workflow, or infrastructure file; no
alert thresholds for the conditions that were already correct.

---

# Current Phase: WF-036 The repo now owns its Scoop manifest (maintenance)

**Workflow:** WF-036; WF-035, WF-034, WF-033, WF-032, WF-031, WF-030 below
**Agent:** DevOps + QAEngineer
**Status:** Committed and pushed (a65cc8c), CI in flight
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-036 `scoop install lightspeed` served an eleven-release-old binary

**Driving evidence:** the onboarding audit flagged the Windows installers as
stale and it was still true after the pipeline was unfrozen: the Chocolatey
feed serves **1.6.3**, the Scoop bucket manifest is pinned to **1.6.3**, and the
latest release is **1.6.14**. Re-checking the cause with a working pipeline
confirmed there is no publish step for either channel - for Scoop, the bucket
held the ONLY copy of the manifest and nothing in this repository referenced it
(`grep -rl scoop dist/ infra/` returned nothing).

**Change:**
- `dist/scoop/lightspeed.json` is now the in-repo source of truth, mirroring how
  `dist/chocolatey` works.
- `infra/scripts/bump-scoop.sh` rewrites version, download URL and sha256 from
  the released Windows asset. It refuses a manifest that is not valid JSON or
  lacks `version` / `architecture.64bit`, rather than writing a half-updated
  document.
- `infra/scripts/test_bump_scoop.sh` (16 checks) covers the happy path, the
  abort-before-write case when the asset is missing, both malformed-manifest
  cases, and that `checkver`/`autoupdate` survive a bump.
- A `bump-scoop` release job commits the bump on every tag, mirroring
  `bump-chocolatey`; the suite is wired into `infra-script-tests` in `ci.yml`.
- `README.md`'s Scoop row now states what is automated instead of implying the
  bucket is current.

**Still manual, and documented rather than glossed:** publishing the copy to
`ShibbityShwab/scoop-bucket` needs a token with write access to that second
repository. `dist/scoop/README.md` gives the `gh api` sync command and the
`SCOOP_TOKEN` automation path.

**Verification:** both workflows parse (ci: 11 jobs / 11 infra steps; release:
9 jobs including `bump-scoop`, with `bump-chocolatey` and `announce` intact).
Eight local suites pass (16 new checks). The push triggered **CI**, **Security
Audit** and **Deploy GitHub Pages** - the Pages run is independent proof of the
WF-035 trigger fix, since this push touched `infra/scripts/**` and nothing under
`web/`.

**Not changed:** no collector logic, no catalog data, no infrastructure, and no
publishing to any external feed.

---

# Current Phase: WF-035 Pages pipeline had no push trigger for its own inputs (maintenance)

**Workflow:** WF-035; WF-034, WF-033, WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** DevOps + InfraDev
**Status:** Committed and pushed (24bce57), Pages run in flight
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-035 the publishing pipeline could not be restarted by a push

**Driving evidence:** after the WF-026..WF-034 release was pushed (`a15f6b3`),
CI and Security Audit ran but **Deploy GitHub Pages did not** - so the public
registry stayed frozen at its 2026-09-20 publication and the stats feed at
01:46Z, even though the push had just proven the freeze was not a repo defect.

**Root cause:** `pages.yml` watched `paths: ['web/**']` on push, while the
pipeline it defines actually depends on `infra/scripts/network-stats.sh`,
`infra/scripts/collect-metrics.sh`, `infra/scripts/recommend-regions.sh` and
`infra/geo/*`. Two consequences, one of which had already bitten:
1. Changing a collector could never refresh the site that collector feeds.
2. The ONLY remaining trigger was the hourly cron, which GitHub throttles on
   inactive repositories - so when the cron went quiet the pipeline had no
   push-based fallback, and the registry sat stale for eleven days while every
   scheduled run reported `success`.

**Change:** the push trigger now covers `web/**`, `infra/scripts/**`,
`infra/geo/**` and `.github/workflows/pages.yml` itself, so a change to any
input - or to the workflow - restarts publishing. YAML verified by parsing:
four path filters, schedule and workflow_dispatch intact, 7 jobs unchanged.

**Verification:** pushing the change (which matches its own path list) triggered
`Deploy GitHub Pages` from a **push** event at 08:10:48Z - the first push-driven
Pages run on record; every previous run was `schedule`. Completion and the
registry refresh are tracked by a live monitor rather than assumed.

**Not changed:** no collector logic, no catalog data, no infrastructure.

---

# Current Phase: WF-034 Fleet-wide traffic stop had no detector (maintenance)

**Workflow:** WF-034; WF-033, WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** SecOps + QAEngineer
**Status:** Committed and pushed (a15f6b3) - first runner verification in progress
**Last updated:** 2026-10-01

---

## 2026-10-01 - Release: nine workflows' worth of maintenance landed on master

**Owner approval:** the maintainer was asked how to proceed and explicitly
authorised "commit and push to master", which is the action this entry records.

**What was pushed:** everything from WF-026 through WF-034 in one commit,
`a15f6b3` (13 files, +1631/-21): the three new detectors (`abuse_flood`,
`stale_history`, `fleet_idle`), the CI `infra-script-tests` job plus three new
suites (`test_bump_packages`, `test_lib_nodes`, `test_geo_catalogs`), the
Chocolatey packer portability fix, the README install-table correction, and the
WAT state/decision records.

**Why this mattered beyond the diff:** the repository had not been pushed since
01:46Z, and CI triggers only on push/PR to `master`. Every verification in
WF-026..WF-034 was therefore local-only. The push started **CI**, **Security
Audit**, and **Deploy GitHub Pages** at 07:53:45Z - the Pages run being what
restores the public site, the signed registry (frozen since 2026-09-20), and
the stats feed that `stale_history` exists to watch.

**Open verification:** the CI run (36833003765) exercises the new
`infra-script-tests` job for the first time on a real runner. Local results
predict green (7 suites pass; `collect_metrics` and `analyze_mesh` fail only
under this host's MinGW curl, which returns empty for `file://`). That
prediction is not yet confirmed, and this entry is written before the run
finishes rather than after.

---

## 2026-10-01 - WF-034 every relay can go silent and the monitor reports clean

**Driving evidence:** re-reading the live fleet at 07:41Z showed the cumulative
counters BYTE-IDENTICAL to the 07:13Z reading - `relay-fra` still
`packets_relayed=19005389`, `drops_abuse_blocked=1263983` - while
`uptime_secs` advanced ~1,700s. All eight relays answer `/health` healthily and
report version 1.6.14, but nothing is moving: the fleet stopped carrying
traffic roughly when the Pages cron stopped firing at 01:46Z.

**The gap:** `zero_relay` (the detector written for the 1.6.3 outage) requires
`$fleet_pkts > 0` - it fires when ONE relay is silent while the fleet is busy.
That guard exists so a quiet relay is not mistaken for a broken one, and it is
correct for its case - but it means a fleet-wide stop matches NOTHING. Proven by
fixture before changing code: an all-idle history produced
`health-anomaly: no anomalies (2 relay(s), 8 snapshot(s), window=3)`. Together
with WF-028 (frozen collector) this left the monitor unable to see either half
of a dead pipeline: it noticed neither a stopped collector nor a stopped fleet.

**Change:** a new `fleet_idle` detector (critical) fires when every reachable
relay relayed zero packets AND created zero sessions across the sustained
window, with a guard so a busy fleet stays silent. It is deliberately the
complement of `zero_relay`, and the two are now documented against each other
so a future reader does not collapse them again.

**Three of my own defects were caught by running the tests, not by reading
code:**
1. The first version omitted the `as $id` binding inside the `$ids[]`
   comprehension, so jq failed to COMPILE - and the script's `2>/dev/null`
   turned that into the empty-result fallback, reported as `0 relay(s)`.
   Diagnosed by unsuppressing stderr, then by isolating the fragment.
2. The suite's existing guard asserted "an all-idle fleet raises nothing,"
   which is exactly the behaviour this workflow reverses. It was rewritten -
   although the first rewrite was wrong too, asserting the single-idle-relay
   case was non-anomalous when `zero_relay` fires there by design.
3. The corrected assertion then matched the phrase "zero_relay" appearing in
   the fleet_idle MESSAGE text, a false positive in the test rather than the
detector; it now matches the emitted type (`zero_relay:`).

**Verification:** `test_health_anomaly.sh` -> all assertions passed
(**82 checks**, up from 77), including a new all-idle case, its critical
severity, the evidence string, and a busy-fleet guard. All seven infra suites
still pass. Against the live history the detector reports exactly the two
real conditions (`stale_history` at 21,764s and `abuse_flood` on relay-fra).

**Not changed:** no source, packaging, workflow, or infrastructure file.

---

# Current Phase: WF-033 Placement geo catalogs get integrity coverage (maintenance)

**Workflow:** WF-033; WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** QAEngineer + InfraDev
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-033 the recommender's reference data had no validation

**Driving evidence:** widening the audit past `infra/scripts` surfaced the
placement geography catalogs, `infra/geo/regions.json` and
`infra/geo/candidates.json`. Their own README states the contract: all
coordinates are WGS 84 decimal degrees, region keys are stable lowercase
slugs, country keys are ISO 3166-1 alpha-2, and `regions.<key>.label` is the
display name. Nothing validated any of it. Unlike a script, a broken catalog
cannot crash a run - it silently degrades the recommender: a country pointing
at a removed region becomes an unmapped cell, a candidate whose region no
longer exists drops out of ranking, and an out-of-range latitude skews every
distance computed from it.

**Change:** new `infra/scripts/test_geo_catalogs.sh` (17 checks) pins the
declared contract: both catalogs parse with `schema_version` 1; region keys are
lowercase slugs and country keys are ISO alpha-2; every country, region alias,
and candidate region resolves to a declared region; all coordinates are
numeric and within +-90 latitude / +-180 longitude; every region has a label;
candidate ids are unique and all recommender params numeric. Wired into
`infra-script-tests` in `ci.yml` as a ninth step.

**The suite includes negative controls, so it can be shown to fail:** a
fixture with a country mapped to a nonexistent region must be DETECTED, and a
fixture with latitude 999 must be DETECTED. A test that only proves today's
data is clean would pass forever even if its own predicates were broken.

**Current data is clean:** 8 regions, 143 countries, 8 relays, 37 aliases,
no dangling references, all coordinates in range - so this round adds
protection, not a fix.

**Three of my own defects were caught while building it**, recorded because
the first is why the third was hard to see: a malformed `region_aliases` jq
expression, an edit that collapsed a newline and concatenated two statements
(caught by `set -u` as an unbound variable), and - most importantly - an
`offenders` helper that swallowed jq errors by echoing a placeholder into the
comparison, so a broken predicate looked like a passing check. The helper now
reports a jq evaluation failure as an explicit FAIL.

**Not changed:** no catalog value, source, or infrastructure file was touched.

---

# Current Phase: WF-032 Chocolatey packer portability and failure clarity (maintenance)

**Workflow:** WF-032; WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** DevOps + RustDev
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-032 the Chocolatey packer broke on macOS and failed cryptically

**Driving evidence:** widening the audit beyond `infra/scripts` surfaced
`dist/chocolatey/build.sh` - the script that produces the `.nupkg` pushed to
the Chocolatey feed. Two defects, both verified:

1. **GNU-only regex.** Version extraction used `grep -oPm1 '(?<=<version>)[^<]+'`,
   and `-P` is a GNU extension: BSD/macOS `grep` rejects it with "invalid
   option". The README tells macOS users to build from source, so the one
   script a packaging contributor needs was broken on their platform. This is
   the only `grep -oP`/`grep -P` use in the entire repository.
2. **A guard that a stub defeats.** The script calls `python3` with no
   presence check, so on a host where the name resolves but execution fails
   (the Windows Microsoft Store alias stub is exactly this) the user sees the
   Store's misleading "Python was not found" prompt rather than a build error.

**Change:** version extraction now uses a portable `sed` expression with an
explicit empty-value check, and the script refuses to continue unless
`python3 -c 'import sys, zipfile'` actually succeeds - testing that the
interpreter RUNS, not merely that a name resolves on PATH.

**Verification:**
- syntax: `bash -n` clean.
- equivalence: the GNU form and the POSIX form both yield `1.6.14` against the
  live `lightspeed.nuspec`, so the replacement is not a behaviour change.
- failure path: on this host the guard now prints
  `build.sh: python3 (with the zipfile module) is required to pack the .nupkg`
  and exits 1, where before it emitted the Store prompt; no `.nupkg` is left
  behind on failure, so a half-built package cannot be pushed.
- `command -v` alone was tried first and found INSUFFICIENT (the Store alias
  satisfies it), which is why the check executes python3 instead.

**Not changed:** the packer's OPC member contract (`lightspeed.nuspec`,
`[Content_Types].xml`, `_rels/.rels`, `tools/chocolateyInstall.ps1`) is
unchanged and was confirmed against the script source and the README. No
workflow runs this script - the release job only bumps `dist/chocolatey` and
commits, so packing remains a manual maintainer step; that is unchanged, and
noted here so the absence of CI coverage is recorded rather than assumed.

---

# Current Phase: WF-031 Documentation truthfulness pass (maintenance)

**Workflow:** WF-031; WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** TechWriter + QAEngineer
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-031 the README promised install commands that cannot work

**Driving evidence:** every package-manager claim in the README was checked
against the live channel, and two were false in a way that wastes a user's
time rather than merely being out of date:

| Channel | README said | Verified reality |
|---|---|---|
| winget | "submitted, awaiting Microsoft review" | **not published at all** - `microsoft/winget-pkgs` returns 404 for the manifest path and the bootstrap PR was closed pending an unsigned CLA |
| Chocolatey | "submitted, pending moderation" | live, but serving **1.6.3** only |
| Scoop | (bare command) | bucket live, pinned to **1.6.3** |
| Homebrew | (bare command) | current - `Formula/lightspeed.rb` at the latest release |

So `winget install ShibbityShwab.LightSpeed` fails outright, and the
Chocolatey/Scoop commands install a binary many releases old, while the page
narrates them as "awaiting review".

**Change:** the README install table now states each channel's real status,
notes the stale pin inline, marks winget as unpublished with a pointer to the
release MSI, and adds a short Windows advisory recommending Releases over a
package manager. Two docs carried stale version framing
(`docs/architecture.md` and `docs/protocol.md` both read "Last updated:
2026-09-22 - Reflects v1.6.5" against a 1.6.14 workspace); their substantive
claims were verified before restamping.

**Verification of the doc claims themselves, not just the wording:**
- game count: `docs/supported-games.md` numbered rows are exactly **19**, and
  `client/src/games/` holds 20 files minus `mod.rs` - an earlier read of "31"
  counted every `|` line including headers and was wrong.
- relay count: the live registry still resolves to **8** nodes.
- protocol: UDP **4433** control / **4434** data are asserted in both
  `proxy/src/config.rs` and `client/src/config.rs` tests; `session_token` is
  present in `protocol/src/header.rs`; TCP framing exists in
  `protocol/src/framing.rs`.

**Not changed:** no source, packaging, workflow, or test file was touched. No
test pins this prose, which is correct - doc wording is not a machine-consumed
value.

---

# Current Phase: WF-030 Shared node resolver gets regression coverage (maintenance)

**Workflow:** WF-030; WF-029, WF-028, WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-030 lib-nodes.sh is shared by every ops script and had no test

**Driving evidence:** continuing the WF-029 audit of untested infra scripts.
`lib-nodes.sh` is not a leaf utility: `deploy.sh` and `mesh-health.sh` both
source it to decide WHICH host an ops command targets, and it was untested.
When it resolves the wrong node, a deploy or a health probe silently runs
against the wrong machine - and its most dangerous failure is returning an
empty list, which a caller would iterate zero times and report success.

**Change:** new `infra/scripts/test_lib_nodes.sh` (35 checks) pins the
contracts the module documents: an `LIGHTSPEED_NODES` array override wins over
the registry; a region-keyed object override normalizes to an array; a bare
string value is treated as the ip; defaults apply (`:4434` data,
`:8080` health/metrics, control `4433`); an explicit field is never overwritten
by a default; a custom `LIGHTSPEED_CONTROL_PORT` applies only where a node
omits one, and a non-numeric value falls back; an empty override (`{}`/`[]`)
falls through to the registry; the registry path is independent of the CWD; and
every failure path (invalid override JSON, missing registry, registry without a
payload, zero nodes) fails loudly. Wired into `infra-script-tests` in `ci.yml`
as an eighth step.

**Three harness bugs of mine were caught by this test, not by review** - worth
recording because each produced a confident wrong answer:
1. The registry fixture wrapped a bare node array; the real document is
   `{"registry": "<json string containing {schema_version, nodes}>"}`. The
   library correctly rejected it, and my first run showed 16 red assertions.
2. `run_nodes` passed `LIGHTSPEED_CONTROL_PORT=6000` as a command rather than
   an environment assignment, so bash reported "command not found".
3. The explicit-field case used a node-shaped OBJECT override. The object form
   is region-keyed (`{"tokyo":"1.2.3.4"}`), so the library exploded the node's
   keys into four junk nodes. The array form is the correct one for explicit
   fields.

**Verification:** `test_lib_nodes.sh` -> all assertions passed (35 checks).
Suites from earlier rounds still green: `test_bump_packages.sh` 21,
`test_health_anomaly.sh` 77. `ci.yml` parses with 11 jobs and 8 infra test
steps. Against the REAL `web/registry.json`, `lightspeed_resolve_nodes` returns
8 nodes - matching the eight live relays - which is independent evidence that
the fixture shape mirrors production.

**Not changed:** no source, packaging, or infrastructure file was touched.

---

# Current Phase: WF-029 Release package-bump scripts get regression coverage (maintenance)

**Workflow:** WF-029; WF-028, WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-029 the two scripts that rewrite shipped installers had no tests

**Driving evidence:** a normalization-corrected audit of `infra/scripts/*.sh`
against `test_*.sh` found 13 scripts with no coverage. Two of them are the
highest-consequence of the set: `bump-chocolatey.sh` and `bump-homebrew.sh`
run automatically in `release.yml` on every tag and rewrite the version plus
EVERY sha256 in the manifests users actually install from. A silent `sed` miss
there publishes a package whose checksum does not match its artifact, and the
failure surfaces as a hash mismatch at install time - long after the release.

**Change:** new `infra/scripts/test_bump_packages.sh` (21 checks) exercises
both scripts with the `gh` binary stubbed on PATH (the stub honours `--jq`, so
the scripts run exactly as in CI, with no network or credentials) against
copies of the real manifests, so the originals are never mutated. It asserts
the happy path (version, download URL, and checksum all move together for a
specific asset) and the failure paths, which are the load-bearing part: a
release missing the Windows zip, a release with the wrong tarball count, a
release missing one of four tarballs, and missing manifest files must each
abort and leave the manifests byte-identical. Wired into the existing
`infra-script-tests` job in `ci.yml` as a seventh step.

**Two false alarms this round, both from my own harness rather than the
scripts** - recorded because acting on either would have meant "fixing"
correct code: a first stub ignored `gh`'s `--jq` flag and returned raw JSON,
which made `bump-homebrew` report "expected 4 client tarballs, found 5"; and a
manually reconstructed jq filter produced empty output from quote-mangling.
Only after the stub faithfully honoured `--jq` did both scripts prove correct:
version, checksum, url, and all four platform digests are rewritten exactly,
and a missing asset aborts BEFORE any write, leaving files untouched.

**Verification:** `bash infra/scripts/test_bump_packages.sh` -> all assertions
passed (21 checks). `git diff --stat -- dist/chocolatey Formula` is empty after
the run, confirming the suite is non-destructive against the real manifests.
The workflow still parses with 11 jobs and the new step present.

**Not changed:** no source, packaging manifest, or infrastructure file was
touched; this round adds test coverage only.

---

# Current Phase: WF-028 Stale-telemetry detection (maintenance)

**Workflow:** WF-028; WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-028 the monitor could not tell "healthy" from "not receiving data"

**Driving evidence:** while surveying live state, the published telemetry was
found frozen. `web/network-history.json` (stats branch) had no commit newer
than `2026-10-01T01:46:16Z`, and the repo's `pushed_at` is
`2026-10-01T01:46:17Z` - the hourly Pages cron stopped the moment repository
activity stopped. By 07:13Z five consecutive hourly slots had been missed while
every other scheduled workflow kept running (Health Monitor fired at 07:02Z),
so the scheduler itself was healthy and the stall was specific to Pages. At
`05:57Z` the anomaly job had fetched that same frozen file, logged
`history: 76 snapshot(s)`, and reported **`no anomalies`** - it was monitoring a
five-hour-old past and calling it healthy. This is the 1.6.3 lesson repeating:
a check that cannot distinguish "fine" from "not observing" is not a check.

**Change:** `infra/scripts/health-anomaly.sh` gains a `stale_history` detector
(warning) with a configurable `--max-staleness` window (default 10800s / 3h).
A pinned clock seam (`LIGHTSPEED_NOW_EPOCH`) keeps assertions deterministic
instead of racing the wall clock. `test_health_anomaly.sh` pins that clock for
the whole suite and adds four cases: just inside the limit (silent), just past
it (fires), the evidence string, and `--max-staleness 0` disabling the check.
`.github/workflows/health-anomaly.yml` now passes `--max-staleness 10800`
explicitly and its stale comment ("written by pages.yml every 6h", actually
hourly) is corrected.

**Design note:** an ABSENT history is deliberately not stale - it has no
snapshots to age - so a first run or a freshly created stats branch cannot
produce a false alert. Verified directly: an empty history exits 0 with no
anomalies.

**Verification:** `test_health_anomaly.sh` -> all assertions passed
(**77 checks**, up from 69). Boundary behaviour proven by pinning the clock:
`+10799s` stays silent, `+10801s` fires, pinned-fresh is clean. Against the
real frozen feed the detector reports `newest snapshot is 20038s old (limit
10800s)` and the run exits 1, so the hourly job now fails (and alerts) instead
of passing green on dead data.

**Two of this round's own mistakes, caught by verification:** the first rule
tested `$newest <= 0` instead of comparing age, so it fired unconditionally
(an always-on alert); and the first test pin assumed fixture timestamps near
1700000000 when the fixtures deliberately use small counters (1000-1700),
which broke 13 checks. Both were found by running the suite, not by reading it.

**Not changed:** no source, packaging, or infrastructure file was touched.

---

# Current Phase: WF-027 Orphaned infra self-tests wired into CI (maintenance)

**Workflow:** WF-027 (CI coverage); WF-026 and WF-023 remain below
**Agent:** DevOps + QAEngineer
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-027 nine infra self-tests were never executed by CI

**Driving evidence:** a coverage audit of `.github/workflows/ci.yml`
against `infra/scripts/test_*.sh` found that of twelve self-test
scripts, only three were reachable from any workflow
(`test_proxy_quic_smoke.sh` and `test_deploy_canary.sh` in `ci.yml`,
`test_health_anomaly.sh` in `health-anomaly.yml`). Nine were referenced
by nothing but themselves: every change to `collect-metrics.sh`,
`analyze-mesh.sh`, `recommend-regions.sh`, `egress-budget.sh`,
`inventory.sh`, or `append-history.sh` shipped unverified.

**Change:** `ci.yml` gains an `infra-script-tests` job (`ubuntu-latest`)
running six of the orphaned suites: `test_collect_metrics.sh`,
`test_recommend_regions.sh`, `test_egress_budget.sh`,
`test_inventory.sh`, `test_append_history.sh` (with a 5-minute bound),
and `test_analyze_mesh.sh`. All read local `file://` fixtures via
bash + curl + jq, so the job needs no services, secrets, or network.

**Three suites deliberately left out of CI, with the reason recorded in
the workflow comment so a future agent does not "fix" the omission:**
`test_handoff_e2e.sh` (requires Linux + root to drive a real relay
in-place exec), `test_relay_updater.sh` (requires `flock`), and
`test_relay_install.sh` (asserts Unix ownership/mode outcomes such as an
0711 runtime dir). Wiring these into a container job would produce false
reds, not signal; they are exercised on the relay hosts themselves.

**Verification:** each suite was run locally before wiring. Passing here:
`egress_budget` (14 checks), `inventory` (28), `recommend_regions` (146).
`collect_metrics` (91 failures) and `analyze_mesh` (2 failures) fail on
this workstation for one proven environmental reason: its MinGW
`curl 8.21.0 (x86_64-w64-mingw32)` returns **empty output with exit 0**
for `file://` URLs, so every fixture read parses an empty document. That
was confirmed directly (`curl -s file://.../curlfix.json` printed nothing
while reporting RC=0), which is why both suites are expected green on the
Linux runner. `append_history` exceeds 60s here because each invocation
costs ~220ms of process-spawn overhead on MSYS (~18ms per fork, uniform
across `jq`, `mktemp`, and even `bash -c true`), so its 400-append cap
test projects to ~88s; that cost is host-specific (the script has no
production caller) and is why the CI step carries a 5-minute bound.
YAML validity was confirmed by parsing the workflow: the job is present
on `ubuntu-latest` with six steps and the other eleven jobs are intact.

**Not changed:** no source, packaging, or infrastructure file was
touched other than `ci.yml`.

---

# Current Phase: WF-026 Fleet abuse-flood detection (maintenance)

**Workflow:** WF-026 (detector hardening); WF-023 remains pending release below
**Agent:** SecOps + QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-026 abuse-flood detector added to the anomaly monitor

**Driving evidence:** the 2026-10-01 review found `relay-fra` absorbing a
sustained abuse-blocked flood (+1,153,206 blocks over a 3-snapshot window,
99.9% of everything it dropped, ~967/minute) while `health-anomaly.sh`
reported `anomalies: []` and the hourly job passed green. The existing
detectors cover `zero_relay`, `auth_spike_no_sessions`, `version_lag`,
`saved_regression`, `negative_saving_spike`, and reachability; none observe
abuse-blocked volume.

**Change:** `infra/scripts/health-anomaly.sh` gains an `abuse_flood`
detector (severity `warning`) with two configurable floors:
`--abuse-min` (default 1000 window abuse blocks) and `--abuse-share`
(default 0.9 of the relay's drops). `infra/scripts/test_health_anomaly.sh`
gains a flood fixture plus three guards (tiny absolute count, mixed
traffic, single-snapshot window) and a JSON contract assertion.

**Two design corrections found by testing, not assumption:**
1. The share must be measured against **drops**, not `relayed + dropped`.
   relay-fra's real flood was 99.9% of its drops but only 46.8% of its
total handled volume, because a busy relay's legitimate traffic masks
the flood in that denominator.
2. The rule must **not** require flat sessions. Gating on
   `sessions_created <= 0` suppressed the very event it was written for:
the real window carried +1,153,206 abuse blocks *and* +37 real sessions.

**Verification:** `bash infra/scripts/test_health_anomaly.sh` -> all
assertions passed (69 checks, up from 53). Run against the live fleet
history and registry with the CI invocation shape, the detector reports:
`[warning] abuse_flood: relay-fra: abuse_blocked +1153206 = 99.9% of drops
(1154074 dropped, 1311829 relayed, sessions +37) over 3 snapshots` and
exits 1, so the hourly job would now fail (and post to Discord) instead
of passing silently.

**Threshold justification:** across the 76-snapshot history, 17
relay-windows hit `abuse >= 1000` *and* `abuse/drops >= 0.9`, spanning
both `relay-fra` and `relay-mad-1` over a week. The rule therefore
describes a recurring fleet condition, not an artifact of one snapshot.

**Not changed:** no source, packaging, or infrastructure file was
touched; publishing to Chocolatey/winget/Scoop remains a maintainer
step (see the 2026-10-01 decision entry).

---

# Current Phase: WF-023 Windows GUI Startup + WinDivert Teardown (community feedback)

**Workflow:** WF-023 (plus WF-025 observability, below)
**Agent:** RustDev + QAEngineer + DevOps
**Status:** **RELEASED** - shipped in v1.6.10 (2026-09-25); both fixes verified
present in the v1.6.14 tag. Issue #59 is still OPEN and the reporter has not been
asked to re-test, which is the only outstanding part of this workflow.
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-023 status was stale: the fix shipped six days ago

**Driving evidence:** the header above read "Implemented, pending release" and had
done since handover, but checking the tags directly showed otherwise. Commit
`6bf0f67` ("fix(windows): resolve GUI startup + WinDivert teardown feedback (#59)")
is an ancestor of **v1.6.10** (2026-09-25), so it is in every release since,
including v1.6.14. Both halves verified present in the v1.6.14 tag rather than
inferred from the commit message:
- `client/src/interceptor/windivert_handle.rs` (the owned handle whose `Drop`
  shuts down and closes) - present in the tag.
- `client-gui/src/single_instance.rs` (the `SetLastError(0)` reset before
  `CreateMutexW`) - present in the tag.

**Why this mattered:** a state file that says "pending release" for something
shipped six days earlier invites the next agent to re-investigate, re-verify, or
believe a fixed bug is still outstanding.

**Still genuinely open:** issue #59 was never followed up. The reporter's last
substantive reply is from 2026-08-28 and the thread's last activity is
2026-09-19, both predating the fix's release. Asking them to retest on 1.6.14+ is
a public comment on their thread, so it is left to the owner rather than posted
unilaterally.

---

## 2026-10-01 - Agent handover and external status review (omo-native)

**Change:** Agent operation handed over from the previous opencode-based agent to
omo-native. The repository held no opencode-specific artifacts, so this is a
tooling change only: `AGENTS.md`, `wat/rules.md`, and `wat/archive/agents.md`
remain the operating contract. See `wat/state/decisions.md` (2026-10-01) for the
full rationale.

**Local prerequisite gap:** this workstation has no Rust toolchain on PATH
(`cargo`, `rustc`, `rustup` all absent), so `[QUALITY_STUB]` commands
(`cargo fmt`, `cargo clippy`, `cargo test`) cannot run here and must be
exercised on a toolchain-equipped host or in CI.

### Findings from the 2026-10-01 review

| Track | Finding |
|-------|---------|
| GitHub feedback | 2 open issues: #137 (Minecraft support request, 2026-09-28, unactioned) and #59 (the Windows/Fortnite report driving this workflow, 17 comments). 6 open Dependabot PRs (#106-#109, #135, #136). Latest release 1.6.14 (2026-09-26); repo at 70 stars / 9 forks. |
| App markets | Chocolatey page is live but serves only **1.6.3** (11 releases behind). Scoop bucket is live but pinned to **1.6.3**. winget is **not published**: bootstrap PR microsoft/winget-pkgs#435790 open since 2026-09-16, and #437292 was closed 2026-09-19 pending the Microsoft CLA. Homebrew is current at **1.6.14** (the tap is this repository, not a separate tap repo). AUR has no `lightspeed-bin`; the taken `lightspeed` name is an unrelated app, and Arch registration is closed. |
| Network | All 8 relays healthy on 1.6.14, ~109h uptime. Sustained abuse-blocked flood on `relay-fra`: 173,080 of the newest window's 173,497 drops (~967/min), fleet drop rate up to 65.06%. No cost or availability breach (`drops_egress_budget` = 0 fleet-wide). `health-anomaly.sh` passed at 2026-10-01T05:57Z without flagging it - no detector covers abuse volume. Resolved by WF-026 above. |
| Packaging automation | The in-repo bump automation is **healthy**: `dist/chocolatey` is correctly at 1.6.14 with a matching sha256 and the release job commits it (`c892747`). The stale feeds are a **publish** gap, not a code gap: there is no `choco push` step in CI (the comment defers it to a manual maintainer push), Scoop has no automation in this repo at all, and winget needs the Microsoft CLA signed. |
| CI health | `ci.yml` is 30/30 successful across the last 30 runs, including `windows-test` and `windows-gui`, the two jobs WF-023's Windows verification gap depends on. |

---

## Summary

This workflow responds to the open feedback on the repo. The driving report is issue #59
(Yughaa): the v1.4.3 GUI does not launch at all on Windows, and the CLI still fails with
`FWP_E_IN_USE` (0x8032000A) on the relaunch after a clean Ctrl+C.

Root causes found by an Oracle-assisted audit plus four parallel explorer agents:

1. **GUI silent death.** `lightspeed-gui` is built with `windows_subsystem = "windows"`,
   so a panic or a returned `Err` produces no window and no output. The v1.4.2 GUI
   overhaul added the only new Windows-specific pre-window early-exit: the named-mutex
   single-instance guard, which read `GetLastError` without a `SetLastError(0)` reset and
   exited silently on the already-running path.
2. **WinDivert handle leak.** `--watch` had no Ctrl+C handling, so Ctrl+C was a hard
   kill; the receive thread parked in a blocking `WinDivertRecv` that an `AtomicBool`
   cannot wake (the `windivert` 0.6 wrapper has no `Drop` and its `shutdown` takes
   `&mut self`); the only close path was `Arc::try_unwrap`, which is unreachable while a
   thread blocks; and the legacy `capture/windivert_redirect.rs` backend (the GUI Boost
   path) never closed either handle.

| Item | Status |
|------|--------|
| GUI: panic hook + `gui-crash.log` + native error dialog | Added |
| GUI: logging initialized before the guard, non-fatal with temp/sink fallback | Fixed |
| GUI: `SetLastError(0)` before `CreateMutexW`, distinct exit code, `--force` bypass | Fixed |
| GUI: fallible tray so a tray failure cannot kill or strand the app | Fixed |
| GUI: `LIGHTSPEED_GUI_RENDERER=glow\|wgpu` escape hatch (glow feature added) | Added |
| Client: owned raw WinDivert handle (`&self` methods, `Drop` shuts down + closes) | Added |
| Client: `WinDivertShutdown` unblocks the parked recv; owner-thread teardown ack | Fixed |
| Client: both WinDivert backends close both handles on every stop path | Fixed |
| Client: `--watch`/`--start-interceptor` handle Ctrl+C and wait for teardown | Fixed |
| Client: GUI Quit waits (bounded) for interceptor and redirect teardown | Fixed |
| Client: firewall rule removal covered by the teardown ack | Fixed |
| Docs: corrected `FWP_E_IN_USE` guidance and single-instance description | Fixed |
| Project Zomboid profile registered in docs (PR #72) | Added |
| Dependabot base64 0.23 + patch group (PRs #61, #73) | Ready to merge |

---

## Verification

- `cargo fmt --all --check` clean; `RUSTFLAGS="-Dwarnings" cargo clippy --workspace
  --all-targets --exclude lightspeed-gui` clean for default, `quic`, `full`, and `ml`;
  `RUSTFLAGS="-Dwarnings" cargo clippy -p lightspeed-gui --all-targets` clean.
- `cargo test --workspace --exclude lightspeed-gui`: all 16 test binaries pass, 0 failed
  (client lib 203 tests, including 10 new `teardown` cases and 4 new `InterceptorHandle`
  stop/ack cases; 6 new engine ack cases). `cargo test -p lightspeed-gui`: 43 pass.
- GUI exercised headlessly on Linux under Xvfb: starts, writes the crash-safe trace log,
  discovers the five community relays, connects and registers a QUIC session, and stays
  alive. A forced second instance is detected by the guard.
- Windows-only code (the full WinDivert impls and the GUI tray/guard FFI) cannot be
  compiled on this host; it is verified by source review against `windivert-sys` 0.10 /
  `windows` 0.48 and must be confirmed by the `windows-test` and `windows-gui` CI jobs.

**Known verification gap:** real Windows runtime behaviour (WinDivert `recv` unblock,
close-after-ack, tray failure, message-box panic reporting) rests on the CI Windows build
plus the Linux-runnable unit tests for the shared decision and teardown logic. A real
Windows run on the reporter's machine is the final confirmation.

---

## Pre-release audit (2026-09-18)

Ran a functional audit on both platforms before any release, per the owner's
request. Everything below was exercised for real, not just built.

Verified working:
- Linux CLI: `--version`, `--list-games` (18), `--list-interfaces`, `--check`
  (user and root), `--status`, `--probe-proxies` (5 relays), `--test-control`
  (QUIC register + ping), `--live-test` (4/4 phases with `--fec`, data relay
  5/5 payload match), `--smoke-test` (full synthetic E2E, teardown, 0 leftover
  nftables tables), `--start-interceptor` + Ctrl+C, `--watch` + Ctrl+C,
  `--demo`, `--benchmark`, `--scan-processes`, `--intercept`, `--write-config`.
- Linux GUI under Xvfb: launch, discovery, QUIC registration, Boost (engages
  the nftables interceptor), Stop Boost, window-close Quit, clean teardown.
- Windows CLI in the VM: all read-only/network commands, `--test-control`,
  `--live-test` (4/4 phases), `--start-interceptor` + Ctrl+C (3/3 clean),
  `--write-config`, `--scan-processes`, `--intercept`, `--check`.
- Windows GUI in the VM: launch, discovery, registration, Boost (engages the
  WinDivert interceptor, firewall rule added), Stop Boost (rule removed,
  threads exit), single-instance guard, crash log path.

Bugs found and fixed during the audit:
- Linux/macOS leaked the kernel redirect rule on Ctrl+C (commit 0d3499f).
- Linux `--start-interceptor` ran forever without root (74dfbc6).
- `--check` reported a dead proxy as reachable (74dfbc6).
- GUI default window clipped the "BOOST MY GAME" button (0e75ca3).
- Windows: the WinDivert port-range filter also matched the client's own QUIC
  to the proxy, and the auto-detect tracker locked the proxy's address as the
  "game server", tunnelling the client's own traffic to itself (ee22be9).
  Found with a connected-UDP synthetic game on the VM; after the fix the
  interceptor locks the real game server (45.77.32.236:9999) and the client
  port (28015) with packets flowing through the relay.

Known issues left open (documented, not release blockers):
- Windows network-layer filtering cannot match `processId` (WinDivert supports
  it only at the Flow layer), so a broad-port-range game like Fortnite uses
  port-range interception plus debounce. The proxy address is now excluded, but
  other apps' UDP inside the range can still be intercepted briefly.
- `--test-control`/`--test-tunnel` print a stub success when built without the
  `quic` feature (release builds have `quic`).
- `--live-test` exits 0 even when a phase fails (manual diagnostic only).
- `config.general.log_level` is parsed but not applied to CLI logging.
- The GUI never calls `start_windivert`/`start_capture`, so those UI branches
  are unreachable; "Disconnect Boost Server" stops the relay loop but not an
  active interceptor (separate "Stop Boost").

Host note: the audit's VM shutdown hit the known RDNA4 `vfio_pci_remove` fault
the `gpu-switch` script documents, leaving the dGPU unbound until a reboot.

---

## Next Action

1. Reboot the host to restore the RX 9070 XT to `amdgpu` after the VM stop left
   it unbound (the vfio-pci teardown fault).
2. The owner decides on a v1.4.4 release (all audit fixes are on `master`).
3. WF-024 candidates: fix the `--live-test` exit code and the no-`quic` stub;
   `--repair-windivert` recovery command; registry cert pinning (F5) and binary
   release signing (C2) from the security backlog.

---

## WF-025: Observability overhaul (2026-09-19)

**Workflow:** WF-025
**Agent:** Sisyphus (Oracle design review + parallel explore/plan/implementation agents)
**Status:** Implemented on `feat/observability-overhaul`, pending review/release

Motivated by a live production probe that showed the proxy's observability was
partly hollow: the latency histogram was never populated, `fec_data_packets_total`
was always 0, `packets_dropped` conflated unauthenticated scanning with real loss,
the rate limiter never fired, telemetry reported a hardcoded game id and empty
country, and the six-hourly stats snapshot had no history.

Delivered (atomic commits `e7d90d8`..`44b4a5b`):

| Item | Change |
|------|--------|
| Response listener | Exactly one listener per session (was two) |
| Session lifetime | `last_activity` refreshed on traffic (was creation-only) |
| Drop semantics | `DropReason` categories + 7 flat `/health` fields; site shows "Packets Filtered" + "Upstream Loss" |
| FEC counter | `fec_data_packets_total` now incremented |
| Latency | Proxy-observed upstream response lag, non-cumulative buckets, 2s bound, discarded counter |
| Rate limiting | Per-IP aggregate tier (5000 pps / 5 MB/s, 65536 cap) + `check_at`; dead `max_connections` removed |
| Game ids | Protocol registry extended to all 18 games with key/id helpers |
| Telemetry | Client sends real game id + locale country; proxy aggregates by (game, country), k>=3 |
| History | Orphan `stats` branch, reset-safe bounded (360) history via `append-history.sh` |

**Verification:** full CI-parity locally (fmt, clippy `-Dwarnings`, release build,
17/17 test binaries, feature matrix `quic`/`full`/`ml`); live `/health` and
`/metrics` exercised; telemetry POST x3 rendered `game="cs2",country="US"`;
temp-repo stats + history fixture produced 2 snapshots with the 7 drop fields.

**Known gaps:** the optional trend UI (`network-history.json` visualization) was not
built; `protocol/src/framing.rs` has a pre-existing isolated-crate clippy warning
(`use std::io` unused without the `tokio` feature) that the workspace build does not hit.

---

# WF-025 — Relay self-update (Phase 0-2)

**Status:** Implemented on `feat/observability-overhaul`, unpushed, pending review/merge.

Delivered: client supervised control reconnect and per-relay token store; token-keyed
proxy auth with TTL/grace; per-path telemetry collection + aggregation; the mesh data
tooling (registry inventory, bounded reset-safe collector, analyzer, web trends); a
versioned release layout with a health-gated installer and rollback; systemd
`Type=notify` + watchdog; a verified self-updater on a timer; `/health` update state; and
in-place `execve` handoff (manifest + fd adoption + session/auth transfer) that keeps
existing client flows alive across a binary swap.

**Verification:** full CI parity (fmt, clippy `-Dwarnings`, release build, 23 test
binaries, `quic`/`full`/`ml`); installer self-test 86 checks, updater 46;
`test_handoff_e2e.sh` (root) proves same-PID 1.4.4->1.4.5 with a live session and a
preserved outbound source port; a real canary installed on relay-nrt (healthy,
`current -> releases/1.4.4-canary-8bac681`, previous release retained).

**Rolled out:** all five production relays now run 1.5.0 under the versioned layout with the
`Type=notify` unit and `handoff.supported=true`; the previous binary is retained for
rollback. Each relay took one health-gated restart (a one-time migration via
`infra/scripts/migrate-to-versioned.sh`, then `relay-install.sh`); future releases apply via
the in-place handoff with no forced reconnect. Relay uptime is already published on the site
per relay.

**Next:** push/merge the branch and cut a 1.5.0 release so the self-updater and the Pages
stats reflect it; then use `collect-metrics.sh` + `analyze-mesh.sh` on live data to tune
relay selection.

---

## WF-026: Demand-driven relay placement recommender (2026-09-19)

**Workflow:** WF-026
**Agent:** Architect / RustDev / InfraDev / QAEngineer
**Status:** Shipped. Merged via PR #90, released as v1.6.0, deployed to all five production relays, and the MMDB is published (`geoip-2026-09`) and synced. v1.6.1 hardens the deploy path: handoff request ownership, automatic `[server] public_ip`, and shipping `relay-updater.sh`.

Motivated by the owner's request to prioritize relay placement by where players actually play.
Investigation found the WF-025 player-country telemetry is opt-in via the CLI `--telemetry`
flag only, the GUI has no toggle, and production relays expose zero game/country series; the
only country tags live were the relay location tags. The owner chose the demand-analysis plus
region-recommender path with a player-region to game-server-region matrix, no infra spend, and
no raw IP storage.

Delivered:
- Proxy-side transient country derivation (DB-IP Lite MMDB via `maxminddb`) aggregating
  `(source_country, destination_country)` session counts with a k>=3 export floor, exposed as
  `lightspeed_geo_*` on `/metrics`.
- `infra/geo/` region and candidate catalogs plus `recommend-regions.sh`, which scores candidates
  by demand-weighted coverage and redundancy and emits ADD/MOVE/NONE with a 3-run stability gate
  and an INSUFFICIENT_DATA floor.
- `collect-metrics.sh` coarsens country pairs to region pairs and persists them in the stats
  branch history; `pages.yml` runs the recommender and commits `.stats/placement.json`.
- Monthly `geoip-release.yml` publishes the pinned MMDB; `relay-updater.sh` and
  `setup-new-node.sh` verify and sync it.
- Privacy/FAQ/NOTICE describe the transient derivation and add the DB-IP attribution.

**Verification:** CI parity locally (fmt, clippy default/quic/full/ml, workspace tests,
`cargo deny`); bash suites 75/59/63/101/28 checks; live `/metrics` geo smoke; collector and
recommender exercised end to end; reviewer pass on five blocker concerns.

**Known gaps:** real history is roughly 60 sessions, so the recommender correctly reports
INSUFFICIENT_DATA until the new proxy build and MMDB reach the fleet; the 64-cell cap is a growth
guard that cannot trigger with the real 8-region catalog; MOVE target selection is region-agnostic.

**Next:** let demand accumulate. The Pages recommender runs every 6h and currently reports
INSUFFICIENT_DATA (no geo cells reach the k>=3 floor yet). Once volume grows it will name the
best next region to deploy and test; any actual relay add or move remains a human decision.

---

## WF-027: Shadow direct path - step 1 (like-for-like direct application RTT)

**Workflow:** WF-027
**Agent:** RustDev + QAEngineer
**Status:** Implemented in the working tree (not committed)

The published "RTT saved" compared an ICMP echo (direct) against the tunnelled
game-traffic round trip (relayed): different instruments, so only an estimate.
Step 1 adds a like-for-like direct application RTT: one of the game's OWN
packets per server per 30s goes out on the direct path unmodified, the server's
reply is timed on a WinDivert inbound sniff handle, and `direct_app_p50_ms`
flows through telemetry, proxy metrics, the collector, and the web trend. The
ICMP estimate stays as the fallback.

Delivered:
- `client/src/latency.rs`: separate shadow-direct tracker (own pending map, 30s
  per-server gate, 5s reply timeout, 300s TTL, 256-sample ring,
  `shadow_direct_p50_ms`); never touches `pending` or the relayed ring.
- `client/src/interceptor/order.rs` + `windows.rs`: `Decision::ShadowDirect`,
  unchanged re-injection of the game's own packet, inbound sniff handle, teardown
  ack extended to four owners.
- `protocol/src/telemetry.rs`: `direct_app_p50_ms` (finite, 0..=10000).
- `client/src/telemetry.rs`: `build_report` populates it.
- `proxy/src/metrics.rs`: `lightspeed_telemetry_direct_app_ms_{sum,count}`.
- `infra/scripts/collect-metrics.sh` + test: `direct_app_ms_{sum,count}`.
- `web/app.js`: prefers the like-for-like value, ICMP estimate as fallback.

**Verification:** `cargo fmt --all --check` clean; clippy `-Dwarnings` default and
`full` clean; `cargo test --workspace --exclude lightspeed-gui` 714 passed, 0
failed; collector suite 100 checks. Windows-only code cannot be compiled on this
host and is compile-checked by CI: the `windows-gui` job builds the client with
`windivert-redirect`, the `windows-test` job builds the feature-off stub.

**Known gap:** real Windows runtime behaviour (sniff handle pairing, direct
re-injection) rests on the CI Windows build plus the Linux-runnable shadow
tracker unit tests.

**Next (step 2):** not defined in this task.
