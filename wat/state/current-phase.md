# Current Phase

**Updated:** 2026-11-18
**History:** older entries live in `wat/state/phases/<YYYY-MM>.md`; this file keeps the live state and the newest entries only.

## Current state

**Workflow:** WF-078 1.11.2 PUBLISHED + BACKLOG PROFILES ON MASTER (2026-11-18: `v1.11.2` published with 39 assets; GitHub/Homebrew/Chocolatey/AUR at 1.11.2, Scoop bucket still blocked on `SCOOP_TOKEN`, winget PR #481953 external; fleet rolling 5/8 at check. Backlog: Tibia now runs on the TCP path (also fixes the latent Minecraft-Java gap), the TCP-in-TCP ban is enforced at the start path, and the EA FC 27 profile (id 28) is added; support replies posted to #167/#140/#153/#148). WF-077 1.11.2 PREPARED; WF-076 #178 FIXES (QUIC control-plane MTU + off-thread detection); WF-075 APP.RS PHASE A; WF-074 1.11.1 PUBLISHED; WF-073 PREPARED; WF-072 OWNER ACTIONS; WF-071 DUE-DILIGENCE; WF-070 REMEDIATION; WF-069 AUDIT; WF-068 AUDIT ROUND; WF-067 ROLLED OUT.
**Agent:** lead + `rust-dev` (task-26, Tibia/TCP wiring), `rust-dev-2` (task-27, FC27 profile), `qa-engineer` (task-28, independent verification). Evidence: `wat/evidence/2026-11-18/{release-1112,lead-gate-profiles,team/rust-dev-tibia,team/rust-dev-fc27,team/qa-profiles,github-audit}`.
**Next workstream (next session):** (1) Watch the fleet finish 1.11.2 (5/8 at check; hourly health-gated). (2) **The 1.11.2 tag predates the Tibia/FC27 work**, so those ride a later release (1.11.3, owner gate); when it ships, close #166/#176 and ask #167/#140/#178/#168 reporters to retest. (3) M1: review the trend after ~1-2 days of field-bearing snapshots; calibrate only then. (4) `#178` H2 (wgpu/hybrid present path) still needs the reporter's A/B data; do not change the default renderer on one report. (5) The website copy still says "24 games" while the registry has 28 - it predates this round and needs a localization pass (English + 8 translations), not a bare edit; tracked for a separate localization job. (6) **owner/external gates**: `SCOOP_TOKEN`, winget PR #481953, §5 retirement. (7) Backlog: #153/#148 await reporter data; Phase B of app.rs only when a feature forces it.
**Status:** **`v1.11.2` is the latest release (2026-11-18, 39 assets)**. GitHub Releases, Homebrew, Chocolatey (moderation queue) and AUR (`lightspeed-bin 1.11.2-1`) serve it; Scoop's source manifest is 1.11.2 but its bucket still needs `SCOOP_TOKEN`; winget is the open bootstrap PR. The fleet is rolling to 1.11.2 (5/8 at check). On `master` (unreleased) are the Tibia TCP switch with the interceptor transport derivation (which also fixes Minecraft Java's latent TCP gap), the TCP-in-TCP start-path ban, the EA FC 27 profile with protocol id 28, and the corrected supported-games/game-requests docs. Verification: QA PASS task-28, Lead gate **1,624 / 0 / 6** workspace + **193 / 0 / 1** GUI, clippy 0; CI **14/14** on `fd2e609` and Security Audit green. The agent layer (WF-058) is DSH-native with Team-mode personas and durable per-persona owners.

## Recent entries

### 2026-11-18 (release + backlog) - WF-078: 1.11.2 published; Tibia/FC27 profiles on master

**Agent:** lead + `rust-dev` (task-26), `rust-dev-2` (task-27), `qa-engineer` (task-28), on the owner's go-ahead to do the release and the remaining backlog.

- **1.11.2 published.** Tag `v1.11.2` on `b5ac0c3`; the release published `2026-11-18T11:42:24Z` with **39 assets**; the host/publish, Homebrew, Chocolatey, Scoop-source and verify-bumps jobs succeeded. `publish-aur` failed exactly as designed on the stale 1.11.1 PKGBUILD; `dist/aur` was bumped to 1.11.2 with the published x86_64/aarch64 sums and the failed job was re-run from the updated master, publishing **`lightspeed-bin 1.11.2-1`** (AUR page; the RPC index lags). Fleet rollout started: **5/8 on 1.11.2** at the check, hourly health-gated.
- **Tibia on the TCP path (`rust-dev`, task-26).** The blocker report was right: the shipped interceptor never read a profile's `transport()`, so a TCP-primary profile (Minecraft Java included) captured nothing. Option A: `assemble_config` derives `capture_tcp_ports` from `tcp_ports()` when the transport is TCP and no secondary range is declared; Tibia reports TCP 7171/7172; the TCP-in-TCP ban (`ensure_game_transport_for`) is enforced in `start_interceptor_with_target`; **Target a process** remains UDP-only (documented). Nine tests + two mutation pairs; the relay must advertise `TCP_TUNNEL` (all community relays do).
- **EA FC 27 (`rust-dev-2`, task-27).** New profile (`FC27.exe`, UDP 21000-21050 enclosing the measured 21006-21039, dynamic matchmaker servers, EA Javelin caveat), registry/protocol id **28** (additive; no existing id moved), supported-games docs and the GUI port hint. Six tests + registry/protocol-id mutations; `--list-games` loads it.
- **QA (`qa-engineer`, task-28).** PASS on the combined tree: TCP lane derivation and Windows filter end-to-end, start-path ban, ProcessAll still UDP-only, FC27 identity/registry/protocol, counts/page guards not weakened, no test removed (12 new test fns -> +24 runs), gate **1,624 / 0 / 6** workspace + **193 / 0 / 1** GUI, clippy 0. It also caught five stale "Tibia is UDP-only" doc lines; those were fixed before the push.
- **Support backlog cleared.** Posted factual replies on #167 (retest 1.11.2; how to tell relay starvation from a WinDivert teardown), #140 (explains the pre-1.11 ALPN alert 120 and points at 1.11.2), #153 and #148 (the exact missing measurements). No promises or dates.
- **Not done / not claimed.** The 1.11.2 tag predates the Tibia/FC27 work, so those are on master for the next release; no live Tibia/FC27 client was exercised (no installs); client-side runtime gating on the relay `TCP_TUNNEL` capability is documented but not implemented (pre-existing for Minecraft Java); the website's "24 games" copy is stale and needs a localization pass.

**Verified:** release assets 39; AUR page 1.11.2-1; QA PASS task-28; Lead gate counts above; CI on `fd2e609` **14/14** green, Security Audit green, Docker green; worktree clean. Evidence: `wat/evidence/2026-11-18/{release-1112,release-watch-1112b,lead-gate-profiles,team/rust-dev-tibia,team/rust-dev-fc27,team/qa-profiles,github-audit}`.
**Open:** fleet rollout to 8/8; the Tibia/FC27 release (owner gate); #166/#176 close with that release; #167/#140/#178/#168 retests; M1 trend; #178 H2 data; website localization; `SCOOP_TOKEN`/winget; §5; Phase B if forced.

### 2026-11-18 (release prep) - WF-077: 1.11.2 prepared; owner tag gate

**Agent:** lead, on owner approval ("Prepare 1.11.2 (hold at tag) + post the #178 update").

- **Release prep.** Workspace version `1.11.1 -> 1.11.2` (`Cargo.toml`; four workspace entries in `Cargo.lock`). The `[Unreleased]` changelog section became `[1.11.2] - 2026-11-18`: the QUIC control-plane MTU cap, the off-thread game auto-detection, and a `Notes` bullet that a downgrade to an older client drops the `tunnel_mtu` key from `config.toml` and returns to the 1492 clamp. No client/relay compatibility or relay-config change.
- **#178 update posted** (owner-approved): https://github.com/ShibbityShwab/lightspeed/issues/178#issuecomment-6975950798 - names both causes, says a patch release is being prepared, asks for an Auto-vs-Manual retest, and promises no date.
- **Gate on the bumped tree.** fmt clean; workspace clippy 0 warnings; **1,600 / 0 / 6** workspace; GUI clippy 0; GUI **193 / 0 / 1**. Evidence `wat/evidence/2026-11-18/release-prep-1112/`.
- **What the tag does.** Tag `v1.11.2` and publish: the release workflow builds/attests/publishes; the eight relays then self-update hourly. Known wrinkle (same as 1.11.1): `publish-aur` will be red on the tag because `dist/aur` is still 1.11.1 until the assets exist - bump it after publish with the new sha256s and run the `workflow_dispatch` `tag=v1.11.2 run_bumps=true` mode to complete Homebrew/Chocolatey/Scoop/AUR; winget stays a gated no-op until #481953 merges, and Scoop's bucket still needs `SCOOP_TOKEN` (the source manifest will bump).
- **Not done (deliberately).** No tag, no publish, no other public post; no relay change.

**Verified:** version/lock consistency at 1.11.2; full gate counts above; `release-notes.sh` can extract the `## [1.11.2]` section; `git diff --check` clean. CI on `d3ce848`: **14/14 jobs green** (Windows Build & Test, Windows GUI Build, Check & Test, QUIC integration, Infra, Localization, coverage, matrices, E2E, smoke); **Security Audit green** on the same commit; Health Monitor green on the preceding fix commit. CodeQL still running at report time.
**Open:** owner tag/publish; after publish the AUR bump + `run_bumps` dispatch, asset/attestation check, fleet rollout to 1.11.2, and the #178/#168 retests; CodeQL completion.

### 2026-11-18 (team) - WF-076: #178 fixes - control-plane MTU cap and off-thread game detection

**Agent:** lead + `rust-dev` (task-20), `rust-dev-2` (task-23), `qa-engineer` (tasks 21/24), `interface-designer` (task-22 triage, task-25 post-fix check), on the fresh #178 report and the 1.11.1 roll-out check.

- **Trigger.** A 1.11.1 user (Win11 25H2, RTX 3050 + Intel hybrid) reported 1-3 s freezes when moving the window or clicking any button. The trace also still showed `sendmsg error ... 10040 ... len: 1472` toward the relay's QUIC control port. `interface-designer` triaged the report read-only (task-22): **H1** - game auto-detection ran `tasklist` plus a PowerShell `Get-CimInstance` query on the egui thread every 5 s (measured 0.43-0.47 s here, 2.5-3.4 s in the reporter's trace); **H2** - a wgpu present-path/hybrid-GPU contribution is plausible but unconfirmed. The 10040 was a separate, definite bug.
- **Fix 1 - QUIC control-plane MTU (`rust-dev`, task-20).** `build_client_config` hard-coded quinn's DPLPMTUD `upper_bound` to `HARD_CAP_UDP_PAYLOAD` (1472) and ignored the effective `path_mtu_cap`. New pure helper `quic_udp_payload_upper_bound(path_mtu)` = `path_mtu - 28` clamped to `[1200, 1472]`; the supervised control connection takes the cap from engine `self.path_mtu_cap`, main's resolved `cli_mtu_cap`, reroute, or `CONSERVATIVE_PATH_MTU` in the verify-game diagnostic. Explicit 1500 -> 1472 unchanged; **default 1492 -> 1464** (8 bytes lower, matching the data plane and strictly safer); 1400 -> 1372. Public registration fns gained an explicit cap parameter; four integration tests updated with `HARD_CAP_PATH_MTU`; an 8-line `test_support` re-export was added (accepted). +4 tests (helper in lib+bin + supervisor wiring); mutation to always-1472 fails at 1492 (left 1472, right 1464), restore passes.
- **Fix 2 - game detection off the egui thread (`rust-dev-2`, task-23).** New `client-gui/src/app/presence.rs`: one dedicated `GamePresence` worker with a tested `PresenceSchedule` (5 s start-to-start, one scan in flight, no catch-up burst), mpsc result/command channels, `shutdown()` joins (also on Drop, and from `shutdown_engine`), and `presence_scan_allowed` gate = Auto only. `poll_game_presence` now only enables the worker and drains the newest result; the synchronous `try_auto_detect_game()` in `new()` is gone. Manual/Custom no longer scans at all (the old guard let `game_manual` through). +7 tests; both mutations fail (manual gate and worker-enabled path) and restore passes.
- **Verification.** QA task-21 and task-24 both PASS on the same combined tree: helper boundaries re-derived, only one `upper_bound` call site, cap threaded from the real sources, quic-off stub compiles, no egui-thread detection call site, worker lifecycle/cadence/join proven, no test weakened or removed. `interface-designer` task-25 PASS: shell unchanged at 320 px; a synthetic detectable process was auto-detected and applied; Manual mode spawned **0** `tasklist`/`powershell` in 15 s while Auto spawned one pair per 5.06 s, never overlapping; UI jitter <=0.27 s over 30 s; clean quit. Lead final gate: fmt clean, workspace clippy 0 warnings, **1,600 / 0 / 6** workspace (client 1,328 + protocol 86 + proxy 186), GUI clippy 0, GUI **193 / 0 / 1** (186 + 7 presence tests).
- **Not verified.** The reporter's EDR-slowed 2.5-3.4 s freeze and a real sub-1500 path are not reproducible here; H2 (wgpu/hybrid present path) needs the reporter's A/B data. No default-renderer change on one report. The fixes are **unreleased**; a 1.11.2 patch release is the owner's gate.

**Verified:** gate counts above; QA PASS both tasks; visual PASS; mutation fail/restore records; evidence paths in the current-state block. CI on `2556c93`: **14/14 jobs green** (Windows Build & Test, Windows GUI Build, Check & Test, QUIC Integration Tests, Infra Script Tests, Localization, Code Coverage, feature matrices, E2E, smoke); **Security Audit green** on the same commit. Deploy Proxy was running at report time (version-keyed; the release is unchanged at 1.11.1).
**Open:** Deploy Proxy/CodeQL completion; next patch release (owner gate); #178 reporter retest; #168 retest; M1 trend; H2 needs data; `SCOOP_TOKEN`/winget; backlog.

### 2026-11-18 (team) - WF-075: app.rs Phase A split (mechanical, token-identical)

**Agent:** lead + `rust-dev` (task-17, executor), `qa-engineer` (task-18, independent verification), `interface-designer` (task-19, real-window visual check), on the plan's decision to run Phase A after the 1.11.x release (the release landed in WF-074).

- **Result.** `client-gui/src/app.rs` **9,316 -> 4,072 lines**; 11 new modules under `client-gui/src/app/`: `tests.rs` (78 tests), `watchdog.rs`, `relay_choice.rs`, `games.rs`, `editor.rs`, `picker.rs`, `theme.rs`, `choice.rs`, `window.rs`, `ledger.rs`, `widgets.rs`; **5,292 lines relocated**; **188 `pub(super)`** annotations (125 items/methods + 63 fields); root `mod`/glob-`use` block plus `pub(crate) use theme::install_fonts;` and `pub(crate) use window::{MAX_WINDOW_HEIGHT, MIN_WINDOW_HEIGHT};`. `app.rs` keeps `TrayState` + the 7 timing consts, the three field-type state enums, `HealthProbe`/`WinnerProbe`, the `LightSpeedApp` struct, `lock_or_recover`/`spawn_rtt_probe`, `new()`, the 71 remaining `impl` methods and the single `impl eframe::App`. Only content adaptations: 20 `include_*!` paths gained one `../` (9 locales, 10 fonts, 1 brand icon) because the modules sit a directory deeper.
- **Gate.** Baseline and every one of A0-A10: `cargo fmt --all --check` clean, `cargo clippy -p lightspeed-gui --all-targets` exit 0 / **0 warnings**, `cargo test -p lightspeed-gui` **186 passed / 0 failed / 1 ignored** (app module 78). Final Lead gate on the integrated tree: workspace fmt clean, workspace clippy 0 warnings, **1,596 / 0 / 6**, GUI clippy 0, GUI **186 / 0 / 1**.
- **Proof of move-only.** QA reconstructed the pre-move tree in a temporary index and compared token streams with visibility/format normalisation: each module is one contiguous slice of HEAD `app.rs`, disjoint; surviving `app.rs` tokens = HEAD minus moved spans minus the test-module wrapper, **21,242 = 21,242**, first mismatch none. Histogram diff `app.rs 30+/5,274-`, 11 modules `+5,324`; Myers mis-reports the move (3,631+/8,875-), so use `--histogram`. 78 `#[test]` survive; no duplicate definitions; the plan's known hazards (switch_is_safe `cfg_attr`, TrayState, the state enums/probes, `pub(crate)` re-exports not also privately imported) all hold.
- **Visual check.** Real window at 320 px (the only width: `main.rs` pins min=max=320): idle + Settings + target-picker sheets render the shell in order rail -> route card -> config body -> action bar -> footer, nothing clipped; UIA trees element-for-element identical against the installed build (100 elements; only version text differs); pixel diff 0.095% idle, confined to live RTT digits. Boosting and disconnected were not attempted (elevation/WinDivert/networking change), named as unverified. A finding: the installed **1.11.0** GUI rewrites `config.toml` without the newer `tunnel_mtu` key, so a downgrade silently drops it; worth a release-note line in the next release.
- **Not done.** Phase B (the 3,666 untested `impl` lines) and Phase C (field regrouping) were deliberately not started; the plan says B only one surface at a time when a feature forces it, C never unless forced. The §1.4 doc/comment drift was not propagated; `start_update_check` remains as-is in the impl layer.

**Verified:** gate counts above on the final integrated tree; QA PASS per check; screenshot/UIA evidence. Evidence: `wat/evidence/2026-11-18/app-phase-a/` (`rust-dev/`, `qa/`, `interface/`, `lead-gate/`). CI on the push `deaf9e6`: **14/14 jobs green** (Windows GUI Build, Windows Build & Test, Check & Test, Localization, Infra Script Tests, Code Coverage, feature matrices, E2E, smoke); **Security Audit green** on the same commit. CodeQL was still running at report time. Tag-style proof: the split compiles and tests identically on the Windows CI runner.
**Open:** CodeQL completion; fleet 1.11.1 rollout (7/8 at check, `relay-nrt` still 1.11.0 on its hourly updater); M1 trend; `#168` retest; owner gates `SCOOP_TOKEN` and winget #481953; next release-note line for the `tunnel_mtu` downgrade caveat.

### 2026-11-18 (release) - WF-074: 1.11.1 published, channels completed, release-gate bug fixed

**Agent:** lead, on owner approval ("Yes - tag v1.11.1 and publish now") after the five-persona consultation recommended preparing it (WF-073).

- **Published.** Annotated tag `v1.11.1` on `d407039`; Release run `44268995907` **success**; release published `2026-11-18T08:58:35Z` with **39 assets**. Verified: the x86_64 Linux tarball sha256 matches its published `.sha256` (`ae0fd50a...`), `gh attestation verify` passes, and the release body is the `[1.11.1]` CHANGELOG section (MTU/EMSGSIZE, firewall, proxy filter, chrono -> time, M1, Actions pins, advisory trim). This is the first end-to-end publish since the hardened release workflow and the first run of the SHA-pinned `release.yml`.
- **Release-gate bug found by the tag.** `release-state` has no checkout and `gh release view` refuses outside a git repository, so `ready=false` and **every** packaging/channel job was skipped. Fixed: `GH_REPO` on the job, plus a `workflow_dispatch` `tag` + `run_bumps=true` mode that runs the same bump jobs for an already-published release (no build, no publish); every bump job now takes the tag from `release-state.outputs.tag` instead of `GITHUB_REF_NAME` (which is `master` on a dispatch). The read-only probe is skipped in bump mode.
- **Channels completed.** AUR PKGBUILD/`.SRCINFO` bumped to 1.11.1 with the published checksums and `publish-aur` published `lightspeed-bin 1.11.1-1`. Bump run `44272258596` completed every channel but its `Publishing channels self-test` failed because `test_publish_aur.sh` hard-coded 1.11.0; the test now derives the shipped version from `dist/aur` and uses the next minor for its bump fixtures. Corrected bump run `44272928238` is **fully green**. Final channel state: GitHub Releases 1.11.1; Homebrew formula 1.11.1; Chocolatey 1.11.1 pushed (HTTP 201, moderation queue); Scoop source 1.11.1 but bucket **not synced** (`SCOOP_TOKEN` missing, owner); AUR 1.11.1-1; winget bootstrap PR **microsoft/winget-pkgs#481953** still open (external).
- **Fleet rollout started.** At the release check the relays were mid-rollout: `relay-sgp-1` and `relay-syd-1` on 1.11.1 (uptime ~500 s), the other six still 1.11.0 on their original process generation; the hourly health-gated updater should finish all eight within ~1 h. M1 counters reset per relay on restart (skipped by the detector, as designed).
- **`#168` follow-up posted** (routine release notice, standing grant): https://github.com/ShibbityShwab/lightspeed/issues/168#issuecomment-6974090664 - asks the reporter to retest with `tunnel_mtu = 1400` and confirm the starving-return-path line is gone; no profile date promised.
- **Docs.** `docs/publishing-channels.md` refreshed to the 2026-11-18 state, including the bump-dispatch recovery mode.

**Verified:** release checksum + Sigstore attestation + changelog body; corrected bump run green on all channel jobs; channel versions read back from GitHub, the Homebrew formula, the Chocolatey nuspec, the Scoop manifest and the AUR RPC; evidence `wat/evidence/2026-11-18/release-prep/`. CI/CodeQL for the release commits are separate workflow runs (the tag run is the release proof).
**Open:** app.rs Phase A (next); fleet rollout completion; `SCOOP_TOKEN` (owner) and winget PR #481953 (external); field M1 trend and the `#168` retest; earlier backlog (registry re-sign not needed, #176/game requests, §5).

### 2026-11-18 (release prep) - WF-073: 1.11.1 prepared; holding at the owner tag gate

**Agent:** lead + consultation panel (`dev-ops`, `qa-engineer`, `sec-ops`, `net-eng`, `interface-designer`), on owner instruction "consult the team for the projects best interest on all this". Shared tasks 12-16 all completed; memos under `wat/evidence/2026-11-18/team/consult-*/memo.md`.

- **Consultation outcome (unanimous A).** Ranked A (prepare 1.11.1) > B (app.rs Phase A) > C (winget) > D (wait). A is the only path to the missing evidence: the MTU fix cannot be field-tested until released (a 10040 datagram never reaches a relay, so no server metric can produce the data), and the next tag is the first end-to-end proof of the hardened SHA-pinned publish path since the 1.11.0 403. B is right after the tag (moving 5,292 tested lines before a release adds ~5k lines of review noise to the only field signal). C is externally blocked (PR microsoft/winget-pkgs#481953 awaiting a moderator). D is circular for MTU and postpones the fix. M1 benefits from shipping now: each relay self-update resets one detector window (skipped), whereas waiting days discards the first baseline.
- **Prep.** Workspace version `1.11.0 -> 1.11.1` (`Cargo.toml`; four workspace entries in `Cargo.lock`); `CHANGELOG.md` gains `[1.11.1] - 2026-11-18` covering the `#168` MTU/EMSGSIZE fix (explicitly field-unconfirmed; the reporter was asked to retest with `tunnel_mtu = 1400`), the stable firewall rule, the stricter proxy destination filter, `chrono -> time` (advisory ignore gone), the M1 handshake instrumentation, SHA-pinned Actions, the 17 -> 7 advisory trim, the lock refresh and the docs/translation updates. No client/relay compatibility or relay-config change.
- **Gate on the bumped tree.** fmt clean; workspace clippy 0 warnings; **1,596 / 0 / 6** workspace; GUI **186 / 0 / 1**; `Cargo.lock` shows 1.11.1 for client/gui/protocol/proxy. Evidence `wat/evidence/2026-11-18/release-prep/`.
- **What the owner gate does.** Tag `v1.11.1` and publish: the release workflow builds the artifacts, attests them and publishes; the eight relays then self-update within ~1 h 10 m (health-gated, expected). Known wrinkle: `publish-aur` will be red on the tag because `dist/aur` is still 1.11.0 until the asset hashes exist - bump it after publish and rerun the failed job (the release itself is unaffected). Winget remains a gated no-op until #481953 merges. No Announcements post is required (no user requirement changed), but the release notes carry the MTU fix.
- **Not done (deliberately).** No tag, no publish, no public post; app.rs Phase A not started before the tag.

**Verified:** version/lock/changelog consistency; full gate counts above; changelog section extractable by `release-notes.sh`'s `## [version]` convention; consultation memos complete. CI on the prep commit is the next proof (see push follow-up).

**Open:** owner tag/publish decision; after publish, asset/checksum/attestation verification + AUR bump; then app.rs Phase A; field M1 data in ~1-2 days and the `#168` retest on 1.11.1.

### 2026-11-18 (owner + continuation) - WF-072: #168 posted, fleet decision, notification cleanup, chrono -> time, M1 instrumentation

**Agent:** lead + `infra-dev` (M1 instrumentation + QA fix), `rust-dev` (chrono migration), `qa-engineer` (independent verification of both and re-verification of the M1 fix), on owner instruction ("1 post it / 2 i will take the team recommendation / 3. can you clean up? / continue with the rest of the workstreams").

- **Owner actions.** (1) Posted the corrected `#168` reply: https://github.com/ShibbityShwab/lightspeed/issues/168#issuecomment-6972238765 (body in `wat/evidence/2026-11-18/github-audit/issue-168-reply-body.md`); the draft's four QA publication flags were applied first. (2) Recorded the owner decision to **keep `control_alpn = "required"` on all eight relays** (Option A, no fleet change) in `docs/fleet-alpn-options-2026-11-18.md` and `wat/state/decisions/2026-11.md`. (3) Notification cleanup, repo-scoped: unread **281 -> 0** (`PUT /repos/.../notifications`), all-activity subscription removed (`DELETE .../subscription`, 204); account-wide other repos deliberately untouched. Evidence `wat/evidence/2026-11-18/github-audit/owner-actions.md`.
- **chrono -> `time` (`rust-dev`, task-8).** Replaced the unmaintained `chrono` at all 7 sites with `time 0.3.55` (workspace features std/formatting/local-offset/macros): new `client/src/clock.rs` centralizes local hour/day-of-week with a documented and tested UTC fallback; process timestamps stay byte-identical UTC `%Y-%m-%d %H:%M:%S` (pinned by an extended test); proxy month key is a pure tested `month_key`; `chrono`/`iana-time-zone*` leave the lock, `num_threads` enters; `deny.toml` drops `RUSTSEC-2026-0543` (7 reasoned entries remain). Mutation check on the timestamp format. Gate: fmt clean, clippy 0 warnings, **1,596 / 0 / 6** workspace (client 1,324, protocol 86, proxy 186; +11), GUI **186 / 0 / 1**.
- **M1 instrumentation (`infra-dev`, tasks 7/10).** `collect-metrics.sh` now parses the four `lightspeed_control_handshakes_total{outcome=...}` series reset-safely into the per-relay history (cumulative/delta/lifetime/totals) plus auth rejections/sessions; `health-anomaly.sh` gains a bounded warning-only `m1_reject_drain` detector (window rate >= 2x the relay's own baseline, >= 100 rejects/h, accepted <= 10/h, sustained and restart-free; old snapshots skipped). QA's F1/F2: the M1 rate now uses a duration matching the numerator span (`wstart -> win[-1]`, floored at 1h), so the documented thresholds are the effective ones and emitted rates are true per-hour (at `--window 3` the pre-fix math was 1.5x); the pre-existing detectors are byte-identical. Tests are additive (collector +118 lines, anomaly +276), including an F1 regression guard where the old denominator would fire and the corrected one stays silent. Docs: `docs/m1-handshake-trend.md`, `docs/data-dictionary.md`.
- **Verification (`qa-engineer`, tasks 9/11).** Independent verification re-derived the semantics from the code, forced a fresh compile after `cargo clean -p`, and re-ran the full gate: **1,596/0/6 + 186/0/1**; chrono absent from the graph, deny ignore absent, month/weekday/timestamp semantics checked; M1 schema contract checked in both directions (every field read is written; 49/49 counters), old-snapshot compatibility, 8/8 independent model cases, then re-verified the F1/F2 fix (its model: 10/10, corrected 900/h vs old 1,350/h; static validator 48/48). POSIX execution is **CI-only** on this host (no bash/jq) - first real execution is the next `Infra Script Tests` and `Health Anomaly Monitor` run.

**Verified:** local Rust gate on the integrated tree **1,596 / 0 / 6** workspace + **186 / 0 / 1** GUI, clippy 0 warnings, fmt clean; `git grep chrono` in source/manifests empty; `deny.toml` parses with 7 entries and no `RUSTSEC-2026-0543`; all workflow YAMLs parse; `git diff --check` clean; localization checks green. CI: the first run `44264655749` on `e6afa75` was green on 13/14 jobs and failed **Infra Script Tests only** - four new M1 comments contained apostrophes inside the single-quoted `JQ_PROGRAM`, closing the shell string so bash executed jq (`row: command not found`). Fixed in `1df9fc2` (wording only); CI run `44265657454` is **14/14 green**, including `Infra Script Tests` (both hermetic suites) and `Windows Build & Test`. `Security Audit` `44264655723` on the chrono-free tree is green (ignore removal proven), `Deploy Proxy` `44264655897` success (version-keyed no-op), `Deploy GitHub Pages` `44264655703` success. This was exactly the CI-only risk QA flagged; the first real execution caught it.
**Open:** next release (owner) proves the MTU fix and the publish path; watch the first M1-bearing collector snapshots and calibrate `M1_ACCEPT_MAX_PER_HOUR`; `Security Audit` must be green without the chrono ignore; app.rs Phase A after the next release; earlier backlog (winget, #176/game requests, §5).

### 2026-11-18 (team) - WF-071: six-persona due diligence on the remaining audit items

**Agent:** lead + Agent Teams: `dev-ops` (F-09), `infra-dev` (F-08), `qa-engineer` (F-12/F-14), `sec-ops` (F-15), `net-eng` (F-02), `interface-designer` (F-13). Owner instruction: "Can you have the team do due diligence on these remaining issues and do what is best for the project?" Six shared tasks created with disjoint write scopes; all six completed with evidence. Lead integrated and re-gated.

- **F-09 (fixed, `dev-ops`).** 90 `uses:` across 12 workflows pinned to full commit SHAs (18 unique refs resolved with `git ls-remote` and cross-checked 18/18 via the GitHub API; annotated tags peeled). `release.yml` (hand-edited cargo-dist output, the highest-privilege workflow) is pinned too; the manual-regeneration note is recorded. Top-level `permissions: contents: read` added to 6 workflows; all 21 existing permission blocks preserved, none widened. 13/13 YAML files parse; the structural old-vs-new comparison reports 0 unexpected diffs and the diff is exactly pins + read blocks. No CI run possible on this host: the next push is the first execution.
- **F-08 (fixed, `infra-dev`).** The earlier `C1041` was **not a build-script race**: this host's repo tree is Low mandatory-integrity labelled, so a build-script exe under `target\` cannot write the Medium-integrity cargo registry source dir where `windivert-sys` compiles `windivert.c` (`C1041`/`D8050`; deterministic on the old lock too; the `cc` bump just forced the first rebuild). `-j 1`, `CFLAGS=/FS`, stale-PDB deletion and an in-tree target dir all fail; a `CARGO_TARGET_DIR` **outside** the tree works. The refreshed lock (142 updating / 8 adding / 15 removing, ~150 packages) is kept and gated green: fmt clean, workspace clippy 0 warnings, **1,585 / 0 / 6** workspace, GUI **186 / 0 / 1**.
- **F-15 (fixed, `sec-ops`).** `deny.toml` had **17** ignores, not 18: 7 GTK3 advisories were withdrawn 2026-08-14, and `protobuf`/`instant` are absent from the graph, so 9 were inert and removed; 8 reasoned entries remain with dependency paths/exposure, a quarterly review date, and `SECURITY.md` corrected to 1.11.x and linked to `docs/security-advisory-review-2026-11-18.md`. The stale duplicate `RUSTSEC-2024-0437` in `.cargo/audit.toml` was removed by the Lead. cargo-deny is CI-only on this host.
- **F-14 (verified, `qa-engineer`).** The shipped v1.11.0 GUI exe carries `RT_GROUP_ICON` id 1 and `RT_ICON` ids 1-4 at 16/32/48/256 px (32 bpp), proven with a stdlib PE parser and Win32 extraction; zip and MSI payloads are byte-identical. The local "no resource compiler" warning is a PATH condition only (SDK `rc.exe` off-PATH); release/CI build is unaffected. No change needed.
- **F-12 (audited, `qa-engineer`).** 282 unread repo notifications (455 account-wide): 244 stale `ci_activity` failures, 18 `review_requested` + 10 `state_change` on old Dependabot PRs, 1 resolved comment. The two live Health Anomaly failures (F-02) were hidden in that pile; latest run is green. Recommendation: bulk-Done the stale set, watch Participating/@mentions (or custom without Actions), and have scheduled monitors open one auto-closing ops-alert issue. Nothing marked read (owner action).
- **F-02 (quantified, `net-eng`).** Eight relays, process-lifetime window (uptime 38.4-38.6 h): **677,561 handshakes, 3,973 accepted, 673,495 alert-120 rejects (99.4%, an upper bound), 93 other rejects, 373 sessions, 6.72 M packets**; 8/8 healthy on 1.11.0/`required`. The stats branch has no ALPN field (cannot trend M1). Memo `docs/fleet-alpn-options-2026-11-18.md` recommends keeping `required`; if a mercy path is wanted, `relay-syd-1` only (lowest accept share, reversible with one command). No fleet change.
- **F-13 (planned, `interface-designer`).** Corrected the line count (`app.rs` is 9,316 lines at HEAD, 8,807 non-blank; 8,735 was the non-blank count at 343530b), inventoried 77 fields / 497 refs / 254 fns / 78 tests (0 construct the app), and delivered `docs/design/app-refactor-plan.md`: Phase A moves the 5,292 test-pinned lines into 10 `app/` submodules (app.rs -> ~4,050, -57%); Phase B (3,666 untested lines) deferred; Phase C (field regrouping) not planned. Recommendation: after the next release, Phase A only.
- **F-03 (corrected, `qa-engineer`).** The held `#168` draft checked out mechanically, but QA flagged four publication defects: the adapter-MTU 1400 workaround was already tried and failed (so 1350 must not be presented as proven), `netsh` needs elevation/adapter name/rollback, "we are not shipping a profile until..." is a public shipping promise, and "fix in progress" should be "fixed on master, not yet released". The draft was corrected and remains **held** (owner post).
- **Decisions logged** in `wat/state/decisions/2026-11.md`: pin Actions, trim the allowlist, keep the lock refresh, defer the app split; the fleet `required`/`off` call and the public reply remain owner gates.

**Verified:** final Lead gate on the integrated tree (Cargo.lock refreshed; docs/workflows only otherwise) with the external target dir: `cargo fmt --all --check` clean; workspace clippy `--all-features` exit 0, **0 warnings**; workspace tests **1,585 / 0 / 6** (client 1,314, protocol 86, proxy 185); GUI clippy exit 0; GUI tests **186 / 0 / 1**; total **1,771 / 0 / 7**. All 13 workflow YAMLs parse; all `uses:` SHA-pinned (independent spot-check of 3 upstream refs, including the peeled annotated tag); `git diff --check` clean. Evidence `wat/evidence/2026-11-18/team/{dev-ops,infra-dev,qa,sec-ops,net-eng,interface-designer}/`.
**Pushed `343530b..c016acc`.** First CI run `44246123560` was green on 13/14 jobs and failed **Localization Checks only**: the English `docs/troubleshooting.md` edit from WF-070 left the 8 localized pages stale. Fixed in `b99529e` by regenerating them (`scripts/localization-sync.py`: 7 machine retranslations + a manual `ja` translation after the public endpoint repeatedly lost code-block placeholders; state hashes updated). CI run `44248065938` on `b99529e` is **14/14 green** (including Windows Build & Test with the new Windows clippy step and Localization Checks); `Security Audit` `44248065917` green; `Deploy Proxy` `44246123575` success (version-keyed no-op); `Deploy GitHub Pages` `44246123457` success; Docker `44246123767` success.

**Open:** owner gates (F-02 decision, the #168 reply, notification cleanup); next release proves the MTU fix and the publish path; M1 collector instrumentation; app.rs Phase A after the release; chrono migration; earlier backlog (winget, #176/game requests, §5).

### 2026-11-18 (remediation) - WF-070: audit fixes landed (MTU/EMSGSIZE, Windows CI lint, firewall, IP filter, docs, deps)

**Agent:** lead. Owner instruction: "Nice work lets start tackling all this" after the WF-069 audit.

- **F-01 (the `#168` bug).** `[tunnel] mtu` / `--mtu` is now an honoured path-MTU cap: default 1492 (preserves the old clamp), range 1280-1500, on `TunnelConfig`/`PmtudConfig`/`PathMtuDiscovery` and threaded through `InterceptorConfig.path_mtu` (engine setter + `build_config_*` default), the Windows interceptor (`classify_at`, `send_datagram_at`) and `UdpRelay::with_pmtud`. On `EMSGSIZE`/`WSAEMSGSIZE` (10040) the interceptor now lowers the effective MTU by 64 bytes (floor 1280), retries the datagram with fragmentation, and records `send_errors`/`last_send_error`; `InterceptorCounters`, `InterceptorSnapshot`, `InterceptorHealth` and `EngineStatus` carry them, and the GUI's no-reply trace says a local send failure "cannot be the cause" and names the OS error. GUI `config.toml` gained `tunnel_mtu` (clamped; no UI field yet). Lifecycle: `client/src/tunnel/{pmtud,transport,relay}.rs`, `client/src/interceptor/{traits,windows}.rs`, `client/src/engine.rs`, `client/src/config.rs`, `client/src/cli.rs`, `client/src/main.rs`, `client/src/modes/intercept_mode.rs`, `client-gui/src/{config,app}.rs`.
- **F-04.** `windows-test` now lints Windows-only code: `cargo clippy -p lightspeed-client --features quic,windivert-redirect --all-targets` and `-p lightspeed-proxy --features quic --all-targets` (the Linux clippy cannot see `#[cfg(windows)]`; the GUI job lints only the GUI crate).
- **F-05.** One stable, program-scoped firewall rule (`LightSpeed WinDivert Tunnel`) replaces the per-port name; start deletes the stable and legacy per-port rule first, so a crash cannot accumulate rules (the 71-rule cleanup is the evidence it did). Unit test pins the name.
- **F-06.** Proxy `is_public_ipv4` now also rejects `0.0.0.0/8`, `192.0.0.0/24`, `198.18.0.0/15`, `240.0.0.0/4`, matching the stricter client copy; boundary tests added.
- **F-10.** Registry count 27 corrected in `games/mod.rs` + design doc; Delta Force row/intro updated with the 2026-11-17 measurement; `protocol.md` documents cap/auto-fallback; `CLI-REFERENCE.md` adds `--mtu`/`--path-mtu-discovery`; `troubleshooting.md` adds the 10040/EMSGSIZE entry + adapter-MTU workaround.
- **F-11.** Removed the unused `windivert` wrapper (use `windivert-sys` with `vendored`) and unused `windows` crate; `rcgen` moved to `[dev-dependencies]`. Manifest warnings are now zero (was 2).
- **F-02 confirmation.** `/metrics`: relay-syd-1 `negotiated_v1=42`, `rejected_no_application_protocol=32,167`, 6 sessions / 17,903 relayed; relay-fra `2,418` vs `289,373`. Relays healthy; the monitor's alert is the stranded pre-1.11.0 client population, not a relay fault. Owner decision on `required` vs a mercy `off` remains open (no fleet change).
- **F-03.** `#168` reply drafted and held for the owner (public post): `wat/evidence/2026-11-18/github-audit/issue-168-reply-draft.md`. Not posted.
- **F-08 attempted and reverted (new build finding).** `cargo update` proposes ~150 compatible updates; the lockfile refresh itself is fine, but a rebuild of the **vendored `windivert-sys`** failed on this host with MSVC `C1041: cannot open program database ... vc140.pdb` when two build-script instances compiled `windivert.c` concurrently (the PDB is written into the shared registry source dir, not the target dir). This is a pre-existing build race exposed by the forced rebuild, not a version conflict; `Cargo.lock` was reverted to the verified state. Take the refresh on a clean CI runner (or with `-j 1`/`CFLAGS=/FS`), and watch the Windows job.
- **Not taken this round:** F-09 Actions pinning, F-12 notification hygiene (owner account), F-13 `app.rs` split, F-14 release-exe icon check, F-15 advisory-review cadence.

**Verified:** full gate on the integrated tree (`wat/evidence/2026-11-18/remediation/`): `cargo fmt --all --check` clean; workspace clippy `--all-features` exit 0 with **0 warnings**; workspace tests **1,585 / 0 / 6** (client 1,314, protocol 86, proxy 185; +24 vs baseline); GUI clippy exit 0; GUI **186 / 0 / 1** (+2). Total **1,771 / 0 / 7**. Mutation check: reverting `effective_path_mtu` to the fixed clamp fails the pmtud cap test and the relay cap test; restoring passes both (`mutation-fail*.log`, `mutation-restored.log`).

**Open:** the held `#168` reply and the F-02 fleet decision (both owner gates); field confirmation of the MTU fix needs a sub-1492 path (the adapter-MTU workaround is interim); F-08 needs a clean-CI-runner dependency refresh (the vendored `windivert-sys` MSVC PDB race must not be hit by a manual refresh on this host); remaining F-09/F-12..F-15.

### 2026-11-18 (audit) - WF-069: full project audit (code, CI, docs, GitHub, fleet), read-only

**Agent:** lead. Owner request: "check on the project audit it and come up with enhancements or fixes that need done" plus "check github for replies notifications, issues and discussions". Report: [`docs/project-audit-2026-11-18.md`](../../docs/project-audit-2026-11-18.md). No code, config, fleet, release or public-post change was made.

- **Baseline (reproduced, not inherited).** `343530b`, clean worktree; `cargo fmt --all --check` clean; workspace clippy exit 0 (0 lint warnings, 2 known manifest warnings); workspace tests **1,561/0/6** (client 1,291, protocol 86, proxy 184); GUI **184/0/1**; total **1,745/0/7**. Logs in `wat/evidence/2026-11-18/lead-gate/`. CI/CodeQL/Security Audit green on `master` (run `44201438078`); `v1.11.0` has 39 assets.
- **F-01 (P1, the headline).** The `#168` Delta Force report is a real client bug, not reporter error: the tunnel clamp is fixed at 1492 bytes and cannot go lower (`protocol/src/header.rs:68`, `client/src/tunnel/pmtud.rs:106,149`), the documented `[tunnel] mtu` knob (`client/src/config.rs:71-73,261`; `client/lightspeed.example.toml:55`) is **never read**, `--path-mtu-discovery` never reaches the GUI `interceptor/windows.rs` path, and send failures there are swallowed (`.is_ok()`/`let _ =` at `:1548,:1568,:1583`) so the GUI blames the relay (`NoReplyFromRelay`) with no OS error. Reported `len: 1404/1472`, `WSAEMSGSIZE 10040`. Fix plan (cap/override + fragmentation retry + `send_errors` + accurate verdict) is in the report §F-01.
- **F-02 (P1, operational).** The scheduled `Health Anomaly Monitor` is **red on two consecutive runs** (`44189992620`, `44093060510`): `relay-syd-1` auth spike + zero relayed (stranded pre-1.11.0 client signature) and `relay-fra` 57.9% load skew. Live read-only `/health` snapshot 2026-11-18: **8/8 healthy 1.11.0 `required`**, 6.60 M relayed / 370 sessions / 166,324 auth rejects; syd 17,903/6/3,365. This corrects the earlier "scheduled runs green" claim. No fleet change; the task-10 rollback question stays an owner decision.
- **F-03 (P1, GitHub).** `#168` has three reporter comments from 2026-11-17 12:38-13:00 UTC with the exact endpoints and the MTU blocker; **no owner reply**. All other open issues (#176/#167/#166/#153/#148/#140) and all discussions already have the owner's last word (2026-11-17 00:45 batch). 282 unread repo notifications, almost all stale `ci_activity`; only one recent non-CI (`#147`, resolved).
- **P2/P3 set.** CI never lints Windows-only client code (`ci.yml:270-285` has clippy installed but unused); per-port Windows Firewall rule names regrow after crashes (`interceptor/windows.rs:2036-2097`, 71 stale rules proved it); proxy `is_public_ipv4` is weaker than the client's duplicate (`proxy/src/abuse.rs:292-356`); ~150 compatible dependency updates pending; Actions tag-pinned/inconsistent and several workflows lack `permissions:`; stale game counts (27 actual vs 20/23 in comments), stale Delta Force row and `protocol.md` PMTUD claim; two unused-dependency warnings (`rcgen`, `windivert`); 8,735-line `client-gui/src/app.rs`; verify the release exe icon.

**Verified:** the four gate commands with counts above; GitHub state via `gh` (issues/PRs/discussions/notifications/runs); live fleet via the public `/health` endpoints (read-only); all evidence under `wat/evidence/2026-11-18/{lead-gate,github-audit,project-audit}/`. Not verified: a live match reproduction of the MTU fix (needs a path with MTU < 1492); `cargo audit`/`cargo deny` locally (not installed; CI green).

**Open:** F-01 code fix + its failing-before test; `#168` reply; task-10 decision on `relay-syd-1`; F-04/F-05; P2/P3 items. Nothing was changed outside this state entry and the two new docs/evidence files.

### 2026-11-18 (hygiene) - Remote branch cleanup: 43 branches -> 2

**Agent:** lead, on owner request ("I see a lot of open or old branches are those necessary can we clean any orphaned ones up?").

- **Classification (43 remote branches).** 40 were head branches of merged PRs with zero unique work (39 ancestors of `master`; `dependabot/cargo/patch-ee65abdbca` was squash-merged as PR #174). One was `dependabot/cargo/eframe-0.37.0`, a closed unmerged branch (PR #158, 2026-10-22) whose bump had already landed on `master` as `d575d48` (`client-gui/Cargo.toml` is `eframe = { version = "0.37", ... }`), so its two commits are superseded. Two are genuinely long-lived and were kept: `master` (default) and `stats` (data-only orphan branch written hourly by `.github/workflows/pages.yml` and read by `health-anomaly.yml`; its tip moves constantly - `af97dac` at cleanup time). No PR was open and no workflow references any deleted branch name.
- **Action.** Deleted the 41 orphaned branches in one `git push origin --delete` (41/41 `[deleted]`, exit 0), then `git fetch --prune` dropped the local tracking refs. Enabled repository setting `delete_branch_on_merge` (`false -> true`) so merged PR head branches are removed automatically from now on - that setting being off is why ten weeks of merged branches had accumulated. `master` is unchanged at `bfd3b87`; the worktree is clean; only the WAT state files changed.
- **Verification.** GitHub branches API after: exactly 2 (`master bfd3b87`, `stats af97dac`); `git ls-remote --heads origin` lists exactly those two refs; local `git branch -r` shows only `origin/master` and `origin/stats`; `delete_branch_on_merge` reads `true`; the two non-ancestor heads stay recoverable as `refs/pull/158/head` (`bbb66d3`) and `refs/pull/174/head` (`b986acf`). Evidence: `wat/evidence/2026-11-18/branch-cleanup/` (`report.md`, `classification.txt`, `delete.log`, after-state files).

**Open:** none. `stats` must not be deleted: the Pages publisher writes it hourly and the anomaly monitor reads it.

### 2026-11-17 (ownership) - Sole-owner licence v1.1 + CLA, and contributor-identity scrub

**Agent:** lead, on owner instruction. Two AI-agent identities were showing in the
repository's Contributors card, and the owner asked for them gone and for the
contributor licence to leave sole authority with the owner.

- **History.** The Contributors card was fed by non-author `Co-authored-by`
  trailers. Rewrote the 25 commits from the merged squash commit to the tip,
  stripping the tool trailers and session links; every tree is byte-identical
  (tip tree `8674abf2b3863fef4c4a64644997ecc4c3e9cf32`) and the commit count
  stays 1033. Force-pushed with lease (`b4dbc01 -> c15c479`), then pushed the
  licensing commit (`dc0abcb`). Deleted the local pre-rewrite backup branch and
  temporary PR refs, expired reflogs and ran `git gc --prune=now`;
  `git log --all` and `git fsck --unreachable --no-reflogs` no longer contain
  the identities.
- **GitHub cache.** `GET /repos/.../stats/contributors` returned 202 and then
  recomputed to the six real contributors; `graphs/contributors-data` and the
  REST contributor list agree. A temporary default-branch swap
  (`master -> stats-sync -> master`, branch deleted) was used to fire
  `DefaultBranchChangedEvent`: after it, the anonymous/public repository page
  and the contributor endpoints show only the real contributors. The owner's
  **logged-in** tab still showed the two cached cards at the time of writing;
  that authenticated sidebar cache lags the public one and clears on GitHub's
  schedule. If it persists, GitHub Support can purge the contributor cache.
- **Licensing.** `LICENSE` is **v1.1**: copyright is ShibbityShwab alone, sole
  ownership/authority is explicit, contributions are assignment-only, all other
  rights are reserved, and third-party terms are separated. New `CLA.md`
  (copyright assignment, moral-rights waiver, patents, no-AI-authorship,
  electronic acceptance) and `docs/licensing.md`; `CONTRIBUTING.md`, the PR
  template, `README.md` and the `AGENTS.md` guardrails now
  require the CLA. `Cargo.toml` (authors, `LicenseRef-LightSpeed-NC-1.1`),
  `deny.toml`, `NOTICE`, WiX `Manufacturer`, AUR, Scoop, Homebrew, winget
  copyright and the site footer name the owner only; historical release
  manifests keep the licence version they shipped under. The retired agent
  brand was also removed from the two historical WAT entries that carried it.
  No code changed.
- **Verification.** Worktree greps for the removed identities and for the old
  collective name are empty; `cargo fmt --all --check`
  clean; `cargo metadata --no-deps` parses; the 56 relative links in the
  changed docs resolve; `git diff --check` clean. Evidence:
  `wat/evidence/2026-11-17/ownership-scrub/` (gitignored).
- **Open.** (1) The authenticated GitHub sidebar cache above; public pages are
  already correct. (2) PR #173's own branch commits (GitHub PR refs are
  immutable) still carry the tool trailer in that PR's commit view; it is not on
  the default branch and does not feed the public contributor list, and removing
  it would need GitHub Support. (3) Contributions merged before the CLA remain
  under the licence in force when submitted until their authors sign
  `CLA.md`; the owner may ask `@devgrunge` (#72), `@Kianmhz` (#173) and
  `@CiroBurro` (one commit). (4) CLA acceptance is enforced by the PR template
  and review, not a bot; a CLA-assistant is an optional follow-up. (5) Next
  release package manifests should carry `LicenseRef-LightSpeed-NC-1.1` and
  the sole-owner copyright. (6) The licensing answer in `docs/faq.md` was left
  unchanged because changing English copy forces the Localization workflow to
  re-translate eight localized docs; run that workflow if the FAQ should link
  the CLA (README, CONTRIBUTING and `docs/licensing.md` already do). (7) Security
  Audit went red on `RUSTSEC-2026-0543` ("chrono is unmaintained", informational,
  no patched release) when the advisory DB updated mid-round; `deny.toml` now
  carries a reasoned ignore so the gate is green, and migrating chrono to
  `jiff`/`time` is tracked follow-up work.

### 2026-11-17 (later) - Backlog follow-ups: publishing channels wired, registry signing fixed, images reviewed, WinDivert debt cleared

**Agent:** lead + `infra-dev-3` (publishing-channel wiring). Follows the audit round above; the owner replied to it and asked for five follow-ups.

- **Owner decision - no rollback; announcements become a rule.** The fleet was never rolled back: the owner accepted the update requirement ("this is a free service and I am fine with people needing to update"). The missing piece was communication, so this round added `[COMMS_STUB]` to `wat/rules.md` (every compatibility/user-visible change ships a Discussions -> Announcements post), a Release Notes "Announcements" step in `CONTRIBUTING.md`, and the first such post: **[discussion #177](https://github.com/ShibbityShwab/lightspeed/discussions/177)** explains that 1.11.0+ is required on the community relays, how to update, and the self-hosted escape hatch. The GUI does prompt on launch (update check -> dialog, `client-gui/src/app.rs` `start_update_check`); users who never restart it will not see the prompt, which is why the announcement exists.
- **Publishing channels audited and repaired.** Root causes: the 1.11.0 release skipped every bump job (the 403), and three channels were never wired at all. Actions taken: **Chocolatey** 1.11.0 pushed through the existing `Choco Push` dispatch (run `44006128768`, `push HTTP 201`; moderated, feed still serves 1.10.3 until approved); **Scoop** bucket updated to 1.11.0 (`bucket/lightspeed.json` sha `d43c5b54`), and a gated `publish-scoop` job + hermetic suite now sync future tags (`SCOOP_TOKEN` is the one owner step); **winget** was never bootstrapped - the two first-version PRs (#435790 1.4.3, #437292 1.4.4) passed validation and sat unmerged until closed, the CLA was already signed and `WINGET_TOKEN` already exists - so the 1.11.0 manifests were refreshed to schema 1.12.0 and a replacement PR opened: **[microsoft/winget-pkgs#481953](https://github.com/microsoft/winget-pkgs/pull/481953)** (moderator review is the remaining external step; `winget-releaser` will publish future versions once it merges); **AUR** has no package at all (the name `lightspeed` is an unrelated app; `lightspeed-bin` is free) - `dist/aur` was bumped to 1.11.0-1 with the published sha256 values, a gated `publish-aur` job added, and the first push needs the owner's AUR account + SSH key (`AUR_SSH_PRIVATE_KEY`); **Homebrew** was already current (in-repo formula 1.11.0). Channel state/procedure: `docs/publishing-channels.md`.
- **Registry: no re-sign needed, tooling fixed instead.** The audit's F2 finding (stale `published_at`) was downgraded after re-reading the code: `published_at` is only a monotonic anti-rollback floor (no expiry), and nothing reads `issued_at`; the live document is authentic and breaking nothing. The tooling defect was real though: `Registry` did not model `issued_at` (any re-sign would silently drop it) and the signer copied timestamps verbatim. `sign_registry` now stamps `published_at = max(now, previous + 1)` and `issued_at = now` by default (`--keep-timestamps` for an unchanged re-sign), `Registry` preserves `issued_at`, two tests pin it, and the operator guide/audit record that signing is rare and deliberately offline - GitHub Pages hosts the file, nothing recurring needs the owner's key. Gate: client registry suite **23/0**, example compiles, fmt/clippy clean.
- **winget deprecation fixed.** The old 1.4.3 bootstrap PR was submitted on manifest schema `1.6.0` and carried `Manifest-Version-Deprecated`; the replacement 1.11.0 PR (#481953) was opened on `1.12.0` (no deprecation label), and on 2026-11-17 the three manifests were bumped again to **`1.28.0`** - the schema current merges use - with the same identifier/MSI/sha256, `winget validate --manifest` passing locally on winget 1.29.380, and the new head re-pushed to the PR branch (validation pipeline re-ran). Remove this as a concern unless the moderator reports something new.
- **AUR published (1.11.0-1).** Done end-to-end through the computer-use browser and the Windows git client: the AUR account key was registered via the account form (Chrome's password-manager autofill supplied the confirmation field; no password was typed or read by the agent), the profile save also required normalizing the Homepage field to `https://chrissinclairdesign.com` because AUR validates it as a full URL, and the first push went through (`ssh -T aur@aur.archlinux.org` works; commit `e690531`; PKGBUILD/.SRCINFO byte-identical to `dist/aur`). The package page now shows **lightspeed-bin 1.11.0-1**, submitter/maintainer `chrissinclair`, three v1.11.0 sources; `AUR_SSH_PRIVATE_KEY` is stored as the repository secret, so `publish-aur` is armed for future tags. AUR's RPC/search index can lag a freshly created package (returns `resultcount: 0` while the page is live) - the verification docs check both.
- **WinDivert cleanup (task-11) closed.** With no `lightspeed-gui` process and no WinDivert service, an elevated one-liner removed the product's stale rules: **614 -> 543 total, `LightSpeed WinDivert Tunnel*` 71 -> 0** (script + log in `wat/evidence/2026-11-17/windivert-cleanup/`); firewall profiles, adapters and DNS untouched.
- **Reporter images actually reviewed.** All 7 image attachments across every issue (open and closed) were downloaded and viewed, and the follow-ups use them: **#148 Aion 2** is settled - the screenshots show the world connection is **TCP** (`ESTABLISHED ...:13328` plus TCP 443) with only a loopback UDP socket, so there is no remote UDP port to profile and the request is blocked on the TCP feature (#176), not on the reporter; **#168 Delta Force**'s screenshot is the in-game region selector (latencies, no endpoints), so the app-trace ask stands. Both issues got image-informed comments ([#148 comment](https://github.com/ShibbityShwab/lightspeed/issues/148#issuecomment-6936528459), [#168 comment](https://github.com/ShibbityShwab/lightspeed/issues/168#issuecomment-6936528994)); `docs/game-requests.md` rows updated.
- **CI follow-through on the new publishers.** The three new hermetic suites first executed in CI and found four real defects, fixed in `d6803f1`, `4d6d191`, `4548048`, `c8bff35`: the scripts were committed 100644, so direct-invocation usage probes exited 126; the Scoop stubbed `gh` never recorded the PUT message the assertions read; the Scoop script validated `GH_TOKEN` before `--bucket-repo` shape, so a malformed repo reported a missing token instead of the usage error; and the AUR publisher parsed `source_*` at column zero while makepkg's `.SRCINFO` is tab-indented, so it refused the shipped file. Final CI run `44010542448` on `c8bff35`: **14/14 jobs green**, including `Release publish self-test`, `Scoop publish self-test` and `AUR publish self-test`.

**Verified:** full local gate after the changes: fmt clean, workspace clippy clean (two pre-existing manifest warnings), **1,745 workspace tests / 0 failed / 7 ignored**, GUI **184/0/1**, GUI clippy clean (`wat/evidence/2026-11-17/lead-gate/final2.log`). CI run `44010542448` on `c8bff35`: **14/14 jobs green**, including the three publisher self-tests. Channel evidence in `wat/evidence/2026-11-17/channels/` (106/106 static checks, `winget validate` success, 3/3 tarball hashes for the AUR PKGBUILD, shellcheck 0 findings, YAML parse); Chocolatey push run `44006128768` 201; Scoop bucket readback 1.11.0; winget PR #481953 open and valid; registry tests 23/0; WinDivert before/after in the cleanup log.

**Open:** winget PR #481953 needs a winget-pkgs moderator (then `winget-releaser` takes over); AUR first push needs the owner's AUR account + `AUR_SSH_PRIVATE_KEY` (and the PKGBUILD bump is still manual - a `bump-aur` job is the follow-up); Chocolatey 1.11.0 is awaiting moderation; the M1 drain (task-10) is unchanged; machine translations remain unreviewed.

### 2026-11-17 - Full GitHub + network audit round: backlog triaged, PRs merged, release pipeline hardened, and the ALPN drain measured (NOT complete)

**Agent:** lead + Agent Teams: `net-eng` (fleet/registry audit), `infra-dev` (release pipeline), `infra-dev-2` (health-monitor instrument), `interface-designer` (GitHub backlog + docs), `dev-ops` (host network + WinDivert hygiene). Five shared tasks; every teammate reported with evidence under `wat/evidence/2026-11-17/`.

- **GitHub audit scope.** 7 open issues, 10 discussions, 3 open PRs, latest release `v1.11.0`; repository default workflow permissions remain `read` (the Release workflow's explicit `contents: write` is what matters); `master` is unprotected; the scheduled `Health Monitor`/`Health Anomaly Monitor` runs are green.
- **Three PRs merged (all CI-green, squashed).** #173 `fix(gui)` (community: action bar kept on screen; the first-time-contributor runs were pending maintainer approval and were approved; all 14 PR CI jobs then passed; local GUI suite 184/0/1 on the merged tree), #174 `webbrowser 1.2.7`, #175 `maxminddb 0.33.0`.
- **Release pipeline root cause + fix (task-9, "403 on POST /releases").** A throwaway-branch probe (`workflow_dispatch`/push, `permissions: contents: write`, run `43989309532`) created a git ref and a draft release with the Actions token: **the 2026-11-16 403 is not reproducible** and the runner log then already showed `Contents: write`, so it was environmental (that window also hit an artifact-egress 503), not a persistent permission gap. The tag path is now hardened rather than re-permissioned: `infra/scripts/publish-release.sh` (idempotent create-or-`--clobber`, bounded 5xx/429/403 retry, actionable persistent-403 verdict, `--dry-run`, refuses `pull_request`), `release-state` gates Homebrew/Chocolatey/Scoop/winget/`verify-bumps` on the **published release with every expected asset** instead of `host` success, `workflow_dispatch` became a read-only probe, and `infra/scripts/test_release_publish.sh` (64 assertions, stateful `gh` stub) runs in `Release` on every event and in `Infra Script Tests`. Background and manual fallback documented in `docs/deploy-proxy.md`.
- **Release-pipeline verification (no tag needed).** `Release` dispatch run `43991546508` on `c837a84`: `plan`, `Release publish self-test` and `Release publish probe (read-only)` success, and every publish/bump/announce job skipped - a PR/dispatch cannot publish. CI run `43991534350` on the same head: **14/14 jobs green** including `Infra Script Tests`, whose new `Release publish self-test` step passed. The real tag path (create-or-clobber, bumps + `verify-bumps`, choco/winget) is proven only by the next release.
- **Health-monitor instrument (F6 follow-up, task-7/task-8).** The `saved_app` metric-name typo and the signed-delta rule had already been fixed in `76fb04b`; this round closed the remaining deploy-burst blind spot: the collector now treats a per-relay **version change** as a restart in addition to a probe-counter rollback (a young process replaced by one that already overtook its counters was invisible). +7 collector-suite assertions and +4 anomaly-suite assertions pinning the producer->detector negative-count chain; `Infra Script Tests` green.
- **Network audit (task-10 meter).** All 8 relays: `healthy`, `1.11.0`, `control_alpn=required`, same processes since the 2026-11-16 rollout restart; 742,116 packets / 133.4 MB relayed, 84 sessions, 55,097 drops (99.2% auth rejections), `rejected_other=0`. **M1 (`rejected_no_application_protocol`) is still increasing**: +390 rejects in 102.9 s (~13,650/h) with 0 accepted handshakes, and +227,826 rejects vs +946 accepted (~241:1) since the flip. The three "flat" relays had zero accepted handshakes in the window, so they are **not** evidence of a clean window. The drain is NOT complete: the `§7.4` replacement gate (accepted >= 200 and M1 == 0 across a multi-hour per-relay window) stays open, and no relay may be declared clean. Both `Deploy Proxy` runs triggered by the merges were version-keyed no-ops (fleet uptimes continuous, config unchanged).
- **Registry audit.** Published `registry.json` signature verifies (39/39 checks) against the compiled operator key and the payload is byte-identical to `web/registry.json`. `published_at` is older than the payload's `issued_at`, but that is cosmetic: `published_at` is a monotonic anti-rollback floor (not an expiry) and nothing reads `issued_at`; no re-sign is needed. The tooling gap is closed — `sign_registry` stamps both timestamps by default and `Registry` models `issued_at` so a re-sign cannot drop it (tests added).
- **GitHub backlog triaged (task-3).** 10 public comments (3 issues, 7 discussions), `blocked` label on #168, discussion #69 closed resolved; every action URL is in `docs/github-triage-2026-11-17.md`. `docs/game-requests.md`/`docs/supported-games.md` corrected: the TCP tunnel **is shipped** (protocol v5, Minecraft Java reference profile), the Tibia profile is UDP-only so it cannot accelerate Tibia, user/custom profiles still reject `tcp_ports`, and #176 is the tracker. No dates or compatibility promises.
- **Host network + WinDivert (task-11).** The host's only active path is a **USB RNDIS tether** (10.193.246.198/24, MTU 1500), no VPN/WARP adapter; all 8 relays reachable; real client control-plane + tunnel traffic to relay-sgp-1 worked (ALPN negotiated, 5/5 pings, 46.4 ms avg). The **71 stale `LightSpeed WinDivert Tunnel` firewall rules remain**: the cleanup was attempted and refused with `Access is denied` because the session runs at Medium integrity (no elevation), and the WinDivert driver/service is absent. The exact elevated one-liner is recorded in `wat/evidence/2026-11-17/host-network/report.md`.
- **Cleanups.** `pooled_estimate` is test-only (the canonical ranking reads per-pair measurements), `docs/architecture.md` names `DestinationAwareSelector`; the `lightspeed-windows` skill now records the real cause of the recurring `cargo test` `Access is denied`: workspace binaries run at **Low mandatory integrity** and cannot write Medium-integrity `%TEMP%`; the verified workaround is `TEMP`/`TMP` inside the workspace (with it, the whole gate is green).

**Verified:** local Lead gate on `c837a84`: `cargo fmt --all --check` clean; workspace clippy clean (only the two pre-existing unused-dependency manifest warnings); **1,741** workspace tests / 0 failed / 7 ignored (31 suites); GUI **184/0/1**; GUI clippy clean (`wat/evidence/2026-11-17/lead-gate/final.log`). CI run `43991534350` (head `c837a84`): **14/14 jobs success** incl. `Infra Script Tests` (new release-publish self-test), `QUIC Integration Tests`, `Feature Matrix (full|quic|ml)`, `Windows Build & Test`, `Windows GUI Build`, `End-to-End Test`, `Localization Checks`. `Release` dispatch `43991546508` success (self-test + read-only probe, publish jobs skipped). `Security Audit`, `CodeQL`, `Docker`, `Deploy Proxy` and `Deploy GitHub Pages` all green on the same head. Fleet stays 8/8 healthy `1.11.0`/`required` with continuous uptimes after both deploy runs. Evidence: `wat/evidence/2026-11-17/{lead-gate,release-probe,release-publish,net-audit,task2-*,host-network,actions.md,replies/}` and `docs/{network-audit,github-triage}-2026-11-17.md`.

**Open:** (1) task-10 - M1 still rising; needs a multi-hour accepted>=200 / M1==0 window, and the **owner decision** on whether to roll a relay back to `off` for stranded pre-1.11.0 clients (no fleet change was made this round); (2) the next tag must prove the real publish path end-to-end (choco/winget publish, bumps, `verify-bumps`); the winget package is still not bootstrapped in `microsoft/winget-pkgs`; (3) the 71 stale WinDivert rules need one elevated command; (4) the repository registry is byte-valid but stale (`published_at`/`issued_at`) and needs the owner's offline signing key; (5) #176 TCP profile/exposure work and the remaining game requests stay blocked on reporter facts; (6) machine translations remain unreviewed.

### 2026-11-16 - WF-067 rollout: 1.11.0 released and the fleet flipped to `required` under an explicit owner traffic-impact decision

**Agent:** lead + `interface-designer` (1.11.0 CHANGELOG, task-8) + `infra-dev` (flip tool, dispatch job and operator procedure, task-7). Follows the stage A/B/F/C/D entry below.

- **Owner authorization (recorded verbatim):** "You are in charge of project do your due diligence and it is fine to impact traffic for a novel update" (2026-11-16). That is the decision the plan reserves for the stage-1 release and the per-relay `required` flips (§11); the rollout was executed against it and the measured impact is recorded here.
- **Due diligence before touching production.** The deploy workflow is version-keyed: our same-version pushes ran six real deploys that were all no-ops ("already at 1.10.3; nothing to do"), so the fleet was still 1.10.3 with no `control_alpn` field. The release path is a tag -> cargo-dist -> GitHub release whose body is the CHANGELOG section. Relays run a verified self-updater against `releases/latest` (sha256 + health-gated). The §7.4 window (n>=200 accepted handshakes) is unreachable on this fleet scale: the busiest relay measured ~120 accepted handshakes/hour, so the gate would need ~100 minutes of quiet traffic before a flip and can never see a clean sample before a client release exists. The 1.11.0 client was built locally and tested against a live stage-0 relay before release: it offered `lightspeed-control/1`, took exactly one alert-120 fallback and registered `alpn=none` - the production shape of §3 row C.
- **Release 1.11.0.** Version bump + the 99-line CHANGELOG entry (changelog body = GitHub release body per CONTRIBUTING) + the flip tool/operator procedure landed in commits `1a66597`, `2de1665`. CI on `2de1665` was green on every job (including `Infra Script Tests`, whose new step first executed the flip self-test). Tag `v1.11.0` pushed.
- **The Release workflow failed at publish and the release was completed manually.** CI run `43881597392`: every build job green (client/proxy/GUI for 5 targets, attestations, global artifacts), then the `host` job failed at "Create GitHub Release" with `HTTP 403: Resource not accessible by integration` on `POST /releases`. Reruns failed identically. The repo-level workflow-permission setting was set to read; changing it to write and rerunning did not help, and the setting was reverted to read (the `token-diag` probe workflow added for diagnosis was removed in `bdae448`). Artifact download also hit `503 Egress is over the account limit` once; retrying succeeded. **Workaround used:** downloaded the run\u0027s build artifacts (verified one sha256 against `sha256.sum`), extracted the CHANGELOG section as the body, and published `v1.11.0` with our own token: 39 cargo-dist assets, latest release, body = changelog. Then bumped the in-repo packaging manifests from the published asset hashes (Scoop, Chocolatey, Homebrew) in `bdae448`; the workflow\u0027s choco/winget publish jobs did not run (board task-9).
- **Stage 2 (inert relay build).** Dispatched `Deploy Proxy` (run `43881814576`) -> all eight relays installed 1.11.0 with `control_alpn=off`, health-gated per relay; verified `/health` `version=1.11.0 control_alpn=off` and the new counter family present on all eight.
- **Canary (relay-fra) and the measured §7.4 Fail.** Dispatched the new flip job (`control_alpn_node=relay-fra`, confirm string) - it edited `/etc/lightspeed/proxy.toml`, `--check`ed the deployed binary, restarted and health-gated the new mode. Live verification: a 1.11.0 client negotiated `lightspeed-control/1` and registered; the **official 1.10.3 client failed with alert 120** exactly as designed. M1 (`rejected_no_application_protocol`) then measured **66 -> 520 in 120 s** with **0 accepted** handshakes in that window: pre-ALPN clients are in the field and retry aggressively. That is the plan\u0027s Fail branch (wait, roll back, or accept); per the owner authorization the decision taken was **accept**, and the fleet flip proceeded. This deviation from the n>=200 clean-window requirement is deliberate and recorded in `docs/quic-alpn-rollout.md` and the decision log.
- **Stage 4 (fleet).** Remaining seven relays flipped one at a time through the same dispatch path (the workflow\u0027s concurrency group serialises them; a first batch-dispatch attempt cancelled most queued runs - recovered by dispatching sequentially and waiting for each). Final state: **8/8 relays healthy, `version=1.11.0`, `control_alpn=required`**, `rejected_other=0` everywhere, `negotiated_v1=17` and `rejected_no_application_protocol=6098` at the snapshot (`wat/evidence/2026-11-16/wf067-rollout/fleet-post-flip.json`). A second live check against relay-lax-1 repeated the result (new client negotiates and registers, old client gets 120).
- **Docs/state follow-through.** `docs/quic-alpn-rollout.md` status/§13 now record the rollout and the measured Fail; `docs/deploy-proxy.md` and `docs/community-network.md` carry a fleet-status note (all eight relays required since 1.11.0; pre-1.11 clients must update); the operator procedure, flip tool and its hermetic self-test are committed. The 1.11.0 CHANGELOG warns operators not to enable `required` before the fallback client is in the field - the project deliberately took the other branch on owner instruction.
- **Local end-to-end verification (2026-11-16, at the owner's request).** Two loopback relays (one `off`, one `required`) built from HEAD, the **published** 1.11.0 Windows client artifact (sha256 verified against the release asset) and the official 1.10.3 client: **30 checks passed / 0 failed** (`wat/evidence/2026-11-16/wf067-local/`). It proves on this host: `--check` accepts `required` and rejects `control_alpn = "optional"` naming the allowed values; both startup lines and `/health` report the configured mode; the released 1.11.0 client registers `alpn=none` after exactly one fallback against an `off` relay and negotiates `lightspeed-control/1` with no fallback against a `required` one; the official 1.10.3 client still works against `off` and is rejected with alert 120 by `required`; the relay metrics count exactly those outcomes (`negotiated_v1=1`, `rejected=1`, `no_alpn=0`, `rejected_other=0`); a real `--test-tunnel` through the required relay registered via ALPN and the relay forwarded all 5 packets (session created); and a rollback (stop -> config `off` -> restart) restores the old client while the new client falls back. Loopback only; the POSIX flip tool's own editing path remains covered by its hermetic CI suite.
- **Live product sanity test on the owner's ROB RUN (2026-11-16, owner in a match).** Built and ran the **1.11.0 GUI** (same HEAD; it links the client with `quic + windivert-redirect`) elevated while `ROB RUN.exe` was live. It auto-detected the game, chose **relay-lax-1**, negotiated **`alpn=lightspeed-control/1`** on the first handshake against the required relay, registered (`session=2495553169`), and relayed the live server session `103.22.193.161:36702`: **11/11 judged windows healthy** (share 96.0-101.7%, streak 0, probe accepted, **0 WARN/ERROR**), and relay-lax-1 `lightspeed_packets_relayed_total` rose **+3,438 packets in 75 s**; client-relay RTT ~211-219 ms vs direct ~292-294 ms in the same windows. Stopped afterwards; the one WinDivert rule the run added was deleted and `ROB RUN` stayed up. The host still carries **71 stale `LightSpeed WinDivert Tunnel` rules** from earlier force-killed instances (recorded as cleanup debt, board task-11). Evidence `wat/evidence/2026-11-16/wf067-local/gui-live-window.log` + `gui-live-summary.md`. The CLI release artifact itself has no `pcap-capture` feature, so its live mode only does keepalive; the GUI is the Windows interception path (verified here) - running the released CLI against a required relay still proved the control plane: ALPN negotiated, registered, 50-60 ms relay RTT.
- **Normal product path installed (2026-11-16, owner request).** Installed the **published 1.11.0 GUI MSI** from the release (sha256 `c4ca8674...` verified; `msiexec /qn` exit 0) to `C:\Program Files\lightspeed-gui\bin\` with Start Menu shortcut and Add/Remove entry `lightspeed-gui 1.11.0`, then launched the **installed** exe elevated while the owner was still in ROB RUN. The installed build auto-detected the game, negotiated **`alpn=lightspeed-control/1`** with relay-lax-1 on the first handshake, registered, opened its WinDivert intercept/inject/shadow handles, locked the live server `3.237.221.191:50082`, and relayed it: 3/3 judged windows healthy, 0 WARN/ERROR, relay-lax-1 `packets_relayed` +70 in 45 s, relay RTT ~210-221 ms vs direct ~281-289 ms. **Left running** (tray icon); it removes its own firewall rule on graceful quit. A normal Start-Menu launch starts unelevated and needs the in-app "Restart as Administrator" click (existing product behaviour). Evidence `wat/evidence/2026-11-16/wf067-local/installed-product.md` + `msi-install.log`.
- **Follow-ups filed:** board task-9 (fix the Release workflow publish path: 403 on `POST /releases`, artifact egress 503, and the skipped choco/winget publish); board task-10 (watch M1 drain to a flat window as the §7.4 replacement, then the later §5 retirement decision); board task-11 (clean stale WinDivert firewall rules left by force-killed instances - 71 found on this host). Public #140/#147 replies remain an owner decision.

**Verified:** release published with 39 assets and the changelog body (`v1.11.0`, latest); fleet 8/8 `required`/healthy on 1.11.0; live new-client negotiation and old-client rejection on two different relays; **local end-to-end suite 30/0** (published client artifact vs loopback off/required relays, official 1.10.3 client, config `--check`, metrics accounting, `--test-tunnel` data plane through a required relay, rollback simulation) plus targeted `proxy --lib --features quic` 159/0 and `integration_alpn` 3/0 on this tree; flip tool first executed in CI (`Infra Script Tests` green on `2de1665`) then live eight times with health gates; CI green on `2de1665` (every job). The 1.11.0 release commit was `2de1665`; the packaging bump `bdae448` and this state commit come after.
**Open:** the release-automation 403 is unresolved (task-9); the choco/winget publishes did not run; pre-1.11.0 clients cannot register on any community relay until they update (owner accepted, M1 is the meter); the §7.4 n>=200 clean window was not met before the flip (measured Fail, explicitly accepted); the fleet is now all-`required` so a relay rollback is `--mode off` + restart, one relay at a time; the machine translations of the 1.11.0 docs are still unreviewed.

### 2026-11-16 - WF-067: the versioned-ALPN control plane lands client-first (stages A/B/F/C/D, unreleased)

**Agent:** lead + `rust-dev` (task-1, stage A) + `rust-dev-2` (task-2, stage B) in parallel, then `interface-designer` (task-3, stage F), `infra-dev` (task-4, stage C), `qa-engineer` (task-5, stage D). The board is session-scoped, so the handoff's task-26/27/30/28/29 were recreated as this session's task-1/2/3/4/5; task-6 (handoff task-4, operator procedure) is queued.

- **task-1 stage A - protocol + client offer/fallback/observability.** `QUIC_CONTROL_ALPN_V1 = b"lightspeed-control/1"` and `quic_control_alpn()` in `protocol/src/control.rs` (re-exported; byte-for-byte + RFC 7301 tests). The client offers the name, retries **once** with an empty offer and only on `ConnectionClosed(crypto(0x78))` (the same `is_alert_120` predicate feeds the verdict, so classifier and fallback cannot drift; timeout/other-alert/app-close never retry), remembers the answer per relay for the process in `supervisor::Entry`/`SupervisorContext` (carried across a destination-driven replacement, no global/disk cache), logs `alpn=<name>|none` on the established line and exposes `ControlClient::negotiated_alpn()`; the verdict now names **both** causes (QUIC-intercepting middlebox **or** ALPN version skew) and says the empty-offer retry already failed. Mutation: retry predicate inverted -> 4 fallback tests fail; restored SHA256-identical.
- **task-2 stage B - relay mode + observability + matrix.** `[network] control_alpn = "off"|"required"` (typed; an unknown value is a startup error naming both allowed values), `off` byte-for-byte today, `required` advertises the shared name. Startup line gains `alpn=off` / `alpn=required (lightspeed-control/1)`; the silent accept-loop rejection became a classified `warn!` + counters; `GET /health` gains `control_alpn`; `lightspeed_control_handshakes_total{outcome=...}` emits all four series from zero so the §7.4 canary gate reads 0 rather than an absent series; the five §6.3 matrix tests pin rows A/C/D/B/E over real handshakes. Smoke script asserts both startup modes and both `/health` values (CI-only: POSIX). Mutation: force `off` under `required` -> row D + row B fail; restored hashes match.
- **task-5 stage D - manual two-relay evidence, 4/4 PASS.** New client vs an `off` relay: one offer line, exactly one fallback warning, `alpn=none`, registered. New client vs a `required` relay: `alpn=lightspeed-control/1` on attempt 1, zero fallback warnings. Both relays' startup lines and `/health` agree; counters split as expected. Decisive row B: the **official v1.10.3 release binary** (sha256 verified) registers against `off` and fails against `required` with alert 120 while the relay logs and counts `rejected_no_application_protocol`. The run-2 claim was mutated by restarting the same relay as `off` (fallback then happens), so negotiation is caused by the relay mode.
- **task-4 stage C - cross-crate interop test in the existing CI job.** `proxy/tests/integration_alpn.rs` (3 tests: rows C->C', D and B over real `ControlClient` <-> `ControlServer` handshakes) runs in `QUIC Integration Tests` with no workflow change; needed one dev-dependency edge and one `ControlClient` re-export in the existing `#[doc(hidden)] test_support` module (Lead-approved scope extension). The first full run of the job command caught a **pre-existing harness regression from stage A**: `client/tests/control_reconnect.rs` counted every quinn `accept()` - including the alert-120 fallback probe - as a connection, so its "close the first connection after ack" branch targeted the rejected handshake and the suite went red (3/3 reproducible). Fixed by indexing only handshake-completed connections; no assertion weakened. Full job command now 25 targets, 1,492 / 0 / 2.
- **task-3 stage F - user docs.** `docs/troubleshooting.md` gained the alert-120 entry (the plan assumed one existed; there was none), quoting the verdict verbatim and distinguishing middlebox from version skew with `--test-control`/`/health` narrowing; `docs/protocol.md` names the identifier, versioning rule and its independence from `PROTOCOL_VERSION`; `docs/CLI-REFERENCE.md` documents `control_alpn`; `docs/README.md` indexes the plan. 14/14 code anchors match the landed code, 31/31 links resolve.
- **CI follow-up (same round): the first push went red once and the cause was a real race.** CI run `43874448523` on `3f268d2` failed only `QUIC Integration Tests` (81 s): `a_required_relay_negotiates_the_shared_name` panicked with `keys may not be consistent: KeyMismatch` at `ControlServer::bind`. The five §6.3 matrix tests bind control servers concurrently in one lib-test binary, and first-boot TLS generation is a check-then-generate path: two callers could both see an empty `LIGHTSPEED_TLS_DIR`, each generate a pair and interleave the writes, pairing one certificate with another's key. It never reproduced locally (158/0 repeatedly) or in `Feature Matrix (quic)`; it needs the two tests to hit the window together. Fix: the load-or-generate body is now `ensure_cert_in(dir)` under a process-wide `TLS_CERT_LOCK` (production behaviour unchanged - one generation, later readers). Regression test `concurrent_first_boot_generation_returns_one_matching_pair` runs 8 barrier-synchronised threads on one empty directory and asserts one certificate/key pair that rustls accepts; **mutation (lock removed) failed 10/10 runs**, restore was SHA256-identical and the full proxy quic lib is 159/0. Re-gated: the CI quic command **1,493/0/2**, workspace **1,557/0/6**, fmt/clippy clean. Evidence `wat/evidence/2026-11-16/wf067-tls-race/`.
- **Lead integration fixes.** `proxy/src/main.rs` now publishes the configured `control_alpn` for `/health` in every build (a non-quic build has no `ControlState`, so it reported a silent `off`). `docs/quic-alpn-rollout.md` header/§6.4/§6.5/§7.2 corrected for what shipped (status, the closed CI gap, `--verbose` instead of `RUST_LOG` - both binaries hard-code the filter, and the quoted `reason="..."` rendering); the same-day audit's "the responder cannot be a LightSpeed relay / `ALPN=[]`" paragraph carries a correction. Board task-6 (operator procedure) remains pending.

**Localization pipeline fixed as part of the docs landing.** The English troubleshooting entry created 8 changed localized pages, and the documented retranslation path failed for every language: the free public translator converts markdown backticks into typographic quotes, so every retranslation lost its fences and `code_commands` rejected it (and the banner branch inserted the warning only when the page did **not** start with an H1). `scripts/localization-sync.py` now masks fenced blocks and inline code spans with `ZXQnQXZ` tokens (probed against the live endpoint for de/ja/ru; `{n}`/private-use/`[[n]]`/%%n%% all failed at least one language), inserts the banner whenever it is missing, retries a dropped token up to 3 times, and falls back to paragraph-at-a-time for a page the whole-document call cannot round-trip (the ru case). The 8 pages were retranslated through the fixed path and `Localization Checks` is green locally; the translations stay unreviewed by design (banner + `localization-report.md`).

**Verified (Lead gate on the integrated tree, counts per command):** `cargo fmt --all --check` clean; workspace clippy clean (only the two pre-existing manifest warnings); **1,557** workspace tests / 0 failed / 6 ignored; **181** GUI / 0 / 1; **1,261** WinDivert-feature / 0 / 4; the CI `QUIC Integration Tests` command **1,493 / 0 / 2** (incl. the new 3-test interop suite, re-run after the TLS-race fix); client quic lib 640 / 0 / 1; proxy quic lib **159** / 0 / 0; localization `--check` + `localized-docs --check` exit 0; `git diff --check` clean. Evidence `wat/evidence/2026-11-16/wf067-{a-client-alpn,b-relay-alpn,c-interop-ci,d-interop,f-docs,lead-gate,tls-race}/`. **CI verification: run `43875765733` on `88efac2` is green** - every job success, including `QUIC Integration Tests` (the job the first push failed), `Proxy QUIC Smoke` (the release-artifact ALPN mode assertions), `Localization Checks` (the retranslation), `Check & Test`, `Windows Build & Test`, `Windows GUI Build`, `Feature Matrix (quic|full|ml)`, `Infra Script Tests`, `macOS Smoke Test`, `Code Coverage`, `End-to-End Test` and `Deploy Canary Tests`; the sibling `Security Audit`, `Docker` and `Deploy Proxy` workflows are green on the same head. The first push (`3f268d2`) was red on `QUIC Integration Tests` only - that race and its fix are the CI follow-up bullet above.

**Open:** the stage-1 client **release** and every per-relay `required` flip remain owner gates (§11) - nothing ALPN-related is released and no relay config was touched; the §7.4 canary window (n>=200, M1==0) has not been run (needs a relay at stage 3); stage D is loopback-only (no WAN/middlebox/IPv6/loss evidence); machine translations are unreviewed; an at-capacity accepted handshake is not counted in `negotiated_v1`/`no_alpn` (observability gap at the cap, not a gate input); the POSIX smoke script is verified by the green `Proxy QUIC Smoke` job (it cannot run on this Windows host); board task-6 (operator procedure) is the next code-free deliverable.

### 2026-11-16 - WF-066: durable team, live validation on ROB RUN, monitor instrument, second ranking retired

**Agent:** lead + `rust-dev-4` (task-22), `rust-dev-3` (task-23), `infra-dev-3` (task-7), `qa-engineer-4` (task-21), `interface-designer-3` (task-24) - every teammate was a **resumed durable instance**, not a new spawn (first round under the rewritten Step 2).

- **Process (`wat/team-mode.md`):** Step 2 now says a teammate is a persona instance, not a task: one live instance per persona, reassign + `send_message` to wake it, and spawn only when two ready tasks need the same persona in parallel, when the context is contaminated (an implementer must never verify its own patch), or when the instance is unusable. The decision log records the active roster and retires all earlier instances; the platform has no delete tool for idle agents, so retirement is by name.
- **task-21 `qa-engineer` - live validation on the owner's ROB RUN (17 m 04 s, LAX):** 129 judged windows (128 healthy, 1 warm-up `too_few_replies`), 0 fallbacks, 0 relay changes, 0 WARN/ERROR. **2 accepted quiet-flow samples in 17 min** while the game kept 176-211 intercepted packets per window -> **falsifier F1 is confirmed: v1 quiet-flow is NOT viable on a chatty game**. Shadow sampler accepted 1 / rejected 204 (0.49%). One telemetry report was proven sent with the app pair omitted (300 s TTL); `Measured` never published (n=1 < 8) - INCONCLUSIVE. The new per-window INFO line carried `probe_accepted=2 probe_rejected=1 shadow_accepted=1 shadow_rejected=161` live, so the Lead's counter fix is proven end to end.
- **task-22 `rust-dev` - deterministic timeout accounting (QA F4):** `probe_counters()` now sweeps expired armed probes under the same lock, so an unanswered probe is counted once even with no further traffic; late replies cannot double-count. +3 tests, mutation-checked. Named semantics change: counters now advance on read. Residual: a probe armed within the last window before process exit is still uncounted (needs a teardown hook).
- **task-23 `rust-dev` - the CLI's second ranking is retired:** the CLI's default `destination` strategy (`selector.rs`) had its own ordering and could disagree with the canonical key (failing-before: old rule picked EU, canonical picks US). `DestinationAwareSelector` is now a thin adapter over `relay_choice::rank`; ~515 lines and five constants removed; Nearest/Ml untouched; `pooled_estimate` is now test-only.
- **task-24 `interface-designer` - two live findings fixed:** the per-window INFO line now carries the shadow rejection breakdown (`shadow_rejected=204(no_icmp=0,floor=204,timeout=0,relay=0)`), and the `Server leg` row states the triangle bound as soon as a fresh direct ping proves it, instead of waiting up to 600 s for the next re-race (same sentence/catalog key, no re-rank, no relay change; `docs/design/interface.md` driving fact updated). Live trace proves the early-bound state existed for 57 s; the screenshot of that interval was a capture miss and is reported as such.
- **task-7 `infra-dev` - monitor instrument:** signed sums are differenced correctly (restart detection from forward-only probes), the scripts read the emitted `lightspeed_telemetry_saved_app_negative_count` (fixtures updated; the old name exists on 0 relays), `saved_regression` needs >=20 weighted samples AND dual-metric agreement and rejects restart-bridged spans, and a new `relay_unreachable_streak` detector covers the ~16-day blind spot (relay-fra 65 snapshots / 389.8 h historically). Real-data replay on 316 snapshots: old rule fires 4 times, new rule 0.
- **New live finding (task-25, fixed in the same round):** a relay choice for a
  *new* game server consumed the *previous* server's fresh direct ping as its
  floor (`follow_game_server` runs before `poll_relay_health` refreshes
  `server_ping`). The direct ping is now a `ServerPing { server, ms, age_secs }`
  record keyed to the server it measured; every consumer (ranking floor input,
  choice-trace comparison, `Server leg` bound, header metric, `Server ping` row,
  latency trace) passes `None` for a foreign ping - never the previous server's
  value. Contract documented in `docs/design/interface.md`; +4 tests and a
  mutation that reproduces the old leak; GUI 177 -> 181. The server-change line
  was not captured live (no rotation in the 15-min watch) - reported as
  unverified, with the pre-fix bug and the matching-server control both shown in
  the trace.

**Verified:** Lead gate on the integrated tree - fmt clean; workspace clippy clean; **1,508** workspace tests across 29 suites, **177** GUI, **1,261** WinDivert-feature and **857** QUIC-lib passed, 0 failed (the drop from WF-065's counts is the 8 retired selector tests plus net test-suite arithmetic). Evidence `wat/evidence/2026-11-16/{wf066-live,latency-timeout-accounting,ranking-reconciliation,infra-monitor-fixes}/`.

**Open:** the next workstream is ALPN stages A/B/F (board tasks 26/27/30 and the dependent 28/29; code is owner-approval-free, only the stage-1 release and per-relay flips are gated). Small cleanups: `pooled_estimate` is test-only, `docs/architecture.md` needs the selector one-liner, translation debt 7x8. The quiet-flow `Measured` state is unreachable on this game and F1 says do not chase it; the v2 relay-vantage probe needs fleet access and its fixed gates. No Discord alert sink exists and the owner does not use Discord - monitor alerts stay as workflow failures/logs (decided 2026-11-16; do not ask for a `DISCORD_WEBHOOK`). The ICMP-blocked server session crossed the watchdog return floor repeatedly (product behaviour, recorded, not investigated). CI green on `76fb04b` (14/14 jobs incl. `Infra Script Tests` and `QUIC Integration Tests`) and on `bc6177d`.

### 2026-11-16 - WF-065: canonical ranking key, validated app telemetry, locale honesty, CI that runs the quic suites

**Agent:** lead + `rust-dev` (task-17) + `rust-dev` (task-18) + `infra-dev` (task-19) + `infra-dev` (task-5) + `qa-engineer` (task-20 independent verification)

Four parallel deliverables, then independent QA, then the Lead re-gated the
integrated tree.

- **task-17 `rust-dev` - canonical whole-path ranking key.** `sort_scored` is now
  `(total_leg_us, jitter_us, relay)`; `JITTER_WEIGHT` and the GUI's interim
  `whole_path_cmp`/`whole_path_order` are deleted (GUI consumes engine order).
  Counterfactual pinned by
  `the_whole_path_total_outranks_the_destination_bucket` (EWR 368,882 us cannot
  outrank LAX 283,474 us); route suite 13 -> 16, GUI 178 -> 175. Named
  consequence: with only the country-centroid prior the ranking cannot separate
  the US coasts (LAX 37.1 vs EWR 41.3 ms vs a true 63 ms), so the client leg
  decides - the argument for a measured L2, documented in
  `docs/server-leg-signal.md` §4.3.
- **task-18 `rust-dev` - the other half of app telemetry.** A shadow-direct
  sample now needs the relay unchanged, a fresh measured ICMP median, the
  absolute 1.0 ms floor AND `>= 0.5x ICMP`; rejections are counted and nothing is
  clamped. `app_saving_report()` is the single publication gate, so an
  unvalidated pair is unrepresentable: the report omits `direct_app_p50_ms` and
  sends `saved_app_pairs = 0`, and the relay stops advancing
  `lightspeed_telemetry_direct_app_ms_*` / `saved_app_ms_*` while unvalidated.
  The live defect (0.4248 ms accepted against a 284.42 ms ICMP path) is pinned by
  `a_shadow_sample_attempted_before_any_icmp_floor_is_not_accepted`.
- **task-19 `infra-dev` - locale is not location.** Metric HELP, the data
  dictionary and the collector header now say **client OS locale country, not
  location** (text only; series, labels and values unchanged); a CI doc-guard
  greps the exact phrase so it cannot regress. **No per-game dimension was added**
  to the public snapshot (decision + revisit condition in the decision log);
  `web/app.js` copy is proposed, not applied.
- **task-5 `infra-dev` - CI now runs the quic-gated integration suites.** New
  `QUIC Integration Tests` job runs
  `cargo test -p lightspeed-client -p lightspeed-proxy --features quic --tests
  --no-fail-fast`; local run 1,471 passed / 0 failed / 2 ignored over 24 targets,
  and the six formerly-dead suites are 16 tests, all passing. No existing gate
  weakened; first CI run is the final verification.
- **task-20 `qa-engineer` - independent verification.** PASS criteria 1-3, 5;
  criterion 4 PARTIAL (no game running -> live INCONCLUSIVE, not fabricated).
  4/4 mutations re-derived and restored byte-identically; grep proved the interim
  GUI re-rank is gone from code/tests/docs (only a decision-log past-tense
  mention). Low findings: shadow rejection counters debug-only and the
  data-dictionary paired-publication rows - **both fixed by the Lead** (the
  per-window INFO line now also carries `shadow_accepted`/`shadow_rejected`);
  F4 lazy timeout accounting and the out-of-scope second ranking in
  `route/selector.rs::compare_candidates` are recorded as open.

**Verified:** Lead gate on the integrated tree - fmt clean; workspace clippy
clean; **1,516** workspace tests across 29 suites, **175** GUI, **1,269**
WinDivert-feature and **861** QUIC-lib passed, 0 failed. QA gate additionally:
the task-5 quic integration command 1,471 / 0 / 2. Evidence
`wat/evidence/2026-11-16/{engine-rank-key,shadow-estimator,infra-cell-labels,ci-quic-suites,wf065-qa}/`.

**CI follow-up (same round):** the first push (`c61c88c`) had a red Windows GUI
job: the Lead's shadow-counter addition pushed `relay_reply_window_message` to
9 arguments and CI's `-D warnings` rejects `clippy::too_many_arguments`, which a
local clippy run only warns about. Fixed structurally by grouping the sampler
counters into tuple arguments (7 params, no lint suppression), verified with
`RUSTFLAGS=-Dwarnings cargo clippy -p lightspeed-gui --all-targets`, and re-gated;
the failing job's log is in the CI run `43854061475`. That run confirmed both new
gates work before the fix: `QUIC Integration Tests` **success** (16 formerly-dead
tests running) and `Infra Script Tests` **success** (the locale doc-guard).

**Open:** the live in-match path for both samplers is unverified (no game
running); task-20 F4 (lazy timeout accounting); `route/selector.rs::compare_candidates`
is a second pre-existing ranking to reconcile or retire; translation debt 7x8.
CI verification is complete: run `43854981269` on `8b48805` is green with
`QUIC Integration Tests` **success** (the six formerly-dead suites) and
`Infra Script Tests` **success** (the locale doc-guard); the first WF-065 push
(`c61c88c`) was red on the Windows GUI job and its fix is recorded above.

### 2026-11-16 - WF-064: a real server-leg signal, an honest route surface, and the v2 NO-GO

**Agent:** lead + `rust-dev` (task-13) + `interface-designer` (task-14) + `infra-dev` (task-15) + `net-eng` (task-8) + `qa-engineer` (task-16 independent verification)

Four parallel deliverables landed on one integrated tree; independent QA then
re-derived the mutation checks and ran the GUI live.

- **task-13 `rust-dev` - v1 quiet-flow server-leg signal.** `latency.rs` arms a
  whole-path probe only after 1 s without inbound and accepts the next reply only
  when the relay is unchanged and `L1 <= RTT <= 5 s`; rejections are counted
  (`behind_floor`/`relay_changed`/`timeout`/`stale`) and never clamped. Accepted
  samples feed `destination::observe_tunnelled`, `DEST_FRESHNESS_TTL = 120 s`
  bounds publication, and the invalid busy-flow pairing is deleted from
  `relayed_p50_ms`/`like_for_like`/telemetry (a separate locked `--diagnose`
  window replaces it there). Prerequisite found and fixed: Windows keepalive never
  fed `record_client_relay_rtt`, so three feeds (engine + both CLI modes) were
  added. `client/tests/latency_telemetry_gate.rs` asserted the old artefact and
  now asserts the truthful `None` + `rejected_stale`.
- **task-14 `interface-designer` - honest route surface.** New `Server leg` row
  with five states (measured n+age / triangle-bounded / prior +/-25 ms / unknown /
  absent on direct), a provable "cannot be faster than direct" verdict, the client
  leg measured as **one** TCP RTT instead of a 2-RTT HTTP wall time, and a
  whole-path ranking key in the GUI (interim; engine canonicalisation is task-17).
  7 new locale keys across en + 8 catalogs (English fallback; translation debt
  7x8). 11 new GUI tests.
- **task-15 `infra-dev` - v2 feasibility: NO-GO today, conditional GO.**
  `docs/relay-server-leg-probe.md` (618 lines) + evidence. Premise corrections:
  `Register.destination` is never populated in production (the relay learns the
  destination from the data-plane header and already logs it), the relay units are
  `DynamicUser`/`NoNewPrivileges` so raw ICMP is impossible and `CAP_NET_RAW` is a
  NO-GO, and the decisive F3 measurement needs fleet access behind fixed gates
  G1-G5. Cost $0.00.
- **task-8 `net-eng` - per-game negative saving: MIXED.** Magnitudes and the
  per-game pattern are the invalid-estimator artefact (the old relayed figure was
  a one-sided lower bound; shadow collapsed to 0.42 ms), but the **sign** of the
  two `fortnite/US` cells survives an artifact-free ICMP bound (`saved_icmp`
  negative on both relays, n=6/4); `arcraiders/RU` is unsettled without an ICMP
  anchor. Two data defects: `client_country` is the OS locale, and the stats
  branch has no per-game dimension. Board tasks 18/19 filed; no drain, no
  per-game routing.
- **task-16 `qa-engineer` - independent verification.** PASS criteria 1-3, 5;
  live PARTIAL (quiet-flow n=0 on this chatty game - falsifier F1 observed). 5/5
  mutations re-derived and restored byte-identically; one pre-existing flake
  named. **Finding fixed by the Lead:** probe counters were `debug!`-only while
  the shipped GUI sink is INFO, so F1 was unmeasurable in release - the per-window
  INFO line now carries `probe_accepted`/`probe_rejected` (tested). Low doc and
  screenshot nits recorded; the "trace stall" claim was not reproduced.

**Verified:** Lead gate on the integrated tree - fmt clean; workspace clippy
clean; **1,491** workspace tests across 29 suites, **178** GUI, **1,245**
WinDivert-feature and **848** QUIC-lib passed, 0 failed. Evidence:
`wat/evidence/2026-11-16/wf064-qa/`, `server-leg-signal/{v1,gui,per-game}/`,
`server-leg-probe/`, `lead-gate.log`.

**Open:** the `Measured` state was not observed live (needs >=8 validated samples
in a 10-minute match; F1 is the falsifier, now measurable via the new counters);
tasks 17 (canonical engine rank key), 18 (shadow estimator + telemetry publication
gate), 19 (locale-vs-location labels + per-game dimension); translation debt 7x8;
`--diagnose` wiring compile/review-verified only; three `no_reply_from_relay`
fallbacks for one ROB RUN server were observed live and the relay-side cause was
not investigated.

### 2026-11-16 - WF-063: fallback state contract, QA parity fixes, and the reason relay choice is blind

**Agent:** lead + `rust-dev` (task-6) + `interface-designer` (task-11) + `net-eng` (task-12); four board tasks completed including the earlier relay-fix chain

Three parallel tasks landed, then the Lead re-gated the integrated tree.

- **task-6 `rust-dev` - QA parity fixes.** The WinDivert fatal-handle arm now
  increments `errors` as well as recording `last_error`/`run_failed` (extracted
  into `record_fatal_receive_error`, unit-tested),
  so a fallback verdict can no longer print `errors=0` beside "os error 6"; the
  "decoded" wording around `packets_reply_data` now matches what is actually
  counted (tunnel-frame decode + non-keepalive, before FEC/TCP payload decode).
  Mutation-checked; WinDivert-feature suite 1,203 passed.
- **task-11 `interface-designer` - the fallback state contract.** The design doc
  now names **eleven** states, including *re-test pending* (`relay_retry_at` from
  `ReplyFallback.due`; the SAME relay is judged again) and *relay recovered* (trace
  only, no banner). The overclaiming "relay is not dropping your traffic" became
  "the outbound leg is clean" - that is what the client can observe. Every judged
  window now emits one trace line
  (`Relayed window judged: intercepted/injected/from_relay/share/streak/verdict`),
  so a live "no false positive" proof is direct instead of absence-based. GUI 167
  tests; live session 09:09-09:13Z: 6 judged windows, shares 33.3-110%, streak 0,
  0 fallbacks/WARN/ERROR, 0 relay changes after relaying started.
- **task-12 `net-eng` - `docs/server-leg-signal.md` (the real "sub-optimal relay"
  cause).** `RegionPrior` is not a fallback: it is the **only reachable**
  destination-leg source at this commit (`destination.rs::note_inbound` records
  nothing and `observe_tunnelled` has no production caller), and both app-RTT
  estimators used by the GUI are **invalid** - 25/25 `relay_app_ms` values are
  below the measured client->relay leg (median ratio 0.129: it times the send
  interval) and the one `shadow_app_ms` sample was 0.42 ms. The relay's
  `/metrics` upstream lag cannot substitute (unpaired, relay-wide, n=0 on 6/8
  relays in a 50 s window). The chooser also compares a **2-RTT HTTP wall time**
  as `client_leg_us` against 1-RTT estimates, and the geometric prior compresses
  US-coast differences 19x. Recommendation: **v1** client-only quiet-flow
  tunnelled RTT (no wire change, $0) to make `Measured` reachable; **v2**
  owner-gated relay-vantage destination probe; keep the prior labelled. Board
  tasks 13/14/15 filed.
- **Retraction recorded:** the WF-062 entry/decision/evidence quoted
  "relay 150 ms faster than direct" from those app-RTT estimators; task-12
  showed they are invalid, so that figure is retracted in the state, the decision
  log and `lead-diagnosis.md`. The watchdog fix stands on the 24.07% ratio case,
  the hopping and the six-hour history poisoning - not on that number. The audit
  F6 relay-fra text now also marks its `saved_app` figures unsupported.

**Verified:** Lead gate on the integrated tree - fmt clean, workspace clippy
clean, **1,449** workspace (29 suites), **167** GUI, **1,203** WinDivert-feature,
**827** QUIC-lib tests passed, 0 failed. CI for the previous fix `46c1a61` is
green in all workflows. Evidence `wat/evidence/2026-11-16/` (relay-hopping/,
server-leg-signal/, rust-dev-f1f2/, lead-gate.log).

**Open:** re-test-pending screenshot blocked (needs a genuine zero-return chain;
forcing it would mean fault injection on the owner's live match); v1/v2 server-leg
work on board tasks 13/14/15; the monitor instrument (task-7), CI quic-suite
coverage (task-5), per-game signal (task-8, now estimator-suspect first),
per-game requests (task-4) remain. The live error-6 path is still only
unit/mutation verified.

### 2026-11-16 - WF-062: the relay watchdog stopped abandoning healthy relays (owner report: "jumps around to different relay")

**Agent:** lead + `interface-designer` (fix, task-9) + `qa-engineer` (independent verification + live run, task-10)

Owner report: the app jumps between relays and picks a sub-optimal one while
idling on the same server. Live-reproduced on ROB RUN (the owner's game):

- **The damning window:** on EWR the watchdog took the session DIRECT at a
  return share of **24.07%** (13 of 54 intercepted) because the old rule wanted
  a quarter of the traffic back in one 8 s window - while the relay was
  returning and injecting replies. (The `relay_app_ms`/`shadow_app_ms`
  comparison printed at the same time said the relay was 150 ms faster, but
  task-12 later showed those app-RTT estimators are invalid - `relay_app_ms` is
  below the client->relay leg - so that number is **not** evidence and is
  retracted here; the ratio, the hopping and the history poisoning are.)
- **The hopping:** an earlier run selected LAX -> EWR -> FRA -> LAX, each with
  `Restarted interception on the new relay`, after watchdog verdicts were written
  into `relay-history` (`594/0`, `545/0`), which then excluded working relays for
  six hours. The relay side was checked and was not at fault (response listener
  forwards server replies; the fixed live run returned 5,147 of them).

**Fix (task-9):** one shared floor `RETURN_SHARE_DENOMINATOR = 8` is the only
definition of "the return path works" (watchdog and history cannot drift); a
fallback needs the same fault over two consecutive non-overlapping judged
windows; retries re-test the SAME relay through `RetestTarget` +
`relay_switch_allowed` (no re-rank, no interception restart); relay-history
records a verdict only for a confirmed zero-return chain, and legacy records load
measurement-only so the poisoned entries stopped excluding working relays.

**Verified (independent, task-10):** gates green (workspace 1,449 / 0, GUI 166 /
0; re-run after a package clean); mutations - denominator 8->4 fails 5 GUI tests
including the live 24.07% replay, streak 2->1 fails 3, forcing the switch guard
fails 1; live ROB RUN run 4 m 29 s: **0** starvation fallbacks, **1** relay
selection / **1** restart total (the startup choice), return share **53.4%**
(`relay-history` 9,816/5,147), `relay_samples` 0 -> 256. Lead re-gate on the
final tree: fmt clean, workspace clippy clean, 1,449 workspace / 166 GUI / 1,201
WinDivert-feature / 827 QUIC-lib passed, 0 failed. Evidence
`wat/evidence/2026-11-16/relay-hopping/` (lead-diagnosis.md, designer.md,
qa-live/qa.md, GUI trace before/after, screenshot).

**Open:** the exact 24.07% window was not re-observed live (unit replay +
mutation cover it); the same-relay re-test was not exercised live (no fallback
fired); judged-healthy windows are silent (F1 -> board task-11, together with the
state-contract inventory and the overclaiming "relay is not dropping your
traffic" wording). The destination-leg estimator is still empty for asymmetric
games, so relay ranking rests on `RegionPrior` -> board task-12. Low findings F2
(`retry_after_fallback=false` chain blocks re-rank while direct), F3 (unknown
locked server allows a background switch) and the 58 stale host firewall rules
from force-killed test instances are noted in the QA report, not fixed here.

### 2026-11-16 - WF-061 team round: independent QA of the #167 fix, an ALPN rollout plan, and a relay-fra correction

**Agent:** lead + `qa-engineer` + `net-eng` + `infra-dev` (Agent Teams: three board tasks, all completed, then Lead integration)

First properly boarded Team-mode round (WF-060 was single-persona). Three
independent tasks ran concurrently; the Lead gated the final tree and integrated.

- **task-1 `qa-engineer` - independent verification of `908b3ac`: PASS 6/6.**
  It re-read the diff, re-ran the full gate twice with identical counts,
  re-derived all six mutation checks itself (each FAIL then PASS, restore
  byte-identical to HEAD blobs), confirmed the quoted live trace lines exist
  verbatim, and caught three evidence-doc nits plus two Low product notes
  (fatal-handle arm does not increment `errors`; "decoded" wording overstates the
  reply counter). Evidence `wat/evidence/2026-11-16/qa/qa-report.md`; Low findings
  filed as board task-6.
- **task-2 `net-eng` - `docs/quic-alpn-rollout.md` (790 lines): decision-ready
  plan.** Versioned identifier `lightspeed-control/1` in `protocol/`; the client
  offers it and falls back once to an empty list on `crypto(0x78)`; the relay gets
  `[network] control_alpn = "off" | "required"` with no new port; the interop
  matrix, proven from the vendored rustls branches, shows rows B and C are the
  same alert in opposite directions, so the client-with-fallback must ship first;
  stages 0-5 with gates/rollback, three-tier tests, observability and a canary
  gate (n>=200 handshakes, zero `rejected_no_application_protocol`), six
  falsifiers, owner gate. No code changed.
- **task-3 `infra-dev` - relay-fra: FIX THE INSTRUMENT, DO NOT DRAIN.** The alert
  reproduces exactly but is a delta-rule artifact (`saved_ms_sum` is signed; a
  decrease was booked as a restart, 60% inflation; the baseline bridged a
  restart), `negative_saving_spike` is dead from a metric-name typo shared by
  scripts and fixtures, and relay-fra is healthy with positive app-level saving.
  The real residual signal is per-game (`fortnite/US` negative on fra and ewr-1).
  `DISCORD_WEBHOOK` does not exist, so the alerts were never delivered. Evidence
  `wat/evidence/2026-11-16/relay-fra/report.md`; audit F6 corrected; board
  task-7/8 filed.

**Verified:** the Lead re-ran the gate on the final tree after every teammate
finished: fmt clean; workspace clippy `--all-targets --all-features` exit 0;
**1,449** workspace tests across 29 suites, **155** GUI, **1,201**
WinDivert-feature and **827** QUIC-lib passed, 0 failed - identical to the QA run
(the only tree addition is the ALPN doc). Evidence
`wat/evidence/2026-11-16/lead-gate.log`; team reports in the same folder.

**Open:** owner gates - approve the ALPN client-first order (stage 1) and each
relay flip separately; create the `DISCORD_WEBHOOK` secret. Agent work queued on
the board: task-4 (relay ALPN operator procedure), task-5 (CI never runs the
quic-gated integration suites), task-6 (QA parity fixes), task-7 (monitor
instrument), task-8 (per-game signal). The live error-6 path still needs an
external WinDivert handle invalidation.

### 2026-11-16 - Issue hygiene: the backlog now says what it means

**Agent:** lead (GitHub maintainer actions, owner-approved)

Executed the audit's F7 item. Public replies: #167 now carries the `908b3ac` fix
summary and the exact retest ask that separates relay-side rejection from a local
injector; #140 carries the corrected alert-120 explanation (the old "Schannel
error" note was wrong), the packet-capture ask and the staged ALPN plan. Closures:
#59 closed completed (fixed in `6bf0f67`, shipped v1.6.10, reporter confirmed
in-thread) and #147 closed as a duplicate of #140 with the diagnosis folded there.
Labels: `game-profile` on all four remaining game requests (the old template's
missing label is why they arrived unlabelled), #148 and #153 moved to `blocked`
with the missing fact named, #168 given its second ask. Docs updated:
`docs/github-feedback-audit-2026-11-16.md` (issue-hygiene section with links) and
`docs/game-requests.md` (request table now says blocked/under evaluation). PR #173
was also sent its review-to-merge message (rebase on master, real-window
screenshots, deduplicate the 420/1400 bounds, qualify the mixed-DPI claim).

**Verified:** `gh api` readback — #59 `closed/completed`; #147 `closed/duplicate`
with the `duplicate` label; #148/#153 `blocked,game-profile`; #166/#168
`game-profile`; #140/#167 open with their new comments. Open backlog 8 -> 6.
Comment URLs are recorded in the audit doc.

### 2026-11-16 - Live ROB RUN check: the new verdict format is confirmed on a real session

**Agent:** lead (verifying `908b3ac` on the owner's live game)

The owner was in ROB RUN (Kefir, Steam, Easy Anti-Cheat) for the whole check. The
debug GUI built from `908b3ac` auto-detected `robrun`, connected (LAX/EWR/FRA
auto relays), intercepted, and moved the Relayed counters (`71 up · 2 down` ->
`155 up · 25 down`). The no-reply watchdog fired twice on the real session with
the new log shape: `errors=0 last_error=none` and a data-reply-only
`from_relay=` count, first live proof of the changed fields and counter.
Screenshots `wat/evidence/2026-11-16/live-gui-*.png`; log lines and analysis in
the "Live ROB RUN check" section of `wf167-diagnosability.md`.

The fallback-to-direct seen on those relays is pre-existing: the same verdicts
run through the 11-13..11-15 sections of the same trace log, and the
`TooFewReplies` decision does not read the reply count.

**Open:** forcing the error-6 branch live does not work by design — `sc stop
WinDivert` parks the driver in STOP_PENDING under an open handle, so a live
invalid handle needs an external invalidation (driver update/EDR). That branch
stays covered by the unit test and mutation check, not by a live run. The
WinDivert service was restored and a fresh GUI is optimizing ROB RUN again.

### 2026-11-16 - WF-060: a dead data plane is visible now, and QUIC alert 120 explains itself

**Agent:** lead (RustDev + QAEngineer contracts; single-threaded)

Picked up the WF-059 audit's two highest-value follow-ups after that session was
disposed mid-turn. Nothing was lost: the tree was clean at `174b1f3` and its CI
run was green; only the in-flight turn died (the owner's "start tackling"
message had landed in the finished `net-eng` subagent session, whose abort in
turn started the parent turn that was then disposed).

What changed (issue #167):

- `EngineStatus.interceptor_error` is now filled from
  `InterceptorStats.last_error`, so the tray's Error state and the restart signal
  exist; `InterceptorHandle::is_active()` also stops reporting an active
  interceptor once the receive loop dies (new `run_failed` flag).
- `ERROR_INVALID_HANDLE` (os error 6) is terminal in the WinDivert receive loop
  instead of consuming the retry budget; both WinDivert backends record a message
  saying interception stopped.
- the no-reply watchdog baselines a new data-reply-only counter
  (`packets_reply_data`), so keepalive echoes and undecodable frames cannot be
  reported as a local injection fault; all three fallback verdicts carry
  `errors=` and `last_error=`.
- QUIC (#140/#147): a peer `CONNECTION_CLOSE` with crypto alert 120 now prints a
  verdict that the responder cannot be a LightSpeed relay (this repo never sets
  ALPN) and is likely a QUIC-intercepting middlebox; the client logs its
  `ALPN=[]` offer once for capture comparison. The ALPN migration itself is
  wire-breaking and stays owner-gated.

**Verified:** `908b3ac` pushed; CI run `43820738304` is **green** (all jobs,
including Feature Matrix (quic), Check & Test and Windows GUI Build). Local gate
on the same tree: fmt clean; workspace clippy `--all-targets --all-features`
clean; 1,449 workspace tests across 29 suites, 155 GUI, 1,201
WinDivert-feature and 827 QUIC-feature passed, 0 failed. Failing-before and
mutation evidence: `wat/evidence/2026-11-16/wf167-diagnosability.md`.

**Open:** the live invalid-handle path is not reproduced end to end (needs an
external WinDivert-handle invalidation during a real match; the owner's Roblox
session had ended before the fix existed). The ALPN rollout needs an owner
release plan (client-with-empty-ALPN-fallback first, then relays). Public issue
replies (#167 diagnosis, #140 verdict, closing #59, folding #147) are
owner-gated. Linux and macOS receive loops do not set `run_failed` yet; #167 is
WinDivert-specific but the liveness fix deserves the same treatment there.

### 2026-11-16 - WF-059: GitHub feedback audit - two process bugs, one real fleet signal, one community PR

**Agent:** lead, with `net-eng`, `rust-dev` and `interface-designer` in Team mode (three board tasks, all completed)

Inventory at 1.10.3: 8 open issues, 10 discussions, 1 open PR. The audit found
and fixed five process defects:

- **Game requests were unlabeled**: the issue template named `game-request`, a
  label that does not exist, so GitHub silently dropped it on every request. It
  now uses `game-profile`.
- **The request form did not ask for the two deciding facts** (gameplay UDP port
  measured in-game, matchmade vs fixed servers), which is why #148/#153/#168
  each needed rounds of "what port?". The template asks now, and the maintainer
  checklist with per-game status and the SDR policy is the new
  `docs/game-requests.md`.
- **The request list in `docs/supported-games.md` was stale** (3 of 7) and said
  nothing about Steam Datagram Relay; both fixed.
- **The anomaly detector's 674-line self-test never ran on PRs**; it is now part
  of CI's Infra Script Tests.
- **A duplicate severity-gate comment** in `health-anomaly.sh` claimed only
  critical anomalies fail the run, contradicting the tested policy from
  `e07774b`. Comment corrected, no behaviour change.

Reported-bug verdicts (full report: `docs/github-feedback-audit-2026-11-16.md`):

- **#140/#147 QUIC error 120**: the peer sent the alert; a repository-built relay
  cannot, because neither side sets ALPN (zero occurrences in tree and history).
  Almost certainly a QUIC-terminating middlebox on the reporter's path. Latent
  defect: a control plane with no ALPN is non-conforming; staged ALPN rollout
  recommended, starting with a `crypto(120)` verdict in
  `client/src/quic/mod.rs::connect_with_destination`.
- **#167 Roblox starvation**: the reported line only exists in v1.8.4; 1.10.2
  retuned the watchdog (20 packets / 8 s) and split the fault variants.
  `injected=0` has seven benign causes. Real bug on master:
  `client/src/engine.rs:1394` never copies `ist.last_error` into
  `snap.interceptor_error`, and `is_active()` still reports true after the recv
  thread dies, so a dead WinDivert handle is invisible.
- **#173 PR**: real defect, correct approach (documented scroll shell), but needs
  CI (the branch is 4 commits behind and fails only on the shadow-gate bug fixed
  in `a2f7315`), real-window screenshots, deduplicated 420/1400 bounds, and a
  corrected multi-monitor claim.

**Verified:** `cargo fmt --all --check` clean; the shell change is a comment and
the workflow change is exercised by this commit's CI; the monitor's red runs
were read from `43790512557`/`43765890171` and are a real `relay-fra`
`saved_regression` (-45 ms vs +4.1 ms baseline).

**Open:** `relay-fra` needs draining or fixing (infra); the QUIC ALPN migration
is wire-visible and needs its own release plan; the `interceptor_error` one-line
fix and the `is_active()`/error-6 changes on #167; PR #173 needs a branch update
and screenshots; close #59 (fixed in 1.6.10) and consolidate #147 into #140.

### 2026-11-16 - Follow-up to WF-058: master's red CI was a stale Linux-only test, not the migration

**Agent:** lead (RustDev contract; test-only change)

The push of the migration exposed a red `CI` on master, but the failure predates
it: run `43681822261` on `4378df1` ("stop the shadow sampler reporting an
inter-packet gap as an RTT") failed the same test. That commit changed
`SHADOW_SAMPLE_INTERVAL` from 30 s to 5 s and updated the tests in
`client/src/latency.rs`, but missed
`shadow_gate_claims_one_copy_per_server_per_interval` in
`client/src/interceptor/shadow_probe.rs`, which still advanced 29 s and expected
no new sample. That module is `#[cfg(target_os = "linux")]`, so the Windows dev
host never compiles or runs it: the local suite stayed green while Linux CI went
red.

**Fix:** the test now derives both boundaries from `SHADOW_SAMPLE_INTERVAL`
(`interval - 1 s` must not claim, `+1 s` must claim), the pattern `latency.rs`
already uses. No production code changed.

**Verified:** `cargo fmt --all --check` clean. The test cannot run on this host
(Linux-only module), so CI is the verifier: run `43801922521` on `a2f7315` is
**green**, including the three jobs that were red (`Check & Test`,
`Feature Matrix (ml)`, `Code Coverage`). Separately, local `cargo test`
currently fails 10 unrelated temp-dir tests with `Access is denied`: a test
binary under `target\` cannot create directories in `%TEMP%` on this host, while
the same binary copied to `%TEMP%` succeeds. That is a host sandbox interaction,
recorded in the `lightspeed-windows` skill, not a code failure.

### 2026-11-16 - WF-058: the repo is now DeepSeek-Harness-native, and the personas run on Team mode

**Agent:** lead (Architect, RustDev, QAEngineer and DevOps contracts applied; no product code touched)

What changed, in the order it matters:

- **A Windows bug was fixed at the root.** `wat/archive/agents.md` matched DSH's
  `AGENTS.md` loader on case-insensitive NTFS and was injected into every session
  as project instructions. Personas moved to `wat/personas/`, the file is gone,
  and `wat/archive/README.md` + `docs/agent-operations.md` state the rule.
- **One harness.** `AGENTS.md` is now a 119-line DSH contract: skills, Team mode,
  tools, gates, Windows rules, state updates. No other harness is referenced.
- **Procedures load on demand.** Four project skills under `.dsh/skills/`
  (`lightspeed-wat`, `lightspeed-team`, `lightspeed-windows`,
  `lightspeed-verify`); discovery verified live in this session.
- **Personas are Team-mode contracts.** Eight files under `wat/personas/` with
  slugs, ownership, must/must-not, definition of done, a paste-ready delegation
  brief and a task template; roster in `personas.json` + schema;
  `wat/team-mode.md` carries the board protocol and failure modes.
- **State is readable again.** `current-phase.md`: 322,758 B / 5,727 lines ->
  under 8 KB / about 125 lines with this entry (4.6 KB immediately after the split), with
  29 duplicate blocks and 15 duplicate entries removed
  and history split into `wat/state/phases/2026-09.md` and `2026-10.md`.
  `decisions.md`: 138,359 B -> a 47-line index over seven monthly files
  (74 entries preserved). Migration tool: `tools/Migrate-PhaseLog.ps1`.
- **Windows procedures are native.** `CONTRIBUTING.md` has platform tabs; the
  three localization bash wrappers became Python entry points plus
  `scripts/Localization.ps1`; CI and Pages updated; `.gitattributes` pins line
  endings; CPython is documented as an explicit prerequisite.
- **Rules match reality.** `wat/rules.md` now has 10 stubs including
  `[SHELL_STUB]`, `[AGENT_STUB]`, `[STATE_STUB]`, and a `[COST_STUB]` consistent
  with decision 2026-09-13 (owner spend $0.00, relays sponsor-funded).

**Verified:** `tools/Check-LightSpeed.ps1 -SkipGui -SkipBuild` -> fmt PASSED;
workspace clippy (`--all-targets --all-features --exclude lightspeed-gui`)
PASSED; `cargo test --workspace --all --exclude lightspeed-gui` PASSED, 29 suites,
**1,444 passed, 0 failed, 6 ignored**. Phase content preserved line by line
(4,613 non-empty lines, 2 intentional drops); 74/74 original decision entries;
persona roster and schema parse with 8 valid slugs; 4/4 skill frontmatters and
live skill discovery; `Test-Path wat/archive/AGENTS.md` true before / false after
with DSH emitting the removal notice; all relative links in changed docs resolve.
Full audit: `docs/agent-system-audit-2026-11-16.md`.

**Not run:** `cargo audit` / `cargo deny` (not installed on this host; the script
reports them SKIPPED, CI's security workflow owns them) and the GUI crate gates
(`-SkipGui`; no GUI source changed). Two pre-existing `unused dependency` manifest
warnings remain in `client/Cargo.toml` (`rcgen`, `windivert`) — untouched by this
change.

**Localization, after CPython 3.12.10 was installed (same day):**
`scripts/Localization.ps1 -Check` -> catalog up to date (403 keys), 8 locale
pages up to date, 48 localized docs carry the banner, translations in step with
English. The bash-to-Python port is therefore executed and green on Windows, not
just structurally ported.

**Open:** Team mode is per-session and must be enabled by the owner (model &
reasoning menu -> Use Agent Teams); the ported localization scripts are now
executed here, but `tools/e2e_test.py` and `tools/load_test.py` still need a live
relay to be exercised.

### 2026-11-14 - WF-057 interface design: the state contract, then the layout

**Agent:** InterfaceDesigner (lead-verified against screenshots of the real window)

The client's worst defects have been state defects, so design work now starts
from a contract rather than a mockup: `docs/design/interface.md` names the nine
states, what each one must say, and which fact drives it. The persona is
`InterfaceDesigner` in `wat/archive/agents.md`.

Order of work: demote the globe so status outranks decoration; put measured
latency and the destination-aware winner into the relay picker; restructure
Settings (two items today, no routing controls); fix the Proxy Manager row that
wraps its delete button onto its own line. Every step is verified by screenshot
at narrow and wide width, boosting and idle, not by the diff.

### 2026-11-12 - audit fixes on a real Windows host: auto-select applies, and the TCP relay has an owner

A full local build/test pass (Rust 1.99.1, MSVC 17.14.45, Windows SDK 10.0.26100)
found and fixed:

- the client-leg race compared the `/health` endpoint (`IP:8080`) against the proxy
  list (`IP:4434`), so every winner was silently discarded: a clean first run stayed
  "No relay yet" and the next launch used the first registry relay. The lookup now
  matches the relay IP at all three race sites and warns on a miss, with two
  regression tests;
- the v5 TCP connection table was relay-global with no owner, so an authenticated
  client could RST or splice another client's connection by presenting its tuple.
  `TcpConnState` now stores the creator; mismatched packets are dropped as auth
  rejections and a SYN for an owned tuple is refused (unit + integration test);
- `--demo` no longer registers a QUIC session first (60 s before, 3 s end to end);
- `--verify-game` reports a custom profile's missing wire id as a note, not a `Fail`;
- `--test-tunnel` no longer claims forwarding it did not observe;
- `docs/deploy-proxy.md` now documents the real TOFU trust model and the 32-bit token;
- a known game server now overrides the client-leg race: the destination-aware
  ranking (server-side leg first) governs, and a re-measurement during a live
  match is staged for the next session instead of restarting the data plane.

**Verified:** fmt; clippy `-D warnings` on the workspace, the GUI, and the WinDivert
client; client lib 566 and bin 557; proxy 176 default (190 with `quic`); GUI 136; route
ranking 10; the fixed binaries launched, and the GUI logged
`Auto-selected the fastest relay 65.20.92.201:4434` on a clean run. The telemetry
opt-in rule conflict remains an owner decision.

## History

| Month | Entries | File |
|---|---|---|
| 2026-09 | 5 | [2026-09.md](phases/2026-09.md) |
| 2026-10 | 58 | [2026-10.md](phases/2026-10.md) |

---

## How this file is maintained

- One dated entry per completed round, appended at the top of **Recent entries**.
- When an entry no longer describes active work, move it verbatim to the month file in `wat/state/phases/` and demote its headings by one level.
- Decisions go to `wat/state/decisions/<YYYY-MM>.md`; evidence goes to `wat/evidence/<YYYY-MM-DD>/`.
- History is append-only. Nothing is deleted, only moved.
