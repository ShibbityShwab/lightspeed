# WF-006 Step 6 — Feedback Triage Guide

This document defines how incoming feedback (GitHub Issues, GitHub Discussions, Discord, Reddit) is triaged, labelled, and routed into the development cycle.

---

## Label Taxonomy

Apply these to GitHub Issues and PRs. Labels should be created in the repo under **Settings → Labels**.

### Type labels
| Label | Colour | Meaning |
|---|---|---|
| `bug` | `#d73a4a` (red) | Something is broken that was working before |
| `feature` | `#0075ca` (blue) | New capability request |
| `game-profile` | `#7057ff` (purple) | Request to add support for a new game |
| `node-request` | `#008672` (teal) | Request for a proxy node in a new region |
| `documentation` | `#0075ca` (blue) | Doc fix or improvement |
| `question` | `#d876e3` (pink) | Support question (close once answered) |

### Status labels
| Label | Colour | Meaning |
|---|---|---|
| `triage` | `#ffa500` (orange) | Newly opened, not yet assessed |
| `confirmed` | `#e4e669` (yellow) | Reproduced / validated |
| `in-progress` | `#00ff00` (green) | Actively being worked |
| `blocked` | `#b60205` (dark red) | Waiting on external dep or info |
| `wontfix` | `#ffffff` (white) | Intentionally not addressing |
| `duplicate` | `#cfd3d7` (grey) | Duplicate of another issue |

### Priority labels
| Label | Colour | Meaning |
|---|---|---|
| `P0-critical` | `#b60205` (dark red) | Production outage / data loss / security |
| `P1-high` | `#d73a4a` (red) | Significant UX breakage, next release |
| `P2-medium` | `#ffa500` (orange) | Improvement, next 2 releases |
| `P3-low` | `#e4e669` (yellow) | Nice to have |

### Community labels
| Label | Colour | Meaning |
|---|---|---|
| `good-first-issue` | `#7057ff` (purple) | Suitable for new contributors |
| `help-wanted` | `#008672` (teal) | External help explicitly wanted |
| `community-node` | `#008672` (teal) | Related to self-hosted node operations |

---

## Triage SLA

| Source | Check frequency | Response SLA |
|---|---|---|
| GitHub Issues | Daily | First triage within **24 hours** of open |
| GitHub Discussions | Every 2 days | Response within **48 hours** |
| Discord #bug-reports | Daily | Acknowledge within **24 hours**; redirect to GitHub Issue |
| Discord #feature-requests | Every 2 days | Acknowledge within **48 hours** |
| Reddit thread comments | After each post launch | Monitor for 72 hours post-launch actively; weekly thereafter |

---

## Triage Decision Tree

```
New Issue opened
│
├─ Is it a security vulnerability?
│   └─ YES → Close public issue immediately, email privately, label P0-critical
│
├─ Is it a duplicate?
│   └─ YES → link to original, label `duplicate`, close
│
├─ Is it missing reproduction steps? (for bugs)
│   └─ YES → comment requesting steps, label `triage` + `question`, set 7-day stale
│
├─ Is it a support question?
│   └─ YES → answer (or link to docs), label `question`, close when resolved
│
├─ Is it a game profile request?
│   └─ YES → label `game-profile` + priority (community demand = P2, otherwise P3)
│             Add to v0.4.0 milestone if prioritised
│
├─ Is it a node region request?
│   └─ YES → label `node-request`, comment asking if they'd self-host, link to infra/scripts/
│
└─ It's a bug or feature
    ├─ Assign type label (bug / feature / documentation)
    ├─ Assign priority (P0–P3) based on impact
    ├─ Assign `triage` initially
    └─ Move to `confirmed` once validated
```

---

## Standard Triage Responses

### Bug — missing reproduction steps
```
Thanks for the report! To help us reproduce this, could you share:

1. OS and version (Windows 10/11, Linux distro, macOS)
2. LightSpeed version (`v0.3.0`?)
3. Game and server region you're trying to connect to
4. The exact error or behaviour you're seeing
5. Relevant log output from the client

Logs are printed to stdout — if you're on Windows, run `lightspeed.exe > lightspeed.log 2>&1` to capture them.
```

### Feature / game profile request — acknowledged
```
Thanks! We track game profile priority by community demand — added to the backlog.

If you'd like to help, the game profile format is documented in [docs/protocol.md](../docs/protocol.md). PRs welcome!
```

### Node region request — redirect to self-host
```
Thanks for the region request! The fastest path to getting a node in your region is self-hosting — it takes about 5 minutes with the setup script:

```bash
curl -sL https://raw.githubusercontent.com/ShibbityShwab/lightspeed/master/infra/scripts/setup-new-node.sh | bash
```

Full infra docs are in [infra/README.md](../infra/README.md). If you run a community node, let us know and we'll add it to the official mesh!
```

### Duplicate — closing
```
This looks like a duplicate of #[X]. I'm closing this and tracking the discussion there.
```

---

## Feedback → Development Pipeline

Feedback feeds directly into the WF-001 (Core Protocol) and WF-004 (Game Support) workflow cycles:

```
GitHub Issues / Discord / Reddit
         │
         ▼
    Triage (this doc)
         │
         ├─ Bug → P0/P1 → hotfix branch → patch release
         │
         ├─ Bug → P2/P3 → milestone: v0.4.0
         │
         ├─ Game Profile → demand count → WF-004 backlog
         │
         ├─ Node Region → self-host redirect → community mesh
         │
         └─ Feature → WF-001 backlog (architect review)
```

---

## v0.4.0 Backlog Seed (from WF-006 Step 6 launch)

These items are pre-seeded based on likely demand from the launch announcements:

| Item | Type | Source | Priority |
|---|---|---|---|
| EU proxy node | node-request | Community demand | P2 |
| India proxy node | node-request | Community demand | P2 |
| Valorant game profile | game-profile | High search volume | ✅ Done (v0.4.0-dev) |
| Apex Legends game profile | game-profile | High search volume | ✅ Done (v0.4.0-dev) |
| Windows GUI / tray app | feature | Barrier to adoption | P2 |
| Opt-in latency telemetry | feature | ML model improvement | P2 |
| macOS binary in release | feature | r/rust feedback expected | ✅ Done (release.yml updated) |

---

## Metrics to Track (monthly)

| Metric | Where | Target (3 months post-launch) |
|---|---|---|
| GitHub Issues opened | GitHub Insights | Trending up (health signal) |
| GitHub Issues closed | GitHub Insights | >80% within 30 days |
| GitHub Stars | GitHub | 100+ by 90 days post-launch |
| GitHub Forks | GitHub | 10+ by 90 days |
| Community nodes online | mesh-health.sh output | 1+ third-party node |
| Discord members | Discord Server Insights | 50+ by 90 days |
| Reddit post engagement | Reddit | 100+ upvotes on at least 1 post |
