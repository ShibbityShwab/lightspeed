# Decision Log

> Canonical index of significant technical and procedural decisions for LightSpeed.
> Entries live in monthly files under [`decisions/`](decisions/); this file is the
> contract and the map, not the log itself.
> Last entry: 2026-11-23

## Where entries live

| Month | File |
|---|---|
| 2026-05 | [2026-05.md](decisions/2026-05.md) |
| 2026-06 | [2026-06.md](decisions/2026-06.md) |
| 2026-07 | [2026-07.md](decisions/2026-07.md) |
| 2026-08 | [2026-08.md](decisions/2026-08.md) |
| 2026-09 | [2026-09.md](decisions/2026-09.md) |
| 2026-10 | [2026-10.md](decisions/2026-10.md) |
| 2026-11 | [2026-11.md](decisions/2026-11.md) |

A new month gets a new file with the same header and separator shape; add its
row here. Counts are deliberately not listed: they drift, and the file itself is
the source of truth.

## How to log a decision

1. Pick the month of the decision date, not the month of the commit.
2. Open `wat/state/decisions/<YYYY-MM>.md` and add the entry at the top.
3. Use [`decisions/TEMPLATE.md`](decisions/TEMPLATE.md): Agent, Status, Rationale, Impact, Alternatives Considered, Verification.
4. If the entry supersedes an older one, add `Superseded by <YYYY-MM-DD>` to the old entry and name the old date in the new one.
5. If the decision changes a rule in `wat/rules.md`, update the rule in the same change.

A decision that is not written here did not happen; the next session will
relitigate it from scratch.

## Format contract

```markdown
### YYYY-MM-DD: Short imperative title

**Agent:** <persona slug(s) or `lead`>
**Status:** Accepted | Proposed | Deprecated | Superseded by YYYY-MM-DD
**Rationale:** Why this decision, in terms of measured impact.
**Impact:** What changes (files, crates, procedures, fleet behaviour).
**Alternatives Considered:** What else was on the table and why rejected.
**Verification:** The command/test/measurement and its counts.
```

## Evolution

Decision entries are append-only. A reversal is a new entry that names the
old date; editing history destroys the audit trail that makes the log useful.
