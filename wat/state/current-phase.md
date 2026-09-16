# Current Phase: WF-022 v1.4.3 Linux Interception + Security Patch

**Workflow:** WF-022
**Agent:** RustDev + NetEng + QAEngineer + DevOps
**Status:** Releasing `v1.4.3`
**Last updated:** 2026-09-16

---

## Summary

Two things drove this release. First, a rustls advisory (RUSTSEC-2026-0285) published
on 2026-09-14 turned the Security Audit workflow red, so the patched rustls ships here.
Second, an Oracle-assisted audit of the Linux interceptor found that Linux interception
was effectively non-functional: server discovery broke on modern `ss`, the fallback mode
blackholed matched traffic, and the receive thread spun a CPU core while idle. All are
fixed, and server rotation is now followed.

| Item | Status |
|------|--------|
| rustls 0.23.45 (RUSTSEC-2026-0285) | Fixed |
| Linux: `ss -unp -a` + State-column-agnostic parser + `/proc/net/udp` fallback | Fixed |
| Linux: destination-less rule removed (no more traffic blackhole) | Fixed |
| Linux: recvmsg busy-spin replaced with `poll(2)` | Fixed |
| Linux: server rotation via 5s scanner poll and a pure `RotationTracker` | Added |
| Linux: teardown removes the currently installed rule | Fixed |
| Linux: `--smoke-test` rewritten as a synthetic end-to-end test | Added |
| Windows GUI fixes from v1.4.2 | Unchanged, still shipped |

---

## Verification

- `cargo fmt --check` clean; `clippy --workspace --all-targets --exclude lightspeed-gui`
  clean for default, `quic`, `full`, and `ml`; `clippy -p lightspeed-gui` clean.
- `cargo test -p lightspeed-client`: 185 lib tests plus the bin target, all pass
  (includes 7 `RotationTracker` cases and 8 `ss`/`/proc` parser fixtures).
- Linux end-to-end, run under sudo against a live relay:
  `sudo ./target/debug/lightspeed --smoke-test --proxy 45.77.32.236:4434` PASSED with
  waiting state, exact-IP rule install, 5/5 relayed and 5/5 injected back with the
  correct source, rotation to a second TEST-NET address with no post-swap traffic to the
  old one, teardown of all `lightspeed_` tables, and an idle CPU sample of 0 ticks/s.
- Live fleet unaffected: `--probe-proxies` finds all five relays, `--test-control`
  registers a session, and no leaked nft tables remain.
- `dist plan` lists the Windows client zip and GUI assets at 1.4.3.
- Post-tag follow-up: master also carries `b6521f1`, which cfg-gates a Linux-only
  `std::time::Duration` import in `smoke_test.rs`. That unused import broke the Windows
  and macOS CI jobs under `RUSTFLAGS=-Dwarnings`. CI is fully green on `b6521f1`
  (all ten jobs) and the Security Audit is green. The v1.4.3 release binaries are
  unaffected because the release workflow does not enable `-Dwarnings`.

**Known verification gap:** the Windows GUI runtime still cannot be executed on this
host, so Windows behavior rests on the CI Windows jobs plus the Linux-runnable unit
tests for shared decision logic. Real-game Linux validation (Fortnite under Proton) has
not been performed; the synthetic end-to-end test is the current guard.

---

## Next Action

1. Confirm the `v1.4.3` release publishes every asset.
2. Refresh `dist/aur` (pkgver plus sha256, rebuild) and submit the winget 1.4.3 manifest
   PR from the existing `ShibbityShwab/winget-pkgs` fork.
3. Reply on the community threads with the v1.4.3 link.
4. **WF-023** candidates: conntrack as a route source for unconnected game sockets
   (Fortnite under Proton); per-generation handoff to remove the rotation silence gate;
   NFQUEUE only if true pass-through is ever required; F5 registry cert pinning and
   binary release signing from the security backlog.
