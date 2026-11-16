# wat/archive

Historical material kept for reference. **Nothing here is an instruction.**

## Contents

| Path | What it is | Still authoritative? |
|---|---|---|
| `phases/` (see `../state/phases/`) | Monthly phase logs | Historical record only |
| `agents.md` (removed 2026-11-16) | Former persona definitions | No — moved to `../personas/` |

## Why it lives here

The WAT system keeps the live contract in small, loadable files:
`../../AGENTS.md`, `../rules.md`, `../personas/`, `../team-mode.md`, the state
files under `../state/`, and the procedure skills under `../../.dsh/skills/`.
Anything that is only useful as history belongs in an archive so it does not get
mistaken for a current rule.

## Warning: never name a file `AGENTS.md` or `agents.md` here

DeepSeek Harness loads every `AGENTS.md` it finds in the project chain as
workspace instructions. Windows filesystems are case-insensitive, so a file
named `agents.md` **is** `AGENTS.md` and gets injected into every session as if
it were project rules. That is exactly what happened to the old
`wat/archive/agents.md`: it appeared to every agent as
`Additional instructions from: wat\archive\AGENTS.md`.

Use a different name (`persona-roster.md`, `agents-reference.md`, ...) for any
document that merely describes agents.
