# Current Phase: WF-035 Pages pipeline had no push trigger for its own inputs (maintenance)

**Workflow:** WF-035; WF-034, WF-033, WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** DevOps + InfraDev
**Status:** Committed and pushed (24bce57), Pages run in flight
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-035 the publishing pipeline could not be restarted by a push

**Driving evidence:** after the WF-026..WF-034 release was pushed (`a15f6b3`),
CI and Security Audit ran but **Deploy GitHub Pages did not** - so the public
registry stayed frozen at its 2026-09-20 publication and the stats feed at
01:46Z, even though the push had just proven the freeze was not a repo defect.

**Root cause:** `pages.yml` watched `paths: ['web/**']` on push, while the
pipeline it defines actually depends on `infra/scripts/network-stats.sh`,
`infra/scripts/collect-metrics.sh`, `infra/scripts/recommend-regions.sh` and
`infra/geo/*`. Two consequences, one of which had already bitten:
1. Changing a collector could never refresh the site that collector feeds.
2. The ONLY remaining trigger was the hourly cron, which GitHub throttles on
   inactive repositories - so when the cron went quiet the pipeline had no
   push-based fallback, and the registry sat stale for eleven days while every
   scheduled run reported `success`.

**Change:** the push trigger now covers `web/**`, `infra/scripts/**`,
`infra/geo/**` and `.github/workflows/pages.yml` itself, so a change to any
input - or to the workflow - restarts publishing. YAML verified by parsing:
four path filters, schedule and workflow_dispatch intact, 7 jobs unchanged.

**Verification:** pushing the change (which matches its own path list) triggered
`Deploy GitHub Pages` from a **push** event at 08:10:48Z - the first push-driven
Pages run on record; every previous run was `schedule`. Completion and the
registry refresh are tracked by a live monitor rather than assumed.

**Not changed:** no collector logic, no catalog data, no infrastructure.

---

# Current Phase: WF-034 Fleet-wide traffic stop had no detector (maintenance)

**Workflow:** WF-034; WF-033, WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** SecOps + QAEngineer
**Status:** Committed and pushed (a15f6b3) - first runner verification in progress
**Last updated:** 2026-10-01

---

## 2026-10-01 - Release: nine workflows' worth of maintenance landed on master

**Owner approval:** the maintainer was asked how to proceed and explicitly
authorised "commit and push to master", which is the action this entry records.

**What was pushed:** everything from WF-026 through WF-034 in one commit,
`a15f6b3` (13 files, +1631/-21): the three new detectors (`abuse_flood`,
`stale_history`, `fleet_idle`), the CI `infra-script-tests` job plus three new
suites (`test_bump_packages`, `test_lib_nodes`, `test_geo_catalogs`), the
Chocolatey packer portability fix, the README install-table correction, and the
WAT state/decision records.

**Why this mattered beyond the diff:** the repository had not been pushed since
01:46Z, and CI triggers only on push/PR to `master`. Every verification in
WF-026..WF-034 was therefore local-only. The push started **CI**, **Security
Audit**, and **Deploy GitHub Pages** at 07:53:45Z - the Pages run being what
restores the public site, the signed registry (frozen since 2026-09-20), and
the stats feed that `stale_history` exists to watch.

**Open verification:** the CI run (36833003765) exercises the new
`infra-script-tests` job for the first time on a real runner. Local results
predict green (7 suites pass; `collect_metrics` and `analyze_mesh` fail only
under this host's MinGW curl, which returns empty for `file://`). That
prediction is not yet confirmed, and this entry is written before the run
finishes rather than after.

---

## 2026-10-01 - WF-034 every relay can go silent and the monitor reports clean

**Driving evidence:** re-reading the live fleet at 07:41Z showed the cumulative
counters BYTE-IDENTICAL to the 07:13Z reading - `relay-fra` still
`packets_relayed=19005389`, `drops_abuse_blocked=1263983` - while
`uptime_secs` advanced ~1,700s. All eight relays answer `/health` healthily and
report version 1.6.14, but nothing is moving: the fleet stopped carrying
traffic roughly when the Pages cron stopped firing at 01:46Z.

**The gap:** `zero_relay` (the detector written for the 1.6.3 outage) requires
`$fleet_pkts > 0` - it fires when ONE relay is silent while the fleet is busy.
That guard exists so a quiet relay is not mistaken for a broken one, and it is
correct for its case - but it means a fleet-wide stop matches NOTHING. Proven by
fixture before changing code: an all-idle history produced
`health-anomaly: no anomalies (2 relay(s), 8 snapshot(s), window=3)`. Together
with WF-028 (frozen collector) this left the monitor unable to see either half
of a dead pipeline: it noticed neither a stopped collector nor a stopped fleet.

**Change:** a new `fleet_idle` detector (critical) fires when every reachable
relay relayed zero packets AND created zero sessions across the sustained
window, with a guard so a busy fleet stays silent. It is deliberately the
complement of `zero_relay`, and the two are now documented against each other
so a future reader does not collapse them again.

**Three of my own defects were caught by running the tests, not by reading
code:**
1. The first version omitted the `as $id` binding inside the `$ids[]`
   comprehension, so jq failed to COMPILE - and the script's `2>/dev/null`
   turned that into the empty-result fallback, reported as `0 relay(s)`.
   Diagnosed by unsuppressing stderr, then by isolating the fragment.
2. The suite's existing guard asserted "an all-idle fleet raises nothing,"
   which is exactly the behaviour this workflow reverses. It was rewritten -
   although the first rewrite was wrong too, asserting the single-idle-relay
   case was non-anomalous when `zero_relay` fires there by design.
3. The corrected assertion then matched the phrase "zero_relay" appearing in
   the fleet_idle MESSAGE text, a false positive in the test rather than the
detector; it now matches the emitted type (`zero_relay:`).

**Verification:** `test_health_anomaly.sh` -> all assertions passed
(**82 checks**, up from 77), including a new all-idle case, its critical
severity, the evidence string, and a busy-fleet guard. All seven infra suites
still pass. Against the live history the detector reports exactly the two
real conditions (`stale_history` at 21,764s and `abuse_flood` on relay-fra).

**Not changed:** no source, packaging, workflow, or infrastructure file.

---

# Current Phase: WF-033 Placement geo catalogs get integrity coverage (maintenance)

**Workflow:** WF-033; WF-032, WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** QAEngineer + InfraDev
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-033 the recommender's reference data had no validation

**Driving evidence:** widening the audit past `infra/scripts` surfaced the
placement geography catalogs, `infra/geo/regions.json` and
`infra/geo/candidates.json`. Their own README states the contract: all
coordinates are WGS 84 decimal degrees, region keys are stable lowercase
slugs, country keys are ISO 3166-1 alpha-2, and `regions.<key>.label` is the
display name. Nothing validated any of it. Unlike a script, a broken catalog
cannot crash a run - it silently degrades the recommender: a country pointing
at a removed region becomes an unmapped cell, a candidate whose region no
longer exists drops out of ranking, and an out-of-range latitude skews every
distance computed from it.

**Change:** new `infra/scripts/test_geo_catalogs.sh` (17 checks) pins the
declared contract: both catalogs parse with `schema_version` 1; region keys are
lowercase slugs and country keys are ISO alpha-2; every country, region alias,
and candidate region resolves to a declared region; all coordinates are
numeric and within +-90 latitude / +-180 longitude; every region has a label;
candidate ids are unique and all recommender params numeric. Wired into
`infra-script-tests` in `ci.yml` as a ninth step.

**The suite includes negative controls, so it can be shown to fail:** a
fixture with a country mapped to a nonexistent region must be DETECTED, and a
fixture with latitude 999 must be DETECTED. A test that only proves today's
data is clean would pass forever even if its own predicates were broken.

**Current data is clean:** 8 regions, 143 countries, 8 relays, 37 aliases,
no dangling references, all coordinates in range - so this round adds
protection, not a fix.

**Three of my own defects were caught while building it**, recorded because
the first is why the third was hard to see: a malformed `region_aliases` jq
expression, an edit that collapsed a newline and concatenated two statements
(caught by `set -u` as an unbound variable), and - most importantly - an
`offenders` helper that swallowed jq errors by echoing a placeholder into the
comparison, so a broken predicate looked like a passing check. The helper now
reports a jq evaluation failure as an explicit FAIL.

**Not changed:** no catalog value, source, or infrastructure file was touched.

---

# Current Phase: WF-032 Chocolatey packer portability and failure clarity (maintenance)

**Workflow:** WF-032; WF-031, WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** DevOps + RustDev
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-032 the Chocolatey packer broke on macOS and failed cryptically

**Driving evidence:** widening the audit beyond `infra/scripts` surfaced
`dist/chocolatey/build.sh` - the script that produces the `.nupkg` pushed to
the Chocolatey feed. Two defects, both verified:

1. **GNU-only regex.** Version extraction used `grep -oPm1 '(?<=<version>)[^<]+'`,
   and `-P` is a GNU extension: BSD/macOS `grep` rejects it with "invalid
   option". The README tells macOS users to build from source, so the one
   script a packaging contributor needs was broken on their platform. This is
   the only `grep -oP`/`grep -P` use in the entire repository.
2. **A guard that a stub defeats.** The script calls `python3` with no
   presence check, so on a host where the name resolves but execution fails
   (the Windows Microsoft Store alias stub is exactly this) the user sees the
   Store's misleading "Python was not found" prompt rather than a build error.

**Change:** version extraction now uses a portable `sed` expression with an
explicit empty-value check, and the script refuses to continue unless
`python3 -c 'import sys, zipfile'` actually succeeds - testing that the
interpreter RUNS, not merely that a name resolves on PATH.

**Verification:**
- syntax: `bash -n` clean.
- equivalence: the GNU form and the POSIX form both yield `1.6.14` against the
  live `lightspeed.nuspec`, so the replacement is not a behaviour change.
- failure path: on this host the guard now prints
  `build.sh: python3 (with the zipfile module) is required to pack the .nupkg`
  and exits 1, where before it emitted the Store prompt; no `.nupkg` is left
  behind on failure, so a half-built package cannot be pushed.
- `command -v` alone was tried first and found INSUFFICIENT (the Store alias
  satisfies it), which is why the check executes python3 instead.

**Not changed:** the packer's OPC member contract (`lightspeed.nuspec`,
`[Content_Types].xml`, `_rels/.rels`, `tools/chocolateyInstall.ps1`) is
unchanged and was confirmed against the script source and the README. No
workflow runs this script - the release job only bumps `dist/chocolatey` and
commits, so packing remains a manual maintainer step; that is unchanged, and
noted here so the absence of CI coverage is recorded rather than assumed.

---

# Current Phase: WF-031 Documentation truthfulness pass (maintenance)

**Workflow:** WF-031; WF-030, WF-029, WF-028, WF-027, WF-026, WF-023 below
**Agent:** TechWriter + QAEngineer
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-031 the README promised install commands that cannot work

**Driving evidence:** every package-manager claim in the README was checked
against the live channel, and two were false in a way that wastes a user's
time rather than merely being out of date:

| Channel | README said | Verified reality |
|---|---|---|
| winget | "submitted, awaiting Microsoft review" | **not published at all** - `microsoft/winget-pkgs` returns 404 for the manifest path and the bootstrap PR was closed pending an unsigned CLA |
| Chocolatey | "submitted, pending moderation" | live, but serving **1.6.3** only |
| Scoop | (bare command) | bucket live, pinned to **1.6.3** |
| Homebrew | (bare command) | current - `Formula/lightspeed.rb` at the latest release |

So `winget install ShibbityShwab.LightSpeed` fails outright, and the
Chocolatey/Scoop commands install a binary many releases old, while the page
narrates them as "awaiting review".

**Change:** the README install table now states each channel's real status,
notes the stale pin inline, marks winget as unpublished with a pointer to the
release MSI, and adds a short Windows advisory recommending Releases over a
package manager. Two docs carried stale version framing
(`docs/architecture.md` and `docs/protocol.md` both read "Last updated:
2026-09-22 - Reflects v1.6.5" against a 1.6.14 workspace); their substantive
claims were verified before restamping.

**Verification of the doc claims themselves, not just the wording:**
- game count: `docs/supported-games.md` numbered rows are exactly **19**, and
  `client/src/games/` holds 20 files minus `mod.rs` - an earlier read of "31"
  counted every `|` line including headers and was wrong.
- relay count: the live registry still resolves to **8** nodes.
- protocol: UDP **4433** control / **4434** data are asserted in both
  `proxy/src/config.rs` and `client/src/config.rs` tests; `session_token` is
  present in `protocol/src/header.rs`; TCP framing exists in
  `protocol/src/framing.rs`.

**Not changed:** no source, packaging, workflow, or test file was touched. No
test pins this prose, which is correct - doc wording is not a machine-consumed
value.

---

# Current Phase: WF-030 Shared node resolver gets regression coverage (maintenance)

**Workflow:** WF-030; WF-029, WF-028, WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-030 lib-nodes.sh is shared by every ops script and had no test

**Driving evidence:** continuing the WF-029 audit of untested infra scripts.
`lib-nodes.sh` is not a leaf utility: `deploy.sh` and `mesh-health.sh` both
source it to decide WHICH host an ops command targets, and it was untested.
When it resolves the wrong node, a deploy or a health probe silently runs
against the wrong machine - and its most dangerous failure is returning an
empty list, which a caller would iterate zero times and report success.

**Change:** new `infra/scripts/test_lib_nodes.sh` (35 checks) pins the
contracts the module documents: an `LIGHTSPEED_NODES` array override wins over
the registry; a region-keyed object override normalizes to an array; a bare
string value is treated as the ip; defaults apply (`:4434` data,
`:8080` health/metrics, control `4433`); an explicit field is never overwritten
by a default; a custom `LIGHTSPEED_CONTROL_PORT` applies only where a node
omits one, and a non-numeric value falls back; an empty override (`{}`/`[]`)
falls through to the registry; the registry path is independent of the CWD; and
every failure path (invalid override JSON, missing registry, registry without a
payload, zero nodes) fails loudly. Wired into `infra-script-tests` in `ci.yml`
as an eighth step.

**Three harness bugs of mine were caught by this test, not by review** - worth
recording because each produced a confident wrong answer:
1. The registry fixture wrapped a bare node array; the real document is
   `{"registry": "<json string containing {schema_version, nodes}>"}`. The
   library correctly rejected it, and my first run showed 16 red assertions.
2. `run_nodes` passed `LIGHTSPEED_CONTROL_PORT=6000` as a command rather than
   an environment assignment, so bash reported "command not found".
3. The explicit-field case used a node-shaped OBJECT override. The object form
   is region-keyed (`{"tokyo":"1.2.3.4"}`), so the library exploded the node's
   keys into four junk nodes. The array form is the correct one for explicit
   fields.

**Verification:** `test_lib_nodes.sh` -> all assertions passed (35 checks).
Suites from earlier rounds still green: `test_bump_packages.sh` 21,
`test_health_anomaly.sh` 77. `ci.yml` parses with 11 jobs and 8 infra test
steps. Against the REAL `web/registry.json`, `lightspeed_resolve_nodes` returns
8 nodes - matching the eight live relays - which is independent evidence that
the fixture shape mirrors production.

**Not changed:** no source, packaging, or infrastructure file was touched.

---

# Current Phase: WF-029 Release package-bump scripts get regression coverage (maintenance)

**Workflow:** WF-029; WF-028, WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-029 the two scripts that rewrite shipped installers had no tests

**Driving evidence:** a normalization-corrected audit of `infra/scripts/*.sh`
against `test_*.sh` found 13 scripts with no coverage. Two of them are the
highest-consequence of the set: `bump-chocolatey.sh` and `bump-homebrew.sh`
run automatically in `release.yml` on every tag and rewrite the version plus
EVERY sha256 in the manifests users actually install from. A silent `sed` miss
there publishes a package whose checksum does not match its artifact, and the
failure surfaces as a hash mismatch at install time - long after the release.

**Change:** new `infra/scripts/test_bump_packages.sh` (21 checks) exercises
both scripts with the `gh` binary stubbed on PATH (the stub honours `--jq`, so
the scripts run exactly as in CI, with no network or credentials) against
copies of the real manifests, so the originals are never mutated. It asserts
the happy path (version, download URL, and checksum all move together for a
specific asset) and the failure paths, which are the load-bearing part: a
release missing the Windows zip, a release with the wrong tarball count, a
release missing one of four tarballs, and missing manifest files must each
abort and leave the manifests byte-identical. Wired into the existing
`infra-script-tests` job in `ci.yml` as a seventh step.

**Two false alarms this round, both from my own harness rather than the
scripts** - recorded because acting on either would have meant "fixing"
correct code: a first stub ignored `gh`'s `--jq` flag and returned raw JSON,
which made `bump-homebrew` report "expected 4 client tarballs, found 5"; and a
manually reconstructed jq filter produced empty output from quote-mangling.
Only after the stub faithfully honoured `--jq` did both scripts prove correct:
version, checksum, url, and all four platform digests are rewritten exactly,
and a missing asset aborts BEFORE any write, leaving files untouched.

**Verification:** `bash infra/scripts/test_bump_packages.sh` -> all assertions
passed (21 checks). `git diff --stat -- dist/chocolatey Formula` is empty after
the run, confirming the suite is non-destructive against the real manifests.
The workflow still parses with 11 jobs and the new step present.

**Not changed:** no source, packaging manifest, or infrastructure file was
touched; this round adds test coverage only.

---

# Current Phase: WF-028 Stale-telemetry detection (maintenance)

**Workflow:** WF-028; WF-027, WF-026, and WF-023 remain below
**Agent:** QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-028 the monitor could not tell "healthy" from "not receiving data"

**Driving evidence:** while surveying live state, the published telemetry was
found frozen. `web/network-history.json` (stats branch) had no commit newer
than `2026-10-01T01:46:16Z`, and the repo's `pushed_at` is
`2026-10-01T01:46:17Z` - the hourly Pages cron stopped the moment repository
activity stopped. By 07:13Z five consecutive hourly slots had been missed while
every other scheduled workflow kept running (Health Monitor fired at 07:02Z),
so the scheduler itself was healthy and the stall was specific to Pages. At
`05:57Z` the anomaly job had fetched that same frozen file, logged
`history: 76 snapshot(s)`, and reported **`no anomalies`** - it was monitoring a
five-hour-old past and calling it healthy. This is the 1.6.3 lesson repeating:
a check that cannot distinguish "fine" from "not observing" is not a check.

**Change:** `infra/scripts/health-anomaly.sh` gains a `stale_history` detector
(warning) with a configurable `--max-staleness` window (default 10800s / 3h).
A pinned clock seam (`LIGHTSPEED_NOW_EPOCH`) keeps assertions deterministic
instead of racing the wall clock. `test_health_anomaly.sh` pins that clock for
the whole suite and adds four cases: just inside the limit (silent), just past
it (fires), the evidence string, and `--max-staleness 0` disabling the check.
`.github/workflows/health-anomaly.yml` now passes `--max-staleness 10800`
explicitly and its stale comment ("written by pages.yml every 6h", actually
hourly) is corrected.

**Design note:** an ABSENT history is deliberately not stale - it has no
snapshots to age - so a first run or a freshly created stats branch cannot
produce a false alert. Verified directly: an empty history exits 0 with no
anomalies.

**Verification:** `test_health_anomaly.sh` -> all assertions passed
(**77 checks**, up from 69). Boundary behaviour proven by pinning the clock:
`+10799s` stays silent, `+10801s` fires, pinned-fresh is clean. Against the
real frozen feed the detector reports `newest snapshot is 20038s old (limit
10800s)` and the run exits 1, so the hourly job now fails (and alerts) instead
of passing green on dead data.

**Two of this round's own mistakes, caught by verification:** the first rule
tested `$newest <= 0` instead of comparing age, so it fired unconditionally
(an always-on alert); and the first test pin assumed fixture timestamps near
1700000000 when the fixtures deliberately use small counters (1000-1700),
which broke 13 checks. Both were found by running the suite, not by reading it.

**Not changed:** no source, packaging, or infrastructure file was touched.

---

# Current Phase: WF-027 Orphaned infra self-tests wired into CI (maintenance)

**Workflow:** WF-027 (CI coverage); WF-026 and WF-023 remain below
**Agent:** DevOps + QAEngineer
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-027 nine infra self-tests were never executed by CI

**Driving evidence:** a coverage audit of `.github/workflows/ci.yml`
against `infra/scripts/test_*.sh` found that of twelve self-test
scripts, only three were reachable from any workflow
(`test_proxy_quic_smoke.sh` and `test_deploy_canary.sh` in `ci.yml`,
`test_health_anomaly.sh` in `health-anomaly.yml`). Nine were referenced
by nothing but themselves: every change to `collect-metrics.sh`,
`analyze-mesh.sh`, `recommend-regions.sh`, `egress-budget.sh`,
`inventory.sh`, or `append-history.sh` shipped unverified.

**Change:** `ci.yml` gains an `infra-script-tests` job (`ubuntu-latest`)
running six of the orphaned suites: `test_collect_metrics.sh`,
`test_recommend_regions.sh`, `test_egress_budget.sh`,
`test_inventory.sh`, `test_append_history.sh` (with a 5-minute bound),
and `test_analyze_mesh.sh`. All read local `file://` fixtures via
bash + curl + jq, so the job needs no services, secrets, or network.

**Three suites deliberately left out of CI, with the reason recorded in
the workflow comment so a future agent does not "fix" the omission:**
`test_handoff_e2e.sh` (requires Linux + root to drive a real relay
in-place exec), `test_relay_updater.sh` (requires `flock`), and
`test_relay_install.sh` (asserts Unix ownership/mode outcomes such as an
0711 runtime dir). Wiring these into a container job would produce false
reds, not signal; they are exercised on the relay hosts themselves.

**Verification:** each suite was run locally before wiring. Passing here:
`egress_budget` (14 checks), `inventory` (28), `recommend_regions` (146).
`collect_metrics` (91 failures) and `analyze_mesh` (2 failures) fail on
this workstation for one proven environmental reason: its MinGW
`curl 8.21.0 (x86_64-w64-mingw32)` returns **empty output with exit 0**
for `file://` URLs, so every fixture read parses an empty document. That
was confirmed directly (`curl -s file://.../curlfix.json` printed nothing
while reporting RC=0), which is why both suites are expected green on the
Linux runner. `append_history` exceeds 60s here because each invocation
costs ~220ms of process-spawn overhead on MSYS (~18ms per fork, uniform
across `jq`, `mktemp`, and even `bash -c true`), so its 400-append cap
test projects to ~88s; that cost is host-specific (the script has no
production caller) and is why the CI step carries a 5-minute bound.
YAML validity was confirmed by parsing the workflow: the job is present
on `ubuntu-latest` with six steps and the other eleven jobs are intact.

**Not changed:** no source, packaging, or infrastructure file was
touched other than `ci.yml`.

---

# Current Phase: WF-026 Fleet abuse-flood detection (maintenance)

**Workflow:** WF-026 (detector hardening); WF-023 remains pending release below
**Agent:** SecOps + QAEngineer + DevOps
**Status:** Implemented, verified locally, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - WF-026 abuse-flood detector added to the anomaly monitor

**Driving evidence:** the 2026-10-01 review found `relay-fra` absorbing a
sustained abuse-blocked flood (+1,153,206 blocks over a 3-snapshot window,
99.9% of everything it dropped, ~967/minute) while `health-anomaly.sh`
reported `anomalies: []` and the hourly job passed green. The existing
detectors cover `zero_relay`, `auth_spike_no_sessions`, `version_lag`,
`saved_regression`, `negative_saving_spike`, and reachability; none observe
abuse-blocked volume.

**Change:** `infra/scripts/health-anomaly.sh` gains an `abuse_flood`
detector (severity `warning`) with two configurable floors:
`--abuse-min` (default 1000 window abuse blocks) and `--abuse-share`
(default 0.9 of the relay's drops). `infra/scripts/test_health_anomaly.sh`
gains a flood fixture plus three guards (tiny absolute count, mixed
traffic, single-snapshot window) and a JSON contract assertion.

**Two design corrections found by testing, not assumption:**
1. The share must be measured against **drops**, not `relayed + dropped`.
   relay-fra's real flood was 99.9% of its drops but only 46.8% of its
total handled volume, because a busy relay's legitimate traffic masks
the flood in that denominator.
2. The rule must **not** require flat sessions. Gating on
   `sessions_created <= 0` suppressed the very event it was written for:
the real window carried +1,153,206 abuse blocks *and* +37 real sessions.

**Verification:** `bash infra/scripts/test_health_anomaly.sh` -> all
assertions passed (69 checks, up from 53). Run against the live fleet
history and registry with the CI invocation shape, the detector reports:
`[warning] abuse_flood: relay-fra: abuse_blocked +1153206 = 99.9% of drops
(1154074 dropped, 1311829 relayed, sessions +37) over 3 snapshots` and
exits 1, so the hourly job would now fail (and post to Discord) instead
of passing silently.

**Threshold justification:** across the 76-snapshot history, 17
relay-windows hit `abuse >= 1000` *and* `abuse/drops >= 0.9`, spanning
both `relay-fra` and `relay-mad-1` over a week. The rule therefore
describes a recurring fleet condition, not an artifact of one snapshot.

**Not changed:** no source, packaging, or infrastructure file was
touched; publishing to Chocolatey/winget/Scoop remains a maintainer
step (see the 2026-10-01 decision entry).

---

# Current Phase: WF-023 Windows GUI Startup + WinDivert Teardown (community feedback)

**Workflow:** WF-023 (plus WF-025 observability, below)
**Agent:** RustDev + QAEngineer + DevOps
**Status:** Implemented, pending release
**Last updated:** 2026-10-01

---

## 2026-10-01 - Agent handover and external status review (omo-native)

**Change:** Agent operation handed over from the previous opencode-based agent to
omo-native. The repository held no opencode-specific artifacts, so this is a
tooling change only: `AGENTS.md`, `wat/rules.md`, and `wat/archive/agents.md`
remain the operating contract. See `wat/state/decisions.md` (2026-10-01) for the
full rationale.

**Local prerequisite gap:** this workstation has no Rust toolchain on PATH
(`cargo`, `rustc`, `rustup` all absent), so `[QUALITY_STUB]` commands
(`cargo fmt`, `cargo clippy`, `cargo test`) cannot run here and must be
exercised on a toolchain-equipped host or in CI.

### Findings from the 2026-10-01 review

| Track | Finding |
|-------|---------|
| GitHub feedback | 2 open issues: #137 (Minecraft support request, 2026-09-28, unactioned) and #59 (the Windows/Fortnite report driving this workflow, 17 comments). 6 open Dependabot PRs (#106-#109, #135, #136). Latest release 1.6.14 (2026-09-26); repo at 70 stars / 9 forks. |
| App markets | Chocolatey page is live but serves only **1.6.3** (11 releases behind). Scoop bucket is live but pinned to **1.6.3**. winget is **not published**: bootstrap PR microsoft/winget-pkgs#435790 open since 2026-09-16, and #437292 was closed 2026-09-19 pending the Microsoft CLA. Homebrew is current at **1.6.14** (the tap is this repository, not a separate tap repo). AUR has no `lightspeed-bin`; the taken `lightspeed` name is an unrelated app, and Arch registration is closed. |
| Network | All 8 relays healthy on 1.6.14, ~109h uptime. Sustained abuse-blocked flood on `relay-fra`: 173,080 of the newest window's 173,497 drops (~967/min), fleet drop rate up to 65.06%. No cost or availability breach (`drops_egress_budget` = 0 fleet-wide). `health-anomaly.sh` passed at 2026-10-01T05:57Z without flagging it - no detector covers abuse volume. Resolved by WF-026 above. |
| Packaging automation | The in-repo bump automation is **healthy**: `dist/chocolatey` is correctly at 1.6.14 with a matching sha256 and the release job commits it (`c892747`). The stale feeds are a **publish** gap, not a code gap: there is no `choco push` step in CI (the comment defers it to a manual maintainer push), Scoop has no automation in this repo at all, and winget needs the Microsoft CLA signed. |
| CI health | `ci.yml` is 30/30 successful across the last 30 runs, including `windows-test` and `windows-gui`, the two jobs WF-023's Windows verification gap depends on. |

---

## Summary

This workflow responds to the open feedback on the repo. The driving report is issue #59
(Yughaa): the v1.4.3 GUI does not launch at all on Windows, and the CLI still fails with
`FWP_E_IN_USE` (0x8032000A) on the relaunch after a clean Ctrl+C.

Root causes found by an Oracle-assisted audit plus four parallel explorer agents:

1. **GUI silent death.** `lightspeed-gui` is built with `windows_subsystem = "windows"`,
   so a panic or a returned `Err` produces no window and no output. The v1.4.2 GUI
   overhaul added the only new Windows-specific pre-window early-exit: the named-mutex
   single-instance guard, which read `GetLastError` without a `SetLastError(0)` reset and
   exited silently on the already-running path.
2. **WinDivert handle leak.** `--watch` had no Ctrl+C handling, so Ctrl+C was a hard
   kill; the receive thread parked in a blocking `WinDivertRecv` that an `AtomicBool`
   cannot wake (the `windivert` 0.6 wrapper has no `Drop` and its `shutdown` takes
   `&mut self`); the only close path was `Arc::try_unwrap`, which is unreachable while a
   thread blocks; and the legacy `capture/windivert_redirect.rs` backend (the GUI Boost
   path) never closed either handle.

| Item | Status |
|------|--------|
| GUI: panic hook + `gui-crash.log` + native error dialog | Added |
| GUI: logging initialized before the guard, non-fatal with temp/sink fallback | Fixed |
| GUI: `SetLastError(0)` before `CreateMutexW`, distinct exit code, `--force` bypass | Fixed |
| GUI: fallible tray so a tray failure cannot kill or strand the app | Fixed |
| GUI: `LIGHTSPEED_GUI_RENDERER=glow\|wgpu` escape hatch (glow feature added) | Added |
| Client: owned raw WinDivert handle (`&self` methods, `Drop` shuts down + closes) | Added |
| Client: `WinDivertShutdown` unblocks the parked recv; owner-thread teardown ack | Fixed |
| Client: both WinDivert backends close both handles on every stop path | Fixed |
| Client: `--watch`/`--start-interceptor` handle Ctrl+C and wait for teardown | Fixed |
| Client: GUI Quit waits (bounded) for interceptor and redirect teardown | Fixed |
| Client: firewall rule removal covered by the teardown ack | Fixed |
| Docs: corrected `FWP_E_IN_USE` guidance and single-instance description | Fixed |
| Project Zomboid profile registered in docs (PR #72) | Added |
| Dependabot base64 0.23 + patch group (PRs #61, #73) | Ready to merge |

---

## Verification

- `cargo fmt --all --check` clean; `RUSTFLAGS="-Dwarnings" cargo clippy --workspace
  --all-targets --exclude lightspeed-gui` clean for default, `quic`, `full`, and `ml`;
  `RUSTFLAGS="-Dwarnings" cargo clippy -p lightspeed-gui --all-targets` clean.
- `cargo test --workspace --exclude lightspeed-gui`: all 16 test binaries pass, 0 failed
  (client lib 203 tests, including 10 new `teardown` cases and 4 new `InterceptorHandle`
  stop/ack cases; 6 new engine ack cases). `cargo test -p lightspeed-gui`: 43 pass.
- GUI exercised headlessly on Linux under Xvfb: starts, writes the crash-safe trace log,
  discovers the five community relays, connects and registers a QUIC session, and stays
  alive. A forced second instance is detected by the guard.
- Windows-only code (the full WinDivert impls and the GUI tray/guard FFI) cannot be
  compiled on this host; it is verified by source review against `windivert-sys` 0.10 /
  `windows` 0.48 and must be confirmed by the `windows-test` and `windows-gui` CI jobs.

**Known verification gap:** real Windows runtime behaviour (WinDivert `recv` unblock,
close-after-ack, tray failure, message-box panic reporting) rests on the CI Windows build
plus the Linux-runnable unit tests for the shared decision and teardown logic. A real
Windows run on the reporter's machine is the final confirmation.

---

## Pre-release audit (2026-09-18)

Ran a functional audit on both platforms before any release, per the owner's
request. Everything below was exercised for real, not just built.

Verified working:
- Linux CLI: `--version`, `--list-games` (18), `--list-interfaces`, `--check`
  (user and root), `--status`, `--probe-proxies` (5 relays), `--test-control`
  (QUIC register + ping), `--live-test` (4/4 phases with `--fec`, data relay
  5/5 payload match), `--smoke-test` (full synthetic E2E, teardown, 0 leftover
  nftables tables), `--start-interceptor` + Ctrl+C, `--watch` + Ctrl+C,
  `--demo`, `--benchmark`, `--scan-processes`, `--intercept`, `--write-config`.
- Linux GUI under Xvfb: launch, discovery, QUIC registration, Boost (engages
  the nftables interceptor), Stop Boost, window-close Quit, clean teardown.
- Windows CLI in the VM: all read-only/network commands, `--test-control`,
  `--live-test` (4/4 phases), `--start-interceptor` + Ctrl+C (3/3 clean),
  `--write-config`, `--scan-processes`, `--intercept`, `--check`.
- Windows GUI in the VM: launch, discovery, registration, Boost (engages the
  WinDivert interceptor, firewall rule added), Stop Boost (rule removed,
  threads exit), single-instance guard, crash log path.

Bugs found and fixed during the audit:
- Linux/macOS leaked the kernel redirect rule on Ctrl+C (commit 0d3499f).
- Linux `--start-interceptor` ran forever without root (74dfbc6).
- `--check` reported a dead proxy as reachable (74dfbc6).
- GUI default window clipped the "BOOST MY GAME" button (0e75ca3).
- Windows: the WinDivert port-range filter also matched the client's own QUIC
  to the proxy, and the auto-detect tracker locked the proxy's address as the
  "game server", tunnelling the client's own traffic to itself (ee22be9).
  Found with a connected-UDP synthetic game on the VM; after the fix the
  interceptor locks the real game server (45.77.32.236:9999) and the client
  port (28015) with packets flowing through the relay.

Known issues left open (documented, not release blockers):
- Windows network-layer filtering cannot match `processId` (WinDivert supports
  it only at the Flow layer), so a broad-port-range game like Fortnite uses
  port-range interception plus debounce. The proxy address is now excluded, but
  other apps' UDP inside the range can still be intercepted briefly.
- `--test-control`/`--test-tunnel` print a stub success when built without the
  `quic` feature (release builds have `quic`).
- `--live-test` exits 0 even when a phase fails (manual diagnostic only).
- `config.general.log_level` is parsed but not applied to CLI logging.
- The GUI never calls `start_windivert`/`start_capture`, so those UI branches
  are unreachable; "Disconnect Boost Server" stops the relay loop but not an
  active interceptor (separate "Stop Boost").

Host note: the audit's VM shutdown hit the known RDNA4 `vfio_pci_remove` fault
the `gpu-switch` script documents, leaving the dGPU unbound until a reboot.

---

## Next Action

1. Reboot the host to restore the RX 9070 XT to `amdgpu` after the VM stop left
   it unbound (the vfio-pci teardown fault).
2. The owner decides on a v1.4.4 release (all audit fixes are on `master`).
3. WF-024 candidates: fix the `--live-test` exit code and the no-`quic` stub;
   `--repair-windivert` recovery command; registry cert pinning (F5) and binary
   release signing (C2) from the security backlog.

---

## WF-025: Observability overhaul (2026-09-19)

**Workflow:** WF-025
**Agent:** Sisyphus (Oracle design review + parallel explore/plan/implementation agents)
**Status:** Implemented on `feat/observability-overhaul`, pending review/release

Motivated by a live production probe that showed the proxy's observability was
partly hollow: the latency histogram was never populated, `fec_data_packets_total`
was always 0, `packets_dropped` conflated unauthenticated scanning with real loss,
the rate limiter never fired, telemetry reported a hardcoded game id and empty
country, and the six-hourly stats snapshot had no history.

Delivered (atomic commits `e7d90d8`..`44b4a5b`):

| Item | Change |
|------|--------|
| Response listener | Exactly one listener per session (was two) |
| Session lifetime | `last_activity` refreshed on traffic (was creation-only) |
| Drop semantics | `DropReason` categories + 7 flat `/health` fields; site shows "Packets Filtered" + "Upstream Loss" |
| FEC counter | `fec_data_packets_total` now incremented |
| Latency | Proxy-observed upstream response lag, non-cumulative buckets, 2s bound, discarded counter |
| Rate limiting | Per-IP aggregate tier (5000 pps / 5 MB/s, 65536 cap) + `check_at`; dead `max_connections` removed |
| Game ids | Protocol registry extended to all 18 games with key/id helpers |
| Telemetry | Client sends real game id + locale country; proxy aggregates by (game, country), k>=3 |
| History | Orphan `stats` branch, reset-safe bounded (360) history via `append-history.sh` |

**Verification:** full CI-parity locally (fmt, clippy `-Dwarnings`, release build,
17/17 test binaries, feature matrix `quic`/`full`/`ml`); live `/health` and
`/metrics` exercised; telemetry POST x3 rendered `game="cs2",country="US"`;
temp-repo stats + history fixture produced 2 snapshots with the 7 drop fields.

**Known gaps:** the optional trend UI (`network-history.json` visualization) was not
built; `protocol/src/framing.rs` has a pre-existing isolated-crate clippy warning
(`use std::io` unused without the `tokio` feature) that the workspace build does not hit.

---

# WF-025 — Relay self-update (Phase 0-2)

**Status:** Implemented on `feat/observability-overhaul`, unpushed, pending review/merge.

Delivered: client supervised control reconnect and per-relay token store; token-keyed
proxy auth with TTL/grace; per-path telemetry collection + aggregation; the mesh data
tooling (registry inventory, bounded reset-safe collector, analyzer, web trends); a
versioned release layout with a health-gated installer and rollback; systemd
`Type=notify` + watchdog; a verified self-updater on a timer; `/health` update state; and
in-place `execve` handoff (manifest + fd adoption + session/auth transfer) that keeps
existing client flows alive across a binary swap.

**Verification:** full CI parity (fmt, clippy `-Dwarnings`, release build, 23 test
binaries, `quic`/`full`/`ml`); installer self-test 86 checks, updater 46;
`test_handoff_e2e.sh` (root) proves same-PID 1.4.4->1.4.5 with a live session and a
preserved outbound source port; a real canary installed on relay-nrt (healthy,
`current -> releases/1.4.4-canary-8bac681`, previous release retained).

**Rolled out:** all five production relays now run 1.5.0 under the versioned layout with the
`Type=notify` unit and `handoff.supported=true`; the previous binary is retained for
rollback. Each relay took one health-gated restart (a one-time migration via
`infra/scripts/migrate-to-versioned.sh`, then `relay-install.sh`); future releases apply via
the in-place handoff with no forced reconnect. Relay uptime is already published on the site
per relay.

**Next:** push/merge the branch and cut a 1.5.0 release so the self-updater and the Pages
stats reflect it; then use `collect-metrics.sh` + `analyze-mesh.sh` on live data to tune
relay selection.

---

## WF-026: Demand-driven relay placement recommender (2026-09-19)

**Workflow:** WF-026
**Agent:** Architect / RustDev / InfraDev / QAEngineer
**Status:** Shipped. Merged via PR #90, released as v1.6.0, deployed to all five production relays, and the MMDB is published (`geoip-2026-09`) and synced. v1.6.1 hardens the deploy path: handoff request ownership, automatic `[server] public_ip`, and shipping `relay-updater.sh`.

Motivated by the owner's request to prioritize relay placement by where players actually play.
Investigation found the WF-025 player-country telemetry is opt-in via the CLI `--telemetry`
flag only, the GUI has no toggle, and production relays expose zero game/country series; the
only country tags live were the relay location tags. The owner chose the demand-analysis plus
region-recommender path with a player-region to game-server-region matrix, no infra spend, and
no raw IP storage.

Delivered:
- Proxy-side transient country derivation (DB-IP Lite MMDB via `maxminddb`) aggregating
  `(source_country, destination_country)` session counts with a k>=3 export floor, exposed as
  `lightspeed_geo_*` on `/metrics`.
- `infra/geo/` region and candidate catalogs plus `recommend-regions.sh`, which scores candidates
  by demand-weighted coverage and redundancy and emits ADD/MOVE/NONE with a 3-run stability gate
  and an INSUFFICIENT_DATA floor.
- `collect-metrics.sh` coarsens country pairs to region pairs and persists them in the stats
  branch history; `pages.yml` runs the recommender and commits `.stats/placement.json`.
- Monthly `geoip-release.yml` publishes the pinned MMDB; `relay-updater.sh` and
  `setup-new-node.sh` verify and sync it.
- Privacy/FAQ/NOTICE describe the transient derivation and add the DB-IP attribution.

**Verification:** CI parity locally (fmt, clippy default/quic/full/ml, workspace tests,
`cargo deny`); bash suites 75/59/63/101/28 checks; live `/metrics` geo smoke; collector and
recommender exercised end to end; reviewer pass on five blocker concerns.

**Known gaps:** real history is roughly 60 sessions, so the recommender correctly reports
INSUFFICIENT_DATA until the new proxy build and MMDB reach the fleet; the 64-cell cap is a growth
guard that cannot trigger with the real 8-region catalog; MOVE target selection is region-agnostic.

**Next:** let demand accumulate. The Pages recommender runs every 6h and currently reports
INSUFFICIENT_DATA (no geo cells reach the k>=3 floor yet). Once volume grows it will name the
best next region to deploy and test; any actual relay add or move remains a human decision.

---

## WF-027: Shadow direct path - step 1 (like-for-like direct application RTT)

**Workflow:** WF-027
**Agent:** RustDev + QAEngineer
**Status:** Implemented in the working tree (not committed)

The published "RTT saved" compared an ICMP echo (direct) against the tunnelled
game-traffic round trip (relayed): different instruments, so only an estimate.
Step 1 adds a like-for-like direct application RTT: one of the game's OWN
packets per server per 30s goes out on the direct path unmodified, the server's
reply is timed on a WinDivert inbound sniff handle, and `direct_app_p50_ms`
flows through telemetry, proxy metrics, the collector, and the web trend. The
ICMP estimate stays as the fallback.

Delivered:
- `client/src/latency.rs`: separate shadow-direct tracker (own pending map, 30s
  per-server gate, 5s reply timeout, 300s TTL, 256-sample ring,
  `shadow_direct_p50_ms`); never touches `pending` or the relayed ring.
- `client/src/interceptor/order.rs` + `windows.rs`: `Decision::ShadowDirect`,
  unchanged re-injection of the game's own packet, inbound sniff handle, teardown
  ack extended to four owners.
- `protocol/src/telemetry.rs`: `direct_app_p50_ms` (finite, 0..=10000).
- `client/src/telemetry.rs`: `build_report` populates it.
- `proxy/src/metrics.rs`: `lightspeed_telemetry_direct_app_ms_{sum,count}`.
- `infra/scripts/collect-metrics.sh` + test: `direct_app_ms_{sum,count}`.
- `web/app.js`: prefers the like-for-like value, ICMP estimate as fallback.

**Verification:** `cargo fmt --all --check` clean; clippy `-Dwarnings` default and
`full` clean; `cargo test --workspace --exclude lightspeed-gui` 714 passed, 0
failed; collector suite 100 checks. Windows-only code cannot be compiled on this
host and is compile-checked by CI: the `windows-gui` job builds the client with
`windivert-redirect`, the `windows-test` job builds the feature-off stub.

**Known gap:** real Windows runtime behaviour (sniff handle pairing, direct
re-injection) rests on the CI Windows build plus the Linux-runnable shadow
tracker unit tests.

**Next (step 2):** not defined in this task.
