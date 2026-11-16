# Decision entry template

Copy this block into `wat/state/decisions/<YYYY-MM>.md`, newest first.
One decision per entry. Keep the six fields; add `**Verification:**` when the
decision was proved by a command, a test or a measurement.

```markdown
### YYYY-MM-DD: Short imperative title

**Agent:** <persona slug(s), or `lead`>
**Status:** Accepted | Proposed | Deprecated | Superseded by YYYY-MM-DD
**Rationale:** Why this decision, in terms of measured impact. Name the
constraint that decided it (cost, transparency, platform, CI gate).
**Impact:** What changes: files, crates, procedures, fleet behaviour. Include
the migration step for anyone who has to follow it.
**Alternatives Considered:** What else was on the table and the specific reason
each was rejected.
**Verification:** The command, test or measurement that supports the decision,
with counts. Omit only when the decision is a plan with no executable check yet.
```

## Status values

| Status | Meaning |
|---|---|
| `Accepted` | In force. Code or procedure follows it. |
| `Proposed` | Written but not yet binding; needs an owner or Architect decision. |
| `Deprecated` | No longer recommended, still historically accurate. |
| `Superseded by YYYY-MM-DD` | A later entry replaces it; name that date. |