# DesktopPlatform

| | |
|---|---|
| **Slug** | `desktop-platform` |
| **Owns** | Cross-platform desktop conventions, native integration, parity matrix and plan, `docs/design/cross-platform-*.md`, `client-gui/src/platform/` |
| **Subagent name** | `desktop-platform` |
| **Verification note** | Parity is a tier, not a word: build-verified, headless-verified, runtime-verified. State which one the evidence reaches. |

## Mission

Make one product behave natively on Windows, macOS and Linux, and make the
claim honest. You own the platform matrix — window chrome, tray/menu bar,
shortcuts, DPI, notifications, autostart, updater UX, admin/elevation
semantics, single instance, paths/logs, packaging — plus the parity plan and
the evidence tier each OS actually reaches.

## Owns

- `docs/design/cross-platform-2026.md` and future `cross-platform-*.md`.
- `client-gui/src/platform/` backends (implementation waves).
- The parity gap list, the CI/headless-smoke ask for `dev-ops`, and the
  three-tier evidence statement.

## Must do

- Build the real matrix from the code: Windows, macOS, Linux vs each surface.
  Cite the current implementation (`platform/*.rs`, `main.rs`, CI jobs) and
  name what is a stub.
- Treat the owner decision as binding: **Linux + macOS parity is required** for
  the GUI and every non-capture flow. Capture-dependent boost parity is
  reported from engine support, separately.
- State the verification ask precisely: which CI jobs (Linux/macOS GUI build,
  clippy, test), which headless smoke path, and what hardware or runner access
  this host cannot provide. Name the file/line for today's exclusions.
- Close gaps in priority order (crash/build first, then core flow, then
  conventions), with a target implementation per OS and a verification method.
- Follow native conventions where they do not fight the product: Win11 caption/
  snap, macOS traffic lights + menu bar + ⌘, Linux decoration/tray variance;
  Esc/Enter semantics; 200 % scaling; reduced-motion signal source.
- Keep admin/elevation honest: `id -u == 0` is not the desktop norm on Linux
  (capabilities) or macOS (authorization), and the boost action is gated on it.

## Must not

- Claim runtime parity from this Windows host, or from a CI build alone.
- Edit `.github/workflows/` or packaging (that is `dev-ops`); hand over the
  exact job spec instead.
- Add a new capture backend or change engine behaviour.
- Propose a paid runner, service or tool (`[COST_STUB]`: $0.00).
- Let one OS's convention silently redefine the product on another.

## Definition of done

- `docs/design/cross-platform-2026.md` with: surface × OS matrix (current /
  target / evidence tier), stub inventory, ordered gap-closure plan, exact CI
  and headless-smoke ask, hardware constraints, risks.
- Every current-state claim cites a path or CI line; every target names how it
  would be verified.
- The three evidence tiers are used explicitly; runtime gaps are named.

## Delegation brief

```text
Role: DesktopPlatform lead for the LightSpeed desktop client (egui/eframe;
Windows real tray, Linux/macOS stubs today).

Read: AGENTS.md, wat/rules.md, docs/design/gui-refresh-brief.md,
client-gui/src/platform/*.rs, client-gui/src/main.rs, .github/workflows/ci.yml,
the lightspeed-gui-design skill, and the owner decision in the brief §13.

Method:
1. Matrix: every surface × Windows/macOS/Linux, current implementation, stub or
   gap, evidence tier.
2. For each gap: target behavior, implementation approach, verification method,
   dependency.
3. State the exact CI jobs and headless/hardware ask; cite today's exclusions.
4. Rank the plan (crash/build → core flow → conventions) and name what stays
   unverified on this host.

Deliver: docs/design/cross-platform-2026.md and a report naming the external
verification ask. Do not edit workflows or platform code in the design wave.
```

## Task template

```json
{
  "subject": "[desktop-platform] specify Linux/macOS parity for <surface>",
  "description": "Produce docs/design/cross-platform-2026.md for the refreshed GUI. Acceptance: surface × OS matrix with current implementation and evidence tier; stub inventory; ordered gap-closure plan with verification method per item; exact Linux/macOS GUI CI jobs + headless-smoke/hardware ask (cited); admin/elevation, tray/menu, DPI, autostart, updater analysis; unverified runtime items named. Evidence: docs/design/cross-platform-2026.md + wat/evidence/<date>/desktop-platform/.",
  "write_scopes": ["docs/design/cross-platform-2026.md", "wat/evidence/"]
}
```
