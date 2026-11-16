# LightSpeed System Rules

> **Canonical rules file for the LightSpeed WAT autonomy system.**
> All agents MUST read and comply with these policy stubs.
> Each stub is a testable acceptance criterion, not a slogan.
> Last updated: 2026-11-16

---

## [COST_STUB] — Zero Owner Spend

**The owner's ongoing infrastructure cost MUST remain exactly $0.00.**

- Use only Always Free tier resources and sponsor-funded community relays.
- No paid services, no usage-based billing that can exceed $0.00, and no trial
  that converts to paid.
- CI/CD must not introduce paid services (blocks paid Codecov plans, paid
  scanning, paid runners).
- Infrastructure must stay self-hostable on free-tier resources.

*History:* the 2026-09-13 decision (see
`wat/state/decisions/2026-09.md`) superseded the earlier "no funded relays at
all" reading: the eight community relays are funded by sponsors, not the owner.
The binding constraint is the owner's spend, and it is still $0.00.

---

## [SAFETY_STUB] — No Harmful Operations

- No DDoS amplification vectors. The proxy validates destination IPs.
- No open relay abuse. Only authenticated sessions may relay traffic.
- The proxy rate-limits per client to prevent bandwidth abuse.
- Human approval is required for destructive operations: infrastructure
  teardown, data deletion, key rotation that invalidates live sessions.
- Anti-reflection: limit unique destinations per client per time window.

---

## [SECURITY_STUB] — Anti-Abuse, Authentication, Rate Limiting

- Session tokens are required for data-plane authentication.
- Rate limiting is enforced per client (PPS and BPS limits).
- Private-destination blocking: the proxy must not forward to RFC 1918,
  loopback, link-local, multicast, or IPv4-mapped IPv6 forms of those.
- Anti-amplification: the proxy tracks the inbound/outbound byte ratio.
- The QUIC control plane uses mTLS for registration and token distribution.
- No secrets in source code. Environment variables or `.env` (gitignored) only.

---

## [TRANSPARENCY_STUB] — Unencrypted Tunnel, No Packet Manipulation

- The tunnel is unencrypted by design; game traffic stays inspectable and
  anti-cheat compatible.
- No payload modification. Packets are forwarded byte-for-byte.
- Original IP addresses are preserved in the tunnel header; game servers see the
  real user IP.
- The protocol is documented and open for inspection.

---

## [QUALITY_STUB] — Tests, Lints, Docs

- New code has unit tests on primary paths; cross-crate changes have integration
  tests.
- The gate is CI parity, run from PowerShell on Windows:

  ```powershell
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
  cargo test --workspace --all --exclude lightspeed-gui
  # Windows-only GUI crate (excluded above, linted nowhere else in CI):
  cargo clippy -p lightspeed-gui --all-targets
  cargo test -p lightspeed-gui
  # When the WinDivert module changed:
  cargo test -p lightspeed-client --features windivert-redirect
  ```

- Zero warnings, zero failures. A filtered run does not satisfy this stub.
- All public APIs have doc comments (`///` for items, `//!` for modules).
- `cargo fmt` before reporting. `cargo deny check` and `cargo audit` are
  separate installations (`cargo install cargo-deny cargo-audit`) and are owned
  by CI's security workflow; run them locally when installed.

---

## [ETHICS_STUB] — No Unfair Advantage, Honest Benchmarking

- LightSpeed is a network optimizer, not a cheat. It must not provide an unfair
  competitive advantage.
- Benchmark comparisons use reproducible, documented methodology.
- Performance claims are backed by measured data, not marketing.
- Do not interfere with anti-cheat systems (EasyAntiCheat, VAC, BattlEye).

---

## [PRIVACY_STUB] — No PII, IP Preservation

- No personally identifiable information is collected.
- Telemetry is opt-in and limited to aggregated latency metrics (p50/p95/p99,
  jitter, FEC stats).
- No IP addresses, identities, or game account data in telemetry.
- The original user IP reaches game servers (transparent tunnel, not a VPN).
- The privacy policy stays public and accurate.

---

## [SHELL_STUB] — Windows-Native Execution

The development host is Windows. Agents must not need a POSIX emulation layer.

- Run commands in **PowerShell 5.1** (`pwsh` is not installed on this host).
- **No Git Bash, no WSL, no `sh -c`, no `bash script.sh`** as a workaround.
  `git` is a Windows build; use `git` directly, not through a bash shell.
- If a task genuinely needs a POSIX-only artifact (Linux relay scripts,
  `infra/scripts/*.sh`, macOS packaging), treat it as remote/CI work and say so;
  do not port it to the desktop or pretend it ran.
- PowerShell 5.1 has no `??`, no ternary, no `ProcessStartInfo.ArgumentList`;
  prefer cmdlets, `-ErrorAction`, and `$LASTEXITCODE` checks.
- CPython 3.12 is installed at `%LOCALAPPDATA%\Programs\Python\Python312` and
  `py` is on the user PATH; a shell started before the install needs a restart
  to see it. Use `py -3` or `scripts/Localization.ps1`. The Store aliases
  `python`/`python3` are stubs (exit 9009) and are never the interpreter.
- `cmd`/batch files must keep CRLF (`.gitattributes` enforces it); everything
  else is LF in the worktree.

---

## [AGENT_STUB] — DeepSeek Harness Is The Harness

DeepSeek Harness (DSH) is the only supported agent harness for this repository.

- Harness-specific config lives in this repo: `AGENTS.md`, `.dsh/skills/`,
  `wat/`. There is no `.opencode/`, no OmO config, and no second harness to keep
  in sync. Do not reintroduce harness-specific state directories.
- Project skills under `.dsh/skills/` are the procedures. Load the relevant one
  with the `skill` tool instead of re-deriving a procedure from memory:
  `lightspeed-wat`, `lightspeed-team`, `lightspeed-windows`,
  `lightspeed-verify`.
- **Team mode** (Agent Teams) is the multi-agent mechanism; see
  `wat/team-mode.md`. Teammates are ordinary subagents named by persona slug.
  The shared task board is the durable record; the Lead owns integration and the
  final answer.
- Tool use follows DSH: `read`/`write`/`edit` for files (never `cat`/`sed`),
  `grep`/`glob` for discovery (never `findstr`/`Get-ChildItem -Recurse` as a
  search), `pwsh` for commands, `ask_user_question` for owner gates.
- Sandbox and approvals are DSH policy: do not work around a denied operation by
  another route; escalate the exact operation once.

---

## [STATE_STUB] — Durable State Protocol

State is how the next session resumes without re-deriving context.

- `wat/state/current-phase.md` holds **current state plus the newest dated
  entries only**. Older entries live in `wat/state/phases/<YYYY-MM>.md`.
- Every completed round appends one dated entry at the top of
  `current-phase.md` and, when a decision was made, one entry to
  `wat/state/decisions/<YYYY-MM>.md`.
- `wat/state/decisions.md` is the index and the format contract; it is not the
  place to append.
- Evidence is written to `wat/evidence/<YYYY-MM-DD>/` (gitignored) and named in
  the state entry and the task report. A claim with no evidence path is
  unverified.
- Rotation is mechanical: when an entry is older than the current month and no
  longer describes the active workstream, move it to its month file rather than
  deleting it. History is append-only; nothing is rewritten.

---

## Evolution

Rules may be added, modified, or deprecated as the project evolves. Changes to
this file must be:

1. Proposed via a PR or a documented decision.
2. Logged to `wat/state/decisions/<YYYY-MM>.md`.
3. Reviewed by at least one other agent or human contributor.
