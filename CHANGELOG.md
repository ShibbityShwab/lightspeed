# Changelog

All notable changes to LightSpeed will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.1] — 2026-05-02

### WAT Modernization
- **AGENTS.md**: Created as industry-standard canonical AI agent instructions file (recognized by Cursor, Copilot, Windsurf, etc.)
- **`.clinerules`**: Slimmed to Cline compatibility hook that delegates to AGENTS.md
- **`.clineskills/`**: Created `@lightspeed` autonomy loop skill and `@debug` iterative test-fix loop skill
- **`wat/`**: Archived 7 static reference files to `wat/archive/` (agents.md, workflows.md, tools.md, autonomy-loop.md, workspace.md, mcp-integration.md, project-goals.md)
- **`wat/TASK.md`**: Created structured task definition template with success criteria, test commands, and rollback
- **`wat/rules.md`**: Added AGENTIC VERIFICATION section — must pass tests+clippy before claiming completion, 3-attempt escalation rule
- **Deleted model-specific stubs**: `.geminirules`, `.antigravityrules`, `wat/run-gemini.txt`, `.agents/` directory

### CI Improvements
- **Security audit**: Added `cargo-audit --deny warnings` job to CI pipeline
- **Windows build/test**: Added `windows-latest` job for full build + test coverage on primary target
- **Benchmark baseline**: Added `cargo bench` job with criterion baseline capture + artifact upload

### Documentation
- **Gamer docs**: Added `docs/glossary.md`, `docs/user-guide.md`, `docs/faq.md`, `docs/troubleshooting.md`, `docs/supported-games.md`
- **Capture mode**: Added `docs/capture-mode-limitations.md` documenting architectural limitations

### Chores & Maintenance
- Initial `proxy/proxy.toml` configuration file
- E2E test tool: `tools/echo28015.py`, `tools/rust_traffic_sim.ps1`, `tools/start_echo.sh`

## [0.5.0] — 2026-08-01

### Linux Interceptor (WF-010—WF-013)
- **Recvmsg refactor**: Replaced Tokio recv_from with raw recvmsg + CMSG capture in dedicated thread. Enables IP_RECVORIGDSTADDR for future kernel-level auto-detect.
- **Debounce auto-detection**: Ported Windows-style debounce logic to Linux interceptor. Tracks candidate server addresses and commits when ≥3 packets arrive in ≤1.5s.
- **`--watch` mode**: Auto-starts interceptor when game process is detected. Stops and resumes watching when game exits. Zero-config flow.
- **`--benchmark` mode**: Direct vs LightSpeed latency comparison with 10 probes, avg/min/max table, and improvement percentage.
- **`--smoke-test` mode**: Full E2E validation — echo server + proxy + interceptor + nftables rule install/cleanup.
- **`--status` mode**: System overview showing version, OS, interceptor backend, root status, running games, nftables rules.
- **`--demo` mode**: Interactive architecture walkthrough with platform detection, game profile, and projected latency table.
- **`--list-games`**: Display all 9 supported games with port ranges and process names.
- **`--write-config`**: Generate a documented `lightspeed.toml` template.
- **`--check` mode (client)**: Environment validation — interceptor, root, game detection, proxy reachability.
- **`--check` mode (proxy)**: Config parsing + port bindability validation.
- **Startup banner**: Running `lightspeed` with no args shows a friendly quick-start banner instead of entering keepalive mode.

### Proxy
- **`--dev` flag**: Skips destination IP validation for local testing.
- **Docker deployment**: Multi-stage Dockerfile + docker-compose.yml for single-node or mesh deployment.
- **`scripts/build-release.sh`**: Stripped release archive builder.
- **`.dockerignore`**: Faster Docker builds.

### Dependencies
- **Recvmsg refactor**: Replaced Tokio recv_from with raw recvmsg + CMSG capture
- **Port-range fallback**: Interceptor now starts without a pre-discovered game server route. Uses nftables `udp dport {range}` match when game isn't running.
- **SO_ORIGINAL_DST recovery**: Retrieves real destination address from netfilter-redirected packets via `getsockopt(fd, SOL_IP, SO_ORIGINAL_DST)`.
- **MockInterceptor**: In-memory `TrafficInterceptor` for CI testing — no root needed.
- **block_on panic fix**: Replaced `tokio::runtime::Handle::block_on` with `std::net::UdpSocket` bind in all three interceptor backends.

### CLI
- `--intercept` — diagnostic mode: shows backend, availability, discovered routes
- `--scan-processes` — ProcessScanner debug: find game processes and UDP routes
- `--start-interceptor` — live MITM mode with graceful Ctrl+C shutdown
- `--server-addr` — override game server for testing without a running game
- `--list-games` — display all 9 supported games with ports and process names
- `--write-config` — generate a documented `lightspeed.toml` template
- `--check` — environment validation: interceptor, root, game, proxy reachability

### Dependency Upgrades
- linfa-linear 0.7→0.8, ndarray 0.15→0.16 (fix PR #14 regression)
- bytes 1→1.12, tracing-subscriber 0.3→0.3.23, rand 0.8→0.9, thiserror 1→2
- libc 0.2 (Linux-only, for SO_ORIGINAL_DST)

### Cross-Platform GUI (PR #20 — @CiroBurro)
- `Platform` trait abstracts OS-specific code (tray, fonts, port detection, admin)
- `LinuxPlatform` (156 LoC): stub tray, Noto Color Emoji, pgrep+ss, pkexec
- `WindowsPlatform` (317 LoC): full tray icon, Segoe UI Emoji, Npcap, UAC
- Proxy Manager UI: add/remove/list proxies at runtime
- Fix: hardcoded log path crash → `dirs::data_local_dir()`
- Fix: placeholder IP crash → `LIGHTSPEED_PROXIES` env var
- egui 0.35 / eframe 0.35 API migration

### Documentation
- `docs/deploy-proxy.md`: Vultr/Oracle quickstart, systemd service, multi-node mesh
- `docs/CLI-REFERENCE.md`: Full CLI reference table
- README quickstart updated for v0.5.0 CLI

### Housekeeping
- 77→0 clippy warnings (crate-level allows for planned API surface)
- 5 stale dependabot PRs closed
- 185→200 tests

## [0.4.2] — 2026-07-29

### CI Fixes
- **Release workflow**: Fixed malformed YAML, orphan `with:` block, and reordered steps
- **Feature gate**: Excluded `windivert-redirect` from full feature set for Linux compatibility
- **Format check**: `cargo fmt --all` for CI compliance
- **E2E test**: Fixed proxy/echo test step in CI pipeline
