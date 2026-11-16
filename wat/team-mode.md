# Team Mode

> How LightSpeed runs a workflow with DeepSeek Harness **Agent Teams** and the
> personas in [`personas/`](personas/README.md).
> Verified against DSH `0.2.2-rc.3` (`@deepseek-ai/dsh-agent-team`,
> `@deepseek-ai/dsh-tool-agent-team`). Last updated: 2026-11-16.

---

## 1. What Team mode is

Agent Teams adds a **durable shared task board** to the ordinary DSH subagent
tree. It does not add a second kind of agent:

- Teammates are normal `subagent` / `subagent_fork` children. Discovery,
  messaging, interruption and inbox waiting stay on the standard subagent tools.
- The root Session owns one board. Every descendant is a member of the same
  Team, at any depth.
- **All members share one working directory and filesystem.** An edit is visible
  to everyone immediately. There is no worktree, no merge, no lock.
- Team is **off by default** for new Sessions and saved per Session.
- Without Team mode the board tools and the Team guidance are absent; nothing
  else changes.

So: Team mode is *coordination*, not isolation. It gives a durable record of who
owns what and what is blocked on what. The personas give each owner a role brief.

---

## 2. Enabling it

In the root conversation: **model & reasoning menu → Use Agent Teams**.

- The `/team on` / `/team off` slash commands no longer exist (removed in
  `v0.2.1-rc.3`).
- The selection is saved and survives restart. Turning it off hides the task
  tools and Team guidance from subsequent steps, while keeping tasks and
  subagents.
- Only the root controls it; descendants inherit the live root's mode.
- Enable it **before** creating the first task in a Session. A task operation
  that starts while Team is off is rejected; admitted operations finish.

When the board tools are present in the tool list (`team_task_create`,
`team_task_get`, `team_task_list`, `team_task_update`), Team mode is on.

---

## 3. Tools and exact semantics

### Team task board

| Tool | Use it for | Required | Notes |
|---|---|---|---|
| `team_task_create` | Add one unowned pending task | `subject`, `description` | Optional `blocked_by` (task ids) and `write_scopes` (workspace-relative paths) |
| `team_task_get` | Read the latest value of one task | `task_id` | Do this before any update that needs a revision |
| `team_task_list` | Browse the board | — | Filters: `status`, `owner`, `unowned`, `ready`, `cursor`, `limit` (1–100); `owner` and `unowned` cannot be combined |
| `team_task_update` | Compare-and-set transition | `task_id`, `expected_revision`, `action` | Actions: `claim`, `release`, `edit`, `set_dependencies`, `complete`, `reopen`, `reassign`, `delete` |

Rules that follow from the implementation:

- **One owner.** The root may reassign to itself or any reachable continuable
  descendant (`owner` = name). Any other member must `claim` a ready task
  itself. There is **no automatic release**: inactivity, interruption and exit do
  not free a task.
- **Compare-and-set.** Every update needs the current `expected_revision`.
  Stale revisions fail with `TEAM_TASK_STALE_REVISION`; re-read and retry.
- **Readiness.** A task is claimable only when everything in `blocked_by` is
  complete. Claiming a blocked task fails.
- **Write scopes are advisory.** Overlapping in-progress scopes produce a
  warning, never a lock, and never authorize a write. `bash`, formatters and
  external processes can still write anywhere.
- **Bounds.** One board holds at most 256 non-deleted tasks (`maxTasks`);
  deleted tasks leave the active list but stay in history.
- **Durability.** `TEAM_TASK_PERSISTENCE_UNCONFIRMED` means the task is already
  visible but the flush did not confirm: read the task before retrying.

### Subagent controls (unchanged by Team)

| Tool | Use it for |
|---|---|
| `subagent` | Start a fresh teammate: `name` (slug), `description`, `prompt`, optional provider/model/reasoning effort |
| `subagent_fork` | Start a teammate that inherits this conversation's completed turns |
| `send_message` | Tell a named agent something; the only way to make an idle agent act |
| `list_agents` | Discover the tree and the exact names to assign or message |
| `wait_agent` | Wait for the caller's next inbox input (a message or completion), not for task changes |
| `interrupt_agent` | Stop a descendant's turn — **disabled in the built-in configs**; assume it is unavailable |

Task changes **do not** end `wait_agent`, add inbox messages, or start idle
agents. If a teammate must act on a board change, `send_message` them.

Names: lowercase letters, digits and hyphens, starting with a letter, ≤32 chars.
The persona slug is the name (`rust-dev`). A second instance appends `-2`.

---

## 4. The LightSpeed team loop

### Step 0 — Lead reads state

`wat/state/current-phase.md` (current state + recent entries) → `wat/rules.md` →
the phase's archive file if the entry points at one → `wat/state/decisions/`
month file for the area. Do this before planning, every Session.

### Step 1 — Lead plans and writes tasks

One task per deliverable, not per persona. Each task carries:

- `subject`: `[<slug>] <imperative title>`.
- `description`: context, **acceptance criteria**, the evidence path
  (`wat/evidence/<YYYY-MM-DD>/...`), and the gate commands.
- `write_scopes`: the persona's `owns` paths, narrowed to what this task touches.
- `blocked_by`: task ids whose output this task consumes.

Sequence the work with dependencies instead of telling agents to "wait".

### Step 2 — Lead spawns one teammate per task

Use the envelope in section 5, then the persona's **Delegation brief** from
`wat/personas/<slug>.md`, then the task id and acceptance criteria.

Start independent tasks in the same step when they do not share write scopes;
they run concurrently. Do not spawn two agents on overlapping scopes.

### Step 3 — Assign or let them claim

Either reassign the task to the teammate's name right after spawning, or leave
it unowned and tell the teammate the task id — they claim it with the current
revision. Reassignment is the Lead's privilege; claiming is everyone's.

### Step 4 — Teammate works

The teammate claims, does the work, runs its persona's gates, records evidence
under `wat/evidence/<YYYY-MM-DD>/`, completes the task with a report, and
returns the same report as its final answer.

### Step 5 — Lead integrates and verifies

The Lead does not trust a teammate's summary. Re-run the gates that matter on
the final tree, read the diff, and check that evidence paths exist. Reviews that
change another persona's scope go back as a new task, not as an edit.

### Step 6 — Lead closes the phase

Only when every required task is complete, the Lead:

1. updates `wat/state/current-phase.md` (new dated entry at the top);
2. appends a decision to `wat/state/decisions/<YYYY-MM>.md` when a decision was
   made;
3. reports the result with commands and counts.

The Lead must wait for required teammates before giving the final answer. A
send is not a completion: `send_message` returning `{ "sent": true }` proves
delivery, not work.

---

## 5. Delegation envelope

Every `subagent` prompt starts with this envelope, then the persona brief, then
the task. The child does not see the Lead's conversation (unless forked), so the
envelope is the contract.

```text
You are a LightSpeed teammate. Repo: C:\Users\ShibbityShwab\Documents\GitHub\lightspeed
(Windows host, PowerShell 5.1 — never invoke bash, Git Bash, WSL or POSIX-only
scripts; if a step needs one, report it as blocked instead).

All paths in this prompt are relative to the repo root above.

Read first, in this order: AGENTS.md; wat/rules.md; the "Current state" section
of wat/state/current-phase.md; your persona file wat/personas/<slug>.md.

Operating rules:
- You share one working directory with the Lead and other teammates. Stay inside
  your task's write scopes; if the work needs a file outside them, stop and ask
  the Lead via send_message.
- Claim the task before starting (team_task_update, action "claim",
  expected_revision from team_task_get). Re-read the revision before any update.
- Evidence goes to wat/evidence/<YYYY-MM-DD>/ and is named in the completion.
- Report commands and counts, never adjectives. If something is unverified, say
  so explicitly.
- Do not commit or push. The Lead owns git state.

When done: complete the shared task with a report containing (1) what changed,
(2) the gate commands and their counts, (3) evidence paths, (4) anything
unverified or blocked. Then return the same report as your final answer.
```

---

## 6. Persona → task mapping

| Task subject | Owner (subagent name) | Default write scopes | Done when |
|---|---|---|---|
| `[architect] decide <question>` | `architect` | `docs/design/`, `wat/state/decisions/` | Decision record with options, impact, falsifier |
| `[rust-dev] <verb> <component>` | `rust-dev` | `client/`, `proxy/`, `protocol/` | Failing-before/passing-after test; fmt/clippy/test green with counts |
| `[infra-dev] <op> <relay/script>` | `infra-dev` | `infra/` | Committed procedure; health/metrics evidence; `$0.00` stated |
| `[net-eng] measure <path>` | `net-eng` | `docs/`, `client/src/route/` | Metric defined, n and window given, falsifier named |
| `[qa-engineer] verify <change>` | `qa-engineer` | `tools/`, `wat/evidence/` | Repro + full-crate counts + evidence paths |
| `[sec-ops] review <surface>` | `sec-ops` | `proxy/`, `deny.toml`, `SECURITY.md` | Findings with severity, exploit story, proof |
| `[dev-ops] <fix> <workflow>` | `dev-ops` | `.github/workflows/`, `dist/`, `packaging/` | Gate preserved, permissions minimal, CI-only steps named |
| `[interface-designer] <surface>` | `interface-designer` | `client-gui/src/app.rs`, `docs/design/` | State inventory + screenshots at two widths/two states |

A workflow usually needs two to four personas, not eight. Typical shapes:

- **Bug fix:** `rust-dev` (fix) → `qa-engineer` (independent verification).
- **Feature:** `architect` (decision) → `rust-dev` (implementation) →
  `qa-engineer` → `interface-designer` if a surface changed → `dev-ops` if a
  release step changed.
- **Fleet/infra change:** `net-eng` (design) → `infra-dev` (rollout) →
  `sec-ops` (abuse review if the data path changed).
- **Security advisory:** `sec-ops` (triage) → `rust-dev` (bump/fix) →
  `qa-engineer` (regression).

---

## 7. Failure modes and what to do

| Symptom | Meaning | Action |
|---|---|---|
| `TEAM_TASK_STALE_REVISION` | Someone changed the task since you read it | `team_task_get` again, re-apply with the new revision |
| Claim rejected as unready | A `blocked_by` task is not complete | Finish the blocker, or ask the Lead to `set_dependencies` if the dependency was wrong |
| `reassign` rejected | Only the root may reassign, and only to a reachable descendant | Check `list_agents`; the name must be a live member |
| `TEAM_TASK_PERSISTENCE_UNCONFIRMED` | Task visible, durability unconfirmed | Read the task before retrying; do not blindly re-apply |
| Board full | `maxTasks` (256) reached | Complete or `delete` stale tasks; deleted tasks leave the active list |
| Teammate idle after a board change | Task changes never wake agents | `send_message` the teammate with the task id |
| Tool list has no `team_task_*` | Team mode is off for this Session | Enable **Use Agent Teams** in the root menu; note it cannot be forced from a child |

## 8. Degraded mode (Team mode off)

Team mode is optional. When it is off, run the same plan with:

1. `todo_write` for the visible plan (this is the Lead's own checklist, not a
   shared board).
2. `subagent` / `subagent_fork` with the same envelope and persona brief; the
   task id becomes a line in the prompt ("deliverable: ...").
3. `ask_user_question` for owner gates.

The cost of degraded mode: no durable ownership, no dependencies, no board
revisions, and no recovery if the session dies mid-flight. Use Team mode for
multi-persona work that spans steps.

## 9. Before the final answer

- [ ] Every required task is `completed` (list the board and check).
- [ ] The Lead re-ran the gates on the final tree; counts recorded.
- [ ] Evidence paths exist on disk.
- [ ] `wat/state/current-phase.md` has the new dated entry.
- [ ] Decision logged if one was made.
- [ ] Unverified items and owner gates are named in the answer.
