# Decision Log

> **Canonical log of significant technical decisions for the LightSpeed project.**
> Each entry includes the date, deciding agent, rationale, and impact.
> Last entry: 2026-10-05

---

### 2026-10-05: A structural check is not a look; the localized pages shipped unstyled and malformed

**Agent:** RustDev + QAEngineer
**Status:** Accepted (implemented in `scripts/generate-web-locales.sh` and `web/app.js`)
**Rationale:** Two defects reached production in `cffad91` that every automated
check had passed. (1) The generated pages sit at `web/<tag>/` but referenced
`styles.css`, `assets/...` and `app.js` relatively, so from `/lightspeed/de/` they
resolved under `/de/` and 404'd - `/de/styles.css` returned 404 while
`/styles.css` returned 200 - leaving all eight translated pages unstyled with no
icon and no script, while the English root page looked correct. The same relative
resolution broke the script's own `fetch('network-stats.json')`. (2) Element units
nest (`<td><strong>$0/forever</strong></td>` is a unit for the cell and one for the
strong), and `render()` applied both; the outer span's offsets are stale once the
inner replacement changes the length inside it, so the outer replacement consumed
part of the inner closing tag - the shipped Spanish page read
`$0/para siemprerong>`. Both were invisible to assertions on `lang`, `hreflang`,
heading counts and byte-diffs, and both were obvious within seconds of rendering
the page.
**Impact:** (a) `localize_urls()` prefixes every relative URL with `../` on a
locale page, and `app.js` resolves its data files against the script's own URL so
it is correct at any page depth. (b) Overlapping units are resolved before
rendering: an enclosing unit whose text is identical to the unit inside it stands
down (the inner one renders the same words and keeps the markup), otherwise the
enclosing one wins because it carries the whole phrase. (c) Two guards now fail
the build on the artifact itself - `stray_text_gt()` for a tag fragment left in
text, `unrooted_urls()` for a relative URL that would resolve under `/<tag>/` -
and they were confirmed to fire on the old output before the fix.
**Alternatives Considered:** a `<base href="../">` element would have fixed the
HTML and the script's fetches in one line, but it changes resolution for every
relative URL and fragment on the page, and these pages already carry absolute
canonical and `hreflang` URLs; explicit prefixing keeps the generated markup
readable and independently checkable. Dropping the enclosing unit in every nested
pair was rejected because a paragraph whose inner text is only a fragment would
keep its English remainder (which is the mixed-language state the old output was
actually in).
**Verification:** rendered in a real engine (Bun.WebView) against a local static
server: `de`, `ru` and the mobile breakpoint all load with the stylesheet applied,
the switcher present, translations in place and live stats populated (62 ms), and
the mobile switcher reachable after opening the hamburger menu. Guards fire on
the previous output (10-26 strays per page) and pass on the new one; the extractor
and generator `--check` runs both exit 0.
**Left unfixed, deliberately:** a block containing an inline link or `<code>` is
skipped as a unit, so ~27 of 174 Spanish text blocks remain English while their
bold fragments are translated. A correct fix needs placeholder-based extraction
and a retranslation of all eight catalogs - a separate wave, not a patch.

---

### 2026-10-05: The English page gets its locale links at deploy time, never in the source

**Agent:** RustDev + QAEngineer
**Status:** Accepted (implemented in `scripts/generate-web-locales.sh` and `.github/workflows/pages.yml`)
**Rationale:** The i18n wave shipped eight translated pages, each with an
`hreflang` alternate set and a language switcher, while `web/index.html` - the
page every visitor and crawler actually lands on - had neither (0 `hreflang`, 0
`lang-switch`), and the generator's `web/locales/alternates.en.html` was written
with no consumer. A reader arriving at the front door therefore could not reach
any translation, and a crawler saw a single-language site. The blocker was that
`web/index.html` is the extractor's source of truth: its element text is what
`en.json`'s 442 positional keys index, so injecting markup into it (the bug wave
3 already hit once) shifts every key on the next extraction.
**Impact:** the English build is generated as a separate artifact,
`web/index.en.html`, from the untouched source; `--check` fails when it is
missing or stale, alongside the eight locale pages; and the Pages workflow
promotes it over `web/index.html` inside the deploy artifact just before upload,
failing closed with an explicit error if it is absent. The tracked source page
keeps the exact text the keys point at, and the published page gains 9
`hreflang` links plus the same no-JavaScript switcher the other locales carry.
**Alternatives Considered:** editing `web/index.html` in place was rejected as
the known key-shifting bug; having the workflow run the generator over the
checked-out source at deploy time was rejected because the site is deliberately
build-less (`web/**` is published verbatim) and the deployed bytes should be the
reviewed bytes; a client-side JS redirect or `Accept-Language` sniff was rejected
because it breaks bookmarking and static caching and cannot serve a crawler.
**Verification:** the output has zero missing element texts and exactly the ten
added switcher strings versus the source; `generate-web-locales.sh --check` and
`extract-web-strings.sh --check` both rc=0; the deploy step was simulated in a
scratch copy of `web/` and the promoted page carries 9 `hreflang` and the
switcher; `cargo fmt --all --check` rc=0 and `cargo test -p lightspeed-gui` 99
passed the same session. **Not** verified: no browser render of the served page,
and nothing has been pushed, so the live site is unchanged.

---

### 2026-10-04: i18n is one catalog format with a fallback, not a translated string literal

**Agent:** RustDev + QAEngineer
**Status:** Accepted (wave 1 of 4 implemented: `client-gui`)
**Rationale:** The GUI carried roughly 170 string literals in `app.rs` alone,
but they are not one population: most are egui widget id salts (`"game_select"`),
`tracing::warn!` log lines, font family names (`"semibold"`), test fixtures and
colour/family names. Translating a widget id silently breaks `ComboBox` state
and translating a log line breaks log-grepping, so a whole-file find-and-replace
would have *looked* complete while corrupting both. The extraction was therefore
driven from the call sites - literals passed to `ui.label`, `ui.button`,
`section_label`, `sheet_title`, `hint_text`, `on_hover_text`, `selectable_label`
and `format!` copy - which is 31 direct call sites plus 9 interpolated strings.
**Impact:**
1. Catalogs are TOML under `client-gui/locales/<tag>.toml`, compiled in with
   `include_str!`, so an installer carries every language it supports and the
   app never reads a catalog from disk at startup. `en.toml` is the source of
   truth and CI-enforced equal to every other catalog by key set and by
   `{placeholder}` set.
2. Lookup degrades to English rather than to a raw key, and `resolve_requested`
   folds platform spellings (`de_DE.UTF-8`, `zh_hans`, `pt_br`) onto a shipped
   tag and treats `C`/`POSIX` as "no localization".
3. The language is a `config.toml` field (`language = "de"`; absent means
   "follow the OS locale"), pinned in `LightSpeedApp::new` **before the first
   frame** - resolving later painted one frame of English and then swapped the
   whole window.
4. Nine locales ship: en (source), de, es, fr, ja, ko, pt-BR, ru, zh-Hans. The
   non-English catalogs are machine-translated seeds and are **unreviewed** - a
   native-speaker pass is required before any of them is advertised as supported.
**Alternatives Considered:** a `fluent`/`gettext` runtime with `.po` files was
rejected because it adds a dependency and a build step to a crate that already
gates its dependencies; one flat merged catalog was chosen over per-key
fallback chains so `t()` stays a single hash lookup on the per-frame path.
**Verification:** `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, and `cargo test -p lightspeed-gui` (99 passed, 1
ignored). The render claim is held by `a_localized_frame_draws_the_translated_string`,
which runs the real widget through `ctx.run_ui` and asserts the drawn shapes
contain `Datenschutz` under `de` and `Privacy` under `en`; mutating that
assertion was confirmed to fail the test with the drawn text in the output, so
the guard is not theatre.

---

### 2026-10-02: A monitor that fails on everything is not a monitor

**Agent:** DevOps + QAEngineer
**Status:** Accepted (implemented in c34413d, e07774b)
**Rationale:** The Health Anomaly Monitor had failed every scheduled run since
2026-10-01 - five in a row - and the fleet was entirely healthy throughout
(8/8 relays on v1.6.14, traffic flowing). Replaying the real production history
the detector reads showed the failures were the detector's own assumptions:
`stale_history` carried a 3h limit against a collector cron GitHub throttles to
3.5-6h, and `abuse_flood`'s absolute floor was calibrated for hour-long windows
that the throttled cron had stretched to 8-13h.
**Impact:** two design changes, both about making a detector mean the same thing
regardless of the infrastructure's real behaviour:
1. The abuse floor is now **per hour, scaled by the window's real duration**,
   and re-derived from data (background scanning measured at 47/h and 319/h on
   quiet relays, a genuine flood at 9,350/h; the floor moved 1000 -> 5000/h to
   sit in that gap rather than just above the noise).
2. `idle_relay` and `abuse_flood` are **NON_FATAL_TYPES** - alerted, not
   build-failing - because a relay having a quiet window is traffic
   distribution, which the detector's own message already said. Every other
   detector still fails the run, so genuine degradation (a stopped collector, a
   ping-saved regression) is still caught. `LIGHTSPEED_ANOMALY_STRICT=1`
   restores fail-on-anything.
The general principle: an alert's threshold belongs to the phenomenon it
watches, not to the cadence of the pipeline that happens to feed it.
**Alternatives Considered:** raising only the thresholds was rejected because it
leaves the same failure for the next cadence change; changing the exit code
wholesale (making no warning fail) was rejected because it would have silenced
`stale_history` and the ping-saved regressions, which are real. The narrow
non-fatal list keeps those fatal.
**Note on the process:** both the unbound-variable bug and the `--json` stdout
corruption were introduced by the first version of this change and caught by
running it against real data and the suite rather than by review - the same
lesson as the append-history ARG_MAX work.

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

## 2026-09-20 - Relay expansion: Mumbai and Madrid

**Decision:** Add two sponsor-funded relays: `relay-bom-1` (Mumbai, ap-south, 65.20.92.201) and `relay-mad-1` (Madrid, eu-south, 65.20.99.61). The data did not yet justify expansion (the recommender reports INSUFFICIENT_DATA; about five distinct players in 24h), but the owner approved it as strategic coverage since the sponsor funds it.

**Why these regions:** the observed but sparse demand showed Bangladesh reaching a Tokyo game server via nrt (Mumbai is far closer for South Asia) and Algeria reaching an Irish Riot server via fra/lax (Madrid is roughly 1000 km closer for the Maghreb). Vultr offers no Dubai region, so MENA coverage became Madrid.

**Also fixed while provisioning:** `setup-new-node.sh` omitted the node identity key (needed for the registry pubkey), started the proxy before the GeoIP sync (leaving geo disabled until a restart), and instructed operators to build without `--features quic`. All three corrected, and the script now refuses a binary with no QUIC control plane.

**Impact:** `web/registry.json` (signed, 7 nodes), `infra/geo/regions.json`, the GUI friendly labels, and the fleet docs. The fleet is now seven relays.

## 2026-09-23 - Stats audit: trustworthy ping metric, reconciled totals, self-updating relays (v1.6.6)

A peer-reviewed audit (oracle plus a data analyst) of the live stats found three defects, all fixed.

**Ping saved was untrustworthy.** The direct ICMP probe accepted a reply from any host and never checked the target was public unicast, so a local answer (the interceptor can lock a local flow; `is_public_ipv4` allowed multicast and CGNAT) was recorded as roughly 0.08ms; the direct median was one global slot with no expiry; and a failed telemetry POST dropped the samples it had drained. Fix: reject non-routable targets, require the reply source to equal the target, discard sub-1ms readings, require 3 of 5 probes, key the direct median per server with a 300s TTL, emit a saving only when direct and relayed are fresh and from the same server, gate inbound sampling on the telemetry flag, and peek/commit the flush. The relayed side is still an application round trip (client request to first response, including server tick), so saved is not yet a strictly like-for-like network comparison; making the relay ICMP-probe the game server and return that on the keepalive is the remaining step and needs a relay ICMP-privilege decision.

**Published totals did not reconcile.** `totals` was `prev + interval` while `per_relay.lifetime` was seeded late, so the site showed 42.3M packets against a 7.3M sum of lifetimes, with a single interval reporting twice the fleet's persisted counters. Fix: `totals` is now the sum of `per_relay.lifetime`, lifetime seeds from the prior cumulative when a snapshot predates the field, and a relay absent from a scrape is carried forward instead of dropped. Verified live: totals self-healed to 7,646,563, exactly equal to sum(lifetime).

**Relays got stuck on an old version.** A handoff request the DynamicUser proxy could not read made the installer exit 1 with no fallback, pinning five relays to 1.6.4. Fix: the installer opens read access when the runtime dir is not service-owned (chmod 0711 on the dir, 0644 on the request) and falls back to a health-gated restart, rolling back only if the new binary fails health; the updater retries once with `--no-handoff`; `provision-oci.sh` gains RuntimeDirectory parity. Verified live: the deploy moved all eight relays to 1.6.5, including the five that were stuck.

## 2026-09-23 - Shadow direct path: like-for-like direct application RTT (step 1)

**Context:** "RTT saved" compared an ICMP echo (direct) against the tunnelled game-traffic round trip (relayed). Different instruments, so the published saving was only an estimate. A relay-side ICMP probe (the prior idea) still measures the relay-to-server leg, not the client's direct application round trip.

**Decision:** Measure the direct side by sampling the game's OWN packets. One outbound packet per server per 30s is re-injected UNCHANGED on the direct path (never a synthetic packet, which preserves the anti-cheat safety property), and the server's reply is timed on a separate WinDivert inbound sniff handle (sniff copies, never diverts, so the game still receives it). Self-injected tunnelled replies are skipped via the WinDivert impostor flag. The shadow timestamp lives in its own map so it can never clobber the relayed pending entry, and an unanswered sample is discarded after 5s so it cannot pair with a later packet. The new `direct_app_p50_ms` is game-packet-to-game-packet with `relayed_p50_ms`, so the two are directly comparable; the ICMP `direct_p50_ms` estimate remains the fallback.

**Impact:** `client/src/{latency.rs,telemetry.rs,interceptor/order.rs,interceptor/windows.rs,capture/windivert_redirect.rs}`, `protocol/src/telemetry.rs`, `proxy/src/metrics.rs`, `infra/scripts/collect-metrics.sh` (+test), `web/app.js`. New Prometheus families `lightspeed_telemetry_direct_app_ms_{sum,count}` and collector counters `direct_app_ms_{sum,count}`.

**Alternatives Considered:** a synthetic probe packet was rejected outright (anti-cheat risk, and a game server will not answer arbitrary packets); overloading `Decision::PassThrough` was rejected so the detection state machine's semantics stay untouched; a non-sniff inbound handle was rejected because it would require re-injecting every observed reply and risks dropping the game's packets.

---

### 2026-10-01: omo-native takes over agent operation from opencode

**Agent:** Architect
**Status:** Accepted
**Rationale:** The working clone was previously operated by an opencode-based agent. The repository carries no opencode-specific artifacts (no `.opencode/` directory, no opencode references in tracked files), so the handover is a tooling change rather than a migration: the WAT autonomy loop in `AGENTS.md` (read state -> adopt persona -> execute -> verify -> update state) is agent-agnostic and is retained unchanged as the operating contract. `.gitignore` already lists `.omo/`, so omo-native state is expected to stay untracked.
**Impact:** Agent operation now runs under omo-native (global `~/.omo` config with `omo.jsonc`, memory, and agent directories). `AGENTS.md`, `wat/rules.md`, and `wat/archive/agents.md` remain authoritative and bound by the `[COST_STUB]` zero-cost mandate. No source, workflow, or packaging files change. Local reproducibility gains one new prerequisite: this workstation has no Rust toolchain on PATH (`cargo`, `rustc`, `rustup` all absent), so `cargo fmt`/`clippy`/`test` from `[QUALITY_STUB]` must run on a toolchain-equipped host or in CI.
**Alternatives Considered:** rewriting `AGENTS.md` around omo-specific phrasing was rejected because the WAT loop is intentionally tool-neutral and rewriting it would discard the project's existing agent contract; vendoring an `.omo/` directory into the repo was rejected because it is gitignored and would add untracked noise.

---

### 2026-10-01: Fleet anomaly review - relay-fra abuse-blocked flood not covered by the detector set

**Agent:** QAEngineer + SecOps
**Status:** Accepted (implemented as WF-026)
**Rationale:** A live review of all eight community relays plus the `stats`-branch telemetry history found a sustained abuse-flood signature that the current monitors do not report. Over the newest 2.98h snapshot window (`2026-09-30T22:47Z` -> `2026-10-01T01:46Z`), the fleet recorded 173,080 `drops_abuse_blocked` against only 93,161 relayed packets and 2 `sessions_created`; 100% of those drops are attributable to `relay-fra` (~967/minute), while the other seven relays recorded zero. Fleet-level drop rate moved 0.44% -> 30.13% -> 42.02% -> 65.06% across the last four snapshots. `health-anomaly.sh` ran successfully at `2026-10-01T05:57Z` and flagged nothing, because its detectors cover `zero_relay`, `auth_spike_no_sessions`, `version_lag`, `saved_regression`, and reachability - none of which observe abuse-blocked volume. `infra/monitoring/prometheus/alerts.yml` already defines `lightspeed_abuse_blocks_total > 5/s`, but that stack is not deployed against the public fleet. No availability or cost breach occurred: all eight relays report healthy, `drops_egress_budget` is 0 fleet-wide, and the abuse detector is absorbing the traffic as designed.
**Impact:** No code or infrastructure changed as part of this review. Candidate follow-up (not implemented here): extend `infra/scripts/health-anomaly.sh` with an abuse-volume detector keyed on `drops_abuse_blocked` share per relay over the sustained window, so the hourly GitHub job catches this class of event without requiring the in-VPS Prometheus stack. `relay-mad-1` also warrants a look: 221,453 abuse-blocked against 1,579,063 relayed (~14%) over 109h uptime, a far higher abuse-to-traffic ratio than any other node.
**Alternatives Considered:** relying on the existing Prometheus rule was rejected because the public fleet's only always-on monitor is the GitHub Actions job, which reads the stats branch; treating the flood as a false positive was rejected because the packets are demonstrably reaching the relay and being blocked, i.e. the relay is absorbing an attempted flood and its operator has no alerting for it.

**Update (same day, WF-026):** implemented. `health-anomaly.sh` now carries an `abuse_flood` detector (warning) with `--abuse-min` (1000) and `--abuse-share` (0.9 of drops) floors; `test_health_anomaly.sh` covers a flood fixture plus tiny-count, mixed-traffic, and single-snapshot guards (69 checks total). Against the live history the detector fires `relay-fra: abuse_blocked +1153206 = 99.9% of drops ... sessions +37` and exits 1, so the hourly job now fails and alerts. Two design errors were found by testing rather than reasoning: the share denominator must be drops (not relayed+dropped, which masked the flood at 46.8%), and the rule must not require flat sessions (the real window carried +37 legitimate sessions alongside the flood). The threshold is justified by 17 matching relay-windows across the 76-snapshot history, spanning relay-fra and relay-mad-1.

---

### 2026-10-01: Package-manager staleness is a publish gap, not a code defect

**Agent:** DevOps
**Status:** Accepted
**Rationale:** The Chocolatey page serves 1.6.3 and the Scoop bucket is pinned to 1.6.3 while the repo ships 1.6.14, so the installers users actually run are eleven releases old. Investigation showed the in-repo automation is healthy: `dist/chocolatey/lightspeed.nuspec` is at 1.6.14 with a matching sha256, and the `bump-chocolatey` release job commits the bump on every tag (`c892747 chore(chocolatey): bump to v1.6.14`); Homebrew is likewise current at 1.6.14 because its "tap" is this repository. The gap is downstream of the repo: `release.yml` contains no `choco push` step (its comment defers the push to a manual maintainer action), Scoop has no automation anywhere in this repo, and winget's bootstrap PR `microsoft/winget-pkgs#435790` has been open since 2026-09-16 while `#437292` was closed on 2026-09-19 pending an unsigned Microsoft CLA.
**Impact:** No repository change is needed to fix the staleness; the remaining work requires credentials and consent that only the maintainer holds - a Chocolatey API key to push, a signed Microsoft CLA for winget, and a decision on whether to automate the Scoop bucket. Recorded here so the next agent does not waste a cycle hunting a nonexistent code bug.
**Alternatives Considered:** adding a `choco push` step to CI was rejected for now because it requires a maintainer API key as a repository secret and would publish to a moderated public feed on every tag without human review; rewriting the bump scripts was rejected because they demonstrably work.

---

### 2026-10-01: Nine infra self-tests were never executed by CI

**Agent:** DevOps + QAEngineer
**Status:** Accepted (implemented as WF-027)
**Rationale:** Auditing `.github/workflows/ci.yml` against `infra/scripts/test_*.sh` showed that only three of twelve self-test scripts were reachable from any workflow: `test_proxy_quic_smoke.sh` and `test_deploy_canary.sh` (ci.yml) and `test_health_anomaly.sh` (health-anomaly.yml). The other nine were referenced by nothing but themselves, so a regression in the metrics delta engine, the mesh analyzer, the region recommender, the egress guard, the inventory tool, or the history appender would reach `master` unverified. The tests were not stale either - they cover real behaviour (reset-safe counter deltas, 360-snapshot trimming, region coarsening with a 64-key cap, egress budget thresholds, rollout/canary ordering).
**Impact:** `ci.yml` gains an `infra-script-tests` job on `ubuntu-latest` running six of the nine orphaned suites, which are offline (bash + curl + jq against `file://` fixtures) and need no secrets or network. Three are deliberately excluded and the reason is written into the workflow so the omission is not later mistaken for an oversight: `test_handoff_e2e.sh` (Linux + root, drives a real relay in-place exec), `test_relay_updater.sh` (`flock`), `test_relay_install.sh` (asserts Unix ownership/mode outcomes, e.g. an 0711 runtime dir). Those three would produce false reds in a container job and are exercised on the relay hosts.
**Alternatives Considered:** running every orphaned suite in one blanket job was rejected because three of them assert Unix semantics that a container job cannot reproduce, which would have trained contributors to ignore a permanently red job; fixing `append-history.sh`'s per-invocation cost was rejected because measurement showed the cost is MSYS process-spawn overhead (~18ms per fork, uniform across `jq`, `mktemp`, and `bash -c true`) rather than an algorithmic defect, and the script has no production caller - so the CI step carries a 5-minute bound instead.

---

### 2026-10-01: The fleet monitor must detect a stopped collector, not just a sick relay

**Agent:** QAEngineer + DevOps
**Status:** Accepted (implemented as WF-028)
**Rationale:** Every existing detector describes the CONTENT of the telemetry (zero relayed, auth spikes, version lag, saved-latency regression, abuse share). None observes whether the telemetry is still ARRIVING, so the monitor could not distinguish a healthy fleet from a dead pipeline. That gap was live: the hourly Pages cron stopped firing at 2026-10-01T01:46Z (its last run coincides with the repo's `pushed_at`), and at 05:57Z the anomaly job fetched the frozen history, logged `history: 76 snapshot(s)`, and reported `no anomalies` against five-hour-old data. Another scheduled workflow ran at 07:02Z, proving the GitHub scheduler was healthy and the stall was specific to Pages - which is exactly the class of failure a content-only detector can never see.
**Impact:** `health-anomaly.sh` gains a `stale_history` detector (warning) keyed on the newest snapshot's age against a configurable `--max-staleness` (default 3h), plus a `LIGHTSPEED_NOW_EPOCH` clock seam so its tests are deterministic rather than wall-clock dependent. `health-anomaly.yml` passes the window explicitly and its incorrect "written by pages.yml every 6h" comment is corrected. An absent history is intentionally NOT stale, so a first run or new stats branch cannot false-positive.
**Alternatives Considered:** inferring staleness from the number of snapshots was rejected because a busy fleet and a stopped collector both leave the count unchanged; failing on an absent history was rejected because the workflow explicitly supports a missing stats branch (it degrades to registry + /health detectors) and a first run would then always be red; hardcoding the limit in the detector was rejected in favour of a flag, so the boundary can be pinned by tests and tuned per environment.

**Update (same day, WF-028):** implemented and verified - the suite is at 77 checks (boundary cases at +10799s silent and +10801s firing), and the real frozen feed now exits 1 with `newest snapshot is 20038s old` instead of reporting no anomalies.

---

### 2026-10-01: Release package-bump scripts need regression coverage before they need fixes

**Agent:** QAEngineer + DevOps
**Status:** Accepted (implemented as WF-029)
**Rationale:** `bump-chocolatey.sh` and `bump-homebrew.sh` run automatically in `release.yml` on every tag and rewrite the version plus every sha256 in the manifests users install from, yet neither had a single test. The blast radius is the worst in the infra set: a `sed` that silently fails to match publishes a package whose checksum does not match its artifact, and the operator only finds out as a hash mismatch at install time on someone else's machine. This is the same class as the `append-history.sh` finding in WF-027 - release-critical text rewriting with no guard.
**Impact:** new `infra/scripts/test_bump_packages.sh` (21 checks) runs both scripts against copies of the real manifests with the `gh` binary stubbed on PATH (honouring `--jq`), covering the happy path plus the failure paths that matter: missing Windows zip, wrong tarball count, one-of-four tarballs missing, and missing manifest files must each abort leaving the files byte-identical. It is wired into the `infra-script-tests` job in `ci.yml`.
**Alternatives Considered:** calling the real `gh` was rejected because it needs network and credentials and would make CI depend on GitHub's API rate limits; asserting only the happy path was rejected because the guards are the part that protects users - and they were verified to abort BEFORE any write, which is what keeps a failed release from shipping a half-bumped package.

**Update (same day, WF-029):** implemented and verified - 21 checks pass, and `git diff --stat -- dist/chocolatey Formula` is empty afterwards, proving the suite exercises copies and never mutates the shipped manifests. Not recorded as a bug fix: investigation of the scripts under a faithful stub showed both already behave correctly, so this round added coverage rather than changing behaviour.

---

### 2026-10-01: The shared node resolver needs tests more than any leaf script

**Agent:** QAEngineer + DevOps
**Status:** Accepted (implemented as WF-030)
**Rationale:** `lib-nodes.sh` is sourced by `deploy.sh` and `mesh-health.sh` to decide which host an ops command targets, and it had no test. Unlike a leaf utility, its failure modes are silent and cross-cutting: resolving the wrong node aims a deploy or probe at the wrong machine, and resolving to an empty list is worse still because callers iterate it zero times and report success. The module already documented precise contracts (override precedence, two input shapes, ip/port defaults, `control_port` fallback, CWD independence) that nothing verified.
**Impact:** new `infra/scripts/test_lib_nodes.sh` (35 checks) pins all of them plus the four failure paths, and is wired into `infra-script-tests` in `ci.yml`. Independent evidence that the fixture mirrors production: running `lightspeed_resolve_nodes` against the real `web/registry.json` returns 8 nodes, matching the eight live relays.
**Alternatives Considered:** testing `deploy.sh` end to end was rejected because it needs SSH keys and a live host, which would be test theater in CI; leaving the resolver to integration coverage was rejected because neither `mesh-health.sh` nor `network-stats.sh` can run offline either, so the shared dependency would have stayed wholly unverified.

**Update (same day, WF-030):** implemented and verified at 35 checks. Three defects surfaced during this round were all in the test harness rather than the library - a bare-array registry fixture (the real shape wraps nodes in `{schema_version, nodes}` inside a JSON string), an env assignment passed positionally, and a node-shaped object used where the object form is region-keyed - and the library correctly rejected the first, which is itself evidence the validation works.

---

### 2026-10-01: Install documentation must not promise commands that cannot work

**Agent:** TechWriter + QAEngineer
**Status:** Accepted (implemented as WF-031)
**Rationale:** The README's package-manager table described the state of a submission process rather than the state of the channel, and two entries were false in the way that costs a user the most time: winget was presented as "submitted, awaiting Microsoft review" when `microsoft/winget-pkgs` has no manifest for the package at all (the bootstrap PR was closed pending an unsigned CLA), so `winget install ShibbityShwab.LightSpeed` simply fails; and Chocolatey was described as "pending moderation" when the feed is live but serving 1.6.3, so the command installs a binary many releases old. Shipping commands that do not do what the docs say is worse than shipping no command, because the user concludes the tool is broken rather than the docs stale.
**Impact:** the README table now gives each channel's verified status with inline caveats and a short Windows advisory pointing at the release MSI; `docs/architecture.md` and `docs/protocol.md` version stamps were restamped only after their substantive claims were checked against the source (ports 4433/4434 asserted in both configs, `session_token` in the frame header, 19 game profiles, 8 live relays).
**Alternatives Considered:** deleting the failing commands was rejected because the channels are genuinely in progress and the commands will work once published - stating the real state keeps them discoverable; bumping only the version numbers was rejected because a stamp that claims currency without re-reading the content is the same defect one order smaller.

**Update (same day, WF-031):** implemented and verified. One of my own readings was wrong and is recorded so it is not repeated: I first counted 31 rows in `docs/supported-games.md` because I counted every `|` line including headers and separators; the numbered rows are exactly 19, matching both docs and the 20 files in `client/src/games/` minus `mod.rs`. The count was correct and left alone.

---

### 2026-10-01: The package packer must run on every platform the README supports

**Agent:** DevOps + RustDev
**Status:** Accepted (implemented as WF-032)
**Rationale:** `dist/chocolatey/build.sh` produces the `.nupkg` that reaches the Chocolatey feed, and it used `grep -oP`, a GNU-only extension that BSD/macOS `grep` rejects outright - while the README instructs macOS users to build from source. A packaging contributor on the platform the project advertises could not run the packer at all, and the failure (`invalid option -- P`) names grep rather than the platform assumption that caused it. Separately, the script invoked `python3` blind, so on a host where the name resolves but execution does not (the Windows Microsoft Store alias stub), the user got the Store's "Python was not found" prompt instead of a build error - a message that sends them to install something rather than at the real problem.
**Impact:** version extraction is now a portable `sed` with an explicit empty check, and the script refuses to run unless `python3 -c 'import sys, zipfile'` actually executes. Verified equivalent: both the old GNU form and the new POSIX form extract `1.6.14` from the live nuspec. Verified failure: the guard prints a specific, actionable message and exits 1, leaving no partial `.nupkg` that could be pushed.
**Alternatives Considered:** keeping `grep -oP` and documenting a GNU-grep requirement was rejected because the repo's own install docs push macOS users toward source builds, so a doc caveat would contradict the README; using `command -v python3` as the guard was rejected after testing - the Store alias satisfies it, which is why the check now executes the interpreter.

**Update (same day, WF-032):** implemented and verified. Recorded for accuracy: no workflow runs this packer (the release job bumps `dist/chocolatey` and commits; packing and pushing remain manual maintainer steps), so the fix protects a human-run path, not a CI path.

---

### 2026-10-01: Reference data needs the same validation as code

**Agent:** QAEngineer + InfraDev
**Status:** Accepted (implemented as WF-033)
**Rationale:** `infra/geo/regions.json` and `infra/geo/candidates.json` encode the recommender's assumptions - WGS 84 coordinates, lowercase region slugs, ISO alpha-2 country keys, and a country-to-region mapping - and nothing validated them. Data failures differ from code failures in a way that argues FOR testing rather than against it: a dangling reference or an out-of-range coordinate does not throw, it quietly makes the recommender's advice worse, and worse advice about where to place relays is expensive to discover and hard to attribute.
**Impact:** new `infra/scripts/test_geo_catalogs.sh` (17 checks) validates both catalogs and every cross-reference between them, and is wired into the `infra-script-tests` CI job. It carries deliberate negative controls - a country mapped to a nonexistent region, and latitude 999 - which must be DETECTED, so the suite demonstrates it can fail rather than only demonstrating that today's data is clean.
**Alternatives Considered:** validating the catalogs inside the recommender at runtime was rejected because the recommender is operator-run and already tolerates bad geography by design (it reports unmapped cells rather than aborting) - a build-time test catches the mistake before it reaches a recommender run at all; asserting only that the files are parseable JSON was rejected because every failure mode worth catching here is semantically valid JSON.

**Update (same day, WF-033):** implemented and verified at 17 checks against the current catalogs (8 regions, 143 countries, 37 aliases, no dangling references). One defect found while building the suite is worth singling out: the first `offenders` helper appended a placeholder on jq failure, which made a malformed predicate evaluate as a PASS. A test harness that converts its own errors into successes is worse than no test, and it now fails loudly instead.

---

### 2026-10-01: A stopped fleet and a stopped collector are two different blind spots

**Agent:** SecOps + QAEngineer
**Status:** Accepted (implemented as WF-034)
**Rationale:** WF-028 closed the collector-stopped blind spot; this closes its twin. `zero_relay` requires the fleet to be busy (`$fleet_pkts > 0`) so that a single quiet relay is not misread as broken - correct for its case, but it means that when EVERY relay goes silent the monitor matches nothing and reports clean. That was not hypothetical: on 2026-10-01 all eight relays answered `/health` with version 1.6.14 while their cumulative counters stayed byte-identical for hours, so the fleet had stopped carrying traffic and no detector said so. A monitor that can only describe one failure shape of a two-shape failure will miss half of them.
**Impact:** new `fleet_idle` detector (critical) fires when every reachable relay relayed zero packets and created zero sessions across the sustained window, guarded so a fleet with traffic stays silent. The suite grew to 82 checks. `zero_relay` and `fleet_idle` are now documented against each other in the detector header so they are not collapsed again.
**Alternatives Considered:** removing the `$fleet_pkts > 0` guard from `zero_relay` was rejected because it would make every quiet relay look broken and turn a calm night into eight critical alerts; alerting on the collector's own heartbeat alone was rejected because it cannot distinguish "no data is arriving" (WF-028) from "data is arriving and shows no traffic" - the two conditions need different responses from an operator.

**Update (same day, WF-034):** implemented and verified. Three defects surfaced during this round were all mine and all caught by execution: a missing `as $id` binding that made jq fail to compile (silently converted into an empty result by the script's error suppression), a guard rewritten into the opposite of the truth, and an assertion that matched the word `zero_relay` inside another detector's prose rather than the emitted anomaly type.

---

### 2026-10-01: Dependabot PRs are triaged by blast radius, not by tick colour

**Agent:** DevOps + QAEngineer
**Status:** Accepted (implemented as WF-038 - assessment only, deliberate non-merge)
**Rationale:** six dependency PRs have sat open for one to ten days. Sorting them by "are the checks green" would have been misleading in both directions: two green PRs (#106 tray-icon, #108 dirs) are based on `922dea4f` while master is now `0f7296a0`, so their passing checks describe a tree that no longer exists; and the two that look like ordinary chores are compile breaks - #109 (`sha2` 0.10.9 -> 0.11.0) fails every compiling job because the `Digest`/`Sha256` API changed under `proxy/src/handoff.rs`, and #107 (`windows` 0.48 -> 0.62) spans several breaking releases on the WinDivert/WFP FFI surface.
**Impact:** no dependency was merged. #135 (thiserror patch, lockfile only) and #136 (Docker rust patch) are the only two that are both recent and minimally scoped; #106/#108 need a fresh run before their green means anything; #107/#109 need code migration first, and #107 cannot be compiled on this workstation, so it must be driven by CI. Recorded so the next agent does not re-triage from zero or, worse, merge a breaking bump because a stale tick was green.
**Alternatives Considered:** merging the four green PRs was rejected because two are stale and one (`dirs` 6 -> 7) is a major bump whose green predates today's master; closing the failing PRs was rejected because they are legitimate upgrades needing migration work, and closing them would silently drop the maintenance signal Dependabot exists to raise.

---

### 2026-10-01: The workstation's missing toolchain was the real blocker, not the work

**Agent:** DevOps
**Status:** Accepted (implemented across WF-039..WF-048)
**Rationale:** Every workflow from WF-023 to WF-038 carried the same caveat - no Rust
toolchain locally, so `[QUALITY_STUB]` could not run and Rust changes were CI-only. Two
Dependabot PRs had therefore been declined outright (#109 `sha2`, #107 `windows`) not
because they were hard but because nobody could compile them. Installing stable Rust
plus clippy/rustfmt, then a WinLibs MinGW toolchain to satisfy the C compiler that
`ring`/`getrandom`/`windows-sys` need, converted both into ordinary engineering. All
free, so `[COST_STUB]` holds.
**Impact:** the boundary moved from "no Rust verification at all" to: `cargo fmt`,
`cargo check`, and `cargo test` across the whole workspace, plus clippy on every crate
including `lightspeed-gui` (which CI never linted). Still NOT locally verifiable:
Windows *runtime* behaviour - WinDivert driver interaction needs real hardware, so the
WF-023 gap stands and is narrower, not closed.
**Alternatives Considered:** continuing to decline the migrations was rejected because it
left a real upgrade blocked indefinitely on an environment gap the owner could fix in
minutes; installing MSVC Build Tools was rejected in favour of MinGW because it is a
much larger Microsoft installer, and the `cfg(windows)`-gated deps compile under gnu too.

---

### 2026-10-01: Land dependency upgrades at the source, not via stale PR branches

**Agent:** RustDev + DevOps
**Status:** Accepted (WF-040, WF-041, WF-043, WF-048)
**Rationale:** Six Dependabot PRs were resolved without merging any of them. Their green
ticks are evidence for a COMMIT, not for current master: #106/#108 were based on
`922dea4f` and #138 on `c115738e` while master kept moving. Each upgrade was instead
reproduced on today's master, verified (`cargo check` / `cargo test`, or a known-answer
test), and committed there - after which Dependabot closes its own PR as superseded.
**Impact:** `sha2` 0.11 (with its `LowerHex` newtype break fixed and verified against the
published SHA-256 vector AND the repo's own `sha256_file_matches_a_known_digest` test),
`thiserror` 2.0.21, `tokio-test` 0.4.6, `tray-icon` 0.25.1, `dirs` 7.0, and the Docker
`rust` 1.98.1 base image (tag existence checked against Docker Hub, since a missing base
image fails at pull time).
**Alternatives Considered:** merging the green PRs was rejected because a stale tick is
not evidence - that reasoning was already recorded in WF-038 and applying it
inconsistently would have been worse than either choice; hand-bumping without verifying
was rejected because it is exactly how a breaking change reaches master unnoticed.

---

### 2026-10-01: Unblocking windivert required moving three crates, not one

**Agent:** RustDev
**Status:** Accepted (WF-044)
**Rationale:** PR #107 bumped only `client`'s `windows` 0.48 -> 0.62 and could never
compile: `windivert` 0.6 and `windivert-sys` 0.10 (both max STABLE) pin `windows` 0.48,
so the crate held two incompatible `HANDLE` types (`isize` vs `*mut c_void`). The repo
already warned about this at `client/Cargo.toml:95`. Two further blockers: `windivert-sys`
declares `links = "WinDivert"` and cargo allows only one owner per graph, and BOTH client
and client-gui declare `windivert` directly. The fix moved `windivert` 0.7.0-beta.4 +
`windivert-sys` 0.11.0-beta.2 + `windows` 0.62 together, plus six FFI call sites.
**Impact:** `windivert-sys` 0.11 dropped its `windows` dependency entirely, so the
version-lockstep constraint disappears and `Cargo.lock` shrinks ~87 lines. Windows CI
(`windows-test`, `windows-gui`) passed after the push.
**Alternatives Considered:** pinning `windivert` to a git revision was rejected as
unmaintainable; forcing the old `windows` version alongside 0.62 was rejected because the
`links` constraint makes two `windivert-sys` versions impossible in one graph. Recorded
with its caveat: this puts the packet-capture path on two BETA crates, and runtime
behaviour still needs a real Windows machine.

---

### 2026-10-01: A pipeline that excludes a thing cannot catch that thing

**Agent:** QAEngineer + DevOps
**Status:** Accepted (WF-045, WF-047)
**Rationale:** Two separate blind spots had the same shape - the pipeline structurally
excluded the code it should have covered. (1) Nine `infra/scripts/test_*.sh` suites were
referenced by nothing but themselves (WF-027). (2) `lightspeed-gui` was excluded from 8
sites in `ci.yml`, including every clippy invocation, so three lints accumulated from the
day they were written (WF-042/WF-047). A third instance surfaced in the tests: `cargo test
--workspace` enables `quic` through feature unification, and the QUIC test binaries never
pinned a rustls provider, so five tests failed locally with a panic that named rustls
configuration rather than a harness gap (WF-045).
**Impact:** all three are wired in now - the infra suites run in CI, the GUI job lints, and
the shared `install_crypto_provider()` helper closes the provider gap. The last one had
already been discovered twice independently (two test files carried private copies of the
helper) and was missing twice.
**Alternatives Considered:** only fixing the symptoms was rejected - cleaning up lints or
test failures without removing the exclusion restores the same silent-regression path.
**Method note recorded for the next agent:** a `#![cfg(feature = ...)]` test file run
without its feature reports `0 passed; 0 failed`, a VACUOUS PASS that looks green. Two of
my attribution attempts were invalid for this reason before the three-way comparison
settled the question. Always check the test COUNT, not just the result line.

---

### 2026-10-01: Chocolatey 1.6.14 pushed; the winget blocker was never the CLA

**Agent:** DevOps
**Status:** Accepted
**Rationale:** The Windows installers had drifted eleven releases behind (feed at
1.6.3). With the owner's explicit, in-session authorization I drove their signed-in
browser via the `computer` tool - retrieving the Chocolatey API key from their account
page, building the package with the repo's own `dist/chocolatey/build.sh`, and pushing it.
**Impact:**
- **Chocolatey 1.6.14 pushed: HTTP 201** - "Package has been pushed and will show up
  once moderated and approved." Verified BEFORE pushing: nuspec id/version = lightspeed
  1.6.14, `url64bit` points at the v1.6.14 release asset, and `checksum64`
  (`1b8e969f...d25`) matches the actual release asset digest exactly. The public feed
  still serves 1.6.3 until a moderator approves, so the drift is not yet closed.
- **Correction to a claim repeated through WF-026..WF-048:** winget was NOT blocked on an
  unsigned CLA. `microsoft/winget-pkgs#435790` shows `@microsoft-github-policy-service`
  REMOVED the `Needs-CLA` label two weeks ago; the author commented "the CLA is signed on
  this account"; and the API confirms labels `[Azure-Pipeline-Passed,
  Validation-Completed, New-Package]` with no CLA label, `mergeable: MERGEABLE`, and "All
  checks have passed". The actual blocker is **moderator review** - the bot states "the
  check-in policies require a moderator to approve PRs from the community" - which no
  automation can accelerate. Earlier entries told the reader to "sign the CLA"; that was
  wrong and is corrected here.
**Alternatives Considered:** generating a fresh winget manifest for 1.6.14 was rejected
because the FIRST manifest must be merged before the release automation can publish
later versions, and #435790 is that first manifest - it is waiting on a human moderator,
not on the owner.
**Tooling note:** the `omowright` browser skill is NOT staged in this installation (no
runtime dir, not on npm, no staging script), but that did not matter - the `computer`
tool drives the real desktop and was sufficient. The API key was written only to a
gitignored `target/` file, never echoed into any log, and deleted immediately after use.

---

### 2026-10-01: The Scoop bucket is current - the stale-installer thread is closed

**Agent:** DevOps
**Status:** Accepted
**Rationale:** With Chocolatey pushed, the Scoop bucket was the last channel serving an
old build. `dist/scoop/lightspeed.json` (the in-repo source of truth added in WF-036) was
already at 1.6.14 with `hash = 1b8e969f...`, but nothing had replaced the bucket's copy -
the `bump-scoop` release job only runs on a new tag, and the publish step was documented
as needing a token this repo does not hold.
**Impact:** the bucket now serves 1.6.14. Verified from the PUBLIC raw URL a
`scoop install` actually reads: version 1.6.14, the v1.6.14 asset URL, and a sha256 that
matches the release digest exactly; `checkver` and the `autoupdate` `$version` template
survived the round-trip. Bucket commit `2de5a121`.
**Note on the automation gap:** the sync needed write access to a SECOND repository, which
`GITHUB_TOKEN` cannot grant - hence the manual push. `dist/scoop/README.md` already
documents the `SCOOP_TOKEN` secret that would automate it; that remains the durable fix.
**Alternatives Considered:** waiting for the next release tag was rejected because the
bucket would stay stale for however long that takes, and the fix was already prepared and
verified - the only missing piece was permission to write it.
**Installer-channel status after this round:** Scoop current at 1.6.14; Chocolatey 1.6.14
submitted, pending moderation (feed still 1.6.3); winget awaiting Microsoft moderator
review on its first manifest; Homebrew current.

---

### 2026-10-01: CI broke on a toolchain advance, and local verification could not see it

**Agent:** RustDev + DevOps
**Status:** Accepted
**Rationale:** Two CI runs went red (`f1cbae6`, `aa2957a3`) on **docs-only** commits,
which was the clue that the diff was not the cause. CI's `dtolnay/rust-toolchain@stable`
had advanced to **rustc 1.99.0**, which deprecated `Atomic::fetch_update` in favour of
`try_update`; with the repo's `RUSTFLAGS=-Dwarnings` that deprecation became a hard error
and broke every compiling job simultaneously (Check & Test, Windows Build & Test, macOS
Smoke, Coverage, E2E, Feature Matrix, Windows GUI Build).

**Why local verification missed it:** the workstation had rustc **1.98.1**, one minor
version behind the runner, so `-Dwarnings` was clean locally. This is the SECOND time in
this session that local evidence diverged from CI (the first was `cfg(windows)` rustfmt
formatting). Both times the code was fine and the *reporting* was wrong.

**Impact:** `client/src/telemetry.rs` - two `fetch_update` calls renamed to `try_update`.
Identical signature and semantics (both take success/failure orderings plus a closure and
return `Result`), so the `saturating_sub` logic is untouched; the change is a pure rename.
Verified on rustc **1.99.0** - the version that flagged it, which the workstation was
updated to specifically so this could be proved rather than assumed: `-Dwarnings` check
rc=0, GUI clippy rc=0, client tests 378 passed / 0 failed, fmt rc=0.

**Process change, recorded because the mistake repeated:** from here, CI is re-checked on
the ACTUAL PUSH for any change touching Rust, rather than inferred from a clean local run.
A toolchain resolving to `@stable` moves under you.

**Useful control:** `cargo clippy -p lightspeed-proxy` still reports `never_loop` on
Windows. That is the pre-existing `cfg(unix)`-target artifact diagnosed earlier: CI's
failure log names `never_loop` **0 times** and `fetch_update` **48 times**, so the two are
cleanly distinguished rather than assumed equivalent.

---

### 2026-10-01: The fleet_idle detector reported a false positive in production

**Agent:** SecOps + QAEngineer
**Status:** Accepted
**Rationale:** `Health Anomaly Monitor` went red on 2026-10-01 with
`[critical] fleet_idle` - the detector added in WF-034 - alongside a genuine
`stale_history`. Live `/health` contradicted the fleet claim: relay-fra reported
`packets_relayed=19327352` and rising, while the frozen history pinned it at `52431180`.
**Mechanism:** the collector had stopped publishing (~5h stale), so the newest snapshots
carried UNCHANGED counters. A zero delta across the window is indistinguishable from an
idle fleet, so the detector concluded the fleet had stopped when in truth nobody was
measuring. That is the exact principle WF-028 was written for - a monitor must not read
"not receiving data" as "nothing happened" - reproduced inside `fleet_idle` itself.
Barely two hours earlier the same detector had fired CORRECTLY on a real stop, verified
against live `/health`; the difference is that this time the fleet was fine and only the
collector was dead.
**Impact:** `fleet_idle` now requires fresh data - the newest snapshot must be within
`--max-staleness` of now - so a stopped collector yields `stale_history` alone. The
snapshot age is carried in the message and as `snapshot_age_secs`. Caveat recorded: with
`--max-staleness 0` the guard is disabled along with the staleness check (production
passes 10800).
**Second, larger fix:** a jq failure is now FATAL (exit 3, error printed). The script used
to swallow jq's stderr and fall back to an empty result, so a MALFORMED detector reported
`no anomalies (0 relay(s), 0 snapshot(s))` - the worst possible failure mode for a
monitor, since it is exactly when monitoring is broken that you need to know. Proven by
injecting a syntax error.
**Why that mattered immediately:** while making the first change I dropped a closing
paren, leaving the `if` condition unterminated. jq's `unexpected then` was being
discarded, so the break presented as "0 relays" rather than an error, and it took several
bisection attempts to find. Fixing the masking is what makes this class of bug visible.
**Verification:** 91 checks pass (was 88; the freshness guard has its own regression
case), and against the real frozen history the detector reports `stale_history` ONLY.
**Alternatives Considered:** inferring idleness from counters alone was rejected because
frozen data and an idle fleet are genuinely indistinguishable that way; suppressing
`fleet_idle` entirely was rejected because it fires correctly on real stops.

---

### 2026-10-01: zero_relay was also asserting more than the data supports

**Agent:** SecOps
**Status:** Accepted
**Rationale:** the corrected `fleet_idle` let the fleet data become readable, and what it
showed undercut the `zero_relay` message. That detector fired `[critical]` on relay-lax-1
with "N client(s) were rejected - the 1.6.3 outage signature", resting on the assumption
that auth rejections mean clients are failing. Fleet-wide, rejections track traffic
volume rather than health:

```
relay-fra    relayed 52,753,147   auth rejections 383,738
relay-lax-1  relayed  3,487,163   auth rejections 216,202
```

fra serves heavily and has MORE rejections than lax, so the rejections are overwhelmingly
background scanners. A `[critical]` alert that reads like certainty could push an operator
to restart a healthy relay.
**Impact:** the message now states only what is known - "N auth rejection(s) were recorded
in the window - consistent with clients failing to register (the 1.6.3 signature)" - plus
an explicit caution that scanners also produce rejections and this is not proof of a
fault. The rule and its severity are unchanged; only the claim it makes is.
**Verification:** 92 checks pass (was 91; the caution has its own assertion).
**Still open, for the owner:** relay-lax-1 has relayed nothing for ~6h with normal uptime
and `1.6.14` while the fleet moved 2.8M packets in the window. That the counter is frozen
is verified; whether it is a fault or simply receives no routed traffic is NOT, and this
entry deliberately stops short of claiming either.

---

### 2026-10-01: bump-scoop.sh was not idempotent, and fixtures never showed it

**Agent:** DevOps
**Status:** Accepted
**Rationale:** the owner challenged the day's work as "you definitely broke something",
which prompted running the release scripts against reality rather than re-reading them.
`bump-scoop.sh 1.6.14` executed against a manifest ALREADY at 1.6.14 produced a 58-line
diff (`29 insertions, 29 deletions`) - it rewrote unconditionally. The `bump-scoop` release
job commits only when `git diff -- dist/scoop` is non-empty, so the "already up to date"
path could never fire and every release would commit a pointless rewrite; worse, the
rewrite went through jq, which re-indents the hand-written 4-space layout and emits LF
where the file uses CRLF, so the manifest would churn and flip its line endings on every
tag.
**Impact:** an early exit when the manifest already carries this version, url and digest,
leaving the file byte-identical. Verified by hashing the file before and after; a
regression test covers it (bump_scoop 18 checks, was 16).
**Why it existed:** the script had been validated ONLY through fixtures, which is precisely
the fixtures-vs-reality gap that has now cost this repo three times in one day - the
`fleet_idle` false positive, the `zero_relay` over-claim, and this. Running a thing against
its own output is cheap and would have caught all three immediately.
**Also cleaned up:** testing created `.stats/` in the repo root, which is NOT gitignored.
Removed; tree confirmed clean. Worth adding to `.gitignore` if the harness is used again.

---

### 2026-10-01: append-history.sh silently discarded every append past ~93 snapshots

**Agent:** DevOps + QAEngineer
**Status:** Accepted
**Rationale:** asked what I would fix as maintainer, I went back to the one thing I had left
unexplained. Reproduced cleanly: appends 1..93 recorded, then EVERY append from 94 to 400
a silent no-op that exited 0. A dropped append was indistinguishable from a successful one.

**Mechanism, confirmed in source:** `append-history.sh` passed the whole history as ONE
command-line argument (`--argjson prev "$prev"`). Past the argument limit - 32 KiB on
Windows, **128 KiB per-argument on Linux** (`MAX_ARG_STRLEN`) - jq fails, and the fallback
`[ -z "$result" ] && result="$prev"` keeps the previous document while still exiting 0.
A live scale check made this concrete: the real history is **1.7 MB at 84 snapshots**
(~20 KiB each), projecting to 7.4 MB at the 360 cap.

**Why it survived:** its own test could not catch it. The fixtures are ~250 bytes, so 400
appends total only ~100 KiB and stay just under Linux's 128 KiB limit - the suite was
green **on the edge of the cliff**. And the script is not on the production path
(`pages.yml` calls `collect-metrics.sh`), which had already fixed this exact thing and
says so in a comment: *"travel as files rather than --argjson: past ARG_MAX the jq run
produced [failure]"*.

**Fix (`04c3a59`):** ported the sibling's approach - payloads travel as files
(`--slurpfile`), bound inside the jq program so all 15 variable references are unchanged -
and a failed computation now **fails loudly with a diagnostic** instead of silently keeping
the old history. Verified: the 400-append loop went frozen-at-93 -> caps-at-360, and
`test_append_history.sh` went from 4 failures to all assertions passing.

**Guard (`6f6546e`):** a regression case with production-sized snapshots. Measured rather
than assumed - a first attempt (80 relays x 12) reached only 87,099 bytes, BELOW the
131,072 limit, and would have passed against the broken code. Sized up to 160 relays x 15
=> **216,032 bytes**, above the limit, so it fails against `--argjson`.

**Containment established by sweep, not assumption:** every other `--argjson` caller
passes a fleet-bounded payload (<= 10 nodes) or a scalar. The two scripts that handle the
big history in production - `collect-metrics.sh` and `health-anomaly.sh` - already use
`--slurpfile`. So the defect was real but contained to one non-production script.

**Withdrawn recommendation:** I had listed `.gitattributes` as a small win. It is not -
setting `eol=lf` repo-wide would renormalise every tracked file on the next checkout.

---

### 2026-10-04: Multipath stays opt-in by default

**Agent:** NetEng + Architect
**Status:** Accepted (standing decision)
**Rationale:** multipath remains default-off. Game traffic is a low-rate single UDP flow
per game server, so doubling the paths buys little on the traffic this product actually
carries while adding bandwidth overhead on every packet and extra load on relays that run
on always-free capacity - a trade that pays off for bulk-transfer VPNs, not for game
sessions. It stays a deliberate opt-in rather than a default.
**Impact:** no default changes. The protocol and the token store already support up to
three simultaneous paths, so users who want the redundancy opt in via `route.multipath`
(a bool, `#[serde(default)]` -> false) and `route.multipath_max_paths` in `RouteConfig`
(`client/src/config.rs`). Clients that never set these keep exactly today's single-path
behaviour.
**Alternatives Considered:** turning multipath on by default was rejected for the bandwidth
and relay-load cost above; removing the capability outright was rejected because the paths
are already implemented and are useful to the operators who explicitly ask for them.
**Verification:** grep of this entry (date plus rationale) in `wat/state/decisions.md`.

