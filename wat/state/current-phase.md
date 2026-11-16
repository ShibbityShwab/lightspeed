# Current Phase

**Updated:** 2026-11-16
**History:** older entries live in `wat/state/phases/<YYYY-MM>.md`; this file keeps the live state and the newest entries only.

## Current state

**Workflow:** WF-058 (agent infrastructure, this entry); product work continues under WF-056/WF-057
**Agent:** lead, with the Architect/RustDev/QAEngineer/DevOps persona contracts (no product code changed)
**Status:** The agent layer is DeepSeek-Harness-native as of 2026-11-16: `AGENTS.md` rewritten, four project skills under `.dsh/skills/`, eight persona contracts under `wat/personas/`, the Team-mode contract in `wat/team-mode.md`, the hot state file reduced from 322 KB to 4.6 KB, decisions split into monthly logs, and the localization pipeline moved off bash. Product state is unchanged: 1.8.4 (`59f20c8`) shipped the server-leg relay ranking, the return-path watchdog and three rustls/webpki advisory fixes; the ARC Raiders transport split is implemented (`dfa1413`) and the live join through it remains the open proof. The 2026-11-14 interface-design entry (WF-057) is still the active product plan.

## Recent entries

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
(Linux-only module), so CI is the verifier. Separately, local `cargo test`
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
