# State protocol

Durable state is how the next session resumes without re-deriving context. It is
also the most-read content in the repository, so it is kept small and split by
purpose.

## Files

| File | Purpose | Size discipline |
|---|---|---|
| `current-phase.md` | Live workstream, current status, newest dated entries | Keep under ~400 lines: move old entries to `phases/` |
| `phases/<YYYY-MM>.md` | One file per month, newest first inside the file | Append-only archive |
| `phases/README.md` | Index: month, entry count, entry titles | Regenerated when a month file changes |
| `decisions.md` | Decision index, format contract, append protocol | Slowly growing, entries not here |
| `decisions/<YYYY-MM>.md` | One file per month of decisions | Append-only |
| `decisions/TEMPLATE.md` | The entry template and status values | Stable |
| `HANDOFF.md` | Context a newcomer cannot infer from the other files | Rewrite when the phase changes |

## End-of-round checklist

1. **State entry** — add one dated `###` entry at the top of the
   *Recent entries* section in `current-phase.md`:

   ```markdown
   ### YYYY-MM-DD - <what changed in one line>

   **Agent:** <persona slug(s)> (Lead-verified)
   What changed, why, and what the next person must know.
   **Verified:** <commands> -> <counts>. Evidence: `wat/evidence/...`.
   **Open:** <what is still unproven or blocked, or "none">.
   ```

2. **Decision** — if a decision was made, append to
   `decisions/<YYYY-MM>.md` using `decisions/TEMPLATE.md`.

3. **Rules** — if the decision changes a policy stub in `rules.md`, update the
   stub in the same change.

4. **Evidence** — write artifacts under `wat/evidence/<YYYY-MM-DD>/` and name
   them in the entry.

5. **Rotation** — when an entry no longer describes active work and belongs to a
   past month, move it verbatim to `phases/<YYYY-MM>.md` and demote its headings
   one level, then update the History table. That is a manual move on purpose: a
   script cannot tell which entries are still load-bearing.

   `tools/Migrate-PhaseLog.ps1` is the one-off tool that performed the
   2026-11-16 migration (dedupe + split). It refuses to run on an already
   migrated log; it is kept so the migration is reproducible, not for routine
   rotation.

## Rules

- **Append-only history.** Move entries, never delete or rewrite them. A reversal
  is a new entry that names the old date.
- **One entry per round**, not per file changed.
- **Counts, not adjectives.** "fmt/clippy clean; client lib 566, proxy 176, GUI
  136" is state; "all good" is not.
- **Open items are explicit.** An empty *Open* line is a claim that nothing is
  outstanding; do not write it if that is not true.
- **No secrets, no PII** in state or evidence (see `[SECURITY_STUB]`,
  `[PRIVACY_STUB]` in `../rules.md`).

## Why the hot file is small

`current-phase.md` used to be 328 KB (~5,700 lines) because every phase was
appended forever. It is read at the start of every session, so its size is a tax
on every request and a source of contradictions (the same entry appearing twice
with different edits). The 2026-11-16 migration deduplicated it and moved
history into `phases/`; the hot file is now around 120 lines and the history is
still complete.
