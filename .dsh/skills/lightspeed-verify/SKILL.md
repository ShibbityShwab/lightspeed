---
name: lightspeed-verify
description: Use before claiming any LightSpeed change is done or verified, when deciding what evidence a change needs, when a test result is ambiguous, or when reviewing another agent's completion report. Defines the gate commands, evidence paths, mutation checks and the meaning of verified for code, GUI, docs, infra and release work.
---

# What "verified" means here

> Paths in this document are relative to the repository root, not to this skill directory.

A claim is verified when someone else can re-run the check and get the same
result. Anything else is a report of intent.

## The gate (code)

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
```

- GUI: `cargo clippy -p lightspeed-gui --all-targets` and
  `cargo test -p lightspeed-gui` (Windows only; the crate is excluded elsewhere).
- WinDivert module: `cargo test -p lightspeed-client --features windivert-redirect`.
  The feature compiles to nothing without it, so a plain run proves nothing.
- Security: `cargo audit`, `cargo deny check`.
- Record **counts per crate** (`N passed, 0 failed`), not "tests pass".
- Never verify with a module filter: a filtered green says nothing about the rest
  of the crate.

## Bug fixes need a failing first

1. Reproduce: a test, a log, or a measurement that fails on the current tree.
2. Fix.
3. Mutation-check: revert the fix in place, confirm the new test fails, restore,
   confirm it passes. Report both outcomes.

A fix with no reproduction and no mutation check is unverified, even if every
gate is green.

## Evidence

- Path: `wat/evidence/<YYYY-MM-DD>/<task-id>-<slug>.<ext>` (gitignored).
- What counts: command transcript, test output with counts, screenshot of the
  real window, benchmark table with n and window, HTTP/metrics output.
- What does not: a summary written by another agent, an exit code alone, a diff.
- Name the path in the task completion and the state entry.

## Per-area minimums

| Area | Minimum evidence |
|---|---|
| Rust code | Failing-before/passing-after test; fmt/clippy/test counts |
| GUI | Screenshots: narrow (~320 px) + wide, connected + disconnected, boosting + idle |
| Protocol | Wire-compat note + tests for both sides of the change |
| Infra | Health endpoint, metrics counters moving, one log line from the new build |
| Workflow | Local reproduction of the steps + the CI-only steps named |
| Docs | Rendered link check; no stale command that the host cannot run |
| Release | Build artifacts, checksums, changelog body = GitHub release body |

## Independence

The author does not grade their own work. On a team run, `qa-engineer` verifies
`rust-dev`'s fix from the acceptance criteria, not from the author's summary.
The Lead re-runs the gates on the final tree before the final answer.

## Reporting an unverified item

Say what is unverified, why, and what would verify it:

> Unverified: the TLS split under a live match. `--live-test` DNS is blocked by
> the relay allowlist; verification needs the owner to run a real join and
> capture `Relayed` counters.

That sentence is worth more than a green claim nobody can check.
