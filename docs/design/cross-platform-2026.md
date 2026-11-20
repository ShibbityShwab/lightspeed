# Cross-platform parity plan: Linux + macOS (2026 desktop refresh)

> **Status:** W1 design deliverable for WF-081 — documentation only. This document
> changes no workflow, packaging, platform code or engine code.
> **Revision:** r17 (2026-11-20, task-26 delta-fix) — removes the stale
> "other docs still carry the old plan" sentence (QA review r4 finding **F21**),
> re-anchors the geometry to the **re-baselined** design set (spec §1.1,
> design-system §5.4), re-derives the citations into the rebased GUI sources, and
> re-runs the refs checker. r4 (task-20) folded in the owner's
> **mobile-only viewport** decision (width pinned 320 px, single column, height
> 420–1400 / initial 760; captures 320×420, 320×760, 320 @ 200 %) and re-baselined
> the evidence to `origin/master` `76aba5d`; r3 (2026-11-19) reconciled QA findings
> R12/R15/R4(ii)/R8. Evidence:
> `wat/evidence/2026-11-20/team/desktop-platform/delta-r17.md` (r17),
> `wat/evidence/2026-11-20/team/desktop-platform/delta-r12.md` (r4),
> `wat/evidence/2026-11-19/team/desktop-platform/revision-r3.md` (r3).
> **Owner:** `desktop-platform` (`wat/personas/desktop-platform.md`).
> **Binding input:** the owner made Linux + macOS parity a hard requirement for the
> GUI and every non-capture flow — [gui-refresh-brief.md](gui-refresh-brief.md) §13
> gate 3. This plan turns that decision into a surface × OS matrix, a stub
> inventory, an ordered gap-closure plan and an exact external-verification ask.
> It does **not** grant parity to anything.
> **Date:** 2026-11-20. **Baseline:** `origin/master` `76aba5d` (16 commits since the
> W1 first pass; the W1 design set is commit `0df57ce` on top), worktree
> `dsh-b74d064d-6fe5`.
> **Companion docs:** [interface.md](interface.md) (binding state contract),
> [gui-refresh-brief.md](gui-refresh-brief.md) (program contract).
> **Skill:** `.dsh/skills/lightspeed-gui-design/SKILL.md` §2 (2026 floor).

Read §1 first: this plan was measured against the brief's **pre-W1** §13 parity
baseline. That draft was wrong in both directions; the Lead has since folded the
corrections into the brief (`docs/design/gui-refresh-brief.md:336-359`) and added
the 2026-11-20 owner decisions, including the mobile-only viewport that supersedes
the 360/440/1120 plan (`docs/design/gui-refresh-brief.md:318-334`). §1 keeps the
record of what changed, with the old text labelled as the pre-W1 draft.

---

## 1. What is actually true today (corrections to the shared baseline)

**Quote-drift caveat.** The "Pre-W1 draft" quotes in the table are the brief text
this document checked in its first pass; the Lead rewrote that table afterwards,
so the quotes are kept to record *what changed*, not as current claims. The docs
gate used by this program (`wat/evidence/2026-11-19/team/desktop-platform/verify-refs.ps1`)
resolves **paths and line numbers only** — it cannot detect that the words at a
cited line changed, and those stale quotes still resolved mechanically at their
old lines. Re-read a quoted passage whenever its target doc is revised; where a
section is authoritative, cite the section as well as the lines.

| # | Baseline claim | What the tree and the published release prove |
|---|---|---|
| C1 | **Pre-W1 draft:** "GUI build/lint/test in CI: Linux excluded everywhere; macOS excluded in `macos-smoke`" (the brief text this document checked in its first pass). The corrected brief records the replacement at `docs/design/gui-refresh-brief.md:307-316` (owner implication) and in its corrected baseline table `docs/design/gui-refresh-brief.md:336-359` (PR-CI row `:343`, release-artifact row `:344`). | **True as a PR gate, incomplete as a build fact.** `ci.yml` excludes the GUI from every Linux job (`.github/workflows/ci.yml:36,39,42,258,264,346,349,352,368`) and from `macos-smoke` (`.github/workflows/ci.yml:219-233`). But the **release** pipeline builds the GUI for Linux x86_64 and both macOS targets: `dist-workspace.toml:18-24` and `client-gui/Cargo.toml:79-84` list them, and the published v1.11.3 release contains `lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz` (10,312,360 B), `lightspeed-gui-x86_64-apple-darwin.tar.xz` (7,880,000 B) and `lightspeed-gui-aarch64-apple-darwin.tar.xz` (7,118,384 B) — `wat/evidence/2026-11-19/team/desktop-platform/release-assets-1113.txt`. The rebase baseline keeps the same asset shape: v1.11.4 publishes `lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz` (10,445,168 B), both macOS GUI tarballs, the shell installer and the Windows MSI/zip — `wat/evidence/2026-11-20/team/desktop-platform/release-assets-1114.txt`. Builds are tag-time only: the build matrix runs when `publishing == 'true'` (`.github/workflows/release.yml:129`), cargo-dist 0.32's documented `pr-run-mode` default is `plan` ([cargo-dist config reference](https://axodotdev.github.io/cargo-dist/book/reference/config.html#pr-run-mode)), and this repo does not override it — so a Linux/macOS GUI build break surfaces at release, never on the PR. **Never linted or tested off Windows.** |
| C2 | **Pre-W1 draft:** "Autostart / updater UX: Windows start-menu/registry + axoupdater; Linux systemd unit (unimplemented); macOS LaunchAgent (unimplemented)" (same pre-W1 draft). The corrected brief now records "Autostart | **not implemented** | **not implemented** | **not implemented**" (`docs/design/gui-refresh-brief.md:348`). | **Autostart is unimplemented on all three OSes.** No code writes a Run key, Startup-folder item, XDG autostart desktop file or LaunchAgent (grep of `client-gui/src` and `client/src` for `autostart|HKCU|shell:startup|LaunchAgent|systemd --user`: zero hits). The MSI's Start Menu shortcut (`docs/install-windows.md:13`) is an install shortcut, not launch-at-login. Only the updater half of the row is real on Windows (`client-gui/src/update.rs`, `client-gui/src/self_update.rs`). |
| C3 | "macOS is untested" (`client-gui/src/platform/mod.rs:7`, `client-gui/src/platform/macos.rs:3,25`; `docs/user-guide.md:32,105`; `docs/install-macos.md:3`) | Still true **at runtime**, but not "unbuilt": the macOS GUI tarballs in v1.11.3 (and again in v1.11.4 at the rebase baseline) are real outputs, and `.github/workflows/release.yml:256-262` gates the release on every build-local matrix job succeeding. Build evidence exists; run evidence does not. |
| C4 | "the GUI has no macOS platform module, so restrict its release targets to Linux and Windows" (`client-gui/Cargo.toml:74-75`, repeated at `dist-workspace.toml:16-17`) | **False comment.** `client-gui/src/platform/macos.rs` exists and is selected at `client-gui/src/platform/mod.rs:118,133-136`; the release-target list in the same manifest includes both Apple targets (`client-gui/Cargo.toml:79-84`). Stale comments only — no behaviour depends on them, but they mislead every reader about installed-versus-unbuilt. |
| C5 | "The GUI works on Linux (the system tray is a stub)" (`docs/user-guide.md:31`) | **Unverified user-facing claim.** Linux evidence today is a release artifact, not a run; no Linux run appears under `wat/evidence/`. Downgrade per §7 until a run exists. |

One-line state: **the code compiles and ships on all three OSes; the platform
behaviour is Windows-only, the Linux/macOS halves are stubs, and the only
evidence outside Windows is a build artifact. This host cannot produce more.**

---

## 2. Evidence vocabulary used in this document

Every row below states the tier the OS actually reaches **today**. The full
statement schema, the per-surface minimum tier and the rule that forbids
collapsing tiers into the word "parity" are in §7.

| Tier | Means | Does **not** mean |
|---|---|---|
| **B** — build-verified | The code compiles for that OS in a named job/run (and, where noted, clippy/test ran). | The window opened; any user flow works. |
| **H** — headless-verified | The process ran on that OS in a non-interactive session and a scripted probe observed a real signal (window created with the expected title, smoke hook exited 0, lock file acquired, a file written). | A human saw the UI; tray, menu, DPI, elevation, autostart or network flows worked. |
| **R** — runtime-verified | A real interactive session on that OS, with named evidence (screenshot/UIA/log) for each state claimed. | Anything not in the evidence table. |

Tier codes used in tables: **B / H / R / — (not reached)**.

---

## 3. Surface × OS matrix

### 3.1 Current implementation (with citations)

Windows is the reference column: it is the only OS whose behaviour is observed
in a real session (the repo's own screenshots/UIA per
[gui-refresh-brief.md](gui-refresh-brief.md) §10 and the WF-075 state entry).

| Surface | Windows (current · tier) | Linux (current · tier) | macOS (current · tier) |
|---|---|---|---|
| **Window chrome / decorations** | Native Win11 decorations from winit; app sets only title, fixed 320 px width, 420–1400 height (`client-gui/src/main.rs:109-115`; the 320 px pinned single-column width is the owner-confirmed product form, 2026-11-20 — G2.2); `windows_subsystem = "windows"` (`client-gui/src/main.rs:6`); brand exe icon compiled by `client-gui/build.rs:45-77`. **R** | Native decorations follow the DE (nothing is overridden in `client-gui/src/main.rs:109-120`); same pinned viewport; Wayland/monitor placement is handled heuristically with a documented `TITLE_BAR_ALLOWANCE` (`client-gui/src/app/window.rs:41-113`). **B** only | Native traffic lights (decorations default on); **no `.app` bundle in the release** — v1.11.3 ships a bare tarball, so no `CFBundleName`/bundle identity; the `.app`/`.dmg` builder `tools/package-macos.sh` (Info.plist template `packaging/macos/Info.plist:11-14`) is manual and invoked by no workflow. **B** only |
| **Tray / menu bar** | Real `tray-icon 0.26.1`: Show / Connect / Disconnect / Quit menu, per-state icon + tooltip, double-click restore, X hides to tray (`client-gui/src/platform/windows.rs:51-165`); `has_system_tray()` reflects real creation (`client-gui/src/platform/windows.rs:177-184,248-250`). The delta adds a fifth state — `TrayState::RelayStarved` (amber-orange, "relay is not returning your game traffic") so a starving relay never paints the green optimizing icon (`client-gui/src/app.rs:70-81`, `client-gui/src/platform/windows.rs:137-158`, pure selector `client-gui/src/app/window.rs:133-159`). **R** | **Stub.** `LinuxTray` is a no-op: `poll_events` returns empty, `set_state` does nothing (`client-gui/src/platform/linux.rs:10-19`), but `new_tray` returns `Some(LinuxTray)` (`client-gui/src/platform/linux.rs:30-32`); `has_system_tray()` keeps the trait default `false` (`client-gui/src/platform/mod.rs:104-106`), so close exits instead of hiding (`client-gui/src/app/window.rs:161-178`) and the "Hide to tray" button is not drawn (`client-gui/src/app.rs:4178-4186`). No menu-bar/application menu is defined anywhere. **B** only | **Stub.** Identical no-op shape (`client-gui/src/platform/macos.rs:13-21,29-34`), documented "no menu-bar integration yet" (`client-gui/src/platform/macos.rs:4,12`), untested (`client-gui/src/platform/macos.rs:3,25`; `client-gui/src/platform/mod.rs:7`). No status item, no application menu, no Dock menu. **B** only |
| **Shortcuts** | No shortcut layer: no `KeyboardShortcut`, no menu accelerators, no global hotkey (grep over `client-gui/src`: zero hits). Keyboard state comes only from egui's own focus/tab handling; sheets close on backdrop click via `response.should_close()` (`client-gui/src/app.rs:3340,3637,3770,3983`), with no explicit Esc/Enter binding. **R** (for the absence) | Same code, same absence. **—** (nothing run) | Same code, same absence. No ⌘Q/⌘, handled by the app, no menu bar to carry them. **B** only |
| **DPI / scaling** | Per-monitor DPI via winit/egui; the auto-size reads monitor height and client-area top in physical pixels then scales (`client-gui/src/app/window.rs:61-113`), and the shell adapts at narrow/short breakpoints (`client-gui/src/app.rs:3998-4000`). 200 % is a design requirement (`docs/design/gui-refresh-brief.md:67`); the mobile-only decision keeps the 320 px width, so the 200 % proof is a **320 px @ 200 %** capture (G2.2). No saved 200 % evidence exists in this repo's design docs. **R** at 100 %; **—** at 200 % | Same code; Wayland/fractional scaling is acknowledged only as an assumption in comments (`client-gui/src/app/window.rs:45-48`). **B** only | Same code; no macOS-specific scaling work. **B** only |
| **Notifications** | **None.** No notification crate or API in `client-gui/Cargo.toml:14-41` and no notification call site; status changes reach the user only through the window and the tray tooltip (`client-gui/src/platform/windows.rs:130-165`). **R** (for the absence) | None, and no tray to fall back on. **—** | None. **—** |
| **Autostart** | **Not implemented** (C2). Launch arguments: only `--force` is parsed, by the single-instance guard (`client-gui/src/single_instance.rs:38-46`); there is no `--autostart`/`--hidden` launch mode. **R** (for the absence) | Not implemented. The brief's "systemd user unit (unimplemented)" is accurate in direction but there is no unit file either. **—** | Not implemented; no LaunchAgent. **—** |
| **Updater UX** | Available and UI-wired: `axoupdater` receipt check plus a GitHub-releases fallback (`client-gui/src/update.rs:92-182`); in-place install is Windows-only by design (`client-gui/src/self_update.rs:1-14`) and ships a UAC-handoff copy (`client-gui/src/app.rs:3964-3972`). **R** | The *check* runs on Linux (same code), but the installer is compiled out: `zip_assets` returns `None` on non-Windows (`client-gui/src/update.rs:210-213`) and `install_blocking` returns "in-place updates run on Windows; update through your package manager" (`client-gui/src/self_update.rs:27-34`). The sheet therefore offers no update action and no package-manager instruction; where an error is shown it is the raw English diagnostic (`client-gui/src/update.rs:234-240`). **B** only | Same as Linux. Additionally the release ships `lightspeed-gui-installer.sh` + `lightspeed-gui-*-update` assets (produced by cargo-dist from `dist-workspace.toml:37` and `client-gui/Cargo.toml:85`, listed in `release-assets-1113.txt`), but the app never invokes an axoupdater run, so receipt-installed mac users are told nothing useful. **B** only |
| **Admin / elevation semantics** | Elevation read from the process token, not `net session` (`client-gui/src/platform/windows.rs:191-238`); relaunch via PowerShell `Start-Process -Verb RunAs`, and a no-op when already elevated (`client-gui/src/platform/windows.rs:266-280`). Boost is gated on this flag in the one action slot (`client-gui/src/app/ledger.rs:39-57`; `client-gui/src/app.rs:4218-4235`). Same user profile, so config survives. **R** for an already-elevated session (the audit session was elevated; the boosted run itself remains the repo-wide unverified state, brief §11). The **non-elevated launch and UAC-decline** branch is **U** on this host: this agent shell is elevated (measured 2026-11-19) and the decline click is on the secure desktop, which no agent input path can drive — fixture and verifier are in §3.2. | **`id -u == 0`** (`client-gui/src/platform/linux.rs:34-46`) — root, not the desktop norm; the engine actually needs root or `CAP_NET_ADMIN` for nftables (`client/src/interceptor/linux.rs:24-27,68-80`). Relaunch uses `pkexec env DISPLAY/XAUTHORITY/LIGHTSPEED_PROXY*` and deliberately omits `WAYLAND_DISPLAY` to force XWayland (`client-gui/src/platform/linux.rs:72-98`); it does **not** forward `HOME`/`XDG_CONFIG_HOME`, so the elevated GUI reads and writes root's config, not the user's (`client-gui/src/paths.rs:34-43`). **B** only | **`id -u == 0`** (`client-gui/src/platform/macos.rs:36-48`), while the engine needs root for `pfctl` (`client/src/interceptor/macos.rs:13-16`). Relaunch is `osascript` `do shell script … with administrator privileges` (`client-gui/src/platform/macos.rs:74-89`): a whole GUI process running as root under a minimal environment — same config-path loss as Linux, plus a root app window, which is against macOS norms. **B** only |
| **Single instance** | Named mutex `Local\LightSpeed-GUI-Single-Instance`, second launch shows a MessageBox and exits 2 (`client-gui/src/single_instance.rs:47-55,77-89,93-120,133-179`). Tests run in the Windows GUI job (`.github/workflows/ci.yml:324`). **R** | Exclusive advisory lock file under the data dir, OS releases it on process death; failures degrade to "continue without guard" with a log line (`client-gui/src/single_instance.rs:183-213`). Its two tests are `#[cfg(not(windows))]` (`client-gui/src/single_instance.rs:280-315`) so **they have never executed in any CI run**. Second-launch notice on non-Windows is `eprintln!` plus an eframe window (`client-gui/src/single_instance.rs:84-88,215-249`) — a console-less app printing to nowhere. **B** only | Same as Linux, same never-run non-Windows tests. **B** only |
| **Paths / logs** | `%LOCALAPPDATA%\Lightspeed\{gui-trace.log,gui-crash.log,relay-history.json,instance.lock}` and `%APPDATA%\light-speed\config.toml`; open-in-OS uses `explorer` (`client-gui/src/paths.rs:9-61`); log reachable from Settings (`client-gui/src/app.rs:3445-3447`). Crash log written, native crash dialog Windows-only (`client-gui/src/crash.rs:124-158`). **R** | `dirs::data_local_dir()`/`dirs::config_dir()` → `~/.local/share/Lightspeed` and `~/.config/light-speed`; open-in-OS uses `xdg-open` (`client-gui/src/paths.rs:12-16,34-61`). **Crash dialog is a no-op** on non-Windows (`client-gui/src/crash.rs:124-158`), and a fatal startup error is only a log/stderr write (`client-gui/src/main.rs:35-44`), invisible to a desktop-launched GUI. **B** only | Same `dirs` calls, but both resolve to `~/Library/Application Support` on macOS — so the `client-gui/src/paths.rs:9-16,34-43` doc comments ("~/.local/share", "~/.config … elsewhere") are wrong for macOS even though the runtime paths are correct; user docs get it right (`docs/user-guide.md:127-129`). `open` is used for open-in-OS (`client-gui/src/paths.rs:49-61`); crash dialog no-op. **B** only |
| **Packaging / install** | `lightspeed-gui-x86_64-pc-windows-msvc.msi` (Start Menu shortcut + uninstaller) and portable zip (`docs/install-windows.md:13-14`); cargo-dist builds it, winget is still an external bootstrap PR, Chocolatey/Scoop are CLI-only. **R** | `lightspeed-gui-x86_64-unknown-linux-gnu.tar.xz` in the release and AUR `lightspeed-bin` installs the GUI for x86_64 only (`dist/aur/PKGBUILD:9-10,38-44,60-63`); no deb/rpm (`dist-workspace.toml:13-15`), no aarch64 Linux GUI. **B** (artifact exists) | `lightspeed-gui-{x86_64,aarch64}-apple-darwin.tar.xz` + `lightspeed-gui-installer.sh` in the release (build evidence above); no `.app`/`.dmg` in CI, ad-hoc `.app` builder is manual (`tools/package-macos.sh:1-38`), no Developer ID/notarization (would be paid — `[COST_STUB]`), Homebrew formula installs the CLI only (`Formula/lightspeed.rb:30-32`). **B** only |
| **Capture capability probe** | The GUI probes `sc query npcap` (`client-gui/src/platform/windows.rs:240-246`) and shows an **"Npcap"** row (`client-gui/src/app.rs:3512-3523`, comment `client-gui/src/app.rs:316-320`) — but the shipped GUI enables `windivert-redirect`, not `pcap-capture` (`client-gui/Cargo.toml:15,65-67`; `client/Cargo.toml:42-56`), so the probe tests a dependency the product does not use. The engine's real probe is `TrafficInterceptor::check_availability()` (`client/src/interceptor/traits.rs:779`; Windows `client/src/interceptor/windows.rs:723`), called only at start (`client/src/engine.rs:1208`). **R** | Probes `which tcpdump || which dumpcap` (`client-gui/src/platform/linux.rs:48-63`) — neither is the backend: the engine uses `nft`/`iptables` plus privilege (`client/src/interceptor/linux.rs:68-80,106`). The same literal "Npcap" row is drawn on Linux (`client-gui/src/app.rs:3522`). **B** only | Probes `tcpdump`/`dumpcap` (`client-gui/src/platform/macos.rs:50-65`) while the engine uses `pfctl` + root (`client/src/interceptor/macos.rs:13-16,140`); same "Npcap" row. **B** only |

### 3.2 Target behaviour and required verification

Targets are ordered by the plan IDs in §5. "Verify" names the method that would
raise the OS from its current tier; nothing here counts as evidence until it runs.

| Surface | Target behaviour (all OSes unless stated) | Verify with | Tier required | Gap |
|---|---|---|---|---|
| **Window chrome** | One product, native frame: Win11 caption/snap, macOS traffic lights + app menu, DE decorations on Linux. **Mobile-only shipped form (owner decision 2026-11-20):** width **pinned 320 px**, single column, height **user-resizable 420–1400** with initial **760**; remember height + position across restarts and restore to a visible monitor; survive live DPI/monitor change; no clipping at 320 px and at **320 px @ 200 %** (`docs/design/gui-refresh-spec.md:45-55`; `docs/design/design-system-2026.md` §5.4; owner decision `docs/design/gui-refresh-brief.md:323`) | Screenshot matrix per OS: **320×420**, **320×760** and **320 px @ 200 %**, plus a restart-geometry check | **R** each OS | G2.2 |
| Tray / menu bar | Windows tray unchanged (state set now includes `RelayStarved`); macOS NSStatusItem + application menu (Show / Connect / Disconnect / Quit, state word in the title/tooltip, template icon per HIG); Linux StatusNotifierItem where the DE supports it (GNOME without the AppIndicator extension has no tray) with an honest no-tray mode: close exits, no "Hide to tray" copy, documented per DE | Unit test: `has_system_tray()` reflects real creation; headless smoke: process stays alive with a status item (no crash); runtime: icon + menu interaction on each OS/DE | **R** (tray interaction); **H** minimum for creation | G1.4, G1.5 |
| Shortcuts | Platform conventions: macOS ⌘Q quit, ⌘, settings, ⌘H hide-to-tray (only when a tray exists), menu-bar items carry them; Windows/Linux Ctrl+Q / Ctrl+, plus Alt menu on Windows; Esc closes the top sheet, Enter confirms its primary action; shortcuts discoverable in tooltips or the sheet footer; no global hotkey in this wave | Unit-test the pure keymap tables; runtime: keyboard-only pass on each OS; screenshots of the focused sheet | **R** each OS | G2.1 |
| **DPI / scaling** | One mobile column stays usable at 100 % and **200 %** on each OS; fractional Wayland scaling; no clipped controls or off-screen action bar at the pinned 320 px width; window constraints in points, not pixels | Screenshots at **320 px @ 200 %** per OS (and 100 % for comparison); headless smoke can assert `pixels_per_point` when a scale is forced | **R** each OS | G2.2 |
| Notifications | **Recommendation: no OS notifications in the refresh (explicit no-claim).** The product's job is a glanceable window; macOS notifications additionally require a signed bundle, which is out of budget. If the owner wants them later: actionable only, opt-in, per-OS mechanism is new platform code | A no-claim is verified by the absence of copy promising notifications; an implementation needs runtime per OS | — (no-claim) | G2.3 |
| Autostart | Opt-in only, off by default: Windows `HKCU\…\CurrentVersion\Run`, Linux `~/.config/autostart/*.desktop`, macOS `~/Library/LaunchAgents/*.plist`; autostart launches hidden to tray where a tray exists, and never starts a second engine; settings toggle reflects the real OS state and removes the entry on disable/uninstall | Unit-test the pure path/content builders + create/remove in a temp dir (H); runtime: enable, log out/in, confirm hidden start on each OS | **H** + **R** (login) | G1.7 |
| Updater UX | Windows path unchanged; Linux/macOS tell the truth: detect the install channel (shell-installer receipt vs AUR/package manager vs bare tarball), show the version and the exact update action for that channel, never draw an install button that cannot act, never surface a raw English diagnostic as copy; UAC copy only on Windows | Unit-test the channel→copy/action decision per cfg; headless smoke opens the update sheet with a fixture; runtime click-through on each OS | **R** each OS | G1.6 |
| Admin / elevation | Separate two facts: `boost requires privilege` (engine truth, per OS) and `this process is privileged`. Gate only the boost action. Linux: probe effective `CAP_NET_ADMIN`/rule-transaction ability rather than `uid == 0`, elevate through `pkexec`, forward the user's config/data paths; macOS: keep the OS authorization prompt but scope it (config paths forwarded; document the root-app trade-off or design a helper); platform-adaptive copy ("Restart as administrator" is Windows wording) | Unit-test the capability parsers (pure); headless: capability probe on the Linux runner; runtime: Linux/macOS unelevated → authorize → boost + boosted screenshot. **Windows: the Needs-elevation overlay and the UAC-decline branch are U** — fixture: an interactive verifier on a **non-elevated standard-user session** launches the installed/debug `lightspeed-gui.exe`, presses the boost action and clicks **No** at the UAC prompt, then confirms the window stays on the Needs-elevation overlay instead of exiting (defect C8; the spec's own §2.2 state 10 records "the interactive UAC-decline path is **U** on this host", `docs/design/gui-refresh-spec.md` §2.2). This agent shell is elevated and the prompt is on the secure desktop, so no agent tool can produce it. | **R** each OS; Windows UAC-decline **U** until the fixture runs | G1.2, G1.3 |
| Single instance | Same semantics on all OSes; the non-Windows guard's tests run in CI; second-launch notice is visible on a desktop-launched app (native dialog or raise-existing-window, not `eprintln!`) | The existing non-Windows tests executed in the new Linux/macOS GUI job (H); runtime second launch on each OS | **H** + **R** | G1.8 |
| Paths / logs | Real per-OS paths in comments and docs (macOS Application Support); crash/fatal errors visible to a desktop user on all OSes (native alert or a "last run failed" surface), with the log reachable from Settings; `open`/`xdg-open` verified | Unit tests for path composition per cfg; headless: force a panic and assert the crash log; runtime: open-log on each OS | **H** + **R** | G1.9 |
| Packaging / install | Keep the published channels truthful: cargo-dist artifacts for Linux x86_64 + macOS both arches; AUR manifest in sync; decide `.app`/`.dmg` (ad-hoc signed, documented Gatekeeper workaround) or keep the bare tarball + shell installer; no paid notarization; aarch64 Linux GUI either added or documented as absent | Artifact existence + install/launch on each OS (R); packaging changes are `dev-ops`' write scope | **B** + **R** | G2.4 |
| Capture probe | One probe, one truth: the core/engine capability answer (`backend`, `available`, `requires`), shown in Settings for every OS with an OS-correct name and the privilege requirement; the GUI stops shelling `sc`/`which`; "Npcap" appears only if `pcap-capture` is actually in the build | Unit-test the DTO/probe mapping per cfg; headless: probe on the Linux runner; runtime: Settings screenshot per OS | **H** + **R** | G1.1 |

### 3.3 Core non-capture flows the engine already supports

Parity is not only the surfaces above. These flows already have cross-platform
implementations in the shared crates, so the Linux/macOS gap is **presentation
and platform shell**, not engine work:

| Flow | Engine state | Consequence for W2 |
|---|---|---|
| Relay discovery / probing | `reqwest` with `native-tls` → schannel / Security.framework / OpenSSL per OS (`client-gui/Cargo.toml:34-38`); health probing in `client-gui/src/discovery.rs` is plain HTTP | Should work on all OSes; needs a headless launch + discovery log line to prove DNS/TLS per OS (H) |
| Game detection | Cross-platform scanner: Windows `tasklist`/`netstat`, Linux `/proc` + `ss`, macOS `ps` + `lsof` (`client/src/interceptor/process_scanner.rs:9-18`); process enumeration likewise (`client/src/processes.rs:7-22`) | Presence worker is a GUI wrapper (`client-gui/src/app/presence.rs`); verify one scan on Linux/macOS in a headless run with a synthetic game process (H) |
| Config / status / commands | Shared engine API consumed today (`client/src/lib.rs`); the GUI owns a second config schema (`client-gui/src/config.rs`) which `architect` is removing (task-1) | Platform work must not entrench the GUI-owned schema; admin/updater/platform capability facts should be published by the shared core (dependency §8) |
| Capture / boost | Engine backends exist per OS: WinDivert (Windows), nftables/iptables + `CAP_NET_ADMIN` (Linux), `pfctl` + root (macOS) (`client/src/interceptor/mod.rs:110-130`; `client/src/interceptor/linux.rs:24-27`; `client/src/interceptor/macos.rs:13-16`) | Boost parity follows the engine and needs an elevated runtime run per OS; **not** promised by this plan (brief §4, §13.3) |
| Client route trace (delta) | New portable module `client-gui/src/client_trace.rs` (1,326 lines) spawns the OS tool — `tracert` on Windows, `traceroute -n -q 1` elsewhere (`client-gui/src/client_trace.rs:20-29,349-375`); display-only location hint, bounded run, never feeds relay ranking | Both arms implemented; Linux/macOS runtime needs `traceroute` in PATH and is **B** only until a headless/runtime run. The two target-conditional arms are process invocations, not a gated module (delta note §3.4) |

### 3.4 Delta check — rebase onto `origin/master` `76aba5d` (2026-11-20)

The W1 design set was rebased from `62c6cf6` onto `origin/master` `76aba5d`
(16 commits; the W1 docs are commit `0df57ce` on top). The delta brings master's
code and docs, not a platform rewrite. This section records what it does and does
not change for parity; the three-tier rule (§2, §7) is unchanged, and the delta
adds no Linux/macOS runtime evidence and promotes no surface.

**Platform matrix: unchanged.** The parity-relevant platform code is the same
kind as r3: `client-gui/src/platform/{linux,macos}.rs` are untouched,
`single_instance.rs`, `paths.rs`, `update.rs` and `self_update.rs` are unchanged,
and no shortcut, notification or autostart code appeared (grep: zero hits). Two
deltas touch a platform surface, and neither changes a matrix row's capability:

- `client-gui/src/platform/windows.rs` gains the `TrayState::RelayStarved` colour
  and tooltip (`client-gui/src/platform/windows.rs:137-158`) so a starving relay
  never shows the green optimizing icon. The tray mechanism and menu are
  unchanged; the state vocabulary grows by one (`client-gui/src/app.rs:70-81`,
  pure selector `client-gui/src/app/window.rs:133-159`). The Linux/macOS stubs
  still ignore `set_state` (S1/S3), so the §3.1 tiers stand.
- the engine's Windows data-plane work (`client/src/interceptor/windows.rs`,
  `client/src/interceptor/order.rs`, the starvation counters) is **Windows-only**
  and adds no Linux/macOS backend; capture/boost parity stays reported separately
  and unclaimed (§3.3).

**New GUI modules are portable Rust.** `server_place` (595 lines),
`server_tables` (139), `server_geofeed_tables` (327), `client_trace` (1,326),
`detection_provider` (184) and `app/choice` (540) are ordinary modules registered
in `client-gui/src/main.rs:9-27` and `client-gui/src/app.rs:36`; five carry no
target `cfg` at all beyond `#[cfg(test)]`. The one exception is `client_trace`,
which has two `#[cfg]` arms around the OS-tool invocation only — `tracert` on
Windows, `traceroute -n -q 1` elsewhere (`client-gui/src/client_trace.rs:349-375`),
both implemented and using the cross-platform `silent_command` wrapper. That is a
platform-conditional process call, **not** a target-gated module: the module
compiles and runs on every target, and the Linux/macOS runtime dependency is only
that `traceroute` exists in `PATH` (B only until run, §3.3). The
detection-provider classifier is installed cross-platform through the engine hook
(`client-gui/src/main.rs:85-86`); the location tables are static data with no OS
calls.

**CI and packaging: unchanged.** `.github/workflows/` and `client-gui/Cargo.toml`
did not change between `62c6cf6` and `76aba5d`, and neither did `dist-workspace.toml`
or the GUI dist metadata, so the `linux-gui`/`macos-gui` ask (§6.1) and the
packaging rows stand. The only packaging delta is the 1.11.4 version bump in the
AUR/Chocolatey/Scoop manifests, and v1.11.4 publishes the same GUI asset shape —
Linux x86_64 tarball, both macOS GUI tarballs, the shell installer and the
Windows MSI/zip — `wat/evidence/2026-11-20/team/desktop-platform/release-assets-1114.txt`.

**Design set re-baselined (delta wave; fix tasks 23/25).** The committed set
carried the W1 360/440/1120 plan only until the delta wave re-cut it. The spec is
now **mobile-only** — width pinned 320 px, one `narrow` column, height
user-resizable 420–1400 (initial 760) — in §1.1 (spec `docs/design/gui-refresh-spec.md:45-55`,
the 559-line delta revision; evidence
`wat/evidence/2026-11-20/team/interface-designer/delta-r13.md`), and
`design-system-2026.md` §5.4 re-cuts the metrics and breakpoints to the single
320 px layout (evidence
`wat/evidence/2026-11-20/team/design-system/delta-r11.md`). The UX audit is
re-baselined in the same wave (`wat/evidence/2026-11-20/team/ux-lead/delta-r10.md`).
The follow-up fix tasks 23 (design-system measurements/citations) and 25 (RU copy
+ audit count) correct measurement and copy only: **neither changes the
`linux-gui`/`macos-gui` CI ask (§6.1) nor the three-tier rule (§2, §7)** — both
stand exactly as written. `interface.md` remains pre-decision until W2-2 applies
the spec's mobile-only amendments, so this plan still names the owner decision
(`docs/design/gui-refresh-brief.md:323`) as the geometry authority beside the
re-baselined spec.

---

## 4. Stub inventory (file:line)

Every item is a placeholder that looks like an implementation, or a piece of
truth that exists only for Windows. Ordered by the surface it breaks.

| # | Stub / gap | Where | What the code actually does |
|---|---|---|---|
| S1 | Linux tray is a no-op | `client-gui/src/platform/linux.rs:10-19` | `poll_events` → empty, `set_state` → `{}`; `new_tray` still returns `Some(LinuxTray)` (`client-gui/src/platform/linux.rs:30-32`), so "exists" and "works" differ |
| S2 | Linux `has_system_tray` fixed false | trait default `client-gui/src/platform/mod.rs:104-106`; Linux does not override | Close exits; "Hide to tray" hidden (`client-gui/src/app.rs:4178-4186`, `client-gui/src/app/window.rs:161-178`) |
| S3 | macOS tray is a no-op | `client-gui/src/platform/macos.rs:13-21,29-34` | Same shape as S1; module doc says "no menu-bar integration yet" (`client-gui/src/platform/macos.rs:4,12`) |
| S4 | macOS backend never executed | `client-gui/src/platform/macos.rs:3,25`; `client-gui/src/platform/mod.rs:7` | Says so itself; no CI job builds/lints/tests it on macOS (`.github/workflows/ci.yml:219-233`) — release build is the only compile proof |
| S5 | Admin = `uid == 0` on Linux | `client-gui/src/platform/linux.rs:34-46` | Root is not the desktop norm; ignores `CAP_NET_ADMIN`, group/`sudo`/polkit models; gates the whole boost action (`client-gui/src/app/ledger.rs:39-57`) |
| S6 | Admin = `uid == 0` on macOS | `client-gui/src/platform/macos.rs:36-48` | Same; while the engine needs root for `pfctl` (`client/src/interceptor/macos.rs:15`) |
| S7 | Elevation relaunch loses user config on Linux | `client-gui/src/platform/linux.rs:72-98` + `client-gui/src/paths.rs:34-43` | `pkexec` scrubs `HOME`/`XDG_*`; only `DISPLAY`, `XAUTHORITY`, `LIGHTSPEED_PROXY*` forwarded → elevated app reads root's `config.toml`; `WAYLAND_DISPLAY` deliberately dropped (forces XWayland) |
| S8 | Elevation relaunch loses user config on macOS | `client-gui/src/platform/macos.rs:74-89` | `do shell script … with administrator privileges` runs a root GUI with a minimal environment; `~/Library/Application Support/light-speed` resolves under root's home |
| S9 | Capture probe checks the wrong dependency (Windows) | `client-gui/src/platform/windows.rs:240-246`; `client-gui/src/app.rs:316-320,3512-3523` | `sc query npcap` for a build that uses WinDivert, not Npcap (`client-gui/Cargo.toml:15,65-67`) |
| S10 | Capture probe checks the wrong dependency (Linux) | `client-gui/src/platform/linux.rs:48-63` | `which tcpdump/dumpcap` for an engine that needs `nft`/`iptables` + root (`client/src/interceptor/linux.rs:68-80`) |
| S11 | Capture probe checks the wrong dependency (macOS) | `client-gui/src/platform/macos.rs:50-65` | `which tcpdump/dumpcap` for an engine that needs `pfctl` + root (`client/src/interceptor/macos.rs:13-16,140`) |
| S12 | Settings draws a Windows-only "Npcap" row on every OS | `client-gui/src/app.rs:3466-3481,3512-3523` | Literal, untranslated (`docs/design/interface.md:217,231` accepts that for now); false on Linux/macOS |
| S13 | macOS/Linux updater has no action | `client-gui/src/update.rs:210-213`; `client-gui/src/self_update.rs:27-34` | No installer asset, so no Install button; the only message is the raw English error (`client-gui/src/update.rs:234-240`) |
| S14 | Crash/fatal dialog is Windows-only | `client-gui/src/crash.rs:124-158` | Non-Windows: no dialog; `client-gui/src/main.rs:35-44` writes the log and exits 1 — a desktop launch shows nothing |
| S15 | Second-instance notice is not visible on non-Windows | `client-gui/src/single_instance.rs:84-88,215-249` | `eprintln!` (no console) + an eframe window that is itself untested on Linux/macOS |
| S16 | Non-Windows single-instance tests never run | `client-gui/src/single_instance.rs:280-315` | `#[cfg(not(windows))]` tests are compiled only off Windows; no job executes them |
| S17 | Locale follows only `LANG`/`LC_*` | `client-gui/src/i18n.rs:256-274` | A Finder/Explorer-launched GUI inherits neither; macOS source of truth is `AppleLanguages`, Windows the user default locale — 9 shipped catalogs can be unreachable by default |
| S18 | Window geometry not remembered | `client-gui/src/main.rs:109-115`; `client-gui/src/app.rs:4282-4289` | Width pinned 320 px (confirmed product form); height auto-sized from content within 420–1400; no remembered height or position, no monitor-visibility restore |
| S19 | No shortcut layer | grep: no shortcut/keymap code in `client-gui/src` | No ⌘/Ctrl bindings, no menu accelerators, no Esc/Enter rules beyond egui defaults |
| S20 | No autostart anywhere | grep: zero hits (C2) | Not a stub with a body — a missing surface, identical on all OSes |
| S21 | No `.app`/`.dmg` in CI | `tools/package-macos.sh:1-38`; `packaging/macos/Info.plist`; no workflow references `package-macos` | Release ships a bare macOS binary in a tarball; the bundle builder is manual and version-argumented (`docs/user-guide.md:112-114` still shows `1.6.5`) |
| S22 | Stale platform comments/doc | `client-gui/Cargo.toml:8,74-78`; `dist-workspace.toml:16-17`; `client-gui/src/main.rs:1-4`; `client-gui/src/paths.rs:9-16,34-43`; `AGENTS.md:99` | Say "Windows app", "no macOS module", "Linux stub", "~/.local/share elsewhere" where the tree/release disagrees |
| S23 | User docs claim unverified runtime | `docs/user-guide.md:31` ("GUI works on Linux") | Must be tiered per §7 until a Linux run exists |

---

## 5. Gap-closure plan (ordered: crash/build → core flow → conventions)

Sequencing rule from the persona contract: **a build/crash defect blocks every
other platform claim; a core non-capture flow blocks parity; conventions last.**
Each item names its write scope note — `dev-ops` owns workflows/packaging, the
W2 implementer owns code (`desktop-platform` for `client-gui/src/platform/` + copy, `interface-designer`
for the shell, `architect`/`rust-dev` for core DTOs).

**W2 v1 sequencing is the spec's §5, not this document's phase order.** The spec's
`W2-4 "Parity"` scope names this plan's **G0.1, G0.2, G1.1–G1.9 and G2.1**
(`docs/design/gui-refresh-spec.md` §5). The three Phase 2 items that scope leaves
out are named where they land here, so no item is silently unsequenced:

- **G2.2** (window geometry + per-OS 200 % proof) ships **with the shell parity**,
  not with W2-4. Per the owner's 2026-11-20 decision the target is the **shipped
  mobile-only form**: width pinned **320 px**, single column, height user-resizable
  **420–1400** with initial **760** — the 360/440/1120 two-column plan is
  withdrawn (owner decision `docs/design/gui-refresh-brief.md:323`; current spec
  `docs/design/gui-refresh-spec.md:45-55`). The shell slice owns
  remembered/clamped geometry and the parity captures **320×420, 320×760 and
  320 @ 200 %** (spec §5 W2-2, §4 shell row — re-baselined to mobile-only in
  §1.1; §3.4), and `desktop-platform` co-owns the per-OS proof.
- **G2.3** (notifications) is an owner **no-claim decision**
  (`docs/design/gui-refresh-spec.md` §7 Q7; owner confirmed 2026-11-20,
  `docs/design/gui-refresh-brief.md:327`) — no W2 slice.
- **G2.4** (`.app`/`.dmg`, aarch64 Linux GUI, channel truth) is an owner/
  `dev-ops` decision **outside W2 v1 unless the owner opts in**
  (`docs/design/gui-refresh-spec.md` §7 Q6; owner confirmed the bare
  tarball/shell-installer path on 2026-11-20, `docs/design/gui-refresh-brief.md:326`).

Windows elevation evidence follows the same rule: the needs-elevation **overlay**
and the **UAC-decline** branch are **U** on this host (fixture in §3.2), the spec's
own §2.2 state 10 records the same `U`, and no row may claim **R** for them; the
Windows handshake implementation itself is spec-owned (§2.2 state 10, §5 W2-4
acceptance).

The spec's `W2-4` scope list (G0.1, G0.2, G1.1–G1.9, G2.1 plus the reduced-motion
Windows signal) is unchanged by the rebase. The delta wave has now re-baselined
the spec's §1.1 to the mobile-only form (`docs/design/gui-refresh-spec.md:45-55`),
so the geometry has a current design-set citation as well as the owner decision
(`docs/design/gui-refresh-brief.md:323`); §5 stays authoritative for scope, and
where a doc is still moving the section name is cited beside the line (§1
quote-drift caveat).

### Phase 0 — crash/build (P0)

| ID | Gap | Implementation approach | Verification method | Tier target | Owner / dependency |
|---|---|---|---|---|---|
| **G0.1** | No PR-time Linux/macOS GUI build, lint or test (C1, S4) | Add the two CI jobs in §6.1 to `ci.yml`; required checks from the first green run. First run will likely surface dead-code/`unused` warnings in the stub backends under `RUSTFLAGS=-Dwarnings` (`.github/workflows/ci.yml:15`) — fix the code, never widen an allow | The jobs themselves: build + clippy + test green on a PR | **B** (+ **H** if G0.2 lands) | `dev-ops` (workflows); `desktop-platform` reviews lint fallout |
| **G0.2** | No deterministic way to prove a window was created on Linux/macOS (S4) | Add a test-only launch mode (`LIGHTSPEED_GUI_SMOKE=1`): skip engine/discovery/tray work, open the real window, log `title`, `pixels_per_point` and the backend, close cleanly, exit 0 (bounded by a timeout). Keep it env-gated so shipped behaviour is unchanged | `HEADLESS` smoke step in both jobs (§6.1): exit 0 + expected log line; Linux under `xvfb-run` with `LIGHTSPEED_GUI_RENDERER=glow` and `LIBGL_ALWAYS_SOFTWARE=1` | **H** (WindowServer permitting on macOS) | `desktop-platform` (code) + `dev-ops` (job step) |
| **G0.3** | A startup crash on Linux/macOS is silent (S14) | Native alert on non-Windows: `zenity`/`kdialog` on Linux, `osascript display dialog` on macOS, falling back to an in-window "last run failed" notice on next start; log path unchanged | Unit-test the message composition; headless: force a panic via env and assert log + exit code; runtime: trigger a fatal on each OS | **H** + **R** | `desktop-platform`; screenshot for R |
| **G0.4** | Stale platform truth in comments/docs (C4, C5, S22, S23) | Correct `client-gui/Cargo.toml:74-78`, `dist-workspace.toml:16-17`, `client-gui/src/main.rs:1-4`, `paths.rs` path docs, `AGENTS.md:99`; retier the user docs per §7 (no behaviour change) | Docs gate: links + claims checked; `cargo metadata` unchanged; release assets unchanged | — | `desktop-platform` + `dev-ops` (packaging comments); lead integrates |

### Phase 1 — core non-capture flows (P1)

| ID | Gap | Implementation approach | Verification method | Tier target | Owner / dependency |
|---|---|---|---|---|---|
| **G1.1** | Capture/admin/updater capability facts are Windows-shell guesses (S9-S12) | Move the probe into the shared core (architect's boundary, task-1): one DTO `{backend, available, requires}` from `TrafficInterceptor::check_availability()` + a privilege requirement; the GUI renders it; delete `Platform::is_capture_available` and the `sc`/`which` shell-outs; "Npcap" row only if `pcap-capture` is compiled in (never in the shipped GUI) | Core unit tests (backend/requires per cfg) + GUI mapping test; headless: Linux runner probe; runtime Settings screenshot per OS | **H** + **R** | `architect`/`rust-dev` (DTO), `desktop-platform` (GUI + `client-gui/src/platform/`), `interface-designer` (`docs/design/interface.md:217` row change is a contract edit) |
| **G1.2** | Linux admin/elevation is root-only and loses config (S5, S7) | Replace `uid == 0` with a capability probe (`CAP_NET_ADMIN` from `/proc/self/status` + a non-destructive `nft`/`iptables` transaction as `check_availability` already does, `client/src/interceptor/linux.rs:68-80`); keep `pkexec` for the actual raise but forward `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_RUNTIME_DIR`, `WAYLAND_DISPLAY` (or prefer a scoped helper); platform-adaptive action copy | Pure parser/command builder tests; headless: capability probe on the Linux runner (expect "not privileged" → honest label); runtime: unelevated → elevate → boost, config survives | **H** + **R** | `desktop-platform`; copy needs `ux-lead`/i18n keys |
| **G1.3** | macOS admin/elevation runs the whole GUI as root and loses config (S6, S8) | Keep the OS authorization prompt, but scope it: pass explicit config/data paths into the authorised instance; document that boost needs authorization; do not require the user to launch the app as root; long-term a privileged helper (SMJobBless) is out of budget, so record the trade-off | Pure command-builder tests; build/clippy/test in the macOS job; runtime on owner hardware: authorize → boost → config intact | **B** + **R** | `desktop-platform`; runtime verifier needed (§6.3) |
| **G1.4** | macOS has no tray/status item and no menu bar (S3) | Build `MacosTray` on `tray-icon` (already in the tree but target-gated to Windows — `client-gui/Cargo.toml:65-67`): NSStatusItem with template icon + menu (Show / Connect / Disconnect / Quit), tooltip/state word, double-click-show, `has_system_tray()` from real creation; window X hides only when it is real. Enable the dependency for `cfg(any(windows, macos))` (no GTK path on macOS) | macOS GUI job: build + clippy + test; headless smoke: process alive with status item (no crash) where WindowServer allows; runtime: menu interaction + hide/restore screenshot | **H** + **R** | `desktop-platform`; dependency surface needs `sec-ops` audit pass |
| **G1.5** | Linux has no tray and no honest no-tray mode (S1, S2) | Prefer StatusNotifierItem over D-Bus (`ksni`-class, pure Rust, no GTK) so GNOME-with-extension and KDE get a tray; `has_system_tray()` = real SNI registration; GNOME without the extension → no tray, close exits, no "Hide to tray" copy, docs say so per DE. `tray-icon`-on-Linux is the fallback and needs a GTK/glib audit decision (`client-gui/Cargo.toml:61-64` records why it was excluded; `docs/security-advisory-review-2026-11-18.md:86-101` shows the GTK/zbus advisory chain) | Linux GUI job: build + clippy + test; headless: tray-creation branch unit-tested, process does not crash without a session bus; runtime: GNOME/KDE icon + menu | **H** + **R** | `desktop-platform` + `sec-ops`/`dev-ops` dependency decision |
| **G1.6** | Updater UX lies by omission on Linux/macOS (S13) | Channel detection: shell-installer receipt → "update with the installer/updater"; AUR → `yay -S lightspeed-bin`-style instruction; bare tarball → releases link; Windows keeps the in-place path. `UpdateStatus` gains the channel; no install button unless an installer exists; no raw English error as copy (translate keys; keep the diagnostic in the log) | Unit tests for channel→(copy key, action) per cfg; headless smoke opens the sheet with a fixture on Linux/macOS; runtime click-through per OS | **H** + **R** | `architect` (DTO field), `desktop-platform`, `interface-designer` (copy), i18n keys ×9 |
| **G1.7** | No autostart on any OS (C2, S20) | Opt-in Settings switch; pure per-OS writers: `HKCU\…\Run` (Windows), `~/.config/autostart/lightspeed-gui.desktop` (Linux, with `X-GNOME-Autostart-enabled=true`), `~/Library/LaunchAgents/dev.lightspeed.gui.plist` (macOS); write/remove are pure functions returning paths+content; launch with a hidden mode (`--autostart` → start hidden when a tray exists, otherwise show); disable and uninstall remove the entry | Unit tests: builders + temp-dir create/remove (H); runtime: enable → log out/in → hidden start, disable → no start, on each OS | **H** + **R** | `desktop-platform`; packaging/uninstall belongs to `dev-ops` |
| **G1.8** | Non-Windows single-instance is untested and its notice is invisible (S15, S16) | Let G0.1's Linux/macOS jobs execute the existing `cfg(not(windows))` tests (no code change needed); replace `eprintln!` notice with a native dialog (or raise the existing window via a wake channel); keep the lock-file degradation behaviour | Linux/macOS GUI job runs the two lock tests (H); runtime: launch twice on each OS and read the notice | **H** + **R** | `desktop-platform` |
| **G1.9** | macOS path docs are wrong; locale detection is env-only (S17, S22) | Fix `paths.rs` comments and per-OS doc; read the OS language properly: macOS `AppleLanguages` (defaults), Windows `GetUserDefaultLocaleName`, Linux `LANG`/`LC_*`; 9 shipped catalogs still honoured; English fallback documented | Unit tests for the locale-resolution order (pure) per cfg; headless: smoke logs the resolved language on each OS; runtime: switch OS language and relaunch on each OS | **H** + **R** | `desktop-platform` (locale may be `architect`/core if it moves) |

### Phase 2 — conventions (P2)

| ID | Gap | Implementation approach | Verification method | Tier target | Owner / dependency |
|---|---|---|---|---|---|
| **G2.1** | No shortcut layer, no menu accelerators, no Esc/Enter rules (S19) | One keymap table per OS (pure data): ⌘Q/⌘,/⌘H vs Ctrl+Q/Ctrl+,; menu-bar items carry the accelerators (macOS/Windows); Esc closes the top sheet, Enter confirms the sheet's primary action; shortcut hints in tooltips/footer | Unit-test the keymap tables + Esc/Enter decision per OS; runtime keyboard-only pass with screenshots | **R** | `desktop-platform` + `interface-designer` |
| **G2.2** | Window geometry not remembered, no monitor restore, no 200 % proof (S18) | Keep the **owner-confirmed mobile-only shipped form** (`docs/design/gui-refresh-spec.md:45-55`; `docs/design/design-system-2026.md` §5.4; owner decision `docs/design/gui-refresh-brief.md:323`): width pinned **320 px**, single column, height user-resizable **420–1400** with initial **760**; add remembered height + position in UI preferences, clamp to the visible monitor work area on restore, and prove **320 px @ 200 %** with no clipping. The 360/440/1120 two-column plan is withdrawn; parity captures are **320×420, 320×760 and 320 @ 200 %**. The re-baselined design set states the same numbers (spec §1.1, design-system §5.4) | Unit-test clamp/persistence logic (pure); headless: smoke asserts remembered geometry; runtime three captures per OS — 320×420, 320×760, 320 @ 200 % | **R** | `interface-designer` (shell, W2-2) + `desktop-platform`; delivered with the shell parity, not W2-4 (§5 sequencing) |
| **G2.3** | Notifications surface (none) | **Decision needed (recommended: explicit no-claim)** — no OS notifications in this refresh; the window + tray state is the notification. If implemented later: actionable only + opt-in; macOS needs a signed bundle; Linux D-Bus; Windows toast. A no-claim must be written down so no copy promises notifications | No-claim: reviewed copy + docs; implementation: runtime per OS | — / **R** | lead/owner decision; `desktop-platform` |
| **G2.4** | macOS packaging has no bundle; aarch64 Linux GUI absent; Linux channels are AUR-only (S21) | `dev-ops` decision with the owner: (a) keep bare tarball + shell installer, document Gatekeeper (`docs/install-macos.md:106-114`); or (b) build an ad-hoc-signed `.app`/`.dmg` in the macOS job from `tools/package-macos.sh` (no paid cert; users right-click-Open). Add an aarch64 Linux GUI target or document the gap; keep AUR in sync via the release bump jobs | Artifact existence + install/launch per OS (B + R); `dist plan` diff reviewed by `dev-ops` | **B** + **R** | `dev-ops`; owner gate for any channel decision |

**Ordering rationale.** G0.x are the only items that can turn a broken platform
into a *visible* break: today a Linux/macOS GUI break is invisible until a tag,
and a Linux/macOS startup crash is invisible to the user. G1.x are the
non-capture flows the owner made mandatory (capability truth, elevation,
tray/menu, updater, autostart, single instance, paths). G2.x are conventions
that make it feel native; they are last because none of them can be verified
until the OS runs at all. **No item in Phase 1/2 is in scope for W1.**

---

## 6. External-verification ask (exact)

### 6.1 CI jobs to add (`dev-ops` owns `.github/workflows/`; not edited here)

Required: both jobs below are PR/push gates. They mirror `windows-gui`
(`.github/workflows/ci.yml:301-324`) and reuse the existing action pins and toolchain
(`1.100.0`, `.github/workflows/ci.yml:23-26`). System packages for the Linux GUI build already
exist in `dist-workspace.toml:42-51` (the release proves they are sufficient).
The delta wave's doc re-baselines — including the fix tasks 23 (design-system
measurements/citations) and 25 (RU copy and audit counts) — change measurement,
citations and copy only; they change neither these jobs nor the three-tier rule,
so the ask below is unchanged from r4.

```yaml
  linux-gui:
    name: Linux GUI Build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@<same pin as ci.yml>
      - uses: dtolnay/rust-toolchain@<same pin as ci.yml>
        with: { toolchain: 1.100.0, components: clippy }
      - uses: Swatinem/rust-cache@<same pin as ci.yml>
      - name: Install GUI system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libpcap-dev libxkbcommon-dev libxkbcommon-x11-0 \
            libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
            libgl1-mesa-dev libgl1-mesa-dri libgtk-3-dev libssl-dev \
            xvfb xdotool
      - name: Build GUI
        run: cargo build -p lightspeed-gui
      - name: Clippy GUI
        run: cargo clippy -p lightspeed-gui --all-targets
      - name: Test GUI
        run: cargo test -p lightspeed-gui
      - name: Headless smoke (advisory until it proves stable)
        env:
          LIGHTSPEED_GUI_SMOKE: "1"
          LIGHTSPEED_GUI_RENDERER: glow
          LIBGL_ALWAYS_SOFTWARE: "1"
        run: timeout 120 xvfb-run -a --server-args="-screen 0 1280x1024x24" ./target/debug/lightspeed-gui
```

```yaml
  macos-gui:
    name: macOS GUI Build
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@<same pin as ci.yml>
      - uses: dtolnay/rust-toolchain@<same pin as ci.yml>
        with: { toolchain: 1.100.0, components: clippy }
      - uses: Swatinem/rust-cache@<same pin as ci.yml>
      - name: Build GUI
        run: cargo build -p lightspeed-gui
      - name: Clippy GUI
        run: cargo clippy -p lightspeed-gui --all-targets
      - name: Test GUI
        run: cargo test -p lightspeed-gui
      - name: Headless smoke (advisory; depends on the runner's WindowServer session)
        env: { LIGHTSPEED_GUI_SMOKE: "1" }
        run: timeout 120 ./target/debug/lightspeed-gui
```

Notes for `dev-ops`:

- **Cost:** standard GitHub-hosted runners are free for a public repository, so
  the macOS multiplier does not introduce spend; `[COST_STUB]` stays $0.00.
  Nothing here needs a paid runner or service (`.github/workflows/ci.yml` is the
  only file that changes).
- **Build step is regression protection, not new proof:** the release already
  builds these targets (§1 C1). The new value is clippy + tests + PR-time
  failure, not "first build".
- **The first Linux/macOS clippy run will be noisy:** `RUSTFLAGS=-Dwarnings`
  (`.github/workflows/ci.yml:15`) means dead-code/unused warnings in the stub backends fail the
  job. Fix the stubs; do not add blanket allows.
- **Smoke gating:** keep the smoke step `continue-on-error: true` until it has
  passed on ~10 consecutive runs (particularly macOS window creation), then
  promote it; never mark a job green by deleting the smoke step silently.
- **Test count is the evidence:** report the per-OS GUI test count on the first
  run; a Linux run with zero tests is a red flag (the non-Windows
  single-instance tests are the ones that must appear).

### 6.2 Is a headless smoke path feasible on GitHub-hosted runners?

| OS | Feasible? | Why / how | Risk |
|---|---|---|---|
| Linux | **Yes, high confidence.** | `xvfb-run` gives an X server; eframe's `glow` renderer + `LIBGL_ALWAYS_SOFTWARE=1` + `libgl1-mesa-dri` (llvmpipe) creates a window without a GPU. The real window title can be asserted with `xdotool search --name '^LightSpeed$'` when the smoke hook is not used. | GL/EGL under Xvfb occasionally fails on a runner image update; the env override (`client-gui/src/main.rs:238-258`) and `glow` fallback keep it recoverable. |
| macOS | **Unproven — must be tested, then either promoted or replaced by owner hardware.** | GitHub-hosted macOS runners are headless but normally have an Aqua/WindowServer session available; many GUI apps create windows there. There is no `xvfb` equivalent, so the only probe is the app itself (smoke hook exit code / log line), not an external window search. | If the runner has no WindowServer session, the app cannot create a window and the smoke must not be a required check; runtime verification then needs a real Mac. |
| Windows | Already runtime-verified today; the existing GUI jobs are unchanged. | `windows-gui` (`.github/workflows/ci.yml:301-324`) already builds/lints/tests. | — |

The smoke hook (G0.2) is what makes either row deterministic: without it, CI can
only observe "process did not crash before the timeout", which is weaker than
"window created with the expected title".

### 6.3 What this Windows host cannot provide (the hardware/runner ask)

This host can read code and run Windows. It **cannot** produce, and no amount of
local work converts into, any of the following:

1. **A Linux or macOS interactive session** — no display server, no VM, no
   remote machine. Every **R**-tier item in §3.2 (tray interaction, shortcuts,
   200 % scaling, elevation→boost, autostart login, updater click-through,
   install/launch) is unproducible here.
2. **Owner hardware or a self-hosted runner** — needed for **R** on Linux and
   macOS, and the only reliable alternative if the macOS hosted smoke proves
   infeasible. Registration is an owner action; a self-hosted runner uses no paid
   service, so `[COST_STUB]` is respected.
3. **The ability to trigger or read CI runs** — workflow edits and CI
   administration are `dev-ops`' write scope; this host can only hand over the
   job spec in §6.1.
4. **A macOS bundle, Developer ID or notarization** — a paid Apple program;
   explicitly out of budget. The realistic target is an ad-hoc-signed `.app` with
   the documented right-click-Open path (`docs/install-macos.md:106-114`).
5. **aarch64 Linux hardware/CI proof** — the release does not build an aarch64
   Linux GUI (`dist/aur/PKGBUILD:9-10`), and this host cannot cross-run one.
6. **Live game + capture runs with elevation** — unchanged from the existing
   limitation (brief §11); boost parity is reported separately from these
   surfaces.
7. **A non-elevated Windows session and a human UAC decliner** — this agent shell
   is elevated (measured) and the UAC prompt is on the secure desktop, which
   automated input cannot drive. The Needs-elevation overlay and the
   **UAC-decline** branch are **U** and need an interactive verifier on a
   standard-user account (fixture in §3.2).

Minimum external ask, in priority order: (a) merge the two jobs (§6.1) and post
their first-run logs; (b) tell us whether the macOS smoke step can create a
window on `macos-latest`; (c) for **R**-tier claims, name which OSes will have an
owner machine or self-hosted runner and who runs the interaction scripts. Without
(b) and (c), the parity status for every surface below Windows stays **B**
(or **H** where the smoke lands).

---

## 7. Three-tier evidence statement schema

### 7.1 Definition and statement templates

Every claim about a non-Windows OS in a report, PR, state entry, release note or
user doc uses one of these sentences, naming the run and the evidence path:

| Tier | Required statement form | Must name |
|---|---|---|
| **Build-verified** | "Build-verified on `<OS>` in `<workflow/run id or local command>` at `<commit>`: `cargo build/clippy/test -p lightspeed-gui` exited 0 with `<counts>`. No window was created." | OS, commit, exact commands, counts |
| **Headless-verified** | "Headless-verified on `<OS>` in `<run id>`: the binary created a window titled `<title>` in a non-interactive session and exited 0 via the smoke hook at `<commit>`; `<what the probe asserted>`. Tray/menu/DPI/elevation/autostart/network flows were **not** exercised." | run id, probe, what it asserted, what it did not |
| **Runtime-verified** | "Runtime-verified on `<OS version, DE/GPU>` by `<who>` at `<commit>`: `<screenshot/UIA/log paths>` show `<state(s)>`; `<flows>` are covered. Other states are named unverified." | OS+DE/GPU, verifier, evidence paths, covered states, uncovered states |

A claim with no tier and no evidence path is not a claim; it is a plan.

### 7.2 Per-surface minimum tier (from §3.2)

| Surface | Minimum for a "parity" sentence | Notes |
|---|---|---|
| Window chrome, DPI, shortcuts | **R** on all three | Visual/interaction facts only a session can prove |
| Tray / menu bar | **H** for creation, **R** for interaction | A created status item is not a working menu |
| Notifications | no-claim (documented) | If ever implemented: **R** |
| Autostart, updater UX, admin/elevation, capture probe, paths/logs | **H** for the pure decision, **R** for the user-visible flow | The decision logic can be tested; the flow cannot. The Windows UAC-decline branch is **U** on this host (no non-elevated interactive session; fixture §3.2) and may not be claimed at any tier. |
| Single instance | **H** (tests in the OS job) + **R** (second launch visible) | |
| Packaging / install | **B** (artifact exists) + **R** (installs and launches) | |
| Capture-dependent boost | **not promised** | Reported per OS from engine support, separately (brief §4, §13.3) |

### 7.3 The never-collapse rule

- The word **"parity"** may be used only when, for **every** surface in §3, each
  OS has reached the minimum tier in §7.2 and the evidence paths are linked.
  Until then, statements are per-surface and per-OS with the tier attached.
- **Build** evidence may never be reported as "works", "runs", "tested",
  "supported", or "available on macOS/Linux".
- **Headless** evidence may never be reported as "UI works", "tray works",
  "shortcuts work", "installer works".
- A **Docker/CI container** run is not a Linux desktop run; a **CI macOS build**
  is not a Mac session.
- The docs gate (`verify-refs.ps1`) resolves **paths and line numbers only**; it
  cannot detect that the *text* at a cited line changed. Re-read a quoted passage
  whenever its target doc is revised — the pre-W1-brief quotes in §1 are the
  worked example.
- User-facing copy and docs follow the same rule: the app may not promise a tier
  the OS has not reached (this is why `docs/user-guide.md:31` must be retiered).
- The tier is stated for the **commit** it was observed at; a later change to
  `client-gui/src/platform/`, the viewport, packaging or the shell invalidates it until re-run.

---

## 8. Risks and dependencies

| # | Risk / dependency | Impact | Mitigation / owner |
|---|---|---|---|
| R1 | **`dev-ops` owns workflows and packaging.** This doc cannot add the jobs. | Parity stays **B**/`—` until the jobs merge. | Hand over §6.1 verbatim; `dev-ops` is the executor, `desktop-platform` reviews the job definition and the first logs. |
| R2 | **No paid runners, services or signing** (`[COST_STUB]`, `wat/rules.md:10-24`) | No macOS notarization, no hosted device farm, no paid UI-test service. | Free GitHub-hosted runners + owner hardware/self-hosted runner; ad-hoc signing only. |
| R3 | **macOS hosted-runner GUI sessions are unproven** | The macOS smoke may be infeasible; macOS stays **H**-blocked. | Test the step first, keep it advisory, fall back to owner hardware (ask §6.3b/c). |
| R4 | **Linux tray dependency chain** (`tray-icon` → GTK/glib) carries open advisories (`docs/security-advisory-review-2026-11-18.md:86-101`); the crate is Windows-only today for that reason (`client-gui/Cargo.toml:61-67`). | Enabling it on Linux can add vulnerability/audit work. | Prefer a pure-Rust StatusNotifierItem path; any dependency change gets a `sec-ops` audit pass and a logged decision before W2 implementation. |
| R5 | **macOS tray enabling** changes the dependency surface (`tray-icon` becomes `cfg(any(windows, macos))`). | Audit/dirty-tree implications. | Same `sec-ops`/`dev-ops` review; no GTK on the macOS path. |
| R6 | **Core boundary timing.** Capability truth (capture/admin/updater) should live in the shared core (`architect`, task-1); implementing it in `client-gui/` first creates the second truth this program exists to remove. | Rework, drift guard failure. | G1.1 depends on the task-1 boundary; platform work in W2 starts with `client-gui/src/platform/` + copy, and capability DTOs follow the core extraction. Coordinate with `interface-designer` (task-5) on `docs/design/interface.md:217`. |
| R7 | **Copy and i18n debt.** Platform-adaptive copy ("Restart as administrator", "Hide to tray", Npcap, updater instructions) ships in 9 locales (`client-gui/src/i18n.rs:16-34`); new keys need all locales or the documented English fallback decision. | `localization-sync.py --check` red (`.github/workflows/ci.yml:216-217`); wrong words on macOS/Linux. | New keys land with the implementation; `ux-lead` owns wording, `desktop-platform` owns platform variants; translations via the repo pipeline. |
| R8 | **Windows regression while touching the platform trait.** The trait is shared; elevation/tray changes can alter the Windows action gate. | Windows product surface regresses. | Keep the Windows gate byte-identical; Windows GUI job (`.github/workflows/ci.yml:301-324`) + runtime screenshots; `qa-engineer` verifies independently. |
| R9 | **Release-artifact decisions are owner/packaging gates.** Changing GUI dist targets, adding `.app`/`.dmg`, or an aarch64 Linux GUI touches `dist/` and release verification (`.github/workflows/release.yml:421-427`). | Release process risk. | `dev-ops` + owner decision; no W1 change; state the tier honestly either way. |
| R10 | **Over-claiming pressure.** The release already ships unverified macOS binaries; docs could drift into "supported". | A false parity claim reaching users. | §7.3 rule enforced by `qa-engineer` (task-6) and the lead's final check; user docs retiered (G0.4). |

---

## 9. What this document does not verify, and decisions it needs

**Not verified by this doc (cannot be, from this host):**

- Any Linux or macOS runtime behaviour: window creation, tray/status item, menu
  interaction, shortcuts, 200 % scaling, elevation→boost, config-survives-elevation,
  autostart at login, updater click-through, install/launch from the published
  artifacts, single-instance notice, crash dialog, locale resolution — and, on
  **Windows**, the Needs-elevation overlay and the UAC-decline branch (needs a
  non-elevated standard-user session plus an interactive click; fixture §3.2).
- Whether the Linux release binary starts at all (only its existence is proven;
  even `ldd`-level dependency claims in `dist/aur/PKGBUILD:15-18` are unverified
  here).
- Whether GitHub-hosted `macos-latest` can create the GUI window.
- Whether `tray-icon` compiles/behaves on macOS as expected, and whether the
  chosen Linux SNI approach behaves on GNOME/KDE.
- macOS Gatekeeper/notarization behaviour and the exact first-launch steps for
  the shipped tarball.
- aarch64 Linux GUI behaviour (no artifact).
- The mac/Linux clippy and test results for the current tree (no job exists).
- Anything about the release pipeline beyond the published asset list: the
  v1.11.3 build job logs were not inspected from here (state records the release
  as 39 assets; this doc adds the per-OS GUI artifact evidence).

**Open decisions for the lead/owner (product or gate):**

1. **Notifications:** confirm the recommended explicit no-claim (§5, G2.3) or
   ask for an implementation. Recommendation: no-claim — the product's job is a
   glanceable window, and macOS notifications need a signed bundle.
2. **Runtime verification access:** which OSes get owner hardware or a
   self-hosted runner, and who runs the interaction scripts (§6.3b/c). Without
   this, Linux/macOS stay below **R** permanently.
3. **macOS packaging:** bare tarball + documented Gatekeeper workaround
   (status quo, honest) vs ad-hoc-signed `.app`/`.dmg` in CI (needs `dev-ops`);
   no paid notarization either way.
4. **Linux tray scope:** SNI-where-available + honest no-tray GNOME mode
   (recommended) vs "no tray on Linux at all" (smaller, but weaker parity).
5. **macOS GUI artifacts in the release:** keep shipping them while untested
   (with an explicit untested/experimental label in the release/install docs) or
   gate them behind runtime verification.
6. **aarch64 Linux GUI:** add a target (build only) or document the gap as
   permanent.

---

## Appendix A — current-state claim index

Paths are relative to the worktree root; line numbers are at the r4 baseline —
`origin/master` `76aba5d` plus the W1 docs commit `0df57ce` (2026-11-20).

- Release build of the GUI for Linux/macOS: `dist-workspace.toml:16-24`,
  `client-gui/Cargo.toml:74-84`, `.github/workflows/release.yml:129,256-262`, and the published
  asset list in `wat/evidence/2026-11-19/team/desktop-platform/release-assets-1113.txt`.
- CI GUI coverage: `.github/workflows/ci.yml:36,39,42` (Linux exclude),
  `.github/workflows/ci.yml:219-233` (macOS smoke exclude),
  `.github/workflows/ci.yml:258,264,346,349,352,368` (other exclusions),
  `.github/workflows/ci.yml:301-324` (the only GUI job),
  `.github/workflows/ci.yml:15` (`-Dwarnings`),
  `.github/workflows/ci.yml:216-217` (`localization-sync --check`).
- Tray: `client-gui/src/platform/windows.rs:51-165,255-257`;
  `client-gui/src/platform/linux.rs:10-19,30-32`;
  `client-gui/src/platform/macos.rs:4,12,13-21,29-34`;
  `client-gui/src/platform/mod.rs:7,104-106`;
  `client-gui/src/app.rs:4178-4186`; `client-gui/src/app/window.rs:161-178`.
- Elevation: `client-gui/src/platform/windows.rs:191-238,266-280`; `client-gui/src/platform/linux.rs:34-46,72-98`;
  `client-gui/src/platform/macos.rs:36-48,74-89`; gate `client-gui/src/app/ledger.rs:39-57`, `client-gui/src/app.rs:4218-4235`;
  engine requirements `client/src/interceptor/linux.rs:24-27,68-80`,
  `client/src/interceptor/macos.rs:13-16,140`,
  `client/src/interceptor/traits.rs:779`.
- Capture probe: `client-gui/src/platform/windows.rs:240-246`; `client-gui/src/platform/linux.rs:48-63`; `client-gui/src/platform/macos.rs:50-65`;
  `client-gui/src/app.rs:316-320,3512-3523`; shipped features `client-gui/Cargo.toml:15,65-67`,
  `client/Cargo.toml:42-56`.
- Updater: `client-gui/src/update.rs:92-182,210-213,234-240`;
  `client-gui/src/self_update.rs:1-14,27-34`; UI `client-gui/src/app.rs:3923-3972`.
- Single instance: `client-gui/src/single_instance.rs:38-55,77-89,183-213,215-249,251-331`.
- Paths/logs/crash: `client-gui/src/paths.rs:9-61`; `client-gui/src/crash.rs:124-158`;
  `client-gui/src/main.rs:35-44,182-201`; docs `docs/user-guide.md:127-129`.
- Window/viewport/DPI: `client-gui/src/main.rs:6,109-120`;
  `client-gui/src/app/window.rs:33-113`; `client-gui/src/app.rs:3998-4000,4282-4289`.
- i18n/locale: `client-gui/src/i18n.rs:16-34,256-274`; `client-gui/locales/en.toml:131,136,165`;
  `docs/design/interface.md:217,231`.
- Shortcuts/notifications/autostart absence: grep over `client-gui/src` (documented
  in the W1 evidence report; zero hits for shortcut, notification, autostart and
  registry/LaunchAgent/XDG mechanisms).
- Packaging/quotes: `dist/aur/PKGBUILD:9-10,15-18,38-44,60-63`;
  `Formula/lightspeed.rb:30-32`; `dist-workspace.toml:13-15,37,42-51,58-59`;
  `tools/package-macos.sh:1-38`; `packaging/macos/Info.plist:11-14`;
  `docs/user-guide.md:30-33,81-118,205-210`; `docs/install-linux.md:3`;
  `docs/install-macos.md:3,106-114`; `docs/install-windows.md:13-14`.
- Delta (2026-11-20 rebase, `62c6cf6..76aba5d`): new GUI modules registered at
  `client-gui/src/main.rs:9-27` and `client-gui/src/app.rs:36`
  (`server_place` — `client-gui/src/server_place.rs`, `server_tables`,
  `server_geofeed_tables`, `client_trace`, `detection_provider`, `app/choice`);
  portability evidence `client-gui/src/client_trace.rs:349-375` (only target
  `cfg` in the six modules, both OS-tool arms implemented);
  cross-platform provider hook `client-gui/src/main.rs:85-86`; tray state
  addition `client-gui/src/app.rs:70-81`, `client-gui/src/platform/windows.rs:137-158`,
  `client-gui/src/app/window.rs:133-159`; packaging version bumps
  `dist/aur/PKGBUILD:26`, `dist/chocolatey/lightspeed.nuspec:5`,
  `dist/scoop/lightspeed.json:2`; release assets
  `wat/evidence/2026-11-20/team/desktop-platform/release-assets-1114.txt`;
  **r17 citation re-derivation** — every anchor into the delta-changed GUI files
  (`app.rs`, `main.rs`, `app/window.rs`, `platform/windows.rs`) was re-derived
  against `0df57ce`; unchanged files keep their W1-era anchors.
- Delta re-baseline (r17): the design set was re-cut to the mobile-only form —
  `docs/design/gui-refresh-spec.md:45-55` (spec §1.1: width pinned 320 px, one
  `narrow` column, height user-resizable 420–1400, initial 760) and
  `docs/design/design-system-2026.md:656-705` (design-system §5.4 metrics and
  breakpoints at 320; the line range is the r17 snapshot — task-23 is re-cutting
  that table, so the plan cites §5.4 by section in the body); the delta fix
  tasks 23 (measurements/citations) and 25 (RU copy + audit counts) do not touch
  CI, packaging or the three-tier rule.
- Policies: `wat/rules.md:10-24` (`[COST_STUB]`), `wat/rules.md:130-148`
  (`[SHELL_STUB]`); `AGENTS.md:63-65,99`; owner decisions
  `docs/design/gui-refresh-brief.md:294-334` (incl. the 2026-11-20 mobile-only
  viewport at `:323`); corrected parity baseline
  `docs/design/gui-refresh-brief.md:336-359`.

## Appendix B — evidence written for this task

- `wat/evidence/2026-11-20/team/desktop-platform/delta-r17.md` — revision r17
  change log: QA review-r4 finding **F21** (stale delta sentence) removal, the
  geometry re-anchor to the re-baselined design set, the citation re-derivation
  into the rebased GUI sources, the task-23/25 no-change note for CI and tiers,
  and the re-run docs-gate result.
- `wat/evidence/2026-11-20/team/desktop-platform/delta-r12.md` — revision r4
  change log: the 2026-11-20 delta (mobile-only geometry + rebase) with the
  platform-matrix and new-module portability checks, the exact commands run and
  the re-run docs-gate result.
- `wat/evidence/2026-11-20/team/desktop-platform/release-assets-1114.txt` — the
  v1.11.4 release asset list (name/size/sha256) fetched 2026-11-20; the GUI asset
  subset shows the same cross-platform shape as v1.11.3.
- `wat/evidence/2026-11-19/team/desktop-platform/report.md` — r2 working notes,
  fact base, commands, and what remains unverified (revision r1/r2).
- `wat/evidence/2026-11-19/team/desktop-platform/revision-r3.md` — revision r3
  change log: QA finding ID → change, with the re-run docs-gate result and the
  session commands used (including the elevation check).
- `wat/evidence/2026-11-19/team/desktop-platform/release-assets-1113.txt` —
  the v1.11.3 release asset list (name/size/sha256) fetched from the GitHub
  releases API on 2026-11-19, with the GUI asset subset called out; this is the
  build-tier evidence for Linux/macOS GUI artifacts.
- `wat/evidence/2026-11-19/team/desktop-platform/verify-refs.ps1` — the docs-gate
  check run against this document: every relative Markdown link resolves from
  `docs/design/`, and every `path:line` citation resolves to an existing file and
  valid line; it also flags non-repo-root (short) paths. Last run after revision
  r17 (output:
  `wat/evidence/2026-11-20/team/desktop-platform/verify-refs-output-r17.txt`):
  **ALL REFERENCES RESOLVE** (the exact count is in `delta-r17.md`). It cannot
  detect changed prose at a resolved line (§7.3); r17 therefore re-read the
  delta-changed source anchors instead of trusting the path-level gate.
