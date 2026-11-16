# Current Phase

**Updated:** 2026-11-16
**History:** older entries live in `wat/state/phases/<YYYY-MM>.md`; this file keeps the live state and the newest entries only.

## Current state

**Workflow:** WF-064 (server-leg signal v1, honest route surface, v2 NO-GO, per-game verdict - this entry); WF-063 fallback state contract and WF-062 relay-watchdog fix are the current product changes, unreleased
**Agent:** lead + `rust-dev` + `interface-designer` + `infra-dev` + `net-eng` + `qa-engineer` (Agent Teams; five board tasks, all completed)
**Status:** 1.10.3 is the latest release (2026-11-14): server ping in the route card, bounded relay re-tests, session-scoped Relayed counters, the retuned no-reply watchdog, destination-aware relay choice, ARC Raiders TCP control-plane support and the TCP-connection ownership fix; 1.10.0 added Jedi Outcast and Tibia. Master is CI-green after the 2026-11-16 shadow-gate test fix. Open GitHub feedback is triaged in `docs/github-feedback-audit-2026-11-16.md` (backlog 6 issues after the 2026-11-16 hygiene round), 10 discussions, 1 community PR whose CI was unblocked. WF-062 stopped the relay watchdog abandoning healthy relays and hopping mid-session; WF-063 added the fallback state contract and a trace per judged window; WF-064 makes the relay-to-server leg measurable (validated quiet-flow RTT), reports it honestly on the route surface, measures the client leg as one RTT, and ranks on the whole path - with the v2 fleet probe at NO-GO pending fixed gates. The ALPN migration is the next owner-gated step. The agent layer (WF-058) is DSH-native with Team-mode personas.

## Recent entries

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
