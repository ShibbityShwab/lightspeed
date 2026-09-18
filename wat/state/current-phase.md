# Current Phase: WF-023 Windows GUI Startup + WinDivert Teardown (community feedback)

**Workflow:** WF-023
**Agent:** RustDev + QAEngineer + DevOps
**Status:** Implemented, pending release
**Last updated:** 2026-09-18

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

Known issues left open (documented, not release blockers):
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
