# Current Phase — WF-020: v1.4.1 Relay Discovery + Naming Fixes

**Workflow:** WF-020
**Agent:** RustDev + QAEngineer + DevOps
**Status:** ✅ Code complete — releasing `v1.4.1`
**Last updated:** 2026-09-14

---

## Summary

Fixed community relay discovery, standardized fleet naming, and replaced the
website's personal RTT numbers with live network-wide health stats, then cut
v1.4.1. Zero-config `--probe-proxies` now finds all five relays (it previously
fell back to nothing instead of the compiled-in registry), discovered relays
feed the rerouting/multipath list, probes honor the configured control port and
report real node IDs, and every node ID is `relay-*`.

| Item | Status |
|------|--------|
| `--probe-proxies` zero-config fallback | ✅ Fixed |
| Registry relays plumbed into rerouting/multipath | ✅ Fixed |
| Probe control-port + real node IDs | ✅ Fixed |
| Registry test isolation (`LIGHTSPEED_REGISTRY_STATE`) | ✅ Fixed |
| Fleet + registry naming standardized on `relay-*` | ✅ Live + re-signed |
| Live network stats (`network-stats.json` + Pages workflow) | ✅ Added |
| `deploy-all.sh` unclosed quote | ✅ Fixed |
| Registry re-signed with operator key | ✅ Verified |

---

## Next Action

1. **Tag and push `v1.4.1`** (cargo-dist CI builds + publishes the release).
2. After the release publishes, optionally refresh `dist/aur/PKGBUILD`
   (pkgver 1.3.2 -> 1.4.1 plus refreshed sha256 sums) for the AUR handoff.
3. **WF-021** candidates: dynamic registry self-registration (Cloudflare Worker),
   TCP game-traffic support (issue #66), Fortnite server re-detection (issue #59).
