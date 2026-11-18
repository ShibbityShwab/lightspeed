# Project audit — 2026-11-18

**Agent:** `lead` (single-agent audit round; no code, config, fleet or public-post changes were made)
**Scope:** whole repository at `343530b` (master), GitHub backlog + notifications, and one read-only live fleet snapshot.
**Method:** read `wat/state/*` → GitHub sweep (`gh`) → static audit of the Rust workspace, CI and docs → reproduce the documented gate on this host → verify the top finding against the code path the GUI actually runs.
**Evidence:** `wat/evidence/2026-11-18/lead-gate/` (gate logs), `wat/evidence/2026-11-18/github-audit/` (issues, discussions, notifications, anomaly runs, fleet health).

---

## Executive summary

The project is in good shape: `master` is clean, the documented local gate is green
(**1,561 workspace tests + 184 GUI tests, 0 failed**), the latest release `v1.11.0`
has 39 assets, and the last `CI`, `CodeQL` and `Security Audit` runs on `master`
are green. This audit found **one high-confidence product bug with a fresh external
report**, **one live fleet alert that the previous state entry does not reflect**,
and a set of maintainability/CI/doc items.

| # | Severity | Finding | Evidence |
|---|---|---|---|
| F-01 | **P1** | Tunnel MTU handling cannot serve paths below 1492 bytes; `[tunnel] mtu` is a dead knob; send errors are swallowed and then blamed on the relay. This is the reported `#168` `sendmsg` / `WSAEMSGSIZE` (10040) failure. | `client/src/interceptor/windows.rs:1513,1548,1583`, `client/src/config.rs:71,261`, `protocol/src/header.rs:68`, `client/src/tunnel/pmtud.rs:106,149`, `client/lightspeed.example.toml:55`, `docs/protocol.md:220` |
| F-02 | **P1** | `Health Anomaly Monitor` is red on two consecutive scheduled runs: `relay-syd-1` auth spike + zero relayed (the stranded pre-1.11.0 client signature), plus `relay-fra` carrying 58% of fleet traffic. The live fleet is 8/8 healthy on 1.11.0/`required`. | run `44189992620` log; live `/health` snapshot in `github-audit/fleet-health.json` |
| F-03 | **P1** | `#168` has three unanswered reporter comments (2026-11-17 12:38–13:00 UTC) that contain the measurement the project asked for *and* a new blocker. The status row in `docs/game-requests.md` is stale. | `#168` comments 8–10; `docs/game-requests.md:70` |
| F-04 | P2 | CI never lints Windows-only client code: the `windows-test` job installs `clippy` but only builds/tests; the Linux clippy step cannot see `#[cfg(windows)]` code. | `.github/workflows/ci.yml:270-285` |
| F-05 | P2 | Windows Firewall rules accumulate: one rule **per ephemeral port** (`LightSpeed WinDivert Tunnel {port}`), removed only on graceful teardown. The 71 stale rules cleaned on 2026-11-17 will regrow after force-kills/crashes. | `client/src/interceptor/windows.rs:2036-2097`; `wat/state/current-phase.md` (2026-11-17 cleanup: 614 → 543 rules) |
| F-06 | P2 | Two divergent `is_public_ipv4` implementations. The proxy's (security-critical) version is weaker than the client's: it misses `0.0.0.0/8`, `192.0.0.0/24`, `198.18.0.0/15`, `240.0.0.0/4`. | `proxy/src/abuse.rs:292-356` vs `client/src/interceptor/process_scanner.rs:126-173` |
| F-07 | P2 | The data plane has three send paths with duplicated/divergent budget+error logic, and the legacy WinDivert engine (`capture/windivert_redirect.rs`, ~1,429 lines) has no live caller. MTU fixes must be applied in each path or they diverge. | `docs/design/custom-games-and-process-targeting.md:63`; `engine.rs:698-930` (no call sites); `capture/windivert_redirect.rs:997-1030` |
| F-08 | P2 | ~150 compatible dependency updates are available in the lockfile (`aws-lc-rs 1.18.1 → 1.19.0`, `aws-lc-sys`, `cc 1.4 → 1.11`, …). | `cargo update --dry-run` |
| F-09 | P2 | GitHub Actions are pinned to mutable major tags, with inconsistent majors (`checkout@v6`/`@v7`, `upload-artifact@v4`/`@v7`); several workflows have no explicit `permissions:`. | `.github/workflows/*.yml` |
| F-10 | P3 | Stale docs/counts: game registry is **27** but code comments say 20 and 23; the `[tunnel] mtu = 1400` example is not implemented; `protocol.md` describes PMTUD as if it governed the GUI path; `#168` status row is stale. | `client/src/games/mod.rs:392,1526,1549`; `docs/design/…:63-67`; `docs/protocol.md:246-262` |
| F-11 | P3 | The gate carries two pre-existing manifest warnings (`rcgen`, `windivert` unused). `rcgen` is test-only; `windivert-sys` has its own `vendored` feature, so the `windivert` crate is unnecessary. | `cargo clippy` output; `client/Cargo.toml:78,91,97` |
| F-12 | P3 | Notification hygiene: 282 unread repository notifications, almost all stale `ci_activity` from August–November; old `review_requested` rows point at PRs closed weeks ago. Only one recent non-CI notification exists (`#147`, already resolved). | `github-audit/notifications.json` |
| F-13 | P3 | `client-gui/src/app.rs` is 8,735 lines (next: `proxy/src/relay.rs` 3,177, `client/src/latency.rs` 2,928). Fine for now, but it is the main maintainability tax on the active product surface. | `Get-ChildItem` line counts |
| F-14 | P3 | Local GUI builds warn `no Windows resource compiler … exe carries no icon` because neither `rc.exe` nor `windres` is on PATH; verify the released GUI exe actually carries `RT_GROUP_ICON` (the build script degrades to a warning). | `client-gui/build.rs`; GUI clippy log |
| F-15 | P3 | `deny.toml` carries 18 advisory ignores (mostly unmaintained GUI transitives). Each is reasoned, but the list needs a periodic re-review; the `chrono` migration is the only one with a tracked code action. | `deny.toml` |

---

## F-01 (P1) — MTU: the real cause behind `#168`, and why the user was misled

### What the reporter measured

`#168` (Delta Force) posted three comments on 2026-11-17 after following the
project's own instructions. They confirmed **UDP gameplay** by using process
targeting with the full port range, captured the exact endpoints:

```
🔍 Locked game server: 43.130.90.129:28000
🔍 Locked game server: 94.74.76.3:28000
🔍 Locked game server: 43.164.65.4:28000
🔍 Locked game server: 43.153.142.239:17869
```

and then hit:

```
⚠️ sendmsg error: Os { code: 10040, kind: Uncategorized,
   message: "A message sent on a datagram socket was larger than the
   internal message buffer …" }, Transmit: { destination: <relay>:4433, len: 1404 }
   (also len: 1472)
```

`10040` is `WSAEMSGSIZE`. The GUI then reports
`Relay return path is starving … fault = NoReplyFromRelay` and switches the
session direct. The reporter runs Zapret, so their effective path MTU is below
the 1500-byte Ethernet default.

### Why the code does this

1. **The tunnel's fixed clamp is 1492 bytes and cannot go lower.**
   `protocol/src/header.rs:68` sets `CONSERVATIVE_PATH_MTU = 1492`; the maximum
   tunnel datagram is 1464 bytes. `PathMtuDiscovery` clamps every observation
   into `[1492, 1500]` (`client/src/tunnel/pmtud.rs:106,131`) and `record_failure`
   resets to 1492 (`pmtud.rs:149`). A path with a real MTU below 1492 therefore has
   **no supported state**: a 1432-byte datagram is classified "fits" and sent with
   don't-fragment set.

2. **The GUI path ignores both MTU knobs.**
   - `[tunnel] mtu` exists in `client/src/config.rs:71-73` with
     `default_mtu() = 1400` (`config.rs:261`), is documented in
     `client/lightspeed.example.toml:55-56` as "Maximum packet size (MTU)",
     and is **never read outside config tests** (verified by grep for
     `tunnel.mtu`/`.mtu` across `client/src`: only `config.rs` tests).
   - `--path-mtu-discovery` feeds only `client/src/tunnel/relay.rs`
     (`refresh_path_mtu`, `classify_at`, `datagram_fits_at`). The GUI boost path is
     `interceptor/windows.rs`, which calls
     `budget::classify(...)` (fixed 1492 clamp, line 1516) and
     `transport::send_datagram(...)` (which itself checks
     `budget::datagram_fits`, i.e. the fixed clamp, `transport.rs:114`). So the
     opt-in discovery has **no effect on the primary Windows product path**.

3. **Send errors are swallowed, so the cause is invisible.**
   `interceptor/windows.rs:1548-1554` and `:1583-1599` inspect only `.is_ok()`
   and never record why a send failed. The parity send at `:1568` is
   `let _ = ...`. `InterceptorCounters` has `errors` (intercept/inject only) and
   `packets_tunnel_sent`, but no send-error counter and no last-send-error string
   (`interceptor/traits.rs:234-241,268-269`). The reply-fault classifier therefore
   cannot distinguish "the relay never answered" from "our datagrams never left
   the machine" and prints the relay-accusing sentence
   (`client-gui/src/app.rs:389-418`). The GUI *does* have an `Unsent` health
   verdict (`app.rs:879-904,925-943`) that says "N of M relayed datagrams never
   left", but its remedy is "switch relay" and it carries no OS error.

4. **The same bug exists in the legacy engine and in `redirect.rs`.**
   `capture/windivert_redirect.rs:1008,1017` send with `let _ =` and
   `:1027` sends without applying the budget at all; the userspace manual-redirect
   path has its own send sites too. Fixing only one path leaves the others broken.

### What the reporter asked for, and the right answer

They asked for (a) MTU fragmentation/clamp handling and (b) a config/CLI flag to
clamp the tunnel packet size. The answer is yes to both, and the config field is
already declared — it just needs to be wired.

### Recommended fix (in order)

1. **Make `[tunnel] mtu` the effective path-MTU cap.** Change the default to 1492
   (preserving today's behaviour) and clamp it to a safe floor (1280). Thread it
   into `budget::classify_at` and `datagram_fits_at` for every send path
   (`relay.rs`, `interceptor/windows.rs`, `redirect.rs`, legacy engine).
2. **Handle `EMSGSIZE` explicitly.** On a send error, retry once with
   don't-fragment cleared (re-use `transport::send_datagram` / `FragmentAllowed`),
   and on a second failure record `send_errors += 1` plus `last_send_error` and
   surface it in `InterceptorCounters`.
3. **Stop blaming the relay.** Add a `LocalSendFailed` fault (or extend the
   existing message) when `send_errors > 0` or the unsent share is high; quote the
   OS error. This is the diagnostic that would have turned `#168` into one round
   instead of three.
4. **Apply the same fix to `capture/windivert_redirect.rs` or delete that engine**
   (see F-07).
5. **Update docs:** `docs/protocol.md` (sub-1492 behaviour), `client/lightspeed.example.toml`
   (`mtu` semantics/default), `docs/CLI-REFERENCE.md` (add `--mtu` if added), and
   the troubleshooting entry for `10040`.
6. **Acceptance:** a failing-before test that drives the classify/send path with a
   path MTU of 1400 and a 1432-byte datagram, plus a mutation check (force the
   clamp back to 1492 → test fails). A live confirmation needs the reporter's
   Zapret path or a loopback socket with an MTU-reduced interface; state that
   limitation in the task.

**Immediate unblock for the reporter (do not promise it as a permanent fix):**
lowering the Windows adapter MTU makes the game emit smaller datagrams, e.g.
`netsh interface ipv4 set subinterface "Ethernet" mtu=1400 store=persistent`
(their suggested 1350–1400 is in the right range). The project can say this in the
`#168` reply while the client fix ships.

---

## F-02 (P1) — the fleet alert is red and the state entry is stale

`wat/state/current-phase.md` (2026-11-17 audit entry) says the scheduled
`Health Monitor` / `Health Anomaly Monitor` runs are green. They are not:

- run `44189992620` (2026-11-17 22:25 UTC): **failure**, exit 1, 3 anomalies:
  - `[critical] auth_spike_no_sessions: relay-syd-1: auth_rejections +3309 with
    sessions_created +0 over 3 snapshots (fleet sessions +143)`
  - `[critical] zero_relay: relay-syd-1: reachable for 3 snapshots but relayed 0
    packets while the fleet relayed 2386089, and 3309 auth rejection(s) …`
  - `[warning] load_skew: relay-fra relayed 57.9% of the fleet (1.7x relay-bom-1)`
- run `44093060510` (2026-11-17 13:28 UTC): **failure**, 7 anomalies
  (`zero_relay` on `relay-nrt` and `relay-sgp-1`, `idle_relay` warnings on five
  relays) in a very quiet window (fleet relayed 62 packets).

**Live read-only snapshot (2026-11-18 ~04:20 UTC, public `/health`):**

| relay | status | version | ALPN | relayed | sessions | auth rejected | uptime |
|---|---|---|---|---|---|---|---|
| relay-lax-1 | healthy | 1.11.0 | required | 375,353 | 23 | 6,868 | 134,109 s |
| relay-ewr-1 | healthy | 1.11.0 | required | 577,463 | 61 | 3,866 | 134,757 s |
| relay-sgp-1 | healthy | 1.11.0 | required | 440,533 | 14 | 35,941 | 134,049 s |
| relay-fra | healthy | 1.11.0 | required | 3,551,788 | 190 | 35,913 | 135,000 s |
| relay-nrt | healthy | 1.11.0 | required | 65,172 | 13 | 15,215 | 134,768 s |
| relay-bom-1 | healthy | 1.11.0 | required | 1,325,918 | 29 | 19,532 | 134,086 s |
| relay-mad-1 | healthy | 1.11.0 | required | 246,724 | 34 | 45,624 | 134,487 s |
| relay-syd-1 | healthy | 1.11.0 | required | 17,903 | 6 | 3,365 | 134,783 s |

All eight are up, same process generation (~37 h uptime), `required` ALPN, and
zero `drops_relay_send_errors`. The fleet totals are **6.60 M packets relayed,
370 sessions, 166,324 auth rejections** — the M1 drain already tracked as task-10
is still visible at fleet scale. `relay-fra` carries 54% of relayed traffic;
`relay-syd-1` has 6 sessions in 37 h and is the relay the monitor flags.

This is **not a new failure of the monitor** — it is the monitor doing its job.
But two things need action:

1. The stale "green" claim must be corrected in state; a red scheduled run is the
   project's only alert channel (`[COMMS_STUB]`/state: no Discord webhook by
   design), so a red that no one reads is the same as no alert.
2. The `relay-syd-1` cluster is consistent with stranded pre-1.11.0 clients in
   AP-Southeast-2 (auth rejects, ~0 sessions, zero relayed in-window). The open
   decision from task-10 remains the owner's: keep `required` and accept the
   stranded clients, or roll one relay back to `off` as a mercy path for them
   (a fleet change; no such change was made by this audit). `relay-fra`'s 58%
   share is the separate load-rebalancing item already documented.

**Recommendation:** treat the two red runs as a task-10 status update, not as a
new detector bug; verify the `relay-syd-1` hypothesis against its metrics endpoint
(`/metrics` exposes the ALPN outcome split, `/health` does not), and record the
owner decision. Optionally have the workflow open/update a single tracking issue
on first failure per week so a red run is not lost in the 282-notification pile.

---

## F-03 (P1) — GitHub: one thread is owed a reply

Full sweep on 2026-11-18:

- **Notifications:** 458 total, 282 on this repo; recent non-CI items are the
  `#147` comment (resolved duplicate, owner answered), release subscriptions, and
  old `review_requested` rows for PRs #158/#159/#160 that are already
  closed/merged. No other reply is pending.
- **Open issues (7):** #176 (TCP profiles), #168 (Delta Force), #167 (bug),
  #166 (EA FC27), #153 (BDO), #148 (Aion 2), #140 (error 120). For all except
  #168 the last comment is the owner's; they are waiting on the reporter (#140,
  #153, #166, #167) or blocked on #176 (#148).
- **Open PRs:** none. All discussions have an owner reply as of 2026-11-17 00:45
  (announcement #177 is live); discussion #2 has a community message but no
  request.
- **#168 is the only unanswered external reply**, and it is the one that matters
  most: it contains the endpoint measurement the project asked for and the new
  MTU blocker.

**Actions:**
1. Reply to #168 (and update #63): UDP gameplay is confirmed; the profile is now
   addable as `dynamic_server = true` with the process target; the blocker is the
   client MTU bug (F-01), with the adapter-MTU workaround as the interim path.
   Do **not** promise a date.
2. Update `docs/game-requests.md:70` (Delta Force row) and the "awaiting facts"
   paragraph at lines 13-18; `docs/supported-games.md` only if a profile lands.
3. Keep #167/#140 open as waiting-on-reporter; no action needed.
4. Notification hygiene: bulk-mark the stale CI/review rows read (owner action),
   so the next real alert is visible.

---

## F-04 (P2) — CI does not lint Windows-only client code

`.github/workflows/ci.yml`:

- `check` (Linux) runs `cargo clippy --workspace --all-targets --exclude lightspeed-gui`
  — cannot see `#[cfg(windows)]` code.
- `windows-test` (`ci.yml:270-285`) installs `clippy` at line 278 but only runs
  `cargo build --release` and `cargo test --workspace --lib`.
- `windows-gui` lints only `lightspeed-gui` (`ci.yml:306`).

Consequently `interceptor/windows.rs`, `windivert_handle.rs`, `socket_owners.rs`,
`qos.rs`'s Windows branch and `transport.rs`'s `windows_set_dontfragment` are
compiled in CI but never linted. They are linted only by a developer running the
documented local gate on Windows (which this audit did, cleanly).

**Fix:** add to `windows-test`:

```yaml
- name: Clippy (Windows code)
  run: |
    cargo clippy -p lightspeed-client --features quic,windivert-redirect --all-targets
    cargo clippy -p lightspeed-proxy --features quic --all-targets
```

(Use the shipped feature set; `--all-features` would pull `pcap-capture`, whose
native SDK is not installed in that job.) The GUI job already covers the GUI crate.

Also note the documented gate in `AGENTS.md`/`QUALITY_STUB` says
`--all-features`, while CI's `check` step omits it; the feature matrix
(`ci.yml:312-338`) covers `quic|full|ml` but without `--all-targets`. That is
defensible (feature matrix exists for this), but the docs and CI should say the
same thing, or the local gate is stricter than CI.

---

## F-05 (P2) — Windows Firewall rules regrow after crashes

`client/src/interceptor/windows.rs:2036-2069` names each rule
`LightSpeed WinDivert Tunnel {port}` and adds it on every boost with the current
ephemeral tunnel port; `:2071-2097` deletes only that exact name during graceful
teardown. A crash, `taskkill`, or a killed GUI process leaves the rule behind.

The 2026-11-17 cleanup proved the cost: **614 → 543 firewall rules**, with 71
stale `LightSpeed WinDivert Tunnel*` rules removed by hand
(`wat/state/current-phase.md`). Nothing prevents the next 71.

**Fix (choose one):**
- **Preferred:** one stable rule named `LightSpeed WinDivert Tunnel` scoped to the
  program (`program=<exe>`), deleted then re-added on start; deletion by exact name
  is idempotent and self-healing. No per-port names, so no accumulation.
- Or keep per-port names but on startup best-effort delete every rule whose name
  starts with `LightSpeed WinDivert Tunnel ` (needs elevation; log if refused).
- Add a unit-testable rule-name builder and a teardown assertion; the `netsh`
  call itself stays Windows-only.

Also update the operator/FAQ note so a user knows how to clear leftovers
(`netsh advfirewall firewall delete rule name="LightSpeed WinDivert Tunnel"`);
the current cleanup command is buried in evidence.

---

## F-06 (P2) — the proxy's destination filter is weaker than the client's

`proxy/src/abuse.rs:292-356` `is_public_ipv4` rejects: unspecified, loopback,
10/8, 172.16/12, 192.168/16, 169.254/16, 100.64/10, the three TEST-NET blocks,
multicast and broadcast. `client/src/interceptor/process_scanner.rs:126-173` is
the stricter implementation: it also rejects **0.0.0.0/8**, **192.0.0.0/24**
(IETF assignments), **198.18.0.0/15** (benchmarking) and its tests pin those
boundaries.

The proxy is the security-critical side (it decides what a relay will send to),
and the two copies can drift silently — indeed they already have. All of the
classic SSRF targets are blocked on both sides (loopback, RFC1918, link-local,
metadata `169.254.169.254`), so this is hardening rather than a live hole.

**Fix:** keep one implementation (a small shared module in `protocol` or a
`lightspeed-net` helper) and use it on both sides; at minimum copy the stricter
ranges and boundary tests into `proxy/src/abuse.rs`. Add tests for
`0.1.2.3`, `192.0.0.1`, `198.18.0.1`, `198.19.255.255`, `240.0.0.1`.

---

## F-07 (P2) — three send paths, and a dead engine

The data plane currently has:

| Path | Where | Budget/MTU handling |
|---|---|---|
| `relay.rs::send_to_proxy` | capture/CLI paths | discovery-aware (`classify_at`, `datagram_fits_at`), returns `Result` |
| `interceptor/windows.rs` | **the GUI boost path** | fixed `classify`, `send_datagram`; errors swallowed |
| `redirect.rs` (manual mode) | GUI Manual server mode | own send sites |
| `capture/windivert_redirect.rs` | **no live caller** | own send sites, `let _ =`, no budget at all |

`docs/design/custom-games-and-process-targeting.md:63-67` already records that
`engine.start_windivert_for_game` has no live caller; this audit confirmed it:
`start_windivert*` has no call sites outside `engine.rs`, and the only caller of
`run_windivert_mode_with_shutdown` is those engine methods. The legacy engine is
kept as "the reference implementation of the UDP MITM and TCP terminator", but its
shared helpers (`build_ipv4_udp`, `parse_ipv4_tcp`) are imported by the live
`interceptor::windows`, so it cannot simply be deleted without moving those.

**Why it matters now:** F-01 has to be fixed in every live path. A fix applied
only to `relay.rs` would leave the GUI unfixed — that is exactly the class of bug
this duplication produces. Recommendation: move the shared packet helpers out,
delete the legacy engine (and its tests) or mark it `#[deprecated]` with a doc
note, and route every live path through one `send_tunnel_datagram(payload, ...)`
helper that owns the budget, fragmentation and error accounting.

---

## F-08 (P2) — dependency drift

`cargo update --dry-run` proposes ~150 lockfile updates, including
`aws-lc-rs 1.18.1 → 1.19.0`, `aws-lc-sys 0.45 → 0.47`, `cc 1.4 → 1.11`,
`bytes 1.12.1 → 1.12.2`, `clap 4.6.7 → 4.6.8`, `hyper 1.11.1 → 1.12.0`.
Dependabot runs Mondays and groups only patch bumps, so the next run may take some
of these; the crypto/`aws-lc` pair is worth taking deliberately and re-running the
gate + `cargo audit`/`cargo deny`.

**Actions:** either wait for Monday's Dependabot PRs and merge them, or run
`cargo update` + the full gate + security checks in one dedicated commit. Do not
mix the `aws-lc` bump into a feature change.

---

## F-09 (P2) — Actions pinning and permissions

- Actions are tagged by floating major (`actions/checkout@v7`, `@v6`,
  `upload-artifact@v4`/`@v7`, `Swatinem/rust-cache@v2`,
  `EmbarkStudios/cargo-deny-action@v2`). A compromised/retagged action would run
  with repo tokens. Pinning to a commit SHA is the standard hardening; Dependabot
  can keep the pins fresh with `pin-digests` semantics for GitHub Actions.
- `ci.yml`, `deploy.yml`, `security.yml`, `benchmark.yml`, `choco-push.yml` have no
  top-level `permissions:`. The repository default is read (state), so this is
  defence in depth: add `permissions: contents: read` at the workflow level and
  job-level writes only where needed (as `release.yml` already does).
- Version consistency: `release.yml` is still on `actions/checkout@v6` while the
  rest is `@v7`; `upload-artifact` is `@v7` in release and `@v4` in `ci.yml`'s
  coverage job. Unify or let Dependabot do it in one PR.

---

## F-10 (P3) — stale docs and counts

| Location | Claim | Truth |
|---|---|---|
| `client/src/games/mod.rs:392` | "the `games` tests enforce the 20-entry count" | registry is **27**; test asserts 27 at `mod.rs:903,1573` |
| `client/src/games/mod.rs:1526,1549` | "at 23 entries" | 27 |
| `docs/design/custom-games-and-process-targeting.md:63-67` | "registry … 23" and "doc comment still says 20" | now 27; the design note is itself stale |
| `client/lightspeed.example.toml:55-56` | "Maximum packet size (MTU). Default: 1400" | field is never read (F-01) |
| `docs/protocol.md:246-262` | DPLPMTUD raises the effective data-plane MTU | only `relay.rs`; the GUI interceptor ignores it |
| `docs/game-requests.md:13-18,70` | Delta Force blocked on measured endpoint | endpoints measured 2026-11-17; blocked on MTU bug (F-01) |
| `AGENTS.md` header | "Last updated: 2026-11-17" | state is 2026-11-18 (cosmetic) |

**Fix:** prefer removing hard-coded counts and pointing at the registry/test
instead of restating the number; update `protocol.md` with the sub-1492 limitation
and which surfaces honour `path_mtu_discovery`; update the `game-requests` rows
with F-03.

---

## F-11 (P3) — two manifest warnings, one dead constant

The gate's only warnings are `cargo::unused_dependencies` for `rcgen` and
`windivert` (`client/Cargo.toml:78,91`). `QUALITY_STUB` says zero warnings.

- `rcgen` is used only by `client/tests/*.rs` (the `quic`-gated integration
  suites). Move it to `[dev-dependencies]`; keep the `quic` feature for the
  other deps.
- `windivert` (the high-level crate) is not used — the code calls `windivert-sys`
  directly (`interceptor/windivert_handle.rs`). `windivert-sys` has its own
  `vendored` feature (verified in the registry cache), so depend on
  `windivert-sys = { …, features = ["vendored"] }` and drop `windivert`. That also
  removes the LGPL-licensed crate from the dependency set.
- `protocol/src/header.rs:57` `MAX_PAYLOAD_SIZE` is defined and re-exported but
  never used; delete it or document why it stays.

Run the full gate after, and confirm the warnings are gone.

---

## F-12 (P3) — notification hygiene

282 unread notifications on this repo; nearly all are `ci_activity` check-suite
failures from August–November (the CI debt that has since been fixed) plus
`review_requested` rows for PRs #158/#159/#160 that are closed/merged. This is
why a real alert (F-02) can hide. Recommend bulk-marking the stale set read /
"Done", and setting notification preferences so CI failures on `master` notify
but per-run check suites do not accumulate. This is owner-account state; the audit
did not change it.

---

## F-13 (P3) — monoliths

| File | Lines |
|---|---|
| `client-gui/src/app.rs` | 8,735 |
| `proxy/src/relay.rs` | 3,177 |
| `client/src/latency.rs` | 2,928 |
| `proxy/src/metrics.rs` | 2,669 |
| `client/src/interceptor/windows.rs` | 2,370 |

No action required for correctness. If the GUI keeps growing, split `app.rs` by
surface (boost/status, relay picker, game editor, settings, dialogs) into
`impl` blocks in separate modules; the tests already pin most behaviours, so the
move is mechanical. Defer until the F-01/F-07 work lands to avoid rebase pain.

---

## F-14 (P3) — the exe icon warning

`client-gui/build.rs` embeds the brand icon via `rc.exe` (MSVC) or `windres`
(GNU) and degrades to a `cargo:warning` if neither is on PATH. This host has
neither, so local builds have no exe icon. The release is built on
`windows-latest`, where the Windows SDK is present but `rc.exe` is not guaranteed
on PATH for the Actions shell.

**Check:** download the 1.11.0 GUI zip/exe from the release and assert the PE
carries `RT_GROUP_ICON` (a small PowerShell/Python check). If it does not, add
`rc.exe` to the build PATH (or install MinGW) in the release job; if it does, no
change is needed, but the local warning is expected.

---

## F-15 (P3) — advisory allowlist review

`deny.toml` ignores 18 advisories, nearly all "unmaintained" crates in the GUI
(gtk/atk/gdk), the ML stack (`instant`), the metrics stack (`protobuf`), and
`chrono`. Each has a reason. The only one with a code action is
`RUSTSEC-2026-0543` (`chrono`), used in 7 places (`client/src/processes.rs`,
`client/src/ml/collector.rs`, `client/src/ml/features.rs`,
`client/src/route/selector.rs`, `proxy/src/budget.rs`); migrating to
`time`/`jiff` is small. Add a "review every quarter / when the advisory DB
changes" note so the allowlist does not become permanent by accident.

---

## Verification baseline (this host, 2026-11-18)

Commands and evidence in `wat/evidence/2026-11-18/lead-gate/`:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | clean (exit 0) |
| `cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui` | exit 0; 0 lint warnings; 2 manifest warnings (F-11) |
| `cargo test --workspace --all --exclude lightspeed-gui` | **1,561 passed / 0 failed / 6 ignored** across 30 binaries: client 1,291/0/4, protocol 86/0/2, proxy 184/0/0 |
| `cargo clippy -p lightspeed-gui --all-targets` | exit 0; only the `windres` warning (F-14) |
| `cargo test -p lightspeed-gui` | **184 passed / 0 failed / 1 ignored** |
| **Total** | **1,745 passed / 0 failed / 7 ignored** |
| `git status` | clean at `343530b` |

The workspace test run needs `TEMP`/`TMP` inside the workspace on this host
(`target/test-temp`) because `target` is Low-integrity-labelled (documented in the
`lightspeed-windows` skill); without it, every `temp_dir()` test fails with
os error 5. That is an environment effect, not a code failure.

GitHub state at audit time: `v1.11.0` (39 assets), CI/CodeQL/Security Audit green
on `master` (run `44201438078`, 2026-11-17 23:40 UTC); Health Anomaly Monitor red
(F-02).

---

## Recommended order for the next sessions

1. **Reply to `#168`** with the measured facts + interim adapter-MTU workaround,
   and update the game-requests row (F-03). This unblocks the reporter while the
   code fix proceeds.
2. **MTU fix** (F-01) in one task, with the failing-before test and the
   `send_errors`/`last_send_error` observability; touch every live send path or
   delete the dead one (F-07). This is the only user-facing correctness fix in
   this audit.
3. **Fleet decision on task-10** (F-02): confirm `relay-syd-1` with `/metrics`,
   then the owner decides `required` vs a mercy rollback; record it. Also update
   the stale "green" claim in state.
4. **CI lint gap** (F-04) and **firewall cleanup** (F-05) — both small, both
   prevent recurring debt.
5. Then the P2/P3 cleanups: proxy IP filter (F-06), dependency bump (F-08),
   Actions pinning/permissions (F-09), docs (F-10), warnings (F-11), notifications
   (F-12).

## What this audit did not do

- No code, config, fleet or release change; no commit; no public post.
- No `#168` reply (it is a public post; the owner can send the drafted text or
  authorize it).
- No live match test; the MTU diagnosis is code-level plus the reporter's log.
  Confirmation needs a path with a real MTU below 1492 (the reporter's Zapret path
  or a loopback interface with a reduced MTU).
- No `cargo audit`/`cargo deny` local run (not installed on this host); CI's
  `Security Audit` is green on `master`.
- No wiki/site rendering check; existing localization checks are green in CI.

---

## Remediation round (same day) — what was fixed and verified

Implemented after the audit, with the full gate re-run on the integrated tree:

| ID | Status | What changed |
|---|---|---|
| **F-01** | **Fixed (code)** | `[tunnel] mtu` / `--mtu` is now a real path-MTU cap (default 1492, clamped 1280-1500): `PathMtuDiscovery` gained `path_mtu_cap`, `TunnelConfig::effective_mtu_cap`, and `PmtudConfig::default`. The Windows interceptor budgets against the effective MTU (`classify_at`, `send_datagram_at`), and on `EMSGSIZE`/`WSAEMSGSIZE` (`is_message_too_long`) lowers the budget by 64 bytes, retries the datagram with fragmentation, and records `send_errors`/`last_send_error`. `InterceptorCounters`/`InterceptorHealth`/`EngineStatus` carry the new fields, and the GUI's no-reply message now says a local send failure "cannot be the cause" and names the OS error instead of blaming the relay. GUI `config.toml` gained `tunnel_mtu`; the CLI gained `--mtu`. Tests: pmtud (cap/floor/cap-above-hard-cap/auto-lower), relay cap, transport classification, engine clamp, GUI clamp + message. |
| **F-04** | **Fixed (CI)** | `windows-test` now runs `cargo clippy -p lightspeed-client --features quic,windivert-redirect --all-targets` and `cargo clippy -p lightspeed-proxy --features quic --all-targets`, so `#[cfg(windows)]` code is linted. |
| **F-05** | **Fixed (code)** | One stable firewall rule (`LightSpeed WinDivert Tunnel`, program-scoped) replaces the per-port name; start deletes the stable and legacy per-port rule first, so a crash cannot accumulate new ones. A unit test pins the name. |
| **F-06** | **Fixed (code)** | Proxy `is_public_ipv4` now rejects `0.0.0.0/8`, `192.0.0.0/24`, `198.18.0.0/15` and `240.0.0.0/4` as well, matching the stricter client copy; boundary tests added. |
| **F-10** | **Fixed (docs)** | Registry counts (27) corrected in `games/mod.rs` and the design doc; Delta Force row/intro updated with the 2026-11-17 measurement and the MTU blocker; `protocol.md` documents the cap/auto-fallback; `CLI-REFERENCE.md` documents `--mtu` and `--path-mtu-discovery`; `troubleshooting.md` has a 10040/EMSGSIZE entry with the adapter-MTU workaround. |
| **F-11** | **Fixed (deps)** | Removed the unused `windivert` wrapper (use `windivert-sys` with `vendored`) and the unused `windows` crate; moved `rcgen` to `[dev-dependencies]`. The gate now has **zero manifest warnings**. |
| **F-02** | Confirmed | `/metrics` on relay-syd-1: `negotiated_v1=42`, `rejected_no_application_protocol=32,167`, 6 sessions, 17,903 relayed. The relay is healthy; ~99.8% of handshakes are pre-1.11.0 clients. Owner decision on `required` vs a mercy `off` remains open. |
| **F-03** | Drafted, not posted | Reply text held in `wat/evidence/2026-11-18/github-audit/issue-168-reply-draft.md`; publishing is an owner gate. |

**Verification (this tree):** `cargo fmt --all --check` clean; workspace clippy
`--all-features` **0 warnings** and exit 0; workspace tests **1,585 passed / 0
failed / 6 ignored** (client 1,314, protocol 86, proxy 185; +24 vs the audit
baseline); GUI clippy exit 0; GUI tests **186 passed / 0 failed / 1 ignored**
(+2). Total **1,771 / 0 / 7**.
**Mutation check:** forcing `effective_path_mtu` back to the old fixed clamp
makes both the pmtud cap test and the relay cap test fail; restoring the code
passes both. Logs in `wat/evidence/2026-11-18/remediation/`.

**Still open from the audit:** F-02's fleet policy decision (owner), F-03's public
reply (owner), F-09 Actions pinning/permissions, F-12 notification hygiene (owner
account), F-13 `app.rs` split, F-14 release-exe icon check, F-15 advisory-review
cadence.

**F-08 (dependency refresh) — attempted, reverted, and a new build finding.** The
~150 compatible updates themselves are fine, but rebuilding the *vendored*
`windivert-sys` on Windows failed with MSVC `C1041: cannot open program database
... vc140.pdb` when two build-script instances compiled `windivert.c`
concurrently (the generated PDB lives in the shared registry source directory,
not the target dir). This is a pre-existing build-system race, not a version
conflict: it only appeared because the update forced a rebuild of that crate.
The lockfile was reverted to the verified commit. To take the refresh later,
either build it on the CI runner (where the crate is compiled once per job), or
pass `-j 1` / `CFLAGS=/FS`, or stage the WinDivert build through a writable
target-local copy. Recommend letting Dependabot land it on a clean CI runner and
watching the Windows job rather than refreshing the lock by hand on this host.

