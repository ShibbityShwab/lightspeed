# Current Phase — WF-019: v1.4.0 Community Relay Network Launch

**Workflow:** WF-019
**Agent:** Architect + InfraDev + RustDev + QAEngineer
**Status:** ✅ Code complete — pending `v1.4.0` tag/release
**Last updated:** 2026-09-13

---

## Summary

Launched a sponsor-funded, community-discoverable global relay network: 5 relays
(Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo) advertised via a signed
static registry on GitHub Pages. Clients now auto-discover the fastest path with
zero configuration. Also added the Roblox game profile, fixed the GUI console
window and installer shortcut, and refreshed the website with a Global Network
section.

| Item | Status |
|------|--------|
| 5-relay network (fra + nrt provisioned) | ✅ Live + healthy |
| Signed registry on GitHub Pages (5 nodes) | ✅ Signed + committed |
| Zero-config client registry discovery | ✅ Wired + tested |
| Roblox game profile (issue #64) | ✅ Added |
| GUI console window (issue #68) | ✅ Fixed |
| Installer Start Menu shortcut (issue #67) | ✅ Fixed |
| Website Global Network section | ✅ Updated |
| Issue triage (#50, #51, #58, #60 closed; #59, #62, #66 responded) | ✅ Done |

---

## Next Action

1. **Tag and push `v1.4.0`** (cargo-dist CI builds + publishes the release).
2. Close #64, #68, #67 once the release ships.
3. **WF-020** candidates: dynamic registry self-registration (Cloudflare Worker),
   TCP game-traffic support (issue #66), Fortnite server re-detection (issue #59).
