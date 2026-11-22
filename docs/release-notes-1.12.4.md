# LightSpeed 1.12.4 — WinDivert handle-shape rollback

**Status: release candidate, 2026-11-22.** Rollback of the two WinDivert
handle-shape changes introduced in 1.12.3 after a fresh-boot field failure.

## Why

On a clean 18:20 boot, 1.12.3 successfully ran one interception session, then
the immediately following start saw an orphaned WinDivert WFP provider even
though no process held a handle. `WinDivertTool` reported no clients, and only
stopping the driver/rebooting cleared it. The two changes unique to 1.12.3 were
the sniff preflight handle and the send-only inject handle. They are removed
pending a controlled real-driver reproduction; 1.12.2's handle shape is
restored.

## Fixed

- **Sniff preflight handle removed.** The teardown latch is now cleared only
  after a real interceptor start succeeds. No extra probe handle is opened
  before a start.
- **Send-only inject handle reverted** to the 1.12.2 normal `false` handle.
- **Retained:** fail-closed teardown with the latched acknowledgement timeout,
  synchronous start-error unwind, and per-packet live relay exclusion.
- **Close failures are logged** per handle instead of being discarded.
- **Start open failures use the actionable remedy**: administrator, stale WFP
  filter, invalid signature, or driver blocked.

## Upgrade notes

- Machines running 1.12.3 with an orphaned WinDivert provider need one real
  Windows restart; 1.12.4 cannot remove that kernel state and does not use the
  probe handle that may have caused it.
- After the restart, launch LightSpeed as administrator.

## Verification status

The code and CI gates are running. The critical check is a real-driver
`start → stop → immediate start` sequence on a **clean boot**, not a unit test.
That run is required before this patch is called verified.
