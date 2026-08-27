# Decision Log

> **Canonical log of significant technical decisions for the LightSpeed project.**
> Each entry includes the date, deciding agent, rationale, and impact.
> Last entry: 2026-08-28

---

## Log Format

```
### YYYY-MM-DD: [Title]

**Agent:** [Agent Name]
**Status:** [Proposed / Accepted / Deprecated / Superseded]
**Rationale:** [Why this decision was made]
**Impact:** [What changes as a result]
**Alternatives Considered:** [What other options were evaluated and why rejected]
```

---

## Entries

### 2026-05-25: Initial Decision Log Created

**Agent:** Architect
**Status:** Accepted
**Rationale:** The WAT autonomy system was incomplete — `wat/rules.md`, `wat/archive/agents.md`, and `wat/state/decisions.md` were referenced by `AGENTS.md` but did not exist. Created all three files to establish the canonical autonomy loop foundation.
**Impact:** AI agents can now follow the full WAT autonomy loop: read state → adopt persona → execute task → verify → update state. All policy stubs (`[COST_STUB]`, `[SAFETY_STUB]`, etc.) are now enforced.
**Alternatives Considered:** Stripping WAT references from AGENTS.md — rejected because the autonomy loop adds value for multi-agent coordination.

---

### 2026-05-25: Protocol Documentation Correction — v2 Header Size

**Agent:** QAEngineer
**Status:** Accepted
**Rationale:** `docs/protocol.md` stated v2 header is "20 + 6 = 26 bytes" but the FEC header in `protocol/src/fec.rs` is `FEC_HEADER_SIZE = 4`, making v2 total 24 bytes. The v1 diagram also labeled byte 1 as "Reserved" instead of "Session Token" (changed in code since v0.3.0). Corrected all values in protocol.md.
**Impact:** Protocol documentation now accurately reflects the wire format. Wire format examples updated to show 4-byte FEC extension (was 6 bytes).
**Alternatives Considered:** Changing FEC header to 6 bytes — rejected because the 4-byte format is already deployed and more efficient.

---

### 2026-05-25: Quinn Upgrade to 0.11.14 — RUSTSEC-2026-0037

**Agent:** SecOps
**Status:** Accepted
**Rationale:** `quinn-proto 0.11.13` has a known DoS advisory (RUSTSEC-2026-0037). `quinn 0.11.14` is a patch release that fixes this. Upgraded via `cargo update -p quinn-proto`.
**Impact:** Resolved the `quinn-proto` DoS advisory. The `quinn` dependency is specified as `"0.11"` so automatic resolution picks up the patch. Advisory entry in `.cargo/audit.toml` can be removed once `cargo-audit` confirms the fix.
**Alternatives Considered:** Upgrading to quinn 0.12 — rejected because it requires code changes to the QUIC control plane (zero test coverage).

---

### 2026-05-25: Clippy Configuration Established

**Agent:** RustDev
**Status:** Accepted
**Rationale:** The project had no `clippy.toml`, relying on tool defaults. Created `.clippy.toml` with `cognitive-complexity-threshold = 30` and `too-many-arguments-threshold = 8` to catch complexity creep early.
**Impact:** Future `cargo clippy` runs will flag methods exceeding these thresholds. Existing code unaffected until thresholds are lowered.
**Alternatives Considered:** More aggressive thresholds (20/6) — rejected to avoid breaking existing code without prior fixes.

---

### 2026-05-25: Decommissioned Infrastructure Cleanup

**Agent:** InfraDev
**Status:** Accepted
**Rationale:** `infra/terraform/` (OCI configs) and `infra/fly/` (never-deployed Fly.io config) are dead code. Moved to `infra/archive/` with a README explaining their historical status.
**Impact:** Reduced confusion about which infrastructure is active. The active deployment uses Vultr (managed via `infra/scripts/` and `infra/docker/`).
**Alternatives Considered:** Deleting outright — rejected because the Terraform configs contain useful reference patterns for future migrations.



### 2026-05-25: CLI and Config Unit Tests Added

**Agent:** QAEngineer
**Status:** Accepted
**Rationale:** `client/src/cli.rs` and `client/src/config.rs` had zero unit tests. Added comprehensive test coverage: 22 CLI tests (default values, all flag combinations, game values, parse_proxy_addr error cases) and 16 config tests (defaults, TOML round-trip, partial config, invalid TOML, file save/load round-trip).
**Impact:** Total test count increased from ~111 to ~137. CLI breakage and silent config bugs are now caught by the test suite.
**Alternatives Considered:** Using clap's built-in test utilities specifically — rejected because clap derive's `try_parse_from` provides more natural API testing.

---

### 2026-05-25: Protocol Documentation Fully Corrected

**Agent:** QAEngineer
**Status:** Accepted
**Rationale:** Completed all protocol doc fixes from the audit. Every reference to v2 header size (26→24 bytes), FEC extension size (6→4 bytes), v1 diagram field name (Reserved→Session Token), and all wire format examples now match the actual implementation in `protocol/src/fec.rs` (FEC_HEADER_SIZE = 4).
**Impact:** Protocol documentation is now a single source of truth. Wire format examples correctly show 4-byte FEC extension at offset 0x14-0x17 with payload starting at 0x18.
**Alternatives Considered:** Reverting FEC header to 6 bytes to match old docs — rejected because 4-byte format is deployed and more efficient.

---

### 2026-06-01: Post-WF-010 Audit Remediation Complete

**Agent:** SysArch + RustDev
**Status:** Accepted
**Rationale:** Performed post-audit remediation of 3 medium-priority and 7 low-priority findings from the WF-010 build-health audit. All fixes compile cleanly and pass tests.
**Impact:**
- **M-5 Threading Safety (`interceptor/traits.rs`):** Added SAFETY comment to `detected_server: std::sync::Mutex` documenting the no-await-point invariant. Migration to `tokio::sync::Mutex` deferred because `snapshot()` must remain callable from synchronous GUI threads.
- **M-6 Firewall Error Handling (`interceptor/windows.rs`, `modes/capture_mode.rs`):** Replaced silent `let _ = ...output()` with `match` error handling in `add_fw_rule()`, `remove_fw_rule()`, and `remove_firewall_rule()`. Failures now emit `tracing::warn!` with stderr context.
- **M-7 FEC Deduplication (`protocol/src/fec.rs` + 5 files):** Extracted `build_fec_data_packet()`, `build_fec_parity_packet()`, and `decode_fec_payload()` into shared helpers in the protocol crate. Eliminated ~120 lines of duplicated packet-building logic across `capture/windivert_redirect.rs`, `tunnel/relay.rs`, `redirect.rs`, `modes/capture_mode.rs`, and `modes/live_test.rs`.
- **L-1 Dependabot:** Added `.github/dependabot.yml` for weekly Cargo and GitHub Actions dependency scans.
- **L-2 CI cargo-audit:** Aligned `ci.yml` with `security.yml` by adding `--locked` to `cargo install cargo-audit`.
- **L-3 Windows GUI CI:** Added `windows-gui` job to `ci.yml` that builds `lightspeed-gui` on `windows-latest` (previously excluded everywhere).
- **L-4 Deprecated `--all` flag:** Removed `--all` from `cargo test --workspace --all --exclude lightspeed-gui` in `ci.yml`.
- **L-5 CHANGELOG ordering:** Moved `## [0.4.1]` section from bottom of `CHANGELOG.md` to the top (newest-first).
- **L-6 PR Template:** Added `.github/PULL_REQUEST_TEMPLATE.md` with type labels, checklist, and testing section.
- **L-7 CODEOWNERS:** Added `.github/CODEOWNERS` with default ownership assignments.
**Alternatives Considered:** Making `InterceptorCounters::snapshot()` async for M-5 — rejected because it would break the GUI thread call site.

### 2026-07-29: Post-Hiatus Environment Validation & WF-011

**Agent:** RustDev + QAEngineer
**Status:** Accepted
**Rationale:** After a 2-month hiatus, the project needed environment validation and progression. `cargo check` revealed that dependabot PR #14 (linfa 0.7→0.8 bump) had broken the ML compilation due to missing `linfa-linear` and `ndarray` version bumps. Additionally, `criterion::black_box` was deprecated in favor of `std::hint::black_box`. WF-011 was defined to add Linux interceptor CLI diagnostic tooling and validate the full toolchain.
**Impact:**
- `client/Cargo.toml`: `linfa-linear` 0.7→0.8, `ndarray` 0.15→0.16
- All bench files: `criterion::black_box` → `std::hint::black_box`
- `client/src/cli.rs`: Added `--intercept` and `--scan-processes` flags
- `client/src/main.rs`: Added dispatch logic for both flags; added `mod interceptor` to binary root
- Live validation: ProcessScanner works, nftables backend available, game config resolution works
- 185 tests, 0 failures; clippy 0 errors
**Alternatives Considered:** Downgrading linfa back to 0.7.1 — rejected because future linfa versions will diverge further; upgrading to match is the sustainable path.

### 2026-07-29: WF-012 — Live Interceptor Mode & Dependabot Triage

**Agent:** RustDev + QAEngineer
**Status:** Accepted
**Rationale:** With WF-011 diagnostic tooling validated, the next logical step was to enable live MITM via the interceptor framework. Additionally, four stale dependabot branches needed triage. All four bumps proved safe and were consolidated into a single commit, superseding the outdated branches (which were based on pre-fix Cargo.toml and would have reverted the linfa fix).
**Impact:**
- `Cargo.toml` + `client/Cargo.toml` + `proxy/Cargo.toml`: bytes 1.12, tracing-subscriber 0.3.23, rand 0.9, thiserror 2
- `client/src/ml/data.rs`: 12× `gen_range` → `random_range` (rand 0.9 API change)
- `client/src/modes/intercept_mode.rs`: New live MITM runner — resolves game, discovers routes, creates/starts interceptor, handles Ctrl+C shutdown
- `client/src/cli.rs`: Added `--start-interceptor` and `--server-addr` flags
- `client/src/main.rs`: Dispatch for `--start-interceptor` with server address override
- Live validation: Full pipeline works (route discovery → interceptor start → nftables attempt) — correctly fails on "Operation not permitted" without root
- 185 tests, 0 failures; clippy 0 errors
**Alternatives Considered:** Using the Engine's `start_interceptor()` method — rejected because the Engine is designed for GUI integration (eframe/Tokio runtime coupling). Direct interceptor usage from the CLI is simpler and avoids the GUI dependency.

### 2026-07-29: WF-013 — MockInterceptor & CI Testability

**Agent:** RustDev + QAEngineer
**Status:** Accepted
**Rationale:** The interceptor pipeline needed automated test coverage. Previously, testing the interceptor required root + kernel modules (nftables) or Windows + Administrator (WinDivert). A MockInterceptor implementing the full `TrafficInterceptor` trait enables CI tests of the interceptor lifecycle without elevated privileges. The mock uses `std::thread::spawn` + `blocking_recv()` to avoid requiring a Tokio runtime in unit tests.
**Impact:**
- `client/src/interceptor/mock.rs`: 118 LoC MockInterceptor with 7 unit tests
- `client/src/interceptor/mod.rs`: 5 new integration tests exercising the full pipeline
- Test count: 185 → 197
**Alternatives Considered:** Using `#[tokio::test]` for mock tests — rejected because it adds a heavy dependency on the Tokio runtime for simple unit tests. Using `std::thread::spawn` with `blocking_recv()` keeps the mock runtime-agnostic.

### 2026-07-29: PR #20 Review — Cross-Platform GUI

**Agent:** RustDev (reviewing)
**Status:** Reviewed — recommended for merge
**Rationale:** CiroBurro's cross-platform GUI refactor is a clean contribution that:
- Extracts OS-specific code behind a `Platform` trait (Linux stub + Windows full)
- Fixes two crashes (hardcoded log path, placeholder IP panic)
- Adds runtime proxy manager UI
- Migrates to egui 0.35 API
Compiles cleanly on Linux (`cargo check -p lightspeed-gui`), merges without conflicts against current master.
**Impact:** Makes `lightspeed-gui` buildable and runnable on Linux (previously Windows-only). The proxy manager removes the need for environment variable restarts.
**Alternatives Considered:** Requesting changes to add persistent proxy storage — deferred to a follow-up PR (contributor explicitly noted "Persistence is intentionally omitted").

### 2026-08-03: Project Cleanup — Harness Consolidation & Provider Genericization

**Agent:** Architect + RustDev
**Status:** Accepted
**Rationale:** The project had accumulated 4 overlapping agentic harnesses (.kilo/, .clineskills/, .clinerules, kilo.jsonc + wat/AGENTS.md) from different eras, plus Vultr/OCI/Fly.io provider sprawl across scripts, CI, and docs. The cleanup consolidates to a single canonical harness (wat/ + AGENTS.md) and makes all hosting documentation provider-agnostic.
**Impact:**
- **Deleted:** `.kilo/` (12 files — duplicate agent definitions), `.clineskills/` (2 files — Cline wrappers), `.clinerules`, `kilo.jsonc`
- **Deleted:** `infra/archive/` (OCI Terraform + Fly.io — dead code, preserved in git history)
- **Deleted:** `infra/docker/Dockerfile.proxy` (duplicate, older Dockerfile)
- **Deleted:** `tools/vultr-mcp/` (stale Vultr-specific MCP server)
- **Deleted:** `tools/e2e_test.js` (duplicate of e2e_test.py)
- **Deleted:** `load-test-results.json` (test artifact, added to .gitignore)
- **Renamed:** `deploy-vultr.sh` → `deploy.sh`, `provision-vultr.sh` → `provision.sh` (all Vultr branding removed)
- **Rewritten:** `infra/README.md` — provider-agnostic, simplified to 3 deployment options
- **Updated:** `deploy.yml` — genericized, references `deploy.sh`
- **Updated:** `docker-compose.yml` — references `Dockerfile` not `Dockerfile.proxy`
- **Updated:** `.gitignore` — cleaned stale entries, added `load-test-results.json`
- **Updated:** `AGENTS.md` — removed Cline/Kilo references
- **Updated:** `wat/archive/agents.md`, `wat/rules.md` — "any Always Free tier provider"
- **Consolidated:** Removed duplicate `security` job from `ci.yml` (already in `security.yml`)
**Alternatives Considered:** Keeping .kilo/ for backward compatibility — rejected because it was never canonical and duplicated wat/. Keeping OCI Terraform — rejected because it's dead code and git history preserves it.

### 2026-08-18: v1.0.0 Release Decisions

**Agent:** RustDev + QAEngineer + InfraDev
**Status:** Accepted
**Rationale:** The v1.0.0 release run consolidated dependency health, installer tooling, documentation, security defaults, and game coverage into a single stable release. The egui stack was bumped to eframe 0.36 / egui_plot 0.37 to stay current, and the version was bumped to 1.0.0.
**Impact:**
- **cargo-dist adoption:** Installer wizard via cargo-dist v0.32.0 replaces hand-rolled packaging, producing platform installers for the release.
- **`require_auth` default unchanged:** Traced the QUIC control-plane / data-plane auth flow; `require_auth` stays `false` by default because the client never stamps the session token into data-plane packets (always sends `0`) and the Docker image builds without the `quic` feature. Full token auth is deferred.
- **Grafana password hardening:** The weak default Grafana password was removed.
- **4 new game profiles:** MapleStory, Genshin, Rocket League, and World of Tanks added to supported-game coverage.
- **Self-hosting guide:** A dedicated guide documents zero-cost self-hosting, replacing the earlier community-proxy-network idea.
- **Deferred to post-1.0:** Issue #39 (configurable ports) and issue #10 (TCP tunnel) are deferred beyond the 1.0.0 release.
**Alternatives Considered:** Hand-rolled installers — rejected in favor of cargo-dist for reproducible, cross-platform packaging. `require_auth = true` default — evaluated and rejected for v1.0.0 because the client does not yet stamp session tokens into data-plane packets (would reject all legitimate clients). Keeping the community-proxy-network idea — rejected in favor of a concrete self-hosting guide. Including configurable ports and the TCP tunnel in 1.0.0 — rejected to keep the release scope focused.

### 2026-08-18: WF-015 — Token Auth & Client Session Stamping

**Agent:** RustDev + QAEngineer + SecOps
**Status:** Accepted
**Rationale:** The v1.0.0 audit revealed the client never stamped the session token into data-plane packets, so `require_auth` could not be safely enabled. WF-015 closes that gap: the client now registers over QUIC and stamps its token into every data-plane header, enabling `require_auth = true` as the secure default.
**Impact:**
- **`client/src/session.rs`:** new process-global atomic session-token holder (defaults to 0).
- **`ControlClient::connect()`:** sets the global token after a successful `RegisterAck`.
- **`register_session()`:** new best-effort registration helper (no-op without the `quic` feature).
- **36 data-plane header sites:** stamped with `.with_session_token(...)` across relay, redirect, engine, interceptors, capture, and modes.
- **Registration wired:** into `main.rs` modes and the GUI engine entry points.
- **`quic` feature:** enabled in the Dockerfile, the cargo-dist client build, and the GUI's client dependency.
- **`require_auth = true`:** now the default in config, shipped tomls, and the provision script.
**Alternatives Considered:** Keeping `require_auth = false` and leaving token stamping as a documented gap — rejected because secure-by-default is the stated goal and the client-side stamping is now complete. Threading the token via `Arc<AtomicU8>` through every struct — rejected in favor of a process-global (a client has one active proxy session, and the global minimizes plumbing across ~15 files).

### 2026-08-18: WF-016/WF-017 — TCP Tunnel & Configurable Ports

**Agent:** RustDev + NetEng + SecOps
**Status:** Accepted
**Rationale:** Issues #10 (TCP tunnel) and #39 (configurable ports) were the last open enhancement requests. The TCP tunnel adds a second transport to the security-critical relay, so the design was validated by an oracle before implementation.
**Impact:**
- **`protocol/framing.rs`:** length-prefixed framing (4-byte BE length + tunnel packet), with a `MAX_FRAME_SIZE` cap rejecting zero/oversized lengths before allocation.
- **Proxy `ClientSender`:** abstracts the response write path (UDP `send_to` vs TCP framed write); a `CancellationToken` drives teardown (send-error alone is insufficient because `Arc` keeps the write half alive).
- **`run_tcp_inbound`:** TCP listener on the data port feeds frames through the same `process_inbound_packet` pipeline — auth, rate-limit, abuse, destination validation, and FEC apply identically.
- **TCP hardening:** connection semaphore (256), read timeout (10 s), `TCP_NODELAY` on both ends.
- **Client `TunnelTransport`:** UDP/TCP enum with `--tcp` flag and `tunnel.transport` config; scope is relay + redirect only (interceptors remain UDP-only).
- **Configurable ports:** `[network]` section (data/control/health) with CLI flags as optional overrides.
- **glib advisory:** documented; blocked on upstream gtk-rs 0.20 (filed tauri-apps/tray-icon#356).
**Alternatives Considered:** A parallel `TcpRelay` type — rejected in favor of a transport enum to avoid duplicating the FEC/header/stats logic. Unifying the TCP read path into `ClientSender` — rejected because UDP uses a batched recvmmsg loop while TCP is accept→frame; only the write path is shared. Extending TCP to the kernel-MITM interceptors — deferred (raw-socket reinjection is UDP-specific).

---

### 2026-08-28: Windows Release WinDivert Fix — v1.2.1

**Agent:** RustDev + QAEngineer
**Status:** Accepted
**Rationale:** The v1.2.0 Windows release shipped without the `windivert-redirect` feature, so the interceptor reported "unsupported" and `WinDivert.dll`/`WinDivert64.sys` were absent (issues #50, #58). Root cause: `[package.metadata.dist] features = ["quic"]` never enabled `windivert-redirect`, and cargo-dist v0.32.0 has no per-target `features` or `include`.
**Impact:**
- **`client/Cargo.toml`:** dist `features` is now `["quic","windivert-redirect"]`. `windivert-redirect` is safe on every target because its `windivert` dependency is `cfg(windows)`-gated (verified: `cargo check --features windivert-redirect` passes on Linux).
- **`client/windivert/`:** vendored official WinDivert 2.2.2 `WinDivert.dll` + signed `WinDivert64.sys` + `LICENSE.windivert`, bundled next to the exe via `[package.metadata.dist] include`.
- **Known limitation:** `include` is package-global, so the two WinDivert binaries also land in Linux/macOS archives (inert, ~200KB).
**Alternatives Considered:** Per-target includes — rejected (not supported in cargo-dist 0.32). Static linking via `windivert-sys` `static` — rejected (LGPL static-link compliance burden). Downloading WinDivert in CI — rejected (vendoring is reproducible and avoids release-time network).
