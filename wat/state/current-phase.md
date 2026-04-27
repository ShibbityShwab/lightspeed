# Current Phase

> **POLICY — READ FIRST:**
> The public launch initiative (Reddit posts, HN, Discord, social media) has been **archived indefinitely** per maintainer request on 2026-04-27.
> The agent MUST NOT proactively suggest launch activities, subreddit posts, Discord setup, or community announcements.
> Reference `docs/archive/` only if the maintainer explicitly asks about it.

---

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-008 Hardening Pass v2 — 🔄 IN PROGRESS |
| **Current Step** | Items L + N + O complete; perf baseline updated; committing + push + redeploy pending |
| **Active Agents** | RustDev, QAEngineer, PerfEngineer |
| **Blocked On** | Nothing |
| **Last Checkpoint** | 2026-04-27T17:56:00+07:00 |
| **Next Action** | `git commit + push` → CI triggers coverage + check jobs → `gh workflow run deploy.yml` to redeploy both nodes |
| **WAT Version** | 0.3.9 |

---

## WF-007 Refinement Backlog

| ID | Item | Effort | Why |
|----|------|--------|-----|
| A | **Refactor `client/src/main.rs`** | Medium | ✅ Done — commit `2b62426` |
| B | **More game profiles** — Overwatch 2, League of Legends, PUBG | Small per game | ✅ Done — commit `077c81f` |
| C | **`proptest`-based property tests** for FEC encoder/decoder | Medium | ✅ Done — commit `33c876f` |
| D | **`cargo bench` benchmark suite** | Medium | ✅ Done — commit `4dabf0d` |
| E | **`cargo audit` + `cargo deny` in CI** | Small | ✅ Done — commit `ba75fcc` |
| F | **macOS CI smoke test** | Small | ✅ Done — commit `17f589e` |
| G | **Opt-in latency telemetry** | Larger | ✅ Done — commit `317e168` |
| H | **Windows GUI / tray app** | Larger | ✅ Done — this session |
| I | **Auto-generate game profile table** in README from `ALL_GAME_KEYS` | Small | ✅ Done — commit `17f589e` |
| J | **Fix `Instant::now()` in `BlockState::new()`** | Small | ✅ Done — commit `ded4e81` |
| K | **`recvmmsg`/`sendmmsg` batched I/O** — relay inbound, Linux only | Medium (~3h) | ✅ Done — commit `408c467` |

---

## Archived Workflows

### WF-006 Progress — ⏸️ ARCHIVED (Steps 5+6 paused indefinitely)

| Step | Name | Status |
|------|------|--------|
| Step 1 | Brand & Positioning | ✅ Complete |
| Step 2 | Landing Page & Docs | ✅ Complete |
| Step 3 | Community Setup | ✅ Complete |
| Step 4 | Beta Release (v0.3.0) | ✅ Complete |
| Step 5 | Launch Announcement | ⏸️ Archived — see `docs/archive/launch-posts-wf006.md` |
| Step 6 | Feedback Loop | ⏸️ Archived — see `docs/archive/discord-setup.md` |

> All WF-006 copy is accurate and ready in `docs/archive/` if the maintainer decides to proceed with a launch at a future date.

---

## Completed Sessions Log

### 2026-04-27 — Tier 3B + production deploy
- `FecHeader::encode_to_array()` — zero-alloc 4-byte FEC header encode ✅
- Zero-alloc relay outbound — single `[u8; 2072]` task-stack buffer replaces 3 BytesMut::with_capacity calls ✅
- `infra/scripts/deploy-vultr.sh` — NODES array populated with production IPs ✅
- Coordinated deploy of v0.4.0-dev to both Vultr nodes (CI run 24981406005, 1m37s) ✅
  - proxy-lax: ✅ healthy, uptime 44s post-deploy
  - relay-sgp: ✅ healthy, uptime 31s post-deploy
- Commits: `82509fa` (3B code), `bd5da23` (deploy script), `e3a7e61` (CHANGELOG)

### 2026-04-27 — WF-008 Hardening Pass v2 (Items L + N + O)
- **Item L:** `FecEncoder` zero-alloc API — `add_packet_inplace`, `emit_parity_to`, `next_block` ✅
  - Eliminates `Bytes::copy_from_slice` per data packet (was K heap allocs per block)
  - Perf: -46% to -85% time reduction across K2–K16, all payload sizes
  - relay.rs hot path updated to three-phase zero-alloc protocol ✅
- **Item N:** `BlockState.received: [Option<Bytes>; 16]` — eliminates `vec![None; k]` per block ✅
  - Decoder cold-path alloc removed; K=4 recovery shows ⚠️ +14-30% over-init regression (cold, negligible)
- **Item O:** CI coverage job — `cargo-llvm-cov`, LCOV artifact, 14-day retention ✅
- Tests: 153 pass, 0 fail ✅
- Build: release 6.29s ✅
- Perf baseline updated with full WF-008 delta section ✅

### 2026-04-27 — Track A perf sprint (item J + docs)
- Remove `Instant::now()` from `BlockState::new()` — watermark GC instead ✅
- FEC encoder K2 benchmark back to v0.3.x baseline (-55% regression eliminated) ✅
- Commits: `ded4e81`, `1a0526b`

### 2026-04-27 — Tier 1 perf sprint + Tier 2 lock-in
- `encode_to_array()` — zero-alloc header encode, 8 hot-path sites updated ✅
- Compact FEC parity — `max_payload_len + 2` bytes instead of fixed 1400 B ✅
- FEC decoder ring buffer — 64-slot Vec replacing HashMap ✅
- Protocol spec + CHANGELOG + perf baseline updated ✅
- Commits: `6d065ff`, `4a72cbd`

### 2026-04-27 — WF-007 sprint: 9-game support + macOS CI + README table
- Overwatch 2, League of Legends, PUBG game profiles added (game_id 7/8/9) ✅
- README Supported Games table expanded 6→9 rows; CLI flag + auto-detect columns added ✅
- macOS CI smoke test job added to `.github/workflows/ci.yml` ✅
- CHANGELOG updated with full sprint detail ✅
- Commits: `077c81f` (profiles), `17f589e` (README + CI + CHANGELOG)

### 2026-04-27 — v0.4.0-dev game profile sprint
- Valorant, Apex Legends, Rust game profiles added ✅
- Launch initiative archived ✅
- Commit: `3bb1b33`

---

## Live Infrastructure (2-Node Vultr Mesh)

> Both nodes run the **same `lightspeed-proxy` binary**. "Topology Role" is an informational
> label only — it describes how the node is used in the mesh, not a separate product or binary.

| Node | IP | Region | Latency (BKK) | Code | Topology Role |
|------|----|--------|----------------|------|---------------|
| **proxy-lax** | 149.28.84.139 | us-west-lax | 206ms | v0.4.0 | Primary US proxy |
| **proxy-sgp** | 149.28.144.74 | asia-sgp | 31ms | v0.4.0 | FEC multipath path / SEA coverage |

| Resource | Value |
|----------|-------|
| **Health URLs** | :8080/health on all nodes |
| **Metrics URLs** | :8080/metrics on all nodes (Prometheus format) |
| **Data Port** | UDP 4434 |
| **Control Port** | UDP 4433 (QUIC disabled) |
| **Landing Page** | https://shibbityshwab.github.io/lightspeed/ |
| **Monitoring** | Prometheus + Grafana (docker compose) |
| **Deployment** | Native binary + systemd (no Docker) — auto-deployed via GitHub Actions |
| **Provider** | Vultr ($300 credit, 60+ months free) |
