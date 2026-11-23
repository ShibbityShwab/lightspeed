# LightSpeed 1.12.4 — WinDivert handle-shape rollback

**Status: release candidate, 2026-11-22.** Rollback of the two WinDivert
handle-shape changes introduced in 1.12.3 after a fresh-boot field failure.

> **Corrected 2026-11-23:** the `v1.12.4` tag was cut while this note still said
> "release candidate", and its real-driver evidence did not establish the claim.
> See
> [Post-release verification correction](#post-release-verification-correction-2026-11-23).
> Historical tags are not rewritten.

## Why

On a clean 18:20 boot, 1.12.3 successfully ran one interception session, then
the immediately following start saw an orphaned WinDivert WFP provider even
though no process held a handle. `WinDivertTool` reported no clients, and only
stopping the driver/rebooting cleared it. The two changes unique to 1.12.3 were
the sniff preflight handle and the send-only inject handle. They are removed
pending a controlled real-driver reproduction; 1.12.2's handle shape is
restored.

## Fixed

- **Updated `url` to 2.5.9** for RUSTSEC-2026-0592, a denial-of-service stack
  overflow in `Url::origin()` on nested `blob:` URLs.
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

## Post-release verification correction (2026-11-23)

This note is kept as written and the tag is not rewritten. The record is
corrected here.

- **1.12.4 was not verified on the real driver.** The real-driver evidence for
  this patch was four rapid start/stop cycles. On review, those cycles carried
  **zero intercepted and zero tunnelled packets**, so they do not establish that
  the `start → stop → immediate start` behaviour was exercised.
- **The 1.12.3 attribution was never proven.** The orphaned WinDivert provider
  was attributed to the two handle-shape changes this patch reverts, but no
  failing-before control reproduced: on a clean boot the 1.12.3 build (tag
  commit `37ff8d0`) ran 30 traffic-bearing Stop/Optimize cycles and reported
  30/30 cycles and 0 failure hits
  (`wat/evidence/2026-11-23/task-107-control-1123-20261123-120914/`). The
  rollback was a reasonable precaution, not a proven fix.
- **1.12.4 was tagged while this note said release candidate.** The published
  release body (the CHANGELOG section) did not carry that status.

The successor design (1.12.5) replaces the handle-shape question with a pinned
non-diverting driver handle and carries an 84/84 real-driver matrix; see
[release-notes-1.12.5.md](release-notes-1.12.5.md). The `v1.12.4` tag and its
published text are unchanged.
