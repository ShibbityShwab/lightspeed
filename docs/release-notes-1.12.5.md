# LightSpeed 1.12.5 — persistent WinDivert capture session

**Status: release candidate, 2026-11-22.** This is an architectural fix for the
repeated `FWP_E_IN_USE` failure, not another teardown tweak.

## Why

Every GUI Optimize was a full WinDivert close/reopen. Four graceful cycles could
pass, then a later one would leave an orphaned WFP provider even though
`WinDivertTool` reported no client handles. The driver's close/reference path is
not reliable enough to be exercised on every user optimization.

## Change

- **Open once, keep warm.** The engine keeps the interceptor handle and its
  tunnel task alive across Stop/Optimize.
- **Stop becomes Pause.** The handle's existing direct/tunnel gate is set to
  direct, so traffic flows normally while the kernel filter stays registered.
- **Optimize resumes in place.** A plan with the same capture scope flips the
  gate back on, updates the relay/control plane, and reuses the same handles.
  No `WinDivertOpen`/`WinDivertClose` occurs.
- **Relay changes still never reopen.** They remain a data-plane update.
- **Only Quit or a scope change closes.** An app Quit, a different game/profile
  or capture scope, or a backend without a gate performs the full teardown.
- A paused session is reported as inactive interception, not as active.

## Verification

- Mock regression: `pause_and_resume_keep_the_capture_session_warm` asserts the
  backend handle survives pause and the same-scope start resumes it.
- Real-driver verification must show repeated Optimize on a clean boot with the
  same WinDivert handles and no new filter opens. That run is the release gate.

## Upgrade

- If the machine is currently in the force-kill stale state, one real reboot is
  still required; 1.12.5 cannot remove orphaned kernel state.
- Normal use: quit LightSpeed from the tray (Quit), not Task Manager/End task.
