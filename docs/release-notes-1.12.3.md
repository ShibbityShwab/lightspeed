# LightSpeed 1.12.3 — WinDivert hardening

**Status: release candidate, 2026-11-21.** Reliability hardening for the Windows
interception path. No protocol, wire, config or CLI-surface change.

> **Corrected 2026-11-23:** the `v1.12.3` tag was cut while this note still said
> "release candidate", and the failure it addressed was never reproduced in a
> controlled environment. See
> [Post-release verification correction](#post-release-verification-correction-2026-11-23).
> Historical tags are not rewritten.

## Why

1.12.2 removed the relay-switch restart that was the main source of stale
WinDivert WFP state. The remaining risk was the other teardown and start paths:
an error after the handles were opened could leave them to be closed
asynchronously, while a retry opened the next filter; and a failed teardown was
not latched, so the next start could still proceed.

## Fixed

- **Fail-closed session ownership.**
  - `InterceptorHandle::stop_and_wait` latches a teardown timeout and refuses to
    report success or close handles on a second call.
  - `LightSpeedEngine` records an incomplete teardown and refuses every later
    start with "restart LightSpeed"; `start_interceptor_plan` also stops any
    existing handle first because the live CLI modes call it directly.
- **Start error paths release handles synchronously.** The tunnel UDP socket is
  bound before `spawn_blocking`/owner tasks are created; the only remaining
  fallible setup steps now unwind while no thread can touch a handle.
- **Driver preflight.** Immediately before a start — never during standalone
  status/capability queries — a sniff handle with a real filter exercises the
  per-handle WinDivert installation path and maps the actionable failures: not
  administrator, stale WFP filter, invalid signature, driver blocked. The probe
  does not block starts on unclassified errors. A successful probe also clears a
  teardown latch left by a transient stop timeout, so one slow teardown cannot
  permanently disable boost until restart.
- **Write-only inject intent.** The inject handle is opened with `send_only`.
  This does not reduce the WFP object count (its `"false"` filter already
  installed no callouts); it makes the write-only contract explicit.
- **Live relay exclusion.** The tunnel task can switch relays through
  `session::current_proxy()` without reopening WinDivert. Packets to a relay
  selected after start are now checked against the live proxy per packet and
  re-injected rather than tunnelled back to a relay.
- **Verification.**
  - Windows CI now executes `cargo test -p lightspeed-client --features
    quic,windivert-redirect --lib`, not only clippy.
  - `windivert_open_close_stress_50_cycles` is an `#[ignore]` real-driver churn
    harness for an elevated Windows shell.
  - GUI Settings reports "Driver: WinDivert ready/missing" instead of probing a
    dependency the shipped build does not use.

## Deliberately not in this patch

The following are still valid hardening items, not regressions:

- consolidating intercept/inject into a single handle (requires real-driver
  verification);
- retiring the unused legacy `capture/windivert_redirect.rs` backend;
- Authenticode verification / protected install path for portable installs;
- machine-wide (`Global\`) single-instance enforcement and removing `--force`
  from the normal elevation path;
- MSI stop-before-upgrade / driver cleanup actions.

## Residual risk

The teardown latch is cleared by a successful start-only sniff+filter probe.
That probe exercises the same per-handle callout/filter installation class as a
real start, but it is not a proof that every old handle object has been
released: if a previous teardown timed out because a handle-owning thread is
genuinely stuck, the probe can succeed beside that zombie. The common timeout
causes this patch addresses are the tunnel task's synchronous firewall cleanup
and a panicked non-handle task; for those, the probe is a valid recovery gate.
A future patch should retain the failed handle and prove its release directly.

## Upgrade notes

- Existing 1.12.2 installs can upgrade in place.
- A machine that already carries stale WinDivert WFP objects still needs one
  Windows restart; 1.12.3 detects that state at preflight and names it, and
  prevents the normal start/stop paths from creating it again.

## Post-release verification correction (2026-11-23)

This note is kept as written and the tag is not rewritten. The record is
corrected here.

- **1.12.3 was tagged while this note said release candidate.** The published
  release body (the CHANGELOG section) did not carry that status.
- **The failure this patch addressed was not reproduced.** The changes here were
  aimed at the stale-WinDivert/`FWP_E_IN_USE` failure, but the controlled control
  run did not reproduce it: the 1.12.3 build itself ran 30 traffic-bearing
  Stop/Optimize cycles on a clean boot with 0 failure hits (5 PASS rows, 1
  BLOCKED control row;
  `wat/evidence/2026-11-23/task-107-control-1123-20261123-120914/`). The sniff
  preflight handle introduced here was later removed in 1.12.4 as a *suspected*
  contributor to an orphaned WFP provider, and that attribution remains
  unproven.
- **The successor design supersedes this approach.** 1.12.5 removes the
  unload/reload churn path with a permanent non-diverting driver pin and carries
  an 84/84 real-driver matrix; see
  [release-notes-1.12.5.md](release-notes-1.12.5.md).
