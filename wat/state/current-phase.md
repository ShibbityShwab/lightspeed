# Current Phase — WF-018: v1.2.3 Post-Release Housekeeping

**Workflow:** WF-018
**Agent:** RustDev + QAEngineer + NetEng
**Status:** ✅ Code complete — pending `v1.2.3` tag/release
**Last updated:** 2026-08-29

---

## Summary

Housekeeping pass over GitHub feedback since v1.2.2. Fixed a critical auth
regression and a WinDivert handle leak, added a game profile, and clarified
installer docs.

| Item | Status |
|------|--------|
| Bug 1 — data-plane auth rejected 100% (issue #59) | ✅ Fixed |
| Bug 2 — WinDivert `FWP_E_IN_USE` handle leak (issue #59) | ✅ Fixed (code) + documented |
| Dead by Daylight profile (issue #51) | ✅ Added |
| client-vs-GUI install confusion (#50, #58) | ✅ Docs clarified |
| CS:GO Legacy (issue #60) | ⏳ Awaiting store page from reporter |

---

## Next Action

1. **Tag and push `v1.2.3`** (cargo-dist CI builds + publishes the release).
2. **Reply to open issues** (#59, #58, #50, #51, #60) with findings.
3. **WF-019**: Consider a single "lightspeed" Windows installer bundling the
   GUI + CLI (the GUI already embeds the client; a unified artifact would
   further reduce install confusion).
