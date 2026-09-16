# Current Phase: WF-021 v1.4.2 Community Feedback Fixes

**Workflow:** WF-021
**Agent:** RustDev + NetEng + QAEngineer + DevOps
**Status:** Releasing `v1.4.2`
**Last updated:** 2026-09-16

---

## Summary

Audited the post-v1.4.1 community feedback (issues #59, #62, #66; discussions
#63, #69) plus live relay metrics, then shipped one release addressing every
actionable report. The headline problem was that the relay fleet was healthy but
almost unused: clients could not see the relays, and Fortnite stalled because the
interceptor locked onto a dead lobby server.

| Item | Source | Status |
|------|--------|--------|
| Windows GUI tray Quit left a zombie | #59 | Fixed |
| GUI stacked multiple instances | #59 | Fixed |
| GUI listed loopback placeholders instead of the real relays | #59, #69 | Fixed |
| No GUI config reset / visibility | #59 | Fixed |
| No QUIC/auth or relay usage diagnostics | #59 | Fixed |
| GUI game list missing supported games | #59 | Fixed |
| Fortnite locked onto the lobby server, live match not tunneled | #59 | Fixed |
| Transient WinDivert recv error tore down the interceptor | #59 | Fixed |
| `--probe-proxies` double fetch and invisible report | #59 | Fixed |
| TCP-only games (Angels Online) cannot be accelerated | #66 | Documented, not implemented |
| Requested profiles (Battlefield 6, Warface, Delta Force) | #63 | Under evaluation, no fabricated ports |
| Example config + per-OS install guides | #59 | Added |
| Windows CLI build (zip, unsupported) | #59 | Added |

---

## Verification

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --exclude lightspeed-gui`
  clean for default, `quic`, `full`, and `ml`; `cargo clippy -p lightspeed-gui` clean.
- `cargo test --workspace`: all suites pass (client 169, GUI 171, plus proxy/protocol).
- Live fleet: `--probe-proxies` discovers all five relays in one pass with a stdout
  report; `--test-control` registers a session; `--live-test` relays 5/5 packets with
  payload match through `relay-sgp-1`.
- `dist plan` lists `lightspeed-client-x86_64-pc-windows-msvc.zip` and the GUI
  Windows assets at 1.4.2.

**Known verification gap:** the Windows GUI runtime (tray Quit, single instance,
discovery) could not be executed on this host. The decision logic is unit tested with
Linux-runnable tests, and Windows compilation is covered by the CI Windows jobs. This
matches existing project policy for Windows-only code.

---

## Next Action

1. Watch the `v1.4.2` release workflow to completion and confirm every asset publishes.
2. Post-release: refresh `dist/aur/PKGBUILD` (pkgver plus sha256 sums) and
   `dist/winget` for 1.4.2.
3. Reply on issues #59, #62, #66 and discussions #69, #63 with the release link.
4. **WF-022** candidates: dynamic registry self-registration; interceptor re-detection
   on Linux; TCP game-traffic support (issue #66) if a design lands; F5 registry cert
   pinning and binary release signing from the security backlog.
