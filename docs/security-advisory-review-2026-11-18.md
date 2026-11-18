# Security advisory allowlist review — 2026-11-18

> Due diligence for audit finding **F-15 (P3)** in
> [`project-audit-2026-11-18.md`](project-audit-2026-11-18.md). Owner: `sec-ops`.
> Scope: `deny.toml` advisory `ignore` list, `SECURITY.md`, and the code
> migration that is the only real action item (`chrono`).
>
> **Verification status: the `deny.toml` advisory gate is verified by CI only.**
> `cargo-deny` and `cargo-audit` are **not installed** on the development host
> (checked: neither is on `PATH`, neither is in `~/.cargo/bin`, no local
> advisory-DB clone exists — `wat/evidence/2026-11-18/team/sec-ops/01-baseline.txt`).
> The only automated proof of the edit is the next `Security Audit` workflow run
> (`.github/workflows/security.yml`, `cargo deny` job). The TOML itself was
> syntax-validated locally with Python's `tomllib` (8 entries parse — evidence
> `08-toml-validation.txt`).

## Summary

- `deny.toml` contained **17** ignore entries, not 18. Audit F-15's count is
  off by one; the only way to reach 18 in the repo's advisory configs is to add
  the duplicate `RUSTSEC-2024-0437` entry in `.cargo/audit.toml`
  (`07-counts-reconciliation.txt`).
- **9 of the 17 entries were inert and have been removed**: the seven GTK3
  advisories were **withdrawn** by the RustSec DB on 2026-08-14, and the
  `protobuf` and `instant` crates are **not in `Cargo.lock` at all**.
- **8 entries remain.** Every one is an `informational = "unmaintained"`
  advisory with no patched release — none is a known vulnerability. `chrono` is
  the only one that needs a code migration this repo controls (7 use sites,
  below); the rest are blocked on upstream dependencies and/or have no security
  finding that requires action.
- Exposure is limited: the GTK3 family and `proc-macro-error` are **never
  compiled for any target** (mutually exclusive `cfg` edges, see the table);
  `paste`/`bincode` compile only under the optional `ml` feature (never in a
  dist feature set); `ttf-parser` and `zvariant`/`zbus_names` are confined to the
  GUI crate (tests included); `chrono` is the only ignored advisory in a
  **shipped, always-on** dependency.
- No shipped component parses network input with any of these crates. The
  nearest thing is `bincode::deserialize` of a **local** model file, and only
  with the `ml` feature enabled.
- `SECURITY.md` was corrected: its supported-version table still said `1.6.x`
  when the current release is 1.11.0.

## Method and baseline

- Reviewed at `HEAD bc88573` with the worktree shared with other teammates;
  `Cargo.lock` had uncommitted changes at review time, so its SHA-256 is
  recorded in `01-baseline.txt` and all versions/counts below are tied to it.
- Dependency paths were derived by walking the package graph in `Cargo.lock`
  (722 packages) with a PowerShell read-only script — full output in
  `04-dep-paths.txt`. No `cargo tree`, build or test was run; the build lock was
  not touched.
- Advisory metadata came from the RustSec advisory DB (raw markdown, 17/17
  fetched HTTP 200) — table and URLs in `06-advisory-db-summary.md`.
- `cargo-deny` semantics are taken from its official docs
  ([config](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html),
  [diagnostics](https://embarkstudios.github.io/cargo-deny/checks/advisories/diags.html)):
  `unmaintained = "all"` is the default (so unmaintained hits are errors), and
  `unused-ignored-advisory = "warn"` is the default — an ignore that matches
  nothing (`advisory-not-detected`, explicitly documented to happen for
  **withdrawn** advisories) is a *warning*, which is why CI stayed green while
  nine stale entries sat in the file.
- Target gating was verified against the repo manifest and the crate manifests
  in the local cargo registry (`11-target-gating.txt`). Note that `Cargo.lock`
  records the **union** of platform-specific edges, so edges exist there that no
  single build compiles.
- **Caveat:** the worktree already carried another teammate's uncommitted
  `Cargo.lock` change at review time. If the pending dependency refresh (audit
  F-08) lands before this is merged, re-run the version checks: a lockfile move
  can take a crate out of an advisory's affected range (making that ignore
  unused) or pull a new one in. The allowlist and this document are tied to the
  lock hash in `01-baseline.txt`.

## The allowlist, advisory by advisory

`Cargo.lock` versions are shown as `crate@version`. "Fix availability" is about
the advisory, not about whether we can act this week.

| Advisory | Crate in lock | Dependency path | Exposure | Advisory class / patched release | Fix availability | Verdict |
|---|---|---|---|---|---|---|
| RUSTSEC-2024-0412 | `gdk@0.18.2` | `lightspeed-gui → tray-icon → muda \| libappindicator → gtk → gdk` | Lockfile-only: `tray-icon` is a **Windows-only** dep of the GUI (`client-gui/Cargo.toml:65`), while its GTK deps are **Linux-only** — no target compiles both | unmaintained (withdrawn **2026-08-14**) | None needed; gtk-rs repo was unarchived, development resumed | **remove** (done) |
| RUSTSEC-2024-0413 | `atk@0.18.2` | same GTK3 chain | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0415 | `gtk@0.18.2` | same GTK3 chain | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0416 | `atk-sys@0.18.2` | same GTK3 chain | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0418 | `gdk-sys@0.18.2` | same GTK3 chain | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0419 | `gtk3-macros@0.18.2` | same GTK3 chain (build-time derive) | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0420 | `gtk-sys@0.18.2` | same GTK3 chain | same lockfile-only | unmaintained (withdrawn **2026-08-14**) | None needed | **remove** (done) |
| RUSTSEC-2024-0370 | `proc-macro-error@1.0.4` | `lightspeed-gui → tray-icon → (libappindicator→glib→glib-macros \| gtk→gtk3-macros) → proc-macro-error` | Build-time proc macro on the same never-compiled GTK path | unmaintained, no patched release | Alternative manyhow/proc-macro2-diagnostics, but it arrives through upstream GTK tooling that is never compiled here | **keep** |
| RUSTSEC-2024-0436 | `paste@1.0.15` | `lightspeed-client (ml) → linfa-linear → argmin → paste` | Build-time macro, `ml` feature only (CI feature matrix; not in any dist feature set) | unmaintained, no patched release | Alternative pastey; removal needs an argmin/linfa change | **keep** |
| RUSTSEC-2024-0437 | *(absent)* | No `prometheus`/`protobuf` in the lock or any manifest (0 matches) | None — crate is not in the graph | **vulnerability** (DoS stack overflow; CVE-2025-53605), patched >= 3.7.2 | Remove the ignore; nothing to fix because nothing is present | **remove** (done) |
| RUSTSEC-2025-0141 | `bincode@1.3.3` | `lightspeed-client` direct dep, optional (`ml` feature) | Runtime deserialization of a **local** model file (`client/src/ml/predict.rs:78`; path from `client/src/ml/mod.rs:76`), never network input; not in a shipped feature set | unmaintained, no patched release — upstream calls 1.3.3 complete | Alternatives wincode/postcard; no security fix required | **keep** |
| RUSTSEC-2024-0384 | *(absent)* | `linfa 0.8.1` no longer depends on `instant` (its deps: `approx, ndarray, num-traits, rand, sprs, thiserror`); 0 matches in the lock | None — crate is not in the graph | unmaintained, no patched release | Remove the ignore | **remove** (done) |
| RUSTSEC-2026-0192 | `ttf-parser@0.25.1` | `lightspeed-gui → eframe → winit → sctk-adwaita → ab_glyph → owned_ttf_parser → ttf-parser` (Linux/BSD GUI build only: winit gates `sctk-adwaita` to non-macOS Unix), plus the `lightspeed-gui` **dev-dependency** `ab_glyph` (tests) | Linux GUI window-decoration title text: parses the locally configured system title font (embedded Cantarell fallback); not network-facing, not in the shipped Windows GUI binary, test-only elsewhere | unmaintained, no patched release | `skrifa` is already in the tree (epaint 0.37 renders fonts with it), but `ab_glyph` now arrives via winit/`sctk-adwaita` and the font-coverage test; removal needs an upstream `sctk-adwaita`/winit change, not our code | **keep** |
| RUSTSEC-2026-0370 | `zvariant@5.15.1` | `lightspeed-gui → eframe → egui-winit → accesskit_winit → accesskit_unix → zbus → zvariant` | Linux GUI build only (`eframe` default features include `accesskit`; `accesskit_unix` is Linux-only); local D-Bus accessibility IPC, no network | unmaintained, no patched release | Merged into zbus 6 as `wire`; blocked on an eframe/accesskit upgrade | **keep** |
| RUSTSEC-2026-0371 | `zbus_names@4.3.4` | same accessibility chain (`zbus`) | Linux GUI build only; local D-Bus | unmaintained, no patched release | Merged into zbus 6 as `names`; blocked upstream | **keep** |
| RUSTSEC-2026-0372 | `zvariant_derive@5.15.1` | same accessibility chain (`zvariant`) | Linux GUI build only, build-time derive; local D-Bus | unmaintained, no patched release | Derive macros in zbus 6; blocked upstream | **keep** |
| RUSTSEC-2026-0543 | `chrono@0.4.45` | `lightspeed-client` and `lightspeed-proxy`, both **direct, non-optional** (`client/Cargo.toml:70`, `proxy/Cargo.toml:46`) | Runtime in every shipped client/proxy binary; wall-clock timestamps and ML time-of-day features only, no untrusted parsing | unmaintained (informational), no patched release | Migrate to jiff/time — **7 use sites, in this repo's control** (scope below) | **keep until replaced** (migration scope below) |

Cross-checks for the two "absent" rows: `Cargo.lock` and every `Cargo.toml`
contain **0** occurrences of `protobuf`, `prometheus` and `instant`
(`07-counts-reconciliation.txt`).

## Changes applied this round

### `deny.toml`

- Removed 9 entries: `RUSTSEC-2024-0412/0413/0415/0416/0418/0419/0420` (GTK3,
  withdrawn) and `RUSTSEC-2024-0437` (`protobuf`, absent) and
  `RUSTSEC-2024-0384` (`instant`, absent). The list is now 8 entries.
- Why this is provably safe: a withdrawn advisory is not reported at all
  (cargo-deny's `advisory-not-detected` diagnostic names withdrawal as the
  cause), and a crate that is not in the lock cannot match an advisory. Removing
  an ignore can therefore only remove `unused-ignored-advisory` warnings; it
  cannot turn a passing gate into a failing one. The one identified failure mode
  would be an advisory DB older than 2026-08-14, which is not expected in CI: the
  job runs `cargo deny check --all-features` with no `--offline` /
  `--disable-fetch`, and cargo-deny fetches the DB up to date when fetching is
  enabled (docs: "the database is always cloned or fetched to be up to date with
  the remote git repository"), with a fetch failure failing the job rather than
  silently reusing an old DB. The weekly schedule also means the DB is refreshed
  even with no pushes.
- Refreshed the reasons of the 8 survivors to name the dependency path, the
  exposure, and the upstream blocker; added a header stating the allowlist
  policy, last review date and next review date.
- `[licenses]`, `[bans]` and `[sources]` are unchanged.

### `SECURITY.md`

- Supported-versions table corrected from `1.6.x` (five minor versions stale) to
  `1.11.x`, with `≤1.10.x` end-of-life; links `docs/quic-alpn-rollout.md` for
  why older clients cannot register on the community relays.
- Added a "Dependency advisories" security-design bullet: `cargo audit` and
  `cargo deny` run per-push and weekly, every ignored advisory carries a written
  reason, and the allowlist is re-reviewed quarterly with a link to this review.

### Not changed (out of the task's write scope) — recommendations

1. **`.cargo/audit.toml` carried a duplicate, stale `RUSTSEC-2024-0437` ignore**
   whose comment said "transitive via prometheus". `prometheus` is not in the
   lock. **Resolved by the Lead on 2026-11-18:** the entry was removed and the
   file now keeps an empty `ignore` list, with `deny.toml` named as the canonical
   allowlist. `cargo-audit` ignores by advisory ID only and does not read
   `deny.toml`, so the duplicate was the artifact, not a mitigation.
2. **Turn stale ignores into a CI failure:** set
   `[advisories] unused-ignored-advisory = "deny"`. After this cleanup the list
   has no unused entries, so the setting would be green now and would fail the
   next time an advisory is withdrawn or a crate is upgraded out of an affected
   range — exactly the drift this review found. Not applied here because it
   changes gate strictness and is a policy decision.
3. **Per-entry expiry:** cargo-deny supports
   `{ id = "...", reason = "...", expiry = "YYYY-MM-DD" }`. Absolute dates are
   safer than durations, because a *duration* is relative to the advisory's
   publish date, not to the review date. This enforces re-justification; not
   applied for the same policy reason.
4. Add this document to the `docs/README.md` index if the docs index is meant to
   be exhaustive (outside this task's write scope).

## Review cadence recommendation

Run one review **per quarter** (next: 2027-02-18) **and on any of these
triggers**:

- the `Security Audit` workflow goes red (per-push or the weekly Monday 06:00
  cron in `.github/workflows/security.yml` picks up new advisories);
- a new RustSec advisory lands for a crate in the allowlist (the weekly run is
  the detector — the review is to re-justify, not to notice);
- a dependency bump (`cargo update`, eframe/egui/accesskit, linfa/argmin)
  changes who pulls an ignored crate, or moves a crate out of an affected range;
- a release: re-read the list before tagging, because `deny.toml` ships in the
  repo and is part of the release's security posture.

Each review must, for every entry: (1) confirm the crate is still in
`Cargo.lock`, (2) re-derive the dependency path, (3) re-check the advisory for a
patched release or withdrawal, (4) confirm the reason text still describes the
exposure, and (5) remove the entry if it is inert. Record the review in WAT state
and `wat/evidence/<date>/team/sec-ops/`. Owner: `sec-ops` (`deny.toml` is in the
persona's ownership).

The natural **removal triggers** for the current 8: an eframe/egui upgrade that
moves `accesskit` to zbus 6 (`zvariant`, `zbus_names`, `zvariant_derive`), a
winit/`sctk-adwaita` change that drops `ab_glyph` (`ttf-parser`; epaint already
uses `skrifa`), and the `chrono` migration (`RUSTSEC-2026-0543`).
`proc-macro-error` and `paste` have no upstream migration on our side; they stay
until the tooling that pulls them is gone.

## `chrono` migration scope (RUSTSEC-2026-0543)

The advisory is informational ("chrono is unmaintained", all versions, no
patched release, alternatives jiff/time — `06-advisory-db-summary.md`). It is
still the highest-value item in the list because `chrono` is compiled into both
shipped binaries, unlike every other kept ignore. The audit counted 7 places in
5 files; that is exact, and **all 7 are unconditional** (no `cfg(feature =
"ml")` on the files or functions — `05-chrono-use-sites.txt`):

| # | Site | Use |
|---|---|---|
| 1 | `client/src/processes.rs:79` | `chrono::DateTime::from_timestamp` + `%Y-%m-%d %H:%M:%S` (UTC) in `format_unix_timestamp` |
| 2 | `client/src/processes.rs:416` | `chrono::Utc::now().timestamp()` in the Linux `/proc` scan |
| 3 | `client/src/ml/collector.rs:118` | `chrono::Local::now()` → hour-of-day / day-of-week |
| 4 | `client/src/ml/features.rs:204` | `chrono::Local::now()` in `extract_features` |
| 5 | `client/src/route/selector.rs:322` | `chrono::Local::now()` hour-of-day (ML feature input) |
| 6 | `client/src/route/selector.rs:327` | `chrono::Local::now()` day-of-week (`%u`, chrono Mon=1) |
| 7 | `proxy/src/budget.rs:218` | `chrono::Utc` year+month → `current_month_key()` (egress budget month) |

Migration notes (scope, not a plan of record — needs an owner/round):

- `time 0.3.55` is **already in `Cargo.lock`** (via `rcgen`/`asn1-rs`/
  `x509-parser`), so making it a direct dependency adds no new crate download;
  `jiff` is **not** currently in the lock and would be a new dependency.
- Sites 1, 2 and 7 are UTC formatting/arithmetic and map cleanly to
  `time::OffsetDateTime` or `jiff::Timestamp`.
- Sites 3-6 need **local** wall-clock time. `time` requires the `local-offset`
  feature and can fail to determine the offset (needs a fallback), while jiff
  resolves the system time zone. Whichever is chosen, the local-time-zone
  semantics must be preserved: these values are ML feature inputs that the
  trainer/collector compute the same way, so changing UTC-offset behaviour would
  silently shift a feature. This is the only non-mechanical part of the work.
- The workspace enables chrono's `serde` feature, but no chrono type is
  serialized (only `DateTime` at `processes.rs:79` appears, in a format call), so
  the feature can be dropped with the crate.
- Suggested single PR: add jiff/time to `[workspace.dependencies]`, convert the 7
  sites, remove `chrono` from both crates, delete the `RUSTSEC-2026-0543` ignore,
  and let the `Security Audit` workflow prove the gate. Keep it separate from the
  optional `ml` work; chrono is unconditional and can land first.

## Evidence

All under `wat/evidence/2026-11-18/team/sec-ops/` (gitignored):

| File | Contents |
|---|---|
| `01-baseline.txt` | review time, HEAD, worktree status, `Cargo.lock`/`deny.toml`/`SECURITY.md` SHA-256, tool availability (`cargo-deny`/`cargo-audit` absent) |
| `02-deny-toml-before.toml` | `deny.toml` as reviewed, before the edit |
| `03-cargo-lock-versions.txt` | resolved version (or "NOT PRESENT") for all 17 advisory crates |
| `04-dep-paths.txt` | full dependency paths from the `Cargo.lock` graph walk |
| `05-chrono-use-sites.txt` | every `chrono` source line, and the 7 use sites |
| `06-advisory-db-summary.md` | RustSec metadata for all 17 advisories + fetch URLs + cargo-deny doc citations |
| `07-counts-reconciliation.txt` | 17 vs 18 reconciliation, `.cargo/audit.toml` duplicate, zero-match checks |
| `08-toml-validation.txt` | `tomllib` parse of the edited `deny.toml` (8 entries) + explicit "cargo-deny not run" |
| `09-diff-deny-security.txt` | `git diff` of `deny.toml` and `SECURITY.md` |
| `10-after-hashes.txt` | SHA-256 after the edit and the final entry count |
| `11-target-gating.txt` | the `cfg` edges proving the GTK path is never compiled, and eframe's default `accesskit` |

## Sources

RustSec advisory DB (one file per advisory, fetched 2026-11-18 — full list in
`06-advisory-db-summary.md`), e.g.:

- <https://rustsec.org/advisories/RUSTSEC-2024-0412.html> (GTK3, withdrawn)
- <https://rustsec.org/advisories/RUSTSEC-2024-0437.html> (protobuf DoS, patched >= 3.7.2)
- <https://rustsec.org/advisories/RUSTSEC-2026-0370.html> (zvariant → zbus 6)
- <https://rustsec.org/advisories/RUSTSEC-2026-0543.html> (chrono unmaintained)

cargo-deny behaviour:

- <https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html> — defaults
  (`unmaintained = "all"`, `unused-ignored-advisory = "warn"`), `ignore`,
  `expiry`/`ignore-expiry`, DB fetch behaviour.
- <https://embarkstudios.github.io/cargo-deny/checks/advisories/diags.html> —
  `advisory-not-detected` ("could happen if the advisory was withdrawn"),
  `advisory-ignored`, `advisory-ignore-expired`.

Repo files: [`deny.toml`](../deny.toml), [`SECURITY.md`](../SECURITY.md),
[`.cargo/audit.toml`](../.cargo/audit.toml),
[`.github/workflows/security.yml`](../.github/workflows/security.yml),
[`Cargo.lock`](../Cargo.lock).
