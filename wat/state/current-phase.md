# Current Phase: WF-023 Windows GUI Startup + WinDivert Teardown (community feedback)

**Workflow:** WF-023 (plus WF-025 observability, below)
**Agent:** RustDev + QAEngineer + DevOps
**Status:** Implemented, pending release
**Last updated:** 2026-09-19

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
