# Agent-system audit and DeepSeek Harness migration — 2026-11-16

| | |
|---|---|
| **Scope** | Everything the repository uses to drive AI agents: `AGENTS.md`, `wat/`, `.dsh/`, docs, dev procedures, helper scripts, `.gitignore` |
| **Host** | Windows 11, Windows PowerShell 5.1, Rust 1.100.0, `node`, `git`; no CPython, no `pwsh`, no Git Bash requirement |
| **Harness** | DeepSeek Harness `0.2.2-rc.3` (desktop profile), Agent Teams available |
| **Trigger** | The previous harnesses (an opencode-driven agent, then one using `.omo/` state) were retired; the repository had never actually been migrated to DSH |
| **Outcome** | Agent layer rebuilt for DSH, persona/Team-mode contract added, state split and deduplicated, Windows procedures made native |

This document is the migration record. It lists what was wrong, what changed,
what was verified, and what is deliberately left for the owner.

---

## Executive summary

1. **A Windows filesystem quirk was poisoning every session.** `wat/archive/agents.md`
   matched DSH's `AGENTS.md` instruction loader (case-insensitive NTFS), so the
   persona document was injected into every session as project instructions — at
   the wrong authority, in every repository directory, for every agent. Verified
   live: the session received `Additional instructions from: wat\archive\AGENTS.md`.
2. **The agent layer predated DSH.** No tool mapping, no Team mode, no project
   skills, no Windows rules, and a "tool-neutral" `AGENTS.md` that described an
   autonomy loop but not how to execute it in this harness.
3. **Personas were archival prose, unusable by Team mode.** Team mode needs
   names, owners, acceptance criteria and write scopes. The old file was a single
   `agents.md` in `archive/` with no slugs and no delegation contract.
4. **The hot state file was 322 KB and one-third duplicated.** `current-phase.md`
   was read at the start of every session; it contained 29 duplicated phase
   blocks and 15 duplicated entries, plus history back to September.
5. **Dev procedures assumed POSIX.** `bash scripts/...`, `python3 tools/...`,
   `nohup ... &`, Git Bash `taskkill //IM`, `wget | bash` — all on a Windows host
   with the Microsoft Store Python stubs and no bash in the workflow.
6. **`[COST_STUB]` contradicted the recorded decisions.** It forbade paid
   services but implied no funded infrastructure, while decision 2026-09-13
   explicitly allows sponsor-funded relays with owner spend at `$0.00`.

The repository now has one harness, one procedure system, one persona roster, a
hot state file under 5 KB, and Windows-native commands everywhere an agent or
contributor is told to run something.

---

## Findings

| ID | Severity | Area | Finding | Resolution |
|---|---|---|---|---|
| A1 | **Critical (Windows)** | Workspace instructions | `wat/archive/agents.md` was loaded as `AGENTS.md` by the instruction loader; personas became project rules in every session | Personas moved to `wat/personas/`, file deleted, rule documented in `wat/archive/README.md` and `docs/agent-operations.md` |
| A2 | **High** | Agent entry point | `AGENTS.md` had no DSH tool mapping, no skill/Team-mode pointers, no Windows execution rules, and duplicated content that belonged in rules/state | Rewritten as a 119-line DSH-native contract that points at skills and state instead of inlining them |
| A3 | **High** | Personas | No slugs, no ownership boundaries, no delegation prompts, no task conventions — nothing Team mode could use | 8 persona contracts + `personas.json`/`personas.schema.json` roster + composition rules in `wat/personas/README.md` |
| A4 | **High** | Procedures | Every procedure was prose that had to be re-read or re-derived each session | 4 project skills in `.dsh/skills/` (`lightspeed-wat`, `-team`, `-windows`, `-verify`); discovery verified live in this session |
| A5 | **High** | State size | `current-phase.md`: **322,758 bytes / 5,727 lines**, with 29 duplicate phase blocks and 15 duplicate dated entries | Deduplicated and split: hot file **4,579 bytes / 71 lines** right after the split, **under 8 KB / about 125 lines** including this round's entry; month archives under `wat/state/phases/` |
| A6 | **Medium** | Decision log | `decisions.md`: 138,359 bytes, mixed heading levels, out-of-order entries, template embedded mid-file | Split into 7 monthly files (74 entries preserved), 47-line index + `TEMPLATE.md` |
| A7 | **Medium** | Windows procedures | Commands told agents to use `bash`, `python3`, Git Bash-specific `taskkill`, `nohup` | PowerShell 5.1 procedures, no POSIX requirement, remote/CI routing for Linux-only work |
| A8 | **Medium** | Localization toolchain | `extract-web-strings.sh` / `generate-web-locales.sh` / `localization-update.sh` hosted embedded Python behind a bash interpreter probe | Pure `.py` entry points + `localization_update.py` orchestrator + `scripts/Localization.ps1`; CI and Pages updated |
| A9 | **Medium** | Rules | `[COST_STUB]` contradicted decision 2026-09-13; no shell/agent/state policy existed | `wat/rules.md` rewritten: 10 stubs, including `[SHELL_STUB]`, `[AGENT_STUB]`, `[STATE_STUB]`, corrected `[COST_STUB]` |
| A10 | **Low** | Line endings | No `.gitattributes`; `core.autocrlf=true` produced CRLF worktrees, matching failures and "LF will be replaced by CRLF" noise | `.gitattributes` pins LF for text, CRLF for `.cmd`/`.bat`; 3 WiX files normalised |
| A11 | **Low** | Pre-push script | `check.cmd` duplicated logic and had drifted from CI (no GUI lint/test); `Check-LightSpeed.ps1` used `Invoke-Expression` | One PowerShell implementation with fmt/clippy/test/GUI/audit/deny/build and skip switches; `check.cmd` is a thin wrapper |
| A12 | **Low** | Harness artefacts | `.omo/` evidence paths in 2 design docs (39 references); `.omo/`, `.cortexkit/` ignore entries | `wat/evidence/` paths and ignore rule; retired-harness ignores kept only as a guard |
| A13 | **Low** | Handoff | `HANDOFF.md` was dated 2026-10-04 (WF-050), claimed no Rust toolchain, and carried Git Bash advice | Rewritten for 2026-11-16 with current host facts and the new state model |
| A14 | **Info** | Repo hygiene | No `.opencode/` directory ever existed; one stray `.git/omoproc.ps1` diagnostic remained in the git directory | Confirmed; stray file removed (not tracked, not part of the repo) |

---

## What each change actually does

### 1. One harness, one instruction chain (A1, A2)

DSH (`@deepseek-ai/dsh-agent-instructions`) loads `AGENTS.md`/`CLAUDE.md` from
the project root down to the session directory, plus `.local` overlays. On a
case-insensitive filesystem, `agents.md` is a valid candidate. The old persona
file therefore appeared as project instructions, and because it lived in
`wat/archive/`, it also re-appeared whenever an agent touched that directory.

The fix is structural, not cosmetic:

- `wat/personas/<slug>.md` — names that can never be instruction candidates.
- `wat/archive/README.md` — states the rule and keeps the history of the bug.
- `docs/agent-operations.md` — warns human contributors.
- `AGENTS.md` — the only instruction file, 119 lines, pointing at skills/state.

Verified in this session: after removing the file, DSH emitted
`Instructions removed: wat\archive\AGENTS.md`; before removal the session had
received it as additional instructions.

### 2. Procedure skills instead of prose (A4)

DSH discovers project skills at `<projectRoot>/.dsh/skills` (rank 100, above user
skills). Each skill is loaded on demand, so its cost is paid only when relevant:

| Skill | Lines | Purpose |
|---|---|---|
| `lightspeed-wat` | 84 | the loop, state protocol, persona adoption |
| `lightspeed-team` | 75 | Team mode, task board, delegation envelope |
| `lightspeed-windows` | 91 | PowerShell 5.1 limits, toolchain reality, GUI quirks |
| `lightspeed-verify` | 72 | gates, evidence, mutation checks, per-area minimums |

Discovery was verified live: the session's skill catalog updated as each
`SKILL.md` was created, and all four are invocable by name.

### 3. Personas that Team mode can actually run (A3)

Each persona file now contains: slug, ownership boundary, must/must-not,
definition of done, a paste-ready **delegation brief**, and a
`team_task_create` template. `wat/team-mode.md` adds the contract:

- one task per deliverable, `subject = "[<slug>] ..."`, acceptance criteria,
  evidence path, advisory `write_scopes`;
- `subagent({ name: "<slug>", ... })` — the name is the slug;
- claim/reassign → work → complete with evidence; compare-and-set revisions;
- Lead integrates and re-verifies on the final tree; task changes never wake an
  agent, so `send_message` is required when a teammate must act;
- degraded mode without Team mode (`todo_write` + subagent envelope).

The roster is machine-readable (`personas.json` + JSON Schema) and validated:
8 personas, slugs match `^[a-z][a-z0-9-]*[a-z0-9]$`, every referenced file
exists.

### 4. State that can be read every session (A5, A6)

| File | Before | After |
|---|---|---|
| `wat/state/current-phase.md` | 322,758 B, 5,727 lines, 29 duplicate blocks | 4,579 B / 71 lines after the split; under 8 KB / ~125 lines with this round's entry |
| `wat/state/phases/` | — | `2026-09.md` (220 lines), `2026-10.md` (2,875 lines), `README.md` index |
| `wat/state/decisions.md` | 138,359 B, out of order, template mid-file | 47-line index + format contract |
| `wat/state/decisions/<YYYY-MM>.md` | — | 7 month files, 74 entries preserved |

Preservation was verified line by line: of 4,613 non-empty lines in the original
phase log, every line is present in the new files except (a) the old H1 title,
replaced by the new file header, and (b) one stale `**Last updated:** 2026-10-18`
line that belonged to a truncated duplicate copy of the WF-025 section. All 74
decision entries were carried across.

`tools/Migrate-PhaseLog.ps1` reproduces the phase migration and refuses to run
on an already-migrated log. Routine rotation is a manual move, described in
`wat/state/README.md`, because only a human/agent can decide when an entry stops
being load-bearing.

### 5. Windows-native procedures (A7, A8, A10, A11)

- `CONTRIBUTING.md` now has Windows and Linux/macOS tabs; every command an agent
  is told to run works in PowerShell 5.1.
- The localization pipeline lost bash entirely: the two heredoc scripts became
  `scripts/extract_web_strings.py` (258 lines) and `scripts/generate_web_locales.py`
  (631 lines) with a `--check`/legacy-argv shim, plus
  `scripts/localization_update.py` and `scripts/Localization.ps1`. CI
  (`localization-checks`, `Localization`) and the Pages message were updated.
- `.gitattributes` makes worktree line endings predictable.
- `tools/Check-LightSpeed.ps1` mirrors CI: fmt, workspace clippy, workspace
  tests, `lightspeed-gui` clippy/tests, `cargo audit`, `cargo deny check`,
  release build; `-SkipGui`, `-SkipAudit`, `-SkipBuild`, `-Fast`.
- CPython is documented as an explicit prerequisite (`winget install
  Python.Python.3.12`), because the Store aliases are stubs. Nothing pretends it
  is installed.

### 6. Rules that match reality (A9)

`wat/rules.md` keeps the seven original stubs (cost corrected, quality made
Windows-explicit) and adds:

- `[SHELL_STUB]` — PowerShell 5.1, no bash/Git Bash/WSL workarounds, POSIX-only
  artifacts are remote/CI work;
- `[AGENT_STUB]` — DSH is the only harness, skills are the procedures, Team mode
  is the multi-agent mechanism;
- `[STATE_STUB]` — the state protocol and evidence discipline.

---

## Migration map

| Old | New | Notes |
|---|---|---|
| `wat/archive/agents.md` | `wat/personas/*.md` | 8 personas, 9 files incl. roster; old file deleted |
| `wat/state/current-phase.md` (all history) | `current-phase.md` (live) + `wat/state/phases/<month>.md` | 29 duplicate blocks + 15 duplicate entries removed |
| `wat/state/decisions.md` (all entries) | `wat/state/decisions.md` (index) + `decisions/<month>.md` | 74 entries, normalised headings, sorted |
| `scripts/extract-web-strings.sh` | `scripts/extract_web_strings.py` | `--check` preserved |
| `scripts/generate-web-locales.sh` | `scripts/generate_web_locales.py` | `--check` preserved |
| `scripts/localization-update.sh` | `scripts/localization_update.py` + `scripts/Localization.ps1` | same four gates, no bash |
| `bash scripts/...` in CI | `python scripts/...` | `ci.yml`, `localization.yml`, `pages.yml` |
| `.omo/evidence/...` in design docs | `wat/evidence/...` | 39 references |
| `tools/check.cmd` logic | `tools/Check-LightSpeed.ps1` | `check.cmd` is a 10-line wrapper |
| "persona" prose in `AGENTS.md` | `wat/personas/` + `wat/team-mode.md` | `AGENTS.md` points, does not duplicate |

---

## Verification

Run on the migrated tree unless noted.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** (exit 0) |
| `cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui` | **PASS** (41 s); 2 pre-existing `unused dependency` manifest warnings in `client/Cargo.toml` (`rcgen`, `windivert`) |
| `cargo test --workspace --all --exclude lightspeed-gui` | **PASS**: 29 suites, **1,444 passed, 0 failed, 6 ignored** |
| `tools/Check-LightSpeed.ps1` behaviour | Reports `cargo audit`/`cargo deny` as SKIPPED when absent instead of failing |
| Phase-log content preservation | 4,613 non-empty lines compared; 2 intentional drops (replaced H1, truncated-duplicate tail line) |
| Decision-log preservation | 74/74 original entries present, unique `date+title`, months 2026-05..2026-11 |
| Duplicate headings after migration | 0 in `phases/*.md` |
| Persona roster | JSON + schema parse; 8 personas; slugs valid; all files exist |
| Skill frontmatter | 4/4 skills: `name` == directory, description present |
| Skill discovery (live) | Session catalog updated as each skill was created; `lightspeed-team` loaded end to end |
| `wat/archive/AGENTS.md` collision | `Test-Path` true before, false after; DSH emitted the removal notice |
| Relative links in changed docs | All resolve (wiki-style extensionless links in `docs/README.md` allowed by design) |
| Workflow files | No tabs; localization commands updated |
| Line endings | `.gitattributes` added; `*.wxs` normalised to LF (diff is line endings only) |

**Not run, deliberately:** `cargo audit` and `cargo deny check` (neither tool is
installed on this host; `Check-LightSpeed.ps1` reports them as skipped and CI's
security workflow owns them), the GUI crate gates (`-SkipGui`: no GUI source
changed), and the localization gates (they need CPython, which this host does not
have — an owner decision, not a workaround). No Rust source, dependency or
workflow gate was weakened to make any of this pass.

---

## Residual risks and follow-ups

1. **Team mode must be enabled per session** by the user (model & reasoning menu
   → Use Agent Teams). It is off by default in DSH; nothing in the repository can
   force it. The roster is ready either way (degraded mode is documented).
2. **CPython is still absent on this host.** Localization and `tools/*.py` cannot
   run locally. Install with `winget install Python.Python.3.12`, or run in CI.
   The ported scripts are structural ports: the embedded Python bodies were moved
   verbatim, but they have **not been executed** on this host.
3. **The 2026-09 phase archive contains `###` entries with inherited dates** for
   undated sections (`Summary`, `Verification`, `Next Action`). They are grouped
   under the month of the nearest preceding dated entry, which is faithful but
   not authoritative for those sections.
4. **`*.wxs` normalisation** rewrites three installer files as one-time line
   endings; no XML content changed.
5. **`docs/agent-operations.md` and this audit** are wiki-synced like every
   `docs/*.md` page; the wiki link rewriter turns relative repo links into
   `github.com/.../blob/master/...` URLs.
6. **Old `.omo/`/`.cortexkit/` ignore entries remain** as a guard so an old
   checkout cannot accidentally commit retired harness state. They can be deleted
   once no worktree has those directories.

## How to reproduce the state migration

```powershell
# On a checkout that still has the pre-2026-11-16 current-phase.md:
powershell -NoProfile -ExecutionPolicy Bypass -File tools\Migrate-PhaseLog.ps1          # dry run
powershell -NoProfile -ExecutionPolicy Bypass -File tools\Migrate-PhaseLog.ps1 -Apply   # write
```

The tool prints block counts, dropped duplicate line ranges, month assignment and
the files it writes. It refuses to run once the log contains a `## History`
section.

---

## Addendum (same day): CPython installed, localization gates run

The owner approved installing CPython, so the one residual risk that could be
closed on this host was closed and exercised:

```powershell
winget install --id Python.Python.3.12 --exact --source winget `
  --accept-package-agreements --accept-source-agreements --disable-interactivity
# -> Python 3.12.10 at %LOCALAPPDATA%\Programs\Python\Python312

powershell -NoProfile -ExecutionPolicy Bypass -File scripts\Localization.ps1 -Check
```

Result: `catalog up to date (403 keys)`; `8 locale page(s) up to date`;
`48 localized page(s) checked`; `localization is in step with English`;
exit 0.

This matters beyond the install itself: it is the **first execution** of the
ported `.py` entry points and the PowerShell runner. The port was structural
(embedded Python bodies moved verbatim), and it produced byte-identical checks
to the bash pipeline — so the bash dependency is gone in practice, not only on
paper. A shell started before the install finds `py` on PATH only after a
restart; `scripts/Localization.ps1 -Python <full path>` is the fallback.

Residual risk 2 above is therefore resolved for this host. The Store aliases
`python`/`python3` still exit 9009 and are documented as non-interpreters.
