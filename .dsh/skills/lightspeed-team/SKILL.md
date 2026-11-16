---
name: lightspeed-team
description: Use when LightSpeed work should run as a team of agents, when the user asks for the team, personas, parallel work, or Agent Teams, or when a multi-step job needs durable task ownership. Covers enabling Team mode in DeepSeek Harness, the shared task board, persona subagents, delegation envelopes and integration.
---

# LightSpeed team work (DeepSeek Harness Agent Teams)

> Paths in this document are relative to the repository root, not to this skill directory.

The full contract is `wat/team-mode.md`; the roster is `wat/personas/README.md`.
Read both before the first spawn in a Session — this skill is the short form.

## Turn it on

Root conversation → **model & reasoning menu → Use Agent Teams**. It is
per-Session, saved, off by default, and cannot be enabled from a child. The
`/team on` command no longer exists. Team mode is on when the tool list contains
`team_task_create`, `team_task_get`, `team_task_list`, `team_task_update`.

## The shape of a team run

1. **Plan tasks, not agents.** One task per deliverable:
   `subject` = `[<slug>] <imperative title>`, `description` = context +
   acceptance criteria + evidence path + gate commands, `write_scopes` = the
   paths this task touches, `blocked_by` = tasks it consumes.
2. **Spawn one teammate per task.** `subagent({ name: "<slug>", description,
   prompt })` with the envelope + persona brief + task id. Use the slug as the
   name; `-2` for a second instance. `subagent_fork` only when the child needs
   this conversation's history.
3. **Assign or claim.** Reassign the task to the new name, or let the teammate
   claim it with the current revision.
4. **Work inside scopes.** Members share one filesystem; overlapping edits are
   real, not theoretical. Write-scope warnings are advisory — the Lead's job is
   to keep scopes disjoint.
5. **Complete with evidence.** The teammate completes the task with a report:
   changed files, gate commands + counts, evidence paths, unverified items.
6. **Lead integrates.** Re-run the gates on the final tree; do not trust a
   summary. Wait for required teammates before the final answer.

## Rules that bite

- Every `team_task_update` needs the current `expected_revision` (CAS). Stale →
  re-read and retry.
- Only the root reassigns; other members claim ready tasks themselves.
- A task is claimable only when `blocked_by` is complete.
- Task changes never wake agents. Use `send_message` when a teammate must act on
  a board change; a sent message is not completed work.
- No automatic ownership release: an interrupted or dead teammate leaves its
  task owned. The Lead reassigns or reopens it.
- `wait_agent` waits for inbox input, not for task state. `interrupt_agent` is
  disabled in the built-in configs.
- Board limit is 256 non-deleted tasks.

## Envelope (paste at the top of every subagent prompt)

```text
You are a LightSpeed teammate. Repo: C:\Users\ShibbityShwab\Documents\GitHub\lightspeed
(Windows host, PowerShell 5.1 — never invoke bash, Git Bash, WSL or POSIX-only
scripts; if a step needs one, report it as blocked instead).

All paths in this prompt are relative to the repo root above.

Read first: AGENTS.md; wat/rules.md; the "Current state" section of
wat/state/current-phase.md; your persona file wat/personas/<slug>.md.

Stay inside your task's write scopes. Claim the task before starting, re-read
the revision before any update. Evidence goes to wat/evidence/<YYYY-MM-DD>/.
Do not commit or push.

When done: complete the shared task with a report of (1) what changed, (2) gate
commands and counts, (3) evidence paths, (4) anything unverified. Return the
same report as your final answer.
```

## Degraded mode

Team mode off is a supported mode. Use `todo_write` for the plan and the same
envelope/persona brief in plain `subagent` prompts; you lose durable ownership,
dependencies and revisions, so keep the plan small.
