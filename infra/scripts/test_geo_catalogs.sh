#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for the placement geography catalogs
#
# infra/geo/{regions,candidates}.json are operator-maintained reference
# data. Nothing executes them, so a bad edit does not fail a build - it
# silently degrades the placement recommender: a country mapped to a
# region that no longer exists becomes an unmapped cell, a dangling
# candidate region drops a location from consideration, and an
# out-of-range coordinate skews every distance it feeds.
#
# Asserted behaviours:
#   (a) both catalogs are valid JSON with schema_version 1
#   (b) region keys are lowercase slugs; country keys are ISO alpha-2
#   (c) every country value resolves to a declared region
#   (d) every region alias resolves to a declared region
#   (e) every candidate region resolves to a declared region
#   (f) all coordinates are numeric and in WGS 84 range
#   (g) every declared region has a label and coordinates
#   (h) candidate ids are unique; all params are numeric
#   (i) a malformed catalog is actually rejected by these checks
#
# No network: the two files are local fixtures.
#
# Usage: bash infra/scripts/test_geo_catalogs.sh
# Exits 0 and prints "geo-catalogs: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEO_DIR="$(cd "$SCRIPT_DIR/../geo" && pwd)"
REGIONS="$GEO_DIR/regions.json"
CANDIDATES="$GEO_DIR/candidates.json"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass() { PASS=$((PASS + 1)); }
fail() {
	printf '  FAIL: %s\n' "$1" >&2
	shift
	for line in "$@"; do printf '        %s\n' "$line" >&2; done
	FAILURES=$((FAILURES + 1))
}

# check <file> <jq-expr-produced-boolean-ish> <message>
# The expression must evaluate to true for a pass; `jq -e` exit status decides.
check() {
	local file="$1" expr="$2" msg="$3"
	if jq -e "$expr" "$file" >/dev/null 2>&1; then
		pass
	else
		fail "$msg" "expr: $expr"
	fi
}

# offenders <file> <expr producing an array of offenders> <message>
offenders() {
	local file="$1" expr="$2" msg="$3"
	local found rc
	found="$(jq -r "$expr" "$file" 2>/dev/null)"
	rc=$?
	if [ "$rc" -ne 0 ] || [ -z "$found" ]; then
		fail "$msg" "the check itself failed to evaluate (jq rc=$rc)" "expr: $expr"
		return
	fi
	if [ "$found" = "[]" ]; then
		pass
	else
		fail "$msg" "found: $found"
	fi
}

if [ ! -f "$REGIONS" ]; then
	printf 'geo-catalogs: FAIL - not found: %s\n' "$REGIONS" >&2
	exit 1
fi
if [ ! -f "$CANDIDATES" ]; then
	printf 'geo-catalogs: FAIL - not found: %s\n' "$CANDIDATES" >&2
	exit 1
fi

# ══════════════════════════════════════════════════════════════
# (a) both catalogs parse and declare the expected schema
# ══════════════════════════════════════════════════════════════
check "$REGIONS" 'type == "object" and .schema_version == 1' "(a) regions.json parses with schema_version 1"
check "$CANDIDATES" 'type == "object" and .schema_version == 1' "(a) candidates.json parses with schema_version 1"
check "$REGIONS" '(.regions | type) == "object" and (.regions | length) > 0' "(a) regions map is a non-empty object"
check "$REGIONS" '(.countries | type) == "object" and (.countries | length) > 0' "(a) countries map is a non-empty object"
check "$CANDIDATES" '(.candidates | type) == "array" and (.candidates | length) > 0' "(a) candidates is a non-empty array"

# ══════════════════════════════════════════════════════════════
# (b) key formats
# ══════════════════════════════════════════════════════════════
offenders "$REGIONS" \
	'[.regions | keys[] | select(test("^[a-z][a-z0-9_-]*$") | not)]' \
	"(b) every region key is a lowercase slug"
offenders "$REGIONS" \
	'[.countries | keys[] | select(test("^[A-Z]{2}$") | not)]' \
	"(b) every country key is ISO 3166-1 alpha-2"

# ══════════════════════════════════════════════════════════════
# (c)-(e) every reference resolves to a declared region
# ══════════════════════════════════════════════════════════════
offenders "$REGIONS" \
	'(.regions | keys) as $r | [.countries | to_entries[] | select(.value as $v | ($r | index($v)) == null) | "\(.key)->\(.value)"]' \
	"(c) every country maps to a declared region"
offenders "$REGIONS" \
	'(.regions | keys) as $r | [(.region_aliases // {}) | to_entries[] | select(.value as $v | ($r | index($v)) == null) | "\(.key)->\(.value)"]' \
	"(d) every region alias resolves"
CAND_REF_OFFENDERS="$(jq -r --slurpfile r "$REGIONS" \
	'($r[0].regions | keys) as $rk | [.candidates[] | select(.region as $x | ($rk | index($x)) == null) | "\(.id)->\(.region)"]' \
	"$CANDIDATES" 2>/dev/null || echo '["<jq error>"]')"
if [ "$CAND_REF_OFFENDERS" = "[]" ]; then
	pass
else
	fail "(e) every candidate region resolves" "found: $CAND_REF_OFFENDERS"
fi

# ══════════════════════════════════════════════════════════════
# (f) coordinates are numeric and in range
# ══════════════════════════════════════════════════════════════
offenders "$REGIONS" \
	'[.regions | to_entries[] | select((.value.lat | type) != "number" or (.value.lon | type) != "number" or .value.lat < -90 or .value.lat > 90 or .value.lon < -180 or .value.lon > 180) | .key]' \
	"(f) every region coordinate is in WGS 84 range"
offenders "$CANDIDATES" \
	'[.candidates[] | select((.lat | type) != "number" or (.lon | type) != "number" or .lat < -90 or .lat > 90 or .lon < -180 or .lon > 180) | .id]' \
	"(f) every candidate coordinate is in WGS 84 range"

# ══════════════════════════════════════════════════════════════
# (g) each region is fully described
# ══════════════════════════════════════════════════════════════
offenders "$REGIONS" \
	'[.regions | to_entries[] | select((.value.label // "") == "") | .key]' \
	"(g) every region has a label"

# ══════════════════════════════════════════════════════════════
# (h) identifier uniqueness and numeric params
# ══════════════════════════════════════════════════════════════
offenders "$CANDIDATES" \
	'[.candidates[].id] as $i | if ($i | length) == ($i | unique | length) then [] else ["duplicate candidate id"] end' \
	"(h) candidate ids are unique"
offenders "$CANDIDATES" \
	'[.params | to_entries[] | select((.value | type) != "number") | .key]' \
	"(h) every recommender param is numeric"

# ══════════════════════════════════════════════════════════════
# (i) the checks have teeth: a broken catalog must be rejected
# ══════════════════════════════════════════════════════════════
BROKEN="$TMP/broken-regions.json"
jq '.countries["ZZ"] = "atlantis"' "$REGIONS" > "$BROKEN"
if jq -e '(.regions | keys) as $r | [.countries | to_entries[] | select(.value as $v | ($r | index($v)) == null)] | length == 0' \
	"$BROKEN" >/dev/null 2>&1; then
	fail "(i) a dangling country->region reference must be detected" "the broken fixture passed the check"
else
	pass
fi

BROKEN2="$TMP/broken-candidates.json"
jq '.candidates[0].lat = 999' "$CANDIDATES" > "$BROKEN2"
if jq -e '[.candidates[] | select(.lat < -90 or .lat > 90)] | length == 0' "$BROKEN2" >/dev/null 2>&1; then
	fail "(i) an out-of-range coordinate must be detected" "the broken fixture passed the check"
else
	pass
fi

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'geo-catalogs: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'geo-catalogs: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
