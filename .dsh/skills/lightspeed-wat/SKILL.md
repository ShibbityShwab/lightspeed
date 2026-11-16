---
name: lightspeed-wat
description: Use when starting, continuing or resuming work in the LightSpeed repository, or when the user says "start the loop", "continue" or "continue the loop". Explains the WAT autonomy loop, where durable state lives, how to pick a persona, which gates apply, and how a round must end.
---

# LightSpeed WAT loop

> Paths in this document are relative to the repository root, not to this skill directory.

LightSpeed is a zero-cost global network optimizer for multiplayer games: a Rust
workspace (`client/`, `proxy/`, `protocol/`, `client-gui/`) plus `infra/`,
`web/`, `dist/` and GitHub Actions. Work is organised as numbered workflows
(WF-NNN) recorded in the WAT state files.

## 1. Read state before anything else

In this order, and stop when the answer is clear:

1. `wat/state/current-phase.md` — the **Current state** section and the newest
   dated entries. This is the live workstream.
2. `wat/state/phases/<YYYY-MM>.md` — the month file the entry names, when you
   need the history behind a decision.
3. `wat/rules.md` — the policy stubs bind every action, especially
   `[COST_STUB]` (owner spend $0.00) and `[SHELL_STUB]` (Windows-native).
4. `wat/state/decisions.md` — index and format; entries are in
   `wat/state/decisions/<YYYY-MM>.md`.

Do not re-derive context that the state files already record. If the state is
stale, fix the state as part of the round.

## 2. Adopt a persona

The roster is `wat/personas/README.md`; one file per persona. The Lead (root
agent) plans, integrates and owns the final answer; teammates are subagents
named by persona slug.

- Single-threaded work: adopt the persona in the report and the tool usage.
- Multi-persona work: use Team mode (`wat/team-mode.md`), one task per
  deliverable, one owner per task.

## 3. Execute in small, verifiable steps

- Write the plan with `todo_write` when the work has more than three steps.
- Reproduce a bug before fixing it; a fix with no failing test is a guess.
- Prefer the tool DSH gives you: `read`/`write`/`edit` for files, `grep`/`glob`
  for discovery, `pwsh` for commands. Never shell out to `cat`, `sed`, `find` or
  `grep` on Windows.
- Keep the change surface narrow: the crates and docs the task names, not the
  neighbour you noticed.

## 4. Verify before claiming

Load the `lightspeed-verify` skill. Minimum gate on this host:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
```

GUI work adds `cargo clippy -p lightspeed-gui --all-targets` and
`cargo test -p lightspeed-gui`; WinDivert work adds
`cargo test -p lightspeed-client --features windivert-redirect`.

Report counts, not adjectives. Evidence goes to `wat/evidence/<YYYY-MM-DD>/`.

## 5. End the round by updating state

1. Append one dated entry at the top of `wat/state/current-phase.md`:
   what changed, why, what was verified (commands and counts), what is open.
2. Append a decision to `wat/state/decisions/<YYYY-MM>.md` when a decision was
   made. Use the template in `wat/state/decisions/TEMPLATE.md`.
3. Keep `current-phase.md` current: when an entry no longer describes active
   work and belongs to a past month, move it to that month's file in
   `wat/state/phases/`. Never delete history.

## 6. Owner gates

Ask one crisp question with `ask_user_question` when a decision is the owner's:
product calls, spending anything, publishing a release, posting publicly, or
touching credentials. Bundle related gates into one question with options.

## Invocation

If the user says "Start the loop", "Continue", or "Continue the loop": read
`wat/state/current-phase.md`, state the next action in one line, and execute it.
