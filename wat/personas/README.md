# LightSpeed Personas

> Team-mode roster for LightSpeed. One file per persona, one slug per teammate.
> Canonical copy: this directory. `wat/personas/personas.json` is the
> machine-readable roster (with `personas.schema.json`) so tooling and checks can
> read it without parsing prose.
> Last updated: 2026-11-16

## What a persona is (and is not)

A persona is **a delegation contract**: a role brief, an ownership boundary, a
definition of done, and a delegation prompt the Lead pastes verbatim into
`subagent` / `subagent_fork` when Agent Teams is enabled.

A persona is **not** a permission. Every teammate runs under the same
`AGENTS.md`, the same `wat/rules.md` policy stubs, the same sandbox and the same
filesystem. A task owned by `sec-ops` can still edit `client/`; the persona only
says who *should*, and the write scope on the shared task says who *is*.

## Roster

| Slug | Persona | Owns (default) | Pairs with |
|---|---|---|---|
| `architect` | Architect | `docs/design/`, `docs/architecture.md`, `docs/protocol.md`, decision records | every persona as reviewer |
| `rust-dev` | RustDev | `client/`, `proxy/`, `protocol/` source + unit/integration tests | `qa-engineer`, `net-eng` |
| `infra-dev` | InfraDev | `infra/`, `infra/scripts/`, Terraform, relay provisioning | `dev-ops`, `sec-ops` |
| `net-eng` | NetEng | routing/FEC/latency design, relay placement, `docs/fleet-load-rebalancing.md` | `rust-dev`, `qa-engineer` |
| `qa-engineer` | QAEngineer | test plans, benchmarks, `tools/*` harnesses, game compatibility | everyone (verifies) |
| `sec-ops` | SecOps | threat model, `deny.toml`, advisories, auth/anti-abuse review | `rust-dev`, `dev-ops` |
| `dev-ops` | DevOps | `.github/workflows/`, `dist/`, `packaging/`, releases | `infra-dev`, `qa-engineer` |
| `interface-designer` | InterfaceDesigner | `client-gui/` surfaces, `docs/design/interface.md` | `rust-dev` |

Slugs are the subagent `name` and the task `owner`. They are lowercase with
hyphens because that is what `subagent` accepts. A second instance of the same
persona appends a number: `rust-dev-2`.

## How a persona is spawned

1. Enable Team mode on the root conversation (model & reasoning menu → **Use
   Agent Teams**). See [`../team-mode.md`](../team-mode.md).
2. Create the task on the shared board: `team_task_create` with
   `subject` = `[<slug>] <imperative title>`, a `description` carrying the
   acceptance criteria and evidence path, and `write_scopes` for the paths the
   work will touch.
3. Start the teammate with the same slug:

   ```text
   subagent({
     name: "rust-dev",
     description: "[rust-dev] fix shadow-sampler RTT gap",
     prompt: "<delegation envelope from wat/team-mode.md>\n\n<persona brief from this file>\n\n<task id + acceptance criteria>"
   })
   ```

4. Reassign the task to that name (`team_task_update` with `action:
   "reassign"`, `owner: "rust-dev"`), or let the teammate claim it. One owner per
   task; the Lead owns integration and the final answer.

A teammate never sees the Lead's conversation unless the Lead used
`subagent_fork`. Treat every delegation prompt as self-contained: repo, rules,
task id, scope, gates, and the shape of the report we need back.

## Persona file layout

Every `wat/personas/<slug>.md` has the same sections so the Lead can compose a
prompt without reading the whole file:

- **Mission** — one paragraph, the reason the persona exists.
- **Owns** — paths this persona is expected to change; the default write scope.
- **Must do** — non-negotiable working rules for this role.
- **Must not** — the failure modes this role has historically produced.
- **Definition of done** — what must be true before the task is completed.
- **Delegation brief** — the block the Lead pastes after the envelope.
- **Task template** — a ready `team_task_create` payload.

## Adding or changing a persona

1. Copy the nearest existing file; keep the section order.
2. Add the slug to `personas.json` (the validator fails on a file that is not
   listed, or a listed file that does not exist).
3. Record the change in `wat/state/decisions/<YYYY-MM>.md` under `architect`.
4. Do not name the file `AGENTS.md` or `agents.md`: on Windows those match the
   workspace-instruction loader and would be injected into every session as
   project rules. That is exactly why this directory exists.
