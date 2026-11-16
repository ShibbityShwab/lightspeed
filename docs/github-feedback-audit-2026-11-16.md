# GitHub feedback audit — 2026-11-16

| | |
|---|---|
| **Scope** | Everything reported on GitHub since the 1.9.x/1.10.x push: open issues, discussions, the one open PR, and the scheduled workflows that report on the fleet |
| **Method** | `gh issue/pr/run list` inventory; full thread reads; three Team-mode investigations (QUIC control plane, Roblox data plane, PR review) with read-only source audits and file:line evidence |
| **Baseline** | `master` at `c190709`, latest release `1.10.3` (2026-11-14), CI green, 24 game profiles |
| **Outcome** | Five process defects fixed, one stale comment corrected, the community PR's CI unblocked, and three reported-bug verdicts with concrete next fixes |

Working notes and raw captures from the three investigations are under
`wat/evidence/2026-11-16/` (gitignored by design; named here for the record).

---

## 1. What is open

### Issues (8)

| # | Title | Kind | State / what is missing |
|---|---|---|---|
| 168 | Delta Force | game profile | under evaluation — which ports carry gameplay; matchmade behaviour |
| 167 | Roblox relay return path starving | bug | regression fixed by 1.10.2 in the code, waiting on reporter retest |
| 166 | EA FC 27 | game profile | near-ready — confirm `21006-21039` is gameplay; mark dynamic servers |
| 153 | Black Desert Online | game profile | port range arrived as "From: Nz / To: LAX"; needs measured range |
| 148 | Aion 2 | game profile | 16 comments, gameplay UDP port still unknown |
| 147 | QUIC error 120 | bug | duplicate subject of #140; reporter's evidence is in the thread |
| 140 | QUIC error 120 on community relays | bug | not reproducible from a clean host; needs the reporter's path evidence |
| 59 | Fortnite routing failure | bug | **fixed in v1.6.10**; still open |

### Discussions (10)

Game requests: Deadlock (#142, #169 duplicate), Battlefield 6 (#172, #63),
Warface (#63), Delta Force (#63), Aion 2 (#63, #149). Resolved support threads:
custom-game saving (#146, both parts shipped), relay list (#69, fixed 1.4.2).
Plus the intro thread (#2) and the announcement (#1).

### Pull request (1)

[#173](https://github.com/ShibbityShwab/lightspeed/pull/173) by `Kianmhz` (fork):
"keep the action bar on screen when the window outgrows the monitor", +94/-4 in
`client-gui/src/app.rs`. Mergeable. Its CI was blocked pending maintainer
approval; this audit approved the three runs.

### Scheduled workflows

`Health Anomaly Monitor` failed on 2026-11-15 and 2026-11-16. It is **not** a
broken workflow: it reported a real `saved_regression` on `relay-fra`
(`ping-saved -45 ms vs baseline 4.1 ms over 5 samples`), which the detector's
own tested policy treats as fatal. The fleet issue is real and open; the
monitor is doing its job.

---

## 2. Findings and fixes

### F1 — Game requests arrive unlabeled (fixed)

`.github/ISSUE_TEMPLATE/game_request.md` set `labels: game-request`, but that
label **does not exist**. GitHub silently drops unknown template labels, so
every game request arrived with no label (`#148`, `#153`, `#166`, `#168` are all
unlabeled). The repo's actual label is `game-profile`.

Fix: template now uses `game-profile`.

### F2 — The request form did not ask for the two facts that decide a request (fixed)

Every stalled game request stalled on the same two questions: the *gameplay* UDP
port (measured in-game, not from a port-forwarding site) and whether match
servers are matchmade. The template asked for a port range but not how to
measure it, and never asked about matchmaking — so the thread becomes rounds of
"what port?" (#148: fourteen comments, port still `37300 ?`; #153: ranges given
as region names; #168: mixed with Steam's query block).

Fix: the template now asks for the measured remote ports, the exact command to
get them, whether the observation was in-game, matchmade vs fixed, and the
Steam Datagram Relay caveat. The maintainer checklist and per-game status moved
to [`docs/game-requests.md`](game-requests.md).

### F3 — The public request list was stale and had no SDR policy (fixed)

`docs/supported-games.md` listed three requests (Battlefield 6, Warface, Delta
Force). Four current requests were missing (Aion 2, Black Desert Online, EA
Sports FC 27, Deadlock), and nothing explained why Valve's relay-routed titles
are a different case.

Fix: current list, the Deadlock/SDR explanation, and a request procedure that
links the new checklist.

### F4 — The anomaly detector's own test never ran on PRs (fixed)

`infra/scripts/test_health_anomaly.sh` is 674 lines with 100+ assertions, but
only the scheduled workflow ran it. A PR could change the detector or its
severity gate and the first execution would be the live 3-hourly schedule.

Fix: `ci.yml`'s `Infra Script Tests` job now runs it.

### F5 — The severity gate's comment contradicted its tested policy (fixed)

`health-anomaly.sh` carried two adjacent "Severity gate" comment blocks. The
first claimed "only a CRITICAL one fails the run", which is not what the code
does and not what the tests assert: `idle_relay`, `abuse_flood` and `load_skew`
are non-fatal, and every other detector fails the run **including** the
warning-severity ones (`stale_history`, `saved_regression`,
`negative_saving_spike`, `version_lag`) — deliberate per commit `e07774b`.

Fix: the stale block is removed and the remaining comment states the tested
policy. No behaviour change; the monitor's two red runs are a real signal, not
a gate bug. That is what the whole F6 investigation confirmed the hard way.

### F6 — `relay-fra` is the fleet's current degradation (open, real)

`saved_regression` means the relay's median ping-saved is now *worse than its own
baseline* (-45 ms against +4.1 ms). Every client routed through Frankfurt is
paying extra latency. This is an `infra-dev`/`net-eng` task, not a monitor bug:
check the relay's own health, its egress path and whether it should be drained.
The alert fires twice a day on the schedule and is in the Discord channel.

### F7 — Issue hygiene (recommended, owner action)

- **Close #59.** It was fixed in `6bf0f67`, shipped in v1.6.10; the thread has
  the evidence and nobody has objected since. Keeping it open makes the backlog
  look worse than it is.
- **Consolidate #147 into #140.** Same subject; the maintainer already says they
  track it in #140.
- **Move the data-blocked requests to `blocked`** with the missing fact named, so
  "what is the status of my game?" has one answer (`docs/game-requests.md`).

### F8 — State drift (fixed)

`wat/state/current-phase.md` still described "1.8.4 shipped" as the current
product state while the repo was on 1.10.3. The live state block is updated with
this audit; the history below it is untouched.

---

## 3. Reported-bug verdicts

### #140 / #147 — QUIC handshake error 120

**Verdict: not a relay bug; almost certainly a QUIC-terminating middlebox on the
reporter's path, plus a latent protocol defect worth fixing.**

- `error 120` is rustls `NoApplicationProtocol`. The string "aborted by peer"
  proves the client received a `CONNECTION_CLOSE`; so **the peer sent the
  alert** (`client/src/quic/mod.rs`, rustls/quinn behaviour verified in the
  dependency sources).
- Neither side sets ALPN — zero occurrences of `alpn` in `client/`, `proxy/`,
  `protocol/` and in the whole git history. With an empty offer on both sides,
  rustls does not negotiate ALPN and a repository-built relay cannot raise 120.
- The reporter's own log shows ~1 s of silence (QUIC Initial PTO, not a 14 ms
  relay round trip) then an undecryptable packet before the close — the shape of
  a local TLS/QUIC shim, not the relay.
- Latent defect: a QUIC control plane with no ALPN is non-conforming (RFC 9001
  §8.1) and *any* strict peer produces exactly this opaque error.

Recommended, in order: (1) classify `crypto(120)` in
`quic/mod.rs::connect_with_destination` and print a verdict naming the responder
and the empty ALPN offer; (2) introduce a versioned ALPN in `protocol/`, set it
on both sides, and have the client retry once with an empty list for old relays
(wire-compatible rollout: client fallback → relays → require); (3) ask the
reporter for a UDP 4433 capture and a clean-host run to close the middlebox
question.

### #167 — Roblox return-path starvation

**Verdict: the reported line is from 1.8.4; the watchdog was already retuned and
the exact failure mode split into named variants in 1.10.2. One real visibility
bug remains on master.**

- The log line's field shape only exists in v1.8.4 (`server=… intercepted=…
  injected=…`); master prints named fault variants (`NoReplyFromRelay`,
  `TooFewReplies`, `InjectionFailed`) and needs 20 intercepted packets over 8 s,
  not 2 packets over 3 s (1.10.2).
- `injected=0` is **not** by itself evidence of a bad relay: seven paths leave
  it at zero while replies are already counted (keepalive echoes, header decode
  failure, unknown source address, FEC parity/duplicates, inject-channel
  overflow, `WinDivertSend` failure, and pure-direct sessions by construction).
  v1.8.4's health snapshot could not even tell "relay returned nothing" from
  "returned data we failed to inject".
- Real bug on master: `client/src/engine.rs:1394` copies
  `snap.interceptor_errors` but never `snap.interceptor_error = ist.last_error`,
  so the GUI's `status.interceptor_error` pill can never appear, and
  `InterceptorHandle::is_active()` still reports `true` after the recv thread has
  died. A dead WinDivert handle (the reporter's `Invalid handle (os error 6)`,
  5 retries then the 6th call fails) is invisible for the rest of the session.
- Recommended minimal fix: the one-line `interceptor_error` copy, then count
  keepalive echoes separately, break on `ERROR_INVALID_HANDLE` instead of
  retrying a dead handle, and add a `run_failed` flag so `is_active()` stops
  lying. Ask the reporter for the two log lines' timestamps and whether
  `from_proxy` was rising — that single fact separates relay-side rejection from
  local injection failure.

### #173 — PR review

**Verdict: merge-with-changes.** The defect is real and reachable (the pinned
footer/action bar is drawn off-screen once the content height exceeds the work
area; in Manual mode the apply button is the only way to use the typed server).
The approach is the documented shell behaviour ("only the config region
scrolls"), the helper is pure and unit-tested, and the 420/1400 bounds still
hold.

Required before merge:

1. CI green. It was blocked pending approval; this audit approved it, and it
   currently fails **only** on `shadow_gate_claims_one_copy_per_server_per_interval`
   — the master bug fixed in `a2f7315`. The PR branch is 4 commits behind
   `master`, so it needs an update to pick that fix up.
2. Real-window screenshots. A layout claim needs a real window: 1080p@125% and
   1440p@125% in Manual mode before/after, 1366×768, a scaling where the cap
   lands on the 420 pt minimum, and the settings sheet at the capped height.
3. Deduplicate the 420/1400 bounds against `main.rs:96-97` (one source per fact),
   or assert they stay equal.
4. Qualify the multi-monitor claim: `inner_rect` is a global coordinate divided
   by the window's scale factor while `monitor_size` is monitor-local; on
   stacked mixed-DPI monitors the filter can be wrong in the conservative
   direction. Add the test or drop the claim.

---

## 4. Actions taken in this audit

| Action | Where |
|---|---|
| Game-request template: correct label, measured-port instructions, matchmaking and SDR questions | `.github/ISSUE_TEMPLATE/game_request.md` |
| Maintainer triage checklist, current request status, SDR policy, duplicate/closure policy | `docs/game-requests.md` (new) |
| Public request list refreshed; request procedure now asks for measured ports and matchmaking | `docs/supported-games.md` |
| Anomaly detector self-test wired into CI | `.github/workflows/ci.yml` |
| Severity-gate comment corrected to the tested policy | `infra/scripts/health-anomaly.sh` |
| Community PR's blocked CI runs approved (3) | GitHub Actions |
| Live state updated to 1.10.3 and this audit | `wat/state/current-phase.md` |

## 5. Recommended next tasks (ranked)

1. **`relay-fra` saved regression** — drain or fix; the monitor will keep alerting
   until it is addressed (`infra-dev`/`net-eng`).
2. **QUIC diagnostic + ALPN rollout** — start with the `crypto(120)` verdict; the
   ALPN migration is wire-visible and needs its own release plan (`net-eng` +
   `rust-dev`).
3. **Interceptor error visibility** — the one-line `engine.rs` fix plus the
   `is_active()` and error-6 changes; this is how #167 becomes diagnosable
   (`rust-dev`).
4. **PR #173** — ask the contributor to update the branch and attach the
   screenshots; then re-review (`interface-designer`).
5. **Issue hygiene** — close #59, consolidate #147 (`lead`).

## 6. Verification

- `cargo fmt --all --check`: clean on the tree at commit time.
- Local unit tests for this audit's changes: none of the touched files are Rust;
  the shell change is a comment only, and the anomaly self-test is now executed
  by CI rather than locally (this Windows host has no bash).
- CI: the audit commit's run is the verifier for the workflow change and the
  docs; the scheduled-workflow findings were read from runs `43790512557` and
  `43765890171`.
- Not verified here: the reporters' networks (no pcap from them), the relay
  fleet (no access), PR #173's screenshots (not produced), and the GUI gates on
  the PR branch (blocked until it is updated).
