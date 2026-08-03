# LightSpeed System Rules

> **Canonical rules file for the LightSpeed WAT autonomy system.**
> All agents MUST read and comply with these policy stubs.
> Last updated: 2026-05-25

---

## [COST_STUB] — Zero Cost Mandate

**Total ongoing infrastructure cost MUST remain exactly $0.00.**

- Use only Always Free tier resources (any Always Free tier provider).
- No paid services, no usage-based billing that could exceed $0.00.
- CI/CD must not introduce paid services (blocks paid Codecov plans, etc.).
- Infrastructure must be self-hostable on free-tier resources.

---

## [SAFETY_STUB] — No Harmful Operations

- No DDoS amplification vectors. Proxy must validate destination IPs.
- No open relay abuse. Only authenticated sessions may relay traffic.
- Proxy rate-limits per-client to prevent bandwidth abuse.
- Human approval required for destructive operations (infrastructure teardown, data deletion).
- Anti-reflection protection: limit unique destinations per client per time window.

---

## [SECURITY_STUB] — Anti-Abuse, Authentication, Rate Limiting

- Session tokens required for data-plane authentication.
- Rate limiting enforced per-client (PPS and BPS limits).
- Private IP blocking: proxy must not forward to RFC 1918, localhost, multicast, link-local.
- Anti-amplification: proxy tracks inbound/outbound byte ratio.
- QUIC control plane uses mTLS for registration and token distribution.
- No secrets in source code. Use environment variables or `.env` (gitignored) for credentials.

---

## [TRANSPARENCY_STUB] — Unencrypted Tunnel, No Packet Manipulation

- Tunnel is unencrypted by design — game traffic remains inspectable (anti-cheat compatible).
- No payload modification. Packets are forwarded byte-for-byte.
- Original IP addresses preserved in tunnel header. Game servers see real user IP.
- Protocol is documented and open for inspection.

---

## [QUALITY_STUB] — Tests Required, Clippy Clean, Documented APIs

- All new code must have unit tests covering the primary code paths.
- Integration tests required for cross-module and cross-crate interactions.
- `cargo clippy --workspace --all-targets --all-features` must pass with zero warnings.
- `cargo test --workspace --all` must pass with zero failures.
- All public APIs must have doc comments (`///` for items, `//!` for modules).
- Run `cargo fmt` before committing.

---

## [ETHICS_STUB] — No Unfair Advantage, Honest Benchmarking

- LightSpeed must not provide an unfair advantage in competitive gaming.
- The tool is a network optimizer, not a cheat or hack.
- Benchmark comparisons must use reproducible, documented methodology.
- Performance claims must be backed by measured data, not marketing.
- Do not interfere with anti-cheat systems (EasyAntiCheat, VAC, BattlEye).

---

## [PRIVACY_STUB] — No PII Collection, IP Preservation

- No collection of personally identifiable information (PII).
- Telemetry is opt-in only and limited to aggregated latency metrics (p50/p95/p99, jitter, FEC stats).
- No IP addresses, user identities, or game account data in telemetry.
- Original user IP must reach game servers (transparent tunnel, not a VPN/anonymizer).
- Privacy policy must be publicly documented and accessible.

---

## Evolution

Rules may be added, modified, or deprecated as the project evolves. Changes to this file must be:
1. Proposed via a PR or documented decision.
2. Logged to `wat/state/decisions.md`.
3. Reviewed by at least one other agent or human contributor.
