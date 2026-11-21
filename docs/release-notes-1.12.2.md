# LightSpeed 1.12.2 — patch release notes

**Status: release candidate, 2026-11-21.** Hotfix for the WinDivert failure that
appeared after selecting relays. No protocol, config or CLI change.

## Fixed

- **Relay selection no longer restarts interception.** The GUI used to stop the
  whole WinDivert session and start it again on every relay change. The tunnel
  task already reads `session::current_proxy()` for each outbound datagram, so
  the restart was unnecessary. Tearing down and recreating the driver handles
  could leave the WinDivert WFP provider/sublayers registered; after two relay
  switches the next start failed with `FWP_E_IN_USE` and then the misleading
  "receive handle is invalid" line. Relay selection now only reconnects the
  control plane and publishes the new proxy; the running interceptor follows it
  without reopening the driver. This matches the CLI's existing `switch_relay`
  behaviour.
- 1.12.1's synchronous handle close is retained for real stops (game exit,
  explicit stop, quit).

## Upgrade notes

- A machine that already has stale WinDivert WFP objects from 1.12.1 needs one
  restart to clear them; this patch prevents relay switches from recreating that
  state.
- After restart, launch LightSpeed as administrator and use relay selection
  normally. No driver reinstall is needed.

## Verification

CI builds/tests on Windows, Linux and macOS, including the required headless
smoke and the GUI boundary guard. Runtime verification: with 1.12.2, selecting
relays repeatedly must leave a single WinDivert session running (no new
"Opening WinDivert intercept handle" lines) and must not produce
`FWP_E_IN_USE`.
