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
- **`client/windivert/`:** vendored official WinDivert 2.2.2 `WinDivert.dll` + signed `WinDivert64.sys` + `LICENSE.windivert`.
- **ZIP/tar:** bundled next to the exe via `[package.metadata.dist] include` (package-global, so they also land in Linux/macOS archives — inert, ~200KB).
- **MSI:** cargo-dist's `include` does NOT populate MSIs, so `client/wix/main.wxs` was hand-edited to add WinDivert `Component`/`File` entries, with `allow-dirty = ["msi"]` at the workspace level (`dist-workspace.toml` — package-level is ignored). The wxs `Source` resolves relative to the process CWD (repo root), so paths are `client\windivert\WinDivert.dll` — a `..\windivert\` path fails in `light` (LGHT0103/exit 103).
**Alternatives Considered:** Per-target includes — rejected (not supported in cargo-dist 0.32). Static linking via `windivert-sys` `static` — rejected (LGPL static-link compliance burden). Downloading WinDivert in CI — rejected (vendoring is reproducible and avoids release-time network).

---

### 2026-08-29: v1.2.3 — Auth Regression Fix + WinDivert Handle Close

**Agent:** RustDev + QAEngineer + NetEng
**Status:** Accepted
**Rationale:** Post-1.2.2 housekeeping triaged GitHub feedback and found two linked Windows bugs (issue #59) plus an install-confusion complaint (#50, #58).

**Bug 1 (critical): data-plane auth rejected 100% of packets.** The proxy revokes data-plane authorization when the QUIC control connection closes (`proxy/src/control.rs` `handle_connection` → `auth.revoke`). The client's `register_session()` (`client/src/quic/mod.rs`) created a throwaway `ControlClient` that dropped immediately after registration, closing the connection and revoking the token before any game traffic flowed. Result: `auth_rejections` spiked while `sessions_created` stayed 0. Fix: `register_session` now moves the `ControlClient` into a background keepalive task (ping every 15s) so the connection — and therefore the data-plane authorization — lives for the process lifetime.

**Bug 2 (Windows): WinDivert `FWP_E_IN_USE`.** The `windivert` crate 0.6 has no `Drop` impl, and LightSpeed never called `WinDivert::close()`, so the WFP filter/callout was never unregistered on shutdown — leaking state that caused `FWP_E_IN_USE` (0x8032000A) on subsequent runs. Fix: `client/src/interceptor/windows.rs` now explicitly closes the capture and inject handles on thread exit and on the inject-open failure path (`CloseAction::Nothing`). The remaining hard-kill case is an upstream WinDivert 2.2.x driver limitation (basil00/WinDivert#196, #294) — documented in `docs/troubleshooting.md`.

**Impact:**
- `client/src/quic/mod.rs`: keepalive task holds the control connection open.
- `client/src/interceptor/windows.rs`: explicit `WinDivert::close()` at 3 points.
- `client/src/games/deadbydaylight.rs` (new): DBD profile, wired into `games/mod.rs` (module, `detect_game`, `auto_detect`, drift-guard test).
- `README.md`, `docs/user-guide.md`, `docs/supported-games.md`, `docs/troubleshooting.md`: "which file do I download" clarification + WinDivert troubleshooting.
- Version bumped 1.2.2 → 1.2.3.
**Alternatives Considered:** Making the proxy keep authorization after connection close (TTL-based re-auth) — rejected because revoke-on-disconnect is correct security semantics once the client holds the connection. Adding a `Drop` impl to the `windivert` crate upstream — rejected (external dep); explicit `close()` is the pragmatic fix.

---

### 2026-09-13: Sponsor-Funded Community Relay Network — COST_STUB Superseded for Relays

**Agent:** Architect + InfraDev
**Status:** Accepted (supersedes `[COST_STUB]` for relay infrastructure only)
**Rationale:** A sponsor now funds the Vultr relay fleet, so the `[COST_STUB]` "$0 forever" mandate no longer applies to relay hosting specifically. LightSpeed launches a 5-relay global network (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo) that is community-discoverable via a signed static registry hosted on GitHub Pages (`https://shibbityshwab.github.io/lightspeed/registry.json`). The operator Ed25519 public key and registry URL are compiled into the client, so a fresh install with zero configuration auto-discovers and probes the community relays and selects the fastest path. The client and proxy software remain free/open-source, and the self-hosted model remains fully supported; only the relay hosting cost is sponsor-covered.
**Impact:**
- `client/src/registry.rs`: `DEFAULT_REGISTRY_URL` and `DEFAULT_OPERATOR_PUBKEY_B64` constants; `resolve_proxy_addr` falls back to registry discovery when no explicit proxy or configured servers are present.
- `web/registry.json`: signed 5-node registry (Schema v1, Ed25519) served from GitHub Pages; operator private key lives at `~/.config/lightspeed/operator-key.pem` (PKCS8), never committed.
- Two new relays provisioned on Vultr (Frankfurt `fra` + Tokyo `nrt`), mirroring the existing LA/NY/Singapore config (token auth on, destination allowlist per community policy).
- Registry hosting is a static signed file (no Cloudflare Worker), keeping infrastructure cost at $0 and avoiding a new account dependency; the Worker in `infra/registry/` remains a reference for future dynamic self-registration.
- The sponsor is not named in any public copy (release notes or website): the network is described as "community-hosted / sponsor-funded".
**Alternatives Considered:** Cloudflare Worker registry (dynamic registration/revocation) — rejected for launch because it requires a Cloudflare account and wrangler credentials not available here, and the static signed file achieves client-side discovery at $0. Continuing to mandate $0 total cost including relays — rejected because the sponsor explicitly funds the fleet. Naming the sponsor publicly — rejected at the user's direction (no donor attribution).

---

### 2026-09-16: v1.4.2 Community Feedback Release

**Agent:** RustDev + NetEng + QAEngineer + DevOps
**Status:** Accepted
**Rationale:** After v1.4.1 the community reported that the Windows GUI was unusable
(issue #59) and that only two relays appeared (discussion #69). Live metrics showed
zero packets relayed across the whole fleet, so the problem was client-side: the GUI
never performed registry discovery, and the Fortnite interceptor locked onto a dead
lobby server. One release addresses every actionable report rather than dribbling out
patches.

**Key decisions:**
- Fortnite server rotation is fixed with a pure `ServerTracker`
  (`client/src/interceptor/order.rs`) that both WinDivert paths share. A pre-seeded
  server anchors the stale timer at seed time so it expires instead of pinning the
  session to a dead address. `dynamic_server()` games skip the seed entirely.
- Transient WinDivert `recv` errors are retried with bounded backoff
  (`client/src/interceptor/recovery.rs`) instead of destroying the handle and tunnel;
  the final error is surfaced through `InterceptorStats::last_error`.
- The GUI discovers relays on a background thread via `registry::discover_relays()`
  and replaces the loopback placeholders. Registration state and relay `/health`
  counters are now visible.
- A second GUI instance shows an "already running" notice and exits rather than
  stacking processes; tray Quit now actually terminates.
- TCP-only games are explicitly out of scope (issue #66): LightSpeed accelerates UDP
  only. The limitation is documented rather than faked with a profile.
- Requested game profiles (Battlefield 6, Warface, Delta Force) are not added without
  citable port and anti-cheat facts.
- The Windows CLI now ships as a zip (unsupported); the GUI stays the Windows path.
- The rustls ring provider is pinned in the GUI: the tree enables both ring and
  aws-lc-rs, which made automatic provider selection panic on the first QUIC call.

**Impact:**
- `client/src/interceptor/{order,recovery,mod,traits,windows}.rs`,
  `client/src/capture/windivert_redirect.rs`, `client/src/games/{mod,fortnite}.rs`
- `client/src/registry.rs` (`discover_relays`/`RelayInfo`), `client/src/main.rs`
  (single-pass probe plus stdout report), `client/Cargo.toml` (Windows target),
  `client/lightspeed.example.toml`
- `client-gui/**` (discovery, config, single instance, diagnostics, game list)
- `docs/**` (install guides, TCP-only guidance, troubleshooting), `web/**`,
  `infra/scripts/network-stats.sh`
- Version bumped 1.4.1 to 1.4.2.
**Alternatives Considered:** Splitting into several patch releases rejected in favour of
one auditable release. Adding TCP game interception rejected for v1.4.2 because the
interceptor and tunnel data plane are UDP-only end to end. Adding unverified game
profiles rejected because invented ports and anti-cheat claims cause user-visible
breakage.

---

### 2026-09-16: Linux Interception Repair and Server Rotation (v1.4.3)

**Agent:** RustDev + NetEng + QAEngineer
**Status:** Accepted
**Rationale:** An Oracle-assisted audit, with kernel behavior verified on the build host,
showed Linux interception was effectively non-functional. Three independent defects
compounded: (1) `ss -unp` on iproute2 7.x no longer prints a State column, so the socket
parser skipped every line, returned an empty list, and never reached the `/proc/net/udp`
fallback, meaning no game server was ever discovered; (2) the destination-less fallback
installed a redirect rule matching `0.0.0.0`/a port range and then tunneled to the
post-NAT destination (`127.0.0.1`), which the relay rejects as private, silently dropping
matched traffic; (3) the receive thread used a nonblocking socket and continued on EAGAIN,
spinning a CPU core while idle. A fourth issue blocked rotation: deleting a redirect rule
does not unhook an established flow, because the conntrack NAT mapping keeps redirecting
it to the listener for 30 to 120 seconds, so a naive rule swap would misroute the old
flow to the new server.

**Key decisions:**
- Reply injection works only from the socket bound to the redirect target port (conntrack
  reverse-NAT), so a session keeps one stable listener port and always injects from it.
- Rotation uses a conservative gate rather than a naive swap: move the exact-IP rule to a
  new server only when the scanner reports it, the old server is absent from the routes,
  and the old flow has been silent, all across two consecutive scans. This avoids the
  misroute window without the complexity (and response-correlation requirements) of a
  full per-generation handoff, which is deferred.
- No rule is installed while no real server is known. Traffic flows normally instead of
  being blackholed. `last_error` is reserved for genuine failures.
- Reliability fixes ship in the same release because rotation is inert without the
  scanner fix, and the busy-spin is a real, user-visible defect.

**Impact:**
- `client/src/interceptor/linux.rs` (waiting state, stable port, scanner poll, swap gate,
  poll(2), teardown, removed dead `recover_original_dst`)
- `client/src/interceptor/rotation.rs` (new pure, cross-platform decision logic)
- `client/src/interceptor/process_scanner.rs` (`ss -unp -a`, State-agnostic parser,
  `/proc` fallback)
- `client/src/modes/smoke_test.rs` (synthetic end-to-end test)
- `client/src/games/mod.rs` (`process_names_for_name`)
- `client/Cargo.lock` and `Cargo.toml` (rustls 0.23.45 for RUSTSEC-2026-0285)
- Version bumped 1.4.2 to 1.4.3.
**Alternatives Considered:** TPROXY is invalid for locally generated traffic (mangle
PREROUTING only); the `MARK` plus policy-routing workaround mutates host routing and can
blackhole all traffic if cleanup fails. NFQUEUE would give true per-packet destination
and pass-through but needs a netlink dependency and fail-open semantics. `SO_ORIGINAL_DST`
returns ENOPROTOOPT for UDP on this kernel. conntrack is a materially better route source
than `ss` for unconnected sockets and is the recommended v1.4.4 addition, but it is not
required once the scanner loop works. Reusing the Windows `Decision::PassThrough` was
rejected outright: on Linux the kernel has already consumed the datagram, so ignoring it
means silent packet loss.

### 2026-09-19: Observability overhaul (honest metrics, latency, telemetry, history)

**Agent:** Sisyphus (orchestrated; Oracle design review, explore/plan agents)
**Status:** Accepted (branch `feat/observability-overhaul`)
**Rationale:** Live production data showed the proxy's observability was partly hollow or
misleading: `record_latency` had no caller so the latency histogram was always empty;
`fec_data_packets_total` was never incremented; `packets_dropped` was almost entirely
unauthenticated scanning and abuse blocks, not loss, yet the public site labelled it
"Packets Dropped"; the rate limiter keyed on IP+source port so rotating ports never
tripped it (`rate_limit_hits_total == 0` in production); opt-in telemetry sent a
hardcoded `game_id = 0` and empty country and the proxy discarded the payload; and the
once-per-six-hours stats snapshot overwrote itself so no trend history existed.
**Key decisions:**
- Latency measures proxy-observed upstream response lag (monotonic send stamp before the
  forward, single `swap(0)` on the first response) and is documented as NOT client RTT;
  samples outside `(0, 2s]` are discarded and counted. The histogram stores per-bucket
  deltas and `to_prometheus` renders the cumulative series (the previous code accumulated
  twice).
- `packets_dropped` stays the sum of all reasons; a `DropReason` enum adds category
  counters (malformed, fec_malformed, session_setup, relay_send_errors) alongside the
  existing auth/abuse/rate-limit counters. `/health` and the stats script expose all
  seven, and the website now says "Packets Filtered" plus an "Upstream Loss" tile.
- The rate limiter is two-tier: the existing per-flow limiter plus a per-IP aggregate
  (5000 pps / 5 MB/s, 65536-IP cap, fail closed). Both windows are debited only when both
  allow. The per-IP subset counter is a subset, not a second bump of the combined counter.
- Telemetry is aggregated in a bounded `Mutex<HashMap<(game_id, country), cell>>` (1024
  cells, k>=3 emission floor). Percentiles are summed as mean-of-medians with that caveat
  in the HELP text; game/country are aggregation labels, never PII.
- History lives on an orphan `stats` branch: the Pages workflow generates the snapshot,
  appends a reset-safe bounded (360) history, commits it, and deploys the artifact. The
  reset rule treats any counter decrease as a relay restart so cumulative totals never
  regress.
- Two correctness bugs were fixed first because they corrupted the session metrics:
  response listeners were spawned twice per session, and `last_activity` was never
  refreshed so sessions expired 300s after creation regardless of traffic.
**Impact:** `proxy/src/{metrics,relay,health,rate_limit,config}.rs`,
`protocol/src/control.rs`, `client/src/{telemetry,main,modes/keepalive}.rs`,
`client/src/games/mod.rs`, `infra/scripts/{network-stats,append-history,test_append_history}.sh`,
`.github/workflows/pages.yml`, `web/{index.html,app.js}`, `.gitignore`,
`client/Cargo.toml` (sys-locale), plus new `proxy/tests/integration_latency.rs`.
**Alternatives Considered:** Client-RTT echo via the tunnel header timestamp was rejected
(duplicates the keepalive measurement and needs protocol semantics changes); true
percentile aggregation from per-client percentiles is statistically invalid, so only sums
and counts are exposed; committing generated stats to `master` or an actions cache was
rejected for repo noise and eviction risk respectively, so an orphan branch holds history.

## 2026-09-19 — Relay self-update with in-place handoff (WF-025)

**Context:** relays must update themselves without forcing clients to reconnect. Building
this exposed a prerequisite failure: the proxy revoked data-plane auth when the QUIC
control connection closed, and the client's keepalive task never reconnected, so any
relay restart permanently deauthorized every connected client until the process restarted.

**Delivered (atomic commits, in order):** client supervised reconnect (races connection
close vs the 15s ping, jittered backoff 250ms..=5s, re-registers, never zeroes a valid
token) and per-relay token store; proxy auth rekeyed by token with IP(+optional data port)
binding, 300s TTL refreshed on control keepalive, 120s transport revive grace, 30s
previous-token window (demotion only for a matching principal AND data port), 5s sweep,
fail-closed cap; per-path telemetry (client collects RTT/jitter/loss/FEC/dedup per relay,
protocol carries `route_legs`, proxy aggregates by relay/game/country); data tooling
(`lib-nodes.sh` reads the signed registry, `collect-metrics.sh` bounded reset-safe
history, `analyze-mesh.sh` anomaly flags, web history trends); versioned release layout
`/opt/lightspeed/releases/<v>` + `current` symlink with a reusable installer (staged
binary checked before publish, health gate, rollback, prune keeping active+previous);
systemd `Type=notify` + 30s watchdog (`sd_notify` implemented directly, no new dep);
a verified self-updater (GitHub release, SHA-256 checked, flock, `update-state.json`) on
an hourly jittered timer; `/health` update state; and Phase 2 in-place `execve` handoff.

**Key decisions:**
- Auth is token-keyed with an IP(+optional port) binding; re-registration demotes only a
  same-principal, same-port token, so two clients behind one NAT never truncate each other.
- Handoff is an in-place `execve` (NOT `SO_REUSEPORT`/eBPF): the old process clears
  `FD_CLOEXEC` on the data listener and every per-session outbound UDP fd, writes a
  root-owned manifest to `/run/lightspeed/handoff.json`, pre-validates the target in a
  child that inherits those fds, announces control shutdown, then execs. The new binary
  adopts the fds, rebuilds sessions with FRESH FEC decoders (a reset costs at most one
  unrecovered packet per straddling block; stale parity cannot cross-contaminate because
  block ids are checked), restores auth with remaining TTLs, re-anchors `Instant` fields
  from stored ages, and binds control/health fresh. Global metrics, abuse/rate state, and
  TCP sessions are intentionally not transferred; `pending_forward_us` is zeroed.
- Failure is safe by construction: anything short of a clean pre-validation restores the
  fd flags and keeps the old process serving; the installer commits the `current` symlink
  only after `/health` reports the new version with a matching ok handoff id plus a soak,
  and otherwise leaves the old process running (zero downtime) or rolls back to the
  previous release if the new one goes unhealthy. Adoption requires the env var, so a
  plain systemd restart cannot adopt. The first Phase-2 rollout is necessarily a blunt
  restart because the running binary predates handoff support.
**Impact:** `proxy/src/{main,relay,auth,health,handoff,notify,metrics}.rs`,
`client/src/{quic,session,telemetry,tunnel,engine}.rs`, `protocol/src/{control,telemetry}.rs`,
`infra/scripts/{relay-install,relay-updater,collect-metrics,analyze-mesh,lib-nodes,test_handoff_e2e}.sh`,
`infra/systemd/*`, `.github/workflows/pages.yml`, `web/*`.
**Alternatives Considered:** `SO_REUSEPORT` coexistence was rejected because plain
reuseport rehashes existing 4-tuples on membership change and the per-process in-memory
auth/control state splits across processes (a client's control plane and data plane can
land on different binaries); eBPF `SK_REUSEPORT` steering fixes the split but adds
kernel/root/build complexity and murky drain semantics. A stable front-supervisor was
rejected for an extra userspace hop, a recursive update problem, and the
response-source-port constraint. Serializing the FEC decoder was rejected as unnecessary
after analysis showed a safe reset. **Verification:** a committed root-only E2E
(`test_handoff_e2e.sh`) proves the same PID switching versions with a live session and the
game server observing the same outbound source port across the exec.

## 2026-09-19 — Fleet rollout to 1.5.0 (one-time restart)

**Context:** the user approved rolling every relay to the new build and accepting one blunt
restart per relay, after which updates are seamless handoffs.

**What was done:** bumped the workspace version to 1.5.0 (which also stops the self-updater
from comparing an unreleased build against the published v1.4.4). Added
`infra/scripts/migrate-to-versioned.sh`, a one-time, idempotent migration for relays still
running an in-place `/usr/local/bin` binary under a `Type=simple` unit: it seeds the running
binary as a `<version>-legacy` release, points `current` at it so rollback has a target, and
installs the canonical `Type=notify` unit (which adds `RuntimeDirectory=lightspeed`, needed
for the handoff manifest/result/request files). It deliberately does not restart;
`relay-install.sh` then performs the single health-gated restart. Rolled ewr first (lowest
traffic), verified, then lax, sgp, fra, and nrt.

**Result:** all five relays run 1.5.0, healthy, `Type=notify` with a 30s watchdog,
`handoff.supported=true`, `current -> /opt/lightspeed/releases/1.5.0`, and the previous
binary retained (`1.3.2-legacy`, plus nrt's `1.4.4-canary`). The new drop categories are
already counting (sgp recorded auth-rejected drops). Every install was health-gated with
automatic rollback available. Future releases are applied by the hourly self-updater through
the in-place handoff, so no client reconnect is forced. **Note:** this first rollout was a
blunt restart, so clients running a pre-reconnect client build may have needed a restart;
the client shipped on this branch reconnects within seconds.

## 2026-09-19 - Demand-driven relay placement recommender

**Context:** The WF-025 observability overhaul added country tags, but player-country telemetry is opt-in via the CLI `--telemetry` flag only and the GUI has no toggle, so production relays expose zero game/country series. The owner asked to optimize relay placement by where players actually play, choosing a player-region to game-server-region demand matrix with no client telemetry change and no raw IP storage.

**Delivered (planned, on branch `feat/relay-placement-recommender`):** proxy-side transient country derivation at session creation from a locally stored DB-IP Lite MMDB (maxminddb crate), aggregated to `(src_country, dst_country)` counters with a k>=3 export floor and 1024-cell cap, exposed as `lightspeed_geo_*` metrics; the collector coarsens to region pairs and persists them into the `stats` branch history; a new `recommend-regions.sh` scores candidate hosting regions by demand-weighted coverage and redundancy and emits an ADD/MOVE recommendation with an INSUFFICIENT_DATA floor.

**Key decisions:**
- Geolocate proxy-side (full coverage, no opt-in bias) rather than client-reported or SSH journal scraping; aggregate only, never store or export raw IPs.
- Coarsen to region pairs in the public history while country pairs stay transient on `/metrics`.
- Publish the MMDB as a pinned monthly `geoip-<YYYY-MM>` release asset.
- Self-tunnel sessions (destination equals the relay's own control/data port) are filtered proxy-side because the aggregate discards destination IPs.

**Alternatives Considered:** offline operator SSH script (rejected: fragile journald parsing, not CI-runnable); client locale country only (rejected: opt-in, empty, self-declared); scoring by "relay path beats direct path" with great-circle distance (rejected: haversine obeys the triangle inequality, so a distance-only model can never show an improvement; real benefit is transit quality and last-mile).

**Impact:** files under `proxy/src/`, `infra/geo/`, `infra/scripts/`, `.github/workflows/geoip-release.yml`, `docs/privacy.md`, `docs/faq.md`, `NOTICE`. No infrastructure spend; no client or protocol change.

## 2026-09-19 - Deploy-path hardening after the 1.6.0 rollout

**Context:** Shipping the geo placement work to the fleet exposed three latent deploy issues: the in-place handoff rejected every attempt because `/run/lightspeed/handoff-request.json` was root-owned `0600` while the proxy runs under `DynamicUser`; `deploy.sh` no-ops when the release version is unchanged, so proxy changes need a version bump to reach relays; and `relay-updater.sh` is never shipped by the deploy pipeline, so updater changes stayed at their provisioning revision.

**Key decisions:**
- `relay-install.sh` gained `--public-ip` (idempotent `[server] public_ip` edit, validated as IPv4) and `--updater` (install the self-updater to `LIGHTSPEED_UPDATER_DEST`), both applied before activation so the new process reads the corrected config on its first start.
- `deploy.sh` passes each node's registry IP as `--public-ip` and ships `infra/scripts/relay-updater.sh` as `--updater`.
- `deploy.yml` additionally triggers on `relay-install.sh`, `relay-updater.sh`, and `lib-nodes.sh` changes.
- The handoff request is chowned to the runtime directory owner after publishing, so the proxy can read it.

**Impact:** `infra/scripts/{relay-install,deploy,test_relay_install}.sh`, `.github/workflows/deploy.yml`, `CHANGELOG.md`, `Cargo.toml`/`Cargo.lock` (1.6.1).

**Alternatives Considered:** a blunt `systemctl restart` fallback for the handoff failure was rejected in favour of fixing the permission mismatch, which also unblocks every future in-place update.

## 2026-09-20 - GUI relay auto-select

**Context:** GUI users landed on the first discovered relay (index 0) on first run, so they did not benefit from later-added relays and could be pinned to a far one.

**Key decisions:**
- The GUI races every discovered relay's `/health` endpoint concurrently, choosing the lowest round trip, and re-races on each discovery while disconnected. `discovery::health_endpoint()` derives port 8080 from the registry advert (which is the 4434 UDP data port) so probes hit the right port.
- Auto-select is on by default and persisted as `auto_select`; picking a relay manually clears it, and a race result is ignored once the user is connected or has pinned a relay (`should_apply_race`).
- Exposed as an "Auto (fastest)" checkbox in the Boost Server row.

**Impact:** `client-gui/src/{app,config,discovery}.rs`; released as 1.6.2.

**Alternatives Considered:** connecting to index 0 immediately and switching later was rejected because it drops a live connection; racing first only delays the initial connect by the probe time.

## 2026-09-20 - Postmortem: QUIC control plane outage (fleet)

**Impact:** Every relay reported healthy but relayed nothing for roughly 12 hours: `packets_relayed=0`, `sessions_created=0`, `auth_rejections` climbing into the tens of thousands. Clients could not register.

**Root causes:**
1. `infra/scripts/deploy.sh` built the proxy without `--features quic` (proxy `default` features are empty), so the installed binary had no control plane. The deploy health gate only checks `/health`, which passes either way.
2. The quic-enabled build then panicked at startup on rustls 0.23.45: quinn pulls rustls with `aws-lc-rs` while the proxy selects `ring`, so two providers are compiled in and `ServerConfig::builder()` panics on ambiguous auto-detection.

**Fixes (v1.6.3):**
- `proxy/src/control.rs` installs the ring `CryptoProvider` explicitly before building the server config.
- `deploy.sh` builds with `--features quic`, refuses to deploy unless the binary contains the `QUIC control plane listening` string, and asserts UDP 4433 is listening after install.

**Verification:** all five relays on 1.6.3 with 4433 and 4434 listening; a real QUIC client (`--live-test --features quic`) registered with relay-sgp-1 and relayed 5/5 with payload match (`sessions_created=1`, `packets_relayed=10`); the live site shows the traffic.

**Lessons:** a health check that omits the control plane is not a health check; build pipelines must pin required features explicitly; the client CLI needs `--features quic` or it silently uses a stub.
