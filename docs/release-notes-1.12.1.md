# LightSpeed 1.12.1 — patch release notes

**Status: release candidate, 2026-11-21.** Patch release for the WinDivert
relay-switch failure reported in the field. No feature, copy, protocol, config or
CLI change.

## Fixed

- **Relay switches no longer leave a stale WinDivert filter behind.** Stopping an
  interceptor returned as soon as its owner threads acknowledged, before the
  handles were actually closed. A relay restart could then open a new filter
  while the old one was still registered (`FWP_E_IN_USE`), and a late close could
  hit a recycled handle value. The platform teardown now closes intercept,
  inject and shadow handles synchronously after every owner acknowledged (and
  deliberately does not close on an acknowledgement timeout). Regression tests
  cover close-after-ack and the timeout path.
- **Real WinDivert open failures are reported.** `WinDivertOpen` signals failure
  with `INVALID_HANDLE_VALUE`, not NULL; the old check treated that sentinel as a
  live handle and surfaced the failure later as
  `WinDivert receive handle is invalid (os error 6); interception stopped`. Both
  failure sentinels are now rejected, and the start error names the stale-filter
  remedy (close a stale LightSpeed process or restart Windows).

## Upgrade notes

- Existing 1.12.0 installs should update before relying on relay switching.
- A machine already carrying a stale filter from 1.12.0 needs one Windows restart
  (or the stale LightSpeed process ended) to clear it; 1.12.1 then reports the
  condition accurately if it ever recurs.

## Verification

Windows/Linux/macOS builds, GUI tests, client/CLI/golden tests, guard, localization
and the now-required headless smoke run in CI. Runtime verification used the real
WinDivert driver on a Windows host: interceptor start and repeated relay-switch
teardown were checked after the fix; the stale-filter path was reproduced before
it.
