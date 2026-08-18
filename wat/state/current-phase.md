# Current Phase — WF-014: v1.0.0 Release

**Workflow:** WF-014  
**Agent:** RustDev + QAEngineer + InfraDev  
**Status:** ✅ v1.0.0 release work completed  
**Last updated:** 2026-08-18

---

## Completed Steps (WF-014)

| Step | Description | Status |
|------|-------------|--------|
| 1 | Dependency health — egui bump to eframe 0.36 / egui_plot 0.37 | ✅ Done |
| 2 | Installer wizard via cargo-dist v0.32.0 | ✅ Done |
| 3 | Self-hosting guide replacing the community-network idea | ✅ Done |
| 4 | Security audit — documented the auth gap (full token auth deferred) | ✅ Done |
| 5 | 4 new game profiles — MapleStory / Genshin / Rocket League / World of Tanks | ✅ Done |
| 6 | Grafana password hardening | ✅ Done |
| 7 | Version bump to 1.0.0 | ✅ Done |

---

## v1.0.0 Release Summary

The v1.0.0 release run addressed dependency health, installer tooling, documentation, security defaults, and game coverage ahead of the public stable release.

- **Dependency health:** egui bumped to eframe 0.36 / egui_plot 0.37 to keep the GUI stack current.
- **Installer wizard:** adopted cargo-dist v0.32.0 to produce platform installers rather than hand-rolled packaging.
- **Self-hosting guide:** a dedicated guide now documents zero-cost self-hosting, replacing the earlier community-proxy-network idea.
- **Security audit:** traced the client/proxy auth flow; `require_auth` stays `false` by default because the client does not stamp session tokens into data-plane packets. Full token auth is documented and deferred.
- **Game profiles:** MapleStory, Genshin, Rocket League, and World of Tanks added, expanding supported-game coverage.
- **Grafana hardening:** the weak default Grafana password was removed.
- **Version:** bumped to 1.0.0.

---

## Next Action

1. **Tag and push the v1.0.0 release** (not yet done — awaiting explicit release step)
2. **WF-015**: Configurable ports (issue #39)
3. **WF-016**: TCP tunnel (issue #10)
