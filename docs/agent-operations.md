# Agent operations

How LightSpeed is driven by AI agents, and where each piece lives. This page is
for humans reviewing the setup or joining the work; the agent-facing contract is
[`AGENTS.md`](../AGENTS.md).

---

## The harness

The only supported harness is **DeepSeek Harness (DSH)**. Procedures are
**project skills** that DSH discovers from this repository, so they cost nothing
until loaded:

| Skill | Path | Load it when |
|---|---|---|
| `lightspeed-wat` | `.dsh/skills/lightspeed-wat/SKILL.md` | starting/continuing work, the autonomy loop |
| `lightspeed-team` | `.dsh/skills/lightspeed-team/SKILL.md` | multi-agent work with Agent Teams |
| `lightspeed-windows` | `.dsh/skills/lightspeed-windows/SKILL.md` | running anything on the Windows host |
| `lightspeed-verify` | `.dsh/skills/lightspeed-verify/SKILL.md` | before claiming a change is verified |

Two earlier harnesses left artefacts behind (an `opencode`-driven agent and a
second one with a `.omo/` state directory). Both are retired. No
harness-specific config is kept in the repo; DSH state lives in `%USERPROFILE%\.dsh`,
not in the checkout.

> **Never name a document `AGENTS.md` or `agents.md` inside a subdirectory.** DSH
> loads `AGENTS.md` from every directory in the project chain as workspace
> instructions, and Windows paths are case-insensitive, so `agents.md` is
> `AGENTS.md`. The old `wat/archive/agents.md` persona file was being injected
> into every session as project rules until 2026-11-16. See
> [`wat/archive/README.md`](../wat/archive/README.md).

---

## The loop

Work is organised as numbered workflows (`WF-NNN`) recorded in durable state.

1. **Read state** — `wat/state/current-phase.md` (live workstream and newest
   entries), then `wat/rules.md` (policy stubs), then the decision index
   `wat/state/decisions.md`.
2. **Adopt a persona** — one of the eight in `wat/personas/`.
3. **Execute** — small steps, reproduction before fix.
4. **Verify** — the same gates CI runs, with counts; evidence under
   `wat/evidence/<YYYY-MM-DD>/`.
5. **Update state** — one dated entry in `current-phase.md`, a decision entry in
   `wat/state/decisions/<YYYY-MM>.md`, history moved to `wat/state/phases/`
   rather than kept in the hot file.

The state protocol is specified in [`wat/state/README.md`](../wat/state/README.md).

---

## Personas

| Slug | Role | Default scope |
|---|---|---|
| `architect` | design and decisions | `docs/design/`, decisions |
| `rust-dev` | engine, relay, wire types | `client/`, `proxy/`, `protocol/` |
| `infra-dev` | relay fleet and infra | `infra/` |
| `net-eng` | routing, FEC, measurement | routing design, fleet docs |
| `qa-engineer` | tests, benchmarks, evidence | `tools/`, `wat/evidence/` |
| `sec-ops` | threat model, advisories | `proxy/`, `deny.toml`, `SECURITY.md` |
| `dev-ops` | CI, releases, packaging | `.github/workflows/`, `dist/` |
| `interface-designer` | `client-gui` state and layout | `client-gui/`, `docs/design/` |

Each persona file contains a mission, ownership boundary, working rules, a
definition of done and a paste-ready delegation brief. The roster is
machine-readable in `wat/personas/personas.json` (schema beside it).

A persona is a delegation contract, **not** a permission: every agent runs under
the same rules, sandbox and filesystem.

---

## Team mode (Agent Teams)

Multi-persona work runs on DSH's shared task board:

1. Root session: **model & reasoning menu → Use Agent Teams** (per-session, off
   by default).
2. The Lead creates one task per deliverable with acceptance criteria, evidence
   path and advisory write scopes.
3. Teammates are ordinary subagents named by persona slug (`rust-dev`).
4. Tasks are claimed and completed with compare-and-set revisions; the Lead
   reassigns, integrates, re-verifies and owns the final answer.

Full contract, tool semantics, delegation envelope and failure modes:
[`wat/team-mode.md`](../wat/team-mode.md).

Team mode is coordination, not isolation: all members share one filesystem. Split
write scopes, use task dependencies, and review the final diff.

---

## Windows development

The host is Windows and the documented workflows are native to it:

- Windows PowerShell 5.1 (`pwsh` is not required); no Git Bash, no WSL.
- Rust 1.100.0 via `rust-toolchain.toml`; MSVC build tools; Npcap/WinDivert for
  the capture path.
- CPython is an explicit prerequisite for localization and `tools/*.py`; the
  Microsoft Store `python`/`python3` aliases are stubs and do not count.
- Linux relay scripts are CI/remote work, not desktop work.

Details and quirks: [`wat/personas/`](../wat/personas/README.md) is the roster;
the `lightspeed-windows` skill is the operational playbook; `CONTRIBUTING.md`
has the human version.

---

## Verification

A claim is verified when someone else can re-run the check:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets   # Windows only
cargo test -p lightspeed-gui
```

Counts, not adjectives. Bug fixes must show a failing-before/passing-after test
and survive a mutation check. Evidence lives under `wat/evidence/` (gitignored)
and is named in the state entry. The `lightspeed-verify` skill has the per-area
minimums.

---

## Where to look

| Question | File |
|---|---|
| What are the rules? | `wat/rules.md` |
| What is happening now? | `wat/state/current-phase.md` |
| What was decided and why? | `wat/state/decisions/<YYYY-MM>.md` |
| What happened in October? | `wat/state/phases/2026-10.md` |
| Who does what? | `wat/personas/README.md` |
| How do agents work together? | `wat/team-mode.md` |
| What can this host actually run? | skill `lightspeed-windows` |
| How is "done" proved? | skill `lightspeed-verify` |
| Why did the agent layer change on 2026-11-16? | [Agent-system audit and DSH migration](agent-system-audit-2026-11-16.md) |
