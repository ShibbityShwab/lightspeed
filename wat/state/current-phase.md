# Current Phase

**Updated:** 2026-11-16
**History:** older entries live in `wat/state/phases/<YYYY-MM>.md`; this file keeps the live state and the newest entries only.

## Current state

**Workflow:** WF-060 (the WF-059 audit's #167 and QUIC follow-ups, this entry); WF-059 closed, product work continues under WF-057 and PR #173
**Agent:** lead (RustDev + QAEngineer contracts; single-threaded)
**Status:** 1.10.3 is the latest release (2026-11-14): server ping in the route card, bounded relay re-tests, session-scoped Relayed counters, the retuned no-reply watchdog, destination-aware relay choice, ARC Raiders TCP control-plane support and the TCP-connection ownership fix; 1.10.0 added Jedi Outcast and Tibia. Master is CI-green after the 2026-11-16 shadow-gate test fix. Open GitHub feedback is triaged in `docs/github-feedback-audit-2026-11-16.md`: 8 issues (2 bug threads, 5 game requests, 1 fixed-but-open), 10 discussions, 1 community PR whose CI was unblocked. WF-060 now surfaces a dead data plane in the status (#167) and turns QUIC alert 120 into a verdict (#140/#147); the ALPN migration is the next owner-gated step. The agent layer (WF-058) is DSH-native with Team-mode personas.

## Recent entries

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
