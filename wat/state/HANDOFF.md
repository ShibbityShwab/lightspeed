# Handoff: LightSpeed, 2026-10-04

Brief for the incoming agent. Start at `wat/state/current-phase.md` (canonical
phase log, WF-050 is this entry), then `wat/rules.md` (the $0-cost rule is the
one that binds hardest) and `wat/archive/agents.md` (personas).

Repo: `ShibbityShwab/lightspeed`, branch `master`, HEAD `22c9623`. Worktree
clean, everything pushed, CI green for every recent commit.

## What this project is

A zero-cost global network optimizer for multiplayer games, run through a
Workflows/Agents/Tools autonomy engine (`wat/`). Crates:

- `client/` — core engine: pcap/WinDivert capture, QUIC control plane, FEC
  tunnel, latency probing, per-game process detection.
- `client-gui/` — egui Windows app (the active workstream this week).
- `proxy/` — the relay fleet.
- `protocol/` — shared wire types.
- `infra/scripts/` — monitoring collectors (health, anomaly detection, stats).
- `web/` — the GitHub Pages site, fed by the collectors.
- `dist/` — Scoop / Chocolatey / winget packaging.
- `.github/workflows/` — CI, Docker, Security Audit, Deploy, health monitors.

Cost rule (`wat/rules.md`): total ongoing infrastructure cost must stay exactly
$0.00 — Always Free tiers only.

## Where the GUI landed this week (all merged, all CI-green)

`b32975d..22c9623`, in order:

- Route globe with **offline** IP geolocation (no external API; a generated
  country/range table), near-hemisphere-only drawing, route arc.
- 320px-minimum window support; the status column reordered so the state banner
  is the first thing read.
- Inter + JetBrains Mono bundled from `client-gui/assets/fonts/` (SIL OFL 1.1;
  the user's condition was "as long as licenses allow it").
- Boost-server pill cluster + checkbox replaced by one dropdown whose selected
  text is the real state — "Auto (fastest)" when auto, the relay name when
  pinned. Auto was already the default on first install; the UI hid it.
- About card (logo, version, GitHub/Website/Releases links, "Star it on
  GitHub"), replacing the in-app brand row that duplicated the OS title bar.
- Admin handling fixed (`0b59cce`): `is_admin` now reads the process token via
  locally-declared advapi32 FFI instead of `net session` (which fails when the
  Server service is stopped — a false "not admin"); `relaunch_as_admin` no
  longer exits the process when already elevated.
- `scripts/dev-gui.ps1 [-Elevate]` — run the debug build directly, no install.
  Refuses to start a second instance (the app's single-instance mutex is shared
  across every build, so a stray instance silently wins).
- `scripts/install-gui.ps1` — installs by content: copies, then compares sizes
  and exits non-zero on mismatch. `Copy-Item -Force` over a running exe
  "succeeds" silently; trusting its exit code caused two false "installed"
  reports (`c5794d2`).
- Bug round `22c9623`, three reachable panics, all mutation-tested:
  1. `client/src/capture/windivert_redirect.rs::parse_ipv4_udp` accepted IHL<5
     (RFC 791 min is 5 words). A malformed datagram then parsed ports out of
     the IP header and routed on garbage. **Network input** — the serious one.
  2. `client-gui/src/globe.rs::draw_land` indexed the embedded land blob
     unchecked inside the paint loop; a truncated blob panics the window.
  3. `client-gui/src/geo.rs::locate` indexed `COUNTRIES[c as usize]` with a
     table-generated u8; `.get(..)?` now.

## Verified state (2026-10-04)

- CI: `push success` for every one of `561203c..22c9623`. Gotcha: querying
  `gh run list --commit <SHORT_SHA>` returns nothing — the API `head_sha`
  filter needs the full SHA; short SHAs match zero runs even when green.
- Scheduled workflows green: Health Monitor, Health Anomaly Monitor, Deploy
  Pages (Oct 3).
- Tests: GUI 71 pass / 0 fail; client 387 pass with `windivert-redirect`
  feature (feature-gated code compiles to nothing without it — always run the
  feature when testing that module); whole workspace green.
- Gates before every push, in order: `cargo fmt --all --check`, clippy with
  `-Dwarnings` (CI sets `RUSTFLAGS: -Dwarnings`), then the crate's FULL suite
  (never a module filter — a filtered green says nothing about the rest).
- Fleet: 8/8 relays on v1.6.14, healthy, traffic flowing (WF-049 verified the
  earlier traffic stop cleared).
- Release v1.6.14 published. Scoop current. **Chocolatey stuck at 1.6.3** —
  package built and correct, push command in `dist/chocolatey/README.md`,
  blocked on the owner's API key. **winget PR #445619 open**, blocked on the
  owner's CLA.

## Pain points (hard-won, do not re-learn these)

1. **Elevation split.** This agent's shell is medium-integrity even when the
   OmO terminal runs as admin — elevation does not propagate into the
   command-runner. Consequences: cannot stop the elevated GUI (taskkill fails
   with "Could not stop process"); the running GUI locks its own exe (build
   fails `Access is denied (os error 5)`); Program Files installs need a UAC
   prompt and the user may never see one. For dev, use `dev-gui.ps1 -Elevate`
   and stop the app from its tray; for installs, `install-gui.ps1` run
   elevated, and verify by size, not exit code.
2. **Windows host quirks.** `python`/`python3` are Microsoft Store stubs that
   exit 49 — use the JS kernel/Bun or `node` for scripting. No `jq`. Files
   check out CRLF; an edit matched against `\n` will miss (the "LF will be
   replaced by CRLF" git warning is normal here). Cargo needs the winlibs
   mingw64 toolchain on PATH. In Git Bash, `taskkill` needs `//IM`. Launch the
   GUI with `nohup ... &`, never `Start-Process` from a script that should not
   spawn console windows. The LSP daemon times out under load — use
   `cargo check` directly.
3. **Verify artifacts, not proxies.** This session's failures were all one
   mode: trusting an exit code or a summary. The install "succeeded" while the
   file sizes differed; a suite read "ok, 0 passed". Read pass counts, compare
   file bytes, screenshot the UI, and mutation-test fixes (revert the fix,
   confirm the test fails) before claiming a bug is fixed.
4. **Single-instance mutex is shared across all builds.** A stray
   `target/debug` instance silently wins over an installed launch. Check
   `tasklist` before launching anything; `dev-gui.ps1` guards this.
5. **egui popups are invisible to the accessibility tree.** Dropdown options
   cannot be read via AX; use screenshots for visual state. Verify a collapsed
   control by its selected text, not the popup.
6. **Remote-write policy.** Routine commits/pushes to master are authorized
   (standing grant); Conventional Commits; each push gated by the checks above.
   Owner-gated actions to surface as ONE crisp question: Chocolatey API key,
   winget CLA, product calls.

## What still needs to happen (ranked)

1. **Rebuild + reinstall the GUI.** The installed Program Files build
   (`590592448` bytes, Oct 3 17:56) predates `22c9623` and is missing the
   globe/geo bounds fixes. `cargo build -p lightspeed-gui`, then
   `scripts/install-gui.ps1` elevated (stop the app from its tray first).
2. **Owner-gated channels:** Chocolatey push (one command in
   `dist/chocolatey/README.md`), winget bootstrap (CLA + PR #445619).
3. **GUI auto-update.** "Check for updates" only notifies. The installed copy
   drifts behind master with every merge; an in-place updater or installer ends
   the recurring reinstall loop. Highest-leverage product improvement.
4. **Mutex-poisoning hardening (deliberate, optional).** ~12
   `engine.lock().unwrap()` in `client-gui/src/app.rs`; one panic while holding
   poisons the lock and every later frame panics. No reachable panic was
   demonstrated, so it was declined as speculative — but as defence-in-depth it
   is a clean, contained change (poison-tolerant wrapper + a test).
5. **Product calls (owner):** #137 (Minecraft support request), #59 (the
   Windows/Fortnite thread).
6. **Keep the WAT state current.** Every round ends by updating
   `wat/state/current-phase.md` and `wat/state/decisions.md` — that is how the
   next agent resumes without re-deriving context.
