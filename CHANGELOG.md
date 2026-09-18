# Changelog

All notable changes to LightSpeed will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **Linux and macOS: kernel redirect rules are now removed on shutdown.** `--start-interceptor` and `--watch` released the nftables table / pf anchor only if the background task happened to finish before the process exited; on Ctrl+C the process exited first, leaving the redirect rule installed (which kept hijacking the game server address to a closed port). Both backends now acknowledge teardown, and `stop_and_wait` blocks until the rule is gone.
- **Linux: `--start-interceptor` fails fast without root.** It previously logged a warning, then ran forever with no rule installed (`nft` reports "Operation not permitted"), so the session looked active while nothing was intercepted. `start()` now probes `CAP_NET_ADMIN` and returns "kernel interception needs root or CAP_NET_ADMIN; re-run with sudo".
- **`--check` no longer reports a dead proxy as reachable.** The proxy check only tested `send_to`, which always succeeds for UDP; it now sends a keepalive and waits up to 1s for the proxy echo, and fails the check when none arrives.
- **GUI: the default window is tall enough to show "BOOST MY GAME".** At the previous 460x420 default the primary action was clipped with no scroll, so it was unreachable without resizing. The default is now 520x660.
- **Windows GUI: startup failures are no longer silent.** The GUI is built with `windows_subsystem = "windows"`, so a panic or a returned error produced no window and no output. It now installs a panic hook that writes `%LOCALAPPDATA%\Lightspeed\gui-crash.log` and shows a native message box, initializes logging before the single-instance guard (falling back to a temp file or a discard sink when the log path is unwritable), and reports a fatal startup error in a dialog instead of exiting silently (issue #59).
- **Windows GUI: the single-instance guard was hardened.** `SetLastError(0)` is called before `CreateMutexW` so a stale last-error cannot be mistaken for `ERROR_ALREADY_EXISTS`; the "already running" path logs and exits with a distinct code and a topmost notice; `--force` or `LIGHTSPEED_GUI_FORCE=1` bypasses the guard for diagnostics.
- **Windows GUI: a failed system tray no longer kills or strands the app.** Tray creation is fallible; when it fails the window still opens and closing it exits instead of hiding with no way back. The eframe renderer can be overridden with `LIGHTSPEED_GUI_RENDERER=glow|wgpu` for machines where wgpu adapter creation fails.
- **Windows: WinDivert handles are now closed on every stop path.** `--watch` had no Ctrl+C handling, the receive thread parked in a blocking `WinDivertRecv` that an `AtomicBool` could not wake, and the legacy GUI redirect backend never closed its handles at all. Handles are now owned directly, unblocked with `WinDivertShutdown`, closed after an explicit owner-thread acknowledgement, and the GUI's Quit waits (bounded) for teardown. This addresses the recurring `FWP_E_IN_USE` (0x8032000A) relaunch failure (issue #59).
- **Windows: actionable `FWP_E_IN_USE` message** when a handle open fails, and firewall rule removal is covered by the teardown acknowledgement so it is no longer left behind on a graceful stop.

### Documentation
- Corrected the `FWP_E_IN_USE` troubleshooting guidance (the earlier "CLI Ctrl+C handler" claim was wrong for `--watch`) and corrected the single-instance behaviour description.
- Fixed the `--live-test` echo flag in the CLI reference (`--echo-server`, not `--echo-addr`).
- Registered **Project Zomboid** (`--game zomboid`, UDP 16261-16262) in the supported-games table and README (PR #72).

### Dependencies
- base64 0.22 to 0.23 (PR #61) and the patch group: toml, clap, quinn, rcgen, eframe, reqwest (PR #73).

## [1.4.3] - 2026-09-16

### Security
- **rustls 0.23.45** (RUSTSEC-2026-0285, GHSA-2mjx-qc3c-rqvc): rustls accepted TLS 1.3 handshake messages sent at the wrong encryption level when they followed a key change in the same record. The handshake transcript stays authenticated, so a network attacker could not alter or complete a handshake, but the QUIC control plane now rejects those messages. Updates rustls 0.23.43 to 0.23.45 and rustls-webpki 0.103.13 to 0.103.15.

### Fixed
- **Linux: game server discovery was silently broken on modern distributions.** `ss -unp` no longer prints a State column on iproute2 7.x, so the socket parser skipped every line and reported no routes, and the `/proc/net/udp` fallback was never reached. The scanner now uses `ss -unp -a`, parses each line independently of the optional State column, and falls back to `/proc/net/udp` when `ss` yields nothing. Without this fix, Linux interception never found the game's server.
- **Linux: removed a game-traffic blackhole.** The interceptor used to install a redirect rule matching `0.0.0.0` or a port range and then tunnel to the post-NAT destination, which resolves to `127.0.0.1`. The relay rejects private destinations, so matched traffic was silently dropped. The interceptor now waits, installs no rule, and lets traffic flow normally until a real server is known.
- **Linux: the packet receive thread no longer busy-spins at 100% CPU while idle.** It blocks in `poll(2)` with a short timeout, and transient receive errors are retried with bounded backoff instead of tearing the interceptor down. The final error is surfaced through the status.
- **Linux: follow a rotating game server.** The interceptor re-scans the game's UDP routes every 5 seconds and moves the exact-IP redirect rule to the new server, so a lobby-to-match rotation no longer leaves the match untunneled. The move is gated conservatively (new server reported, old server absent, old flow silent, across two consecutive scans) so an established connection is never misrouted.
- **Linux: shutdown removes the rule that is actually installed** rather than only the one known at startup, so a redirect rule can no longer be left behind to hijack game traffic after the client exits.

### Changed
- **Linux `--smoke-test` is now a real end-to-end test.** It drives a synthetic game process over a never-routed TEST-NET address through a mock relay and asserts the full path: kernel redirect, tunnel, response injection, server rotation with no misrouting, teardown, and no idle CPU spin. Previously it only checked that an nftables rule appeared and disappeared.

## [1.4.2] - 2026-09-16

### Fixed
- **Windows GUI tray Quit**: choosing Quit from the tray menu now terminates the process cleanly instead of leaving a zombie behind.
- **Single-instance guard**: launching the GUI a second time now shows an "already running" notice and exits instead of stacking another instance.
- **Zero-config relay discovery in the GUI**: the GUI now performs the same registry discovery as the CLI and lists the real community relays, instead of showing loopback placeholders.
- **GUI config persistence**: settings now persist across restarts, and a reset-to-defaults action restores the shipped defaults.
- **GUI diagnostics**: the status view now reports QUIC/auth registration state and per-relay packet counters, so a failed registration or a stalled relay is visible instead of silent.
- **GUI game list**: the game picker now covers every supported game profile.
- **Fortnite dynamic-server re-detection**: when a match ends and the lobby rotates to a new server, the interceptor now re-detects the new server instead of stalling on the old one.
- **Windows interceptor resilience**: transient WinDivert receive errors are retried instead of tearing the interceptor down.
- **`--probe-proxies`**: now performs a single discovery/probe pass and prints a visible report, instead of repeating the pass or exiting silently.
- **Docs**: corrected flag drift and refreshed stale references.

### Added
- **Example `lightspeed.toml`**: a documented example config ships with the release.
- **Windows CLI build**: a Windows command-line zip is now published alongside the GUI (unsupported; the GUI is the recommended Windows path).
- **Per-OS install guides**: dedicated Windows, macOS, and Linux quick-start guides.

### Tooling
- **Website**: the network section now renders per-relay packet counters (relayed and dropped) from the health snapshot.
- **`network-stats.sh`**: the health snapshot now includes each relay's `sessions_created` count.

## [1.4.1] - 2026-09-14

### Fixed
- **Zero-config relay discovery (`--probe-proxies`)**: the probe/report path now falls back to the compiled-in community registry URL and operator key (like the normal tunnel path does), so a default-config client lists all community relays instead of reporting none.
- **All discovered relays are used**: registry discovery results now feed the rerouting/multipath server list, so a zero-config client can use every relay instead of only the first selected one.
- **Probe accuracy**: relay probes now honor the configured QUIC/control port (previously hardcoded to 4433) and report each relay's real `node_id`.
- **Registry test isolation**: the registry fetch tests now use an isolated rollback-state path (`LIGHTSPEED_REGISTRY_STATE`), so a real `~/.lightspeed-registry-state` left by a live fetch no longer makes them fail.

### Changed
- **Consistent relay naming**: community node IDs are now `relay-*` everywhere (registry, provisioning scripts, docs). The published registry lists `relay-lax-1`, `relay-ewr-1`, `relay-sgp-1`, `relay-fra`, and `relay-nrt`.
- **Website**: the landing page now shows live, network-wide relay stats generated from relay health checks (online count, version, uptime, last checked) instead of a single tester's personal RTT numbers.

### Tooling
- **`network-stats.sh`**: reads the signed registry, probes each relay's `/health`, and writes `web/network-stats.json`; the Pages workflow runs it on every deploy and every 6 hours.
- **`deploy-all.sh`**: fixed an unclosed quote that made the script fail `bash -n`.

## [1.4.0] - 2026-09-13

### Added
- **Community relay network**: 5 sponsor-funded relays (Los Angeles, New Jersey, Singapore, Frankfurt, Tokyo) form a global, community-discoverable network.
- **Zero-config registry discovery**: the client now fetches and verifies a signed community registry and auto-probes relays when no proxy is configured; the registry URL and operator key are compiled into the client.
- **Roblox** game profile: `--game roblox`, high-ephemeral UDP range (49152-65535), process `RobloxPlayerBeta.exe`, Byfron (Hyperion) anti-cheat.
- **Bodycam** game profile: `--game bodycam`, peer-to-peer over Steam (no dedicated servers), no anti-cheat, process `Bodycam-Win64-Shipping.exe` (Steam App ID 2406770).

### Fixed
- **GUI console window** (#68): the Windows GUI no longer opens a console window (`windows_subsystem = "windows"`).
- **Start Menu shortcut** (#67): the GUI MSI installer now creates a Start Menu shortcut.

### Changed
- Website: added a Global Network section and refreshed stale copy (v1.4.0, 17 games, 200+ tests).
- Docs: the community relay network is now documented as live, with the signed registry URL and the zero-config discovery flow.

## [1.3.2] - 2026-08-31

### Security
- **Proxy response-source validation**: the relay's outbound response listener now drops datagrams whose source is not the game server, closing the off-path injection vector on the proxy side.
- **Control-connection cap**: concurrent QUIC control connections are now bounded (previously only `max_clients` at registration was enforced), so idle pre-registration connections can't exhaust resources.
- **Open-relay warning**: the proxy warns on startup when `destination_allowlist` is empty, flagging the forward-to-anything behavior for shared relays.
- **Collision-free session IDs**: session IDs now retry on collision instead of silently overwriting an existing session.
- **Registry SSRF guard**: the client requires https, enforces a 10s timeout, and rejects private/internal IP hosts for the registry URL.
- **Registry cert pre-pinning**: registry nodes can publish a `cert_fingerprint`; discovery pre-pins each relay's control-plane cert against the registry's value, upgrading trust-on-first-use to trust-from-start for registry-discovered relays.
- **Binary checksum verification**: `provision-oci.sh` verifies the proxy binary against the release `.sha256` before installing.
- **Registry worker input validation**: the worker validates node fields and caps payload size before mutating the registry.
- **ML model size cap**: model files above 128MB are rejected to prevent memory exhaustion.

## [1.3.1] — 2026-08-31

### Security
- **Control plane no longer MITM-able.** The client used `SkipServerVerification` (accepted any TLS certificate), so an on-path attacker could impersonate the proxy and own the control plane. It now pins the proxy's self-signed certificate fingerprint on first connect and rejects any change (SSH `known_hosts` model), and verifies the TLS handshake signature so a peer must hold the private key matching the pinned cert. Pins persist per-address in `~/.lightspeed-proxy-fingerprints`.
- **Proxy certificate is now persistent** (`LIGHTSPEED_TLS_DIR`, default `/var/lib/lightspeed/tls`) instead of regenerated per boot, so the pinned fingerprint stays stable. Relay provisioning adds `StateDirectory=lightspeed`.

## [1.3.0] — 2026-08-31

### Security (from the exploitability-driven audit)
- **Session token widened 8 → 32 bits** (protocol v3/v4). The old 8-bit token (256 values, IP-only bound) was brute-forceable in ≤256 packets to inject packets into a victim's session. 4 billion values makes this infeasible. Breaking wire change: old clients/proxies reject each other cleanly.
- **Control-plane session leak fixed** — disconnect cleanup was dead code, so the sessions table leaked forever and eventually exhausted `max_clients`, rejecting all new clients.
- **`require_auth` now defaults `true`** — a config that listed other `[security]` keys but omitted `require_auth` silently disabled auth.
- **Rate-limiter state is now bounded** — `cleanup()` is called so spoofed datagrams can't grow the table unbounded.
- **Off-path response injection closed** — interceptors reject datagrams whose source is not an active relay.
- **Registry rollback protection** — `published_at` monotonicity enforced, so a replayed older registry can't resurrect a revoked node.
- **Privileged tools invoked by absolute path** — `nft`/`iptables`/`pfctl` no longer resolve from PATH as root.

## [1.2.9] — 2026-08-31

### Fixed
- **Probing auth-enabled proxies**: `--probe-proxies` (and route selection) sent keepalive probes with no session token before registering, so auth-enabled proxies rejected every probe and reported "all proxies unhealthy". Keepalives are now answered without auth (they are harmless liveness pings), so probing works before registration.

## [1.2.8] — 2026-08-31

### Added
- **FEC + multipath now coexist**: the client keeps a per-relay FEC decoder and decodes each relay's response stream independently, while the proxy echoes the client's sequence in FEC data responses so duplicates across paths are deduplicated. The FEC/multipath mutual exclusion is removed.

### Changed
- The proxy now echoes the client's sequence in FEC data responses (parity keeps its own counter).

## [1.2.7] — 2026-08-31

### Added
- **Multipath path scoring**: per-relay win/loss tracking with adaptive ordering — relays that consistently lose are demoted, and win rates are logged each re-route cycle.
- **Configurable multipath path count**: `[route] multipath_max_paths` (default 2).

### Changed
- **FEC and multipath are now mutually exclusive**: enabling both disables FEC with a warning, because the proxy's response FEC is per-relay and two relays produce incompatible parity streams.

## [1.2.6] — 2026-08-31

### Added
- **Continuous re-routing**: the client auto-selects the best relay from `[proxy] servers`, re-probes every 30s, and switches mid-session when a meaningfully better path appears. `--start-interceptor` and `--watch` now auto-select from configured servers (previously fell back to localhost).
- **Multipath routing**: spread each packet across the top-N relays with response deduplication (first response wins, duplicates dropped). Enable with `[route] multipath = true`.

### Changed
- The proxy now echoes the client's sequence in non-FEC data responses (instead of a proxy-local counter) so the client has a stable dedup key across relay paths.

## [1.2.5] — 2026-08-30

### Fixed
- **Critical: the release proxy was missing the QUIC control plane.** The proxy's `quic` feature (which serves the control plane on UDP 4433) was not enabled in cargo-dist builds, so every release binary ran with `require_auth = true` but no way for clients to register — rejecting 100% of data-plane traffic in production. Added `features = ["quic"]` to the proxy's `[package.metadata.dist]`.

## [1.2.4] — 2026-08-29

### Relay network (self-hosted proxy mesh)
- **OCI Always-Free provisioning**: `infra/scripts/provision-oci.sh` replaces the placeholder `provision.sh` — one-click provisioning of relays on Oracle Cloud's Always Free tier (idempotent VCN/subnet/IGW/security-list, ARM-first launch with automatic `E2.1.Micro` fallback, cloud-init self-install + Ed25519 node identity).
- **Region guidance**: `infra/README.md` documents which OCI region to pick per game-server region, plus the Always-Free constraints (2 OCPU ARM per tenancy, home-region-only, ~10 TB egress, idle reclamation).
- **ARM64 Linux release**: added `aarch64-unknown-linux-gnu` to the cargo-dist targets so the Always-Free Ampere instances have a prebuilt proxy binary.

### Relay security
- **Destination allowlisting**: `[security] destination_allowlist = ["104.26.0.0/16", ...]` restricts a relay to forwarding only to listed CIDR prefixes — the key defense that makes a community relay safe to open (prevents open-relay DDoS/reflection).

### Community proxy registry
- **Signed node list + revocation**: `client/src/registry.rs` defines the registry format and verifies the operator's Ed25519 signature (`ring`).
- **Operator tooling**: `client/examples/sign_registry.rs` signs a node list offline; `infra/registry/worker.js` is a reference Cloudflare Worker for serving the signed list.
- **Client discovery**: `--probe-proxies --registry <url>` (or `[registry] url` in config) fetches, verifies, and probes community relays alongside configured proxies.

## [1.2.3] — 2026-08-29

### Critical fix: data-plane auth rejected all traffic (issue #59)
- **Root cause:** the client's `register_session()` created a throwaway QUIC control connection that closed immediately after registration. The proxy revokes data-plane authorization when the control connection closes, so the token was revoked before any game traffic flowed — the proxy rejected 100% of data-plane packets (`auth_rejections` spiked, `sessions_created` stayed 0).
- **Fix:** the client now keeps the control connection alive for the process lifetime (background keepalive ping every 15s), so the data-plane token stays authorized.

### Windows: WinDivert handle leak → `FWP_E_IN_USE` (issue #59)
- The `windivert` crate has no `Drop` impl, and LightSpeed never called `WinDivert::close()`, so the WFP filter/callout was never unregistered on shutdown. The interceptor now explicitly closes its capture and inject handles on stop and on the inject-open failure path, preventing stale WFP state that caused `FWP_E_IN_USE` (0x8032000A) on subsequent runs.
- Documented the remaining WinDivert 2.2.x driver limitation (hard-kill leaves stale WFP state until a full shutdown) in `docs/troubleshooting.md`.

### New game profile
- **Dead by Daylight** (issue #51): `--game deadbydaylight` — UDP 27000–27050, EAC-compatible, process `DeadByDaylight-Win64-Shipping.exe`.

### Docs: client vs GUI clarification (issues #50, #58)
- The README and user guide now lead with a "which file do I download?" table: Windows players download `lightspeed-gui` (standalone — it already embeds the client), Linux/macOS players use `lightspeed-client`, and only self-hosters need `lightspeed-proxy`. You never need both the client and the GUI.

## [1.2.2] — 2026-08-28

### Windows GUI WinDivert fix
- The `lightspeed-gui` MSI + ZIP now bundle `WinDivert.dll` + `WinDivert64.sys` next to the GUI exe (same fix as the client in 1.2.1). Fixes the "WinDivert.dll not found" error when launching the GUI (issues #50, #58).

## [1.2.1] — 2026-08-28

### Windows interception fix
- **WinDivert bundled in the Windows release**: the cargo-dist build now enables the `windivert-redirect` feature and ships the official WinDivert 2.2.2 `WinDivert.dll` + signed `WinDivert64.sys` next to the exe. Fixes the v1.2.0 "unsupported — WinDivert requires the 'windivert-redirect' Cargo feature" error and the missing-DLL startup failures (issues #50, #58).

### Dependencies
- rcgen 0.13→0.14, pcap 2.4→2.5, Rust (Docker base) 1.88→1.98.

## [1.2.0] — 2026-08-18

### TCP Tunnel (issue #10)
- The client↔proxy leg can now run over TCP for networks that block or limit UDP. Length-prefixed framing preserves packet boundaries, and the same auth / rate-limit / abuse / FEC pipeline applies. `--tcp` flag plus `tunnel.transport` config; `TCP_NODELAY` on both ends.

### Configurable Ports (issue #39)
- Proxy ports (data / control / health) are configurable in a `[network]` section of `proxy.toml`, with the `--data-bind` / `--control-bind` / `--health-bind` CLI flags as optional overrides.

### Known issue
- **glib advisory (medium, GUI-only)**: `glib` 0.18.x has an unsoundness in `VariantStrIter` (patched in 0.20.0). Blocked on a tray-icon/libappindicator upgrade to gtk-rs 0.20; tracked upstream at tauri-apps/tray-icon#356.

## [1.1.0] — 2026-08-18

### Security
- **Token authentication**: the client now registers over the QUIC control plane and stamps its session token into every data-plane packet header. `require_auth` now defaults to `true`, so unregistered clients are rejected.
- **`quic` in release builds**: the Docker image and cargo-dist release binaries build the `quic` feature so registration works out of the box.

### Known issue
- **glib advisory (medium, GUI-only)**: `glib` 0.18.x has an unsoundness in `VariantStrIter` (patched in 0.20.0). Blocked on a tray-icon/libappindicator release that adopts gtk-rs 0.20; the GUI never uses the affected iterator.

## [1.0.0] — 2026-08-18

### Installer Wizard
- **cargo-dist v0.32.0**: shell, PowerShell, and MSI installers with free Sigstore/GitHub attestations.
- **Cross-platform matrix**: client + proxy for Linux, Windows, and macOS (x86_64 + Apple Silicon); GUI for Linux and Windows.

### Self-Hosted Proxy Model
- **`docs/deploy-proxy.md`**: expanded into a full self-hosting guide, replacing the earlier "community proxy network" idea.

### New Game Profiles
- MapleStory (`--game maplestory`), Genshin Impact (`--game genshin`), Rocket League (`--game rocketleague`), World of Tanks (`--game wot`).

### Security
- **Grafana**: removed the weak default admin password (now requires `GRAFANA_ADMIN_PASSWORD`).
- **Auth caveat documented**: `require_auth` stays `false` by default; the client does not yet stamp session tokens into data-plane packets, so full token auth is deferred.

### Dependencies
- **egui**: eframe 0.35→0.36 and egui_plot 0.36→0.37 (GUI MSRV is now Rust 1.95).

## [0.5.1] — 2026-08-01

### CI Fixes
- **Cross-platform builds**: Added `#[cfg(target_os)]` guards to interceptor modules — fixes macOS and Windows compilation
- **Windows GUI Build**: Moved `windivert` dependency to correct Windows target; removed `pcap-capture` from GUI deps
- **Security**: Updated `crossbeam-epoch` 0.9.18→0.9.20, `quinn-proto` 0.11.14→0.11.16
- **cargo-deny**: Added RUSTSEC-2026-0190, RUSTSEC-2026-0192 ignores; added CC0-1.0, Ubuntu-font-1.0 licenses
- **Benchmarks**: Removed broken `--save-baseline` flag causing CI failures
- **Format**: Fixed trailing newline in interceptor module

### Documentation
- **CHANGELOG**: Restored all historical version sections (0.4.0–0.1.0) that were truncated
- **README**: CLI reference table updated for v0.5.0

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

## [0.4.0] — 2026-04-27

### Added (2026-04-27) — Item H: Windows GUI / tray app

Native Windows GUI client: system-tray icon + egui status window.

- **`client-gui/`** — new `lightspeed-gui` workspace crate (Windows-only binary).
- **`client/src/engine.rs`** — `LightSpeedEngine` / `EngineStatus`: background async keepalive loop driven from a non-Tokio GUI thread via a `Handle`. Sends keepalives every 5 s, measures RTT, maintains rolling 120-sample history.
- **`client/src/lib.rs`** — `lightspeed_client` library target; re-exports `LightSpeedEngine` and `EngineStatus` for GUI consumption.
- **`client-gui/src/main.rs`** — builds a dedicated multi-thread Tokio runtime, auto-connects to the LAX proxy, launches eframe.
- **`client-gui/src/app.rs`** — `LightSpeedApp` implements `eframe::App`: tray icon, RTT sparkline, connect dialog, 1 Hz repaint.
- **`Cargo.toml`** (workspace) — added `"client-gui"` member.
- **`.github/workflows/ci.yml`** — all Linux/macOS `cargo` commands now carry `--exclude lightspeed-gui`.

### Added — Item G: Opt-in latency telemetry

Anonymous, aggregated network-quality reporting — **off by default**, enabled with `--telemetry`. No PII is ever transmitted.

- **`protocol/src/telemetry.rs`** — `TelemetryReport` struct with PII regression test.
- **`client/src/telemetry.rs`** — `TelemetryCollector` ring-buffer, HTTP/1.0 POST, periodic 15-min flush.
- **`proxy/src/health.rs`** — `POST /telemetry` handler on `:8080`.
- **`proxy/src/metrics.rs`** — `lightspeed_telemetry_reports_total` Prometheus counter.

### Performance — WF-008: zero-alloc FEC + CI coverage
- **`TunnelHeader::encode_to_array()`** — stack-based header encode, ~7× faster.
- **Compact FEC parity** — `(max_payload_len + 2)` bytes instead of fixed 1400 B.
- **`recvmmsg` batched inbound** — 32-datagram batches on Linux, 5–10× pps improvement.

### Added — WF-007: 9-game support + macOS CI
- **Overwatch 2**, **League of Legends**, **PUBG** game profiles.
- **macOS CI smoke test** job in CI pipeline.

### Deployed — v0.4.0-dev promoted to production
- Both Vultr nodes (proxy-lax, relay-sgp) updated. Health checks passing.

## [0.3.0] — 2026-03-20

### What's New in v0.3.0
- **FEC (Forward Error Correction)**: XOR-based parity with encode/decode, 5 tests.
- **WARP integration**: Cloudflare WARP IP detection and routing logic.
- **Multi-proxy support**: Multiple proxy servers with failover.
- **Keepalive mode**: Continuous tunnel keepalive with RTT measurement.
- **UDP redirect mode**: Full UDP redirect with iptables/nftables integration.
- **Linux ARM64 support**: Cross-compilation for ARM64 targets.
- **Documentation**: Protocol spec, architecture, security audit, test reports.

## [0.2.0] — 2026-02-23

### Beta Release: Live Infrastructure Verified
- **Vultr deployment**: proxy-lax (US-West) and relay-sgp (Singapore) nodes live.
- **Tunnel relay**: End-to-end UDP tunnel verified across all nodes.
- **FEC module**: 8 tests passing.
- **WARP routing**: Unit tested.
- **UDP redirect**: Tested with game traffic simulation.
- **Infrastructure research**: ISP path analysis, relay strategy, ExitLag gap analysis.

## [0.1.0] — 2026-02-22

### Initial MVP Release

The first release of LightSpeed — a zero-cost, open-source global network optimizer for multiplayer games.

#### Client (`lightspeed`)
- UDP Tunnel Engine with Tokio, keepalive, stats, timeout handling.
- 20-byte binary tunnel header with encode/decode, session tokens, sequence numbers.
- QUIC Control Plane for proxy discovery and health checks.
- Game Profiles for Fortnite, CS2, Dota 2.
- Route Selection Framework with nearest-proxy, multipath, failover.
- ML Route Prediction with 11 features, Random Forest via linfa, heuristic fallback.
- Packet Capture Abstraction (Windows/Linux/macOS).
- TOML-based config with CLI overrides via clap.

#### Proxy Server (`lightspeed-proxy`)
- UDP Relay Loop with concurrent client support.
- Session Management with token-based auth and automatic timeout.
- Rate Limiting per-IP and per-session.
- Abuse Detection: destination validation, amplification prevention, private IP blocking.
- Prometheus-compatible metrics endpoint.
- HTTP health check endpoint.
- QUIC Control Server.

#### Protocol (`lightspeed-protocol`)
- 20-byte binary tunnel header format.
- Binary-encoded control messages.

#### Testing
- 52 tests total, 100% pass rate.
- E2E tunnel lifecycle, concurrent relay, security integration tests.
- Performance: 162μs tunnel overhead, ≤5ms target achieved.

#### Security
- Token-based session authentication.
- Per-IP/session rate limiting.
- Destination validation (blocks private, localhost, multicast).
- Amplification prevention.
- No Critical or High audit findings.

[Unreleased]: https://github.com/ShibbityShwab/lightspeed/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/ShibbityShwab/lightspeed/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.4.2...v0.5.0
[0.4.2]: https://github.com/ShibbityShwab/lightspeed/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/ShibbityShwab/lightspeed/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ShibbityShwab/lightspeed/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ShibbityShwab/lightspeed/releases/tag/v0.1.0