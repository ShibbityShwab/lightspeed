#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for recommend-regions.sh
#
# Proves the load-bearing behaviours of the relay placement
# recommender:
#   (1) empty/missing history -> INSUFFICIENT_DATA, exit 0, valid JSON
#   (2) 25 sessions in one kept cell -> status OK, non-empty ranking
#   (3) a dominant (>=40%) leader -> ADD
#   (4) a 5% leader with no stability history -> NONE
#   (5) three prior identical runs -> ADD via the stability streak
#   (6) MOVE is gated on stability_runs: 2 consecutive runs -> NONE,
#       3 consecutive runs -> MOVE
#   (7) a relay reset flag treats the current cumulative geo count as
#       the delta (not current - previous)
#   (8) a garbage history file -> valid JSON, exit 0, INSUFFICIENT_DATA
#   (9) two runs on identical inputs produce identical documents apart
#       from generated_at
#  (10) catalog integrity: every referenced region key exists and every
#       coordinate is in range in infra/geo/regions.json and
#       infra/geo/candidates.json
#  (11) a lone candidate has a null margin and cannot fire the ADD gate
#  (12) a large accumulated history still yields a real window (ARG_MAX)
#
# Tests (3)-(7) use a synthetic three-region catalog under $TMP with
# fully controlled geometry; tests (1), (2), (8), (9), (10) exercise
# the real infra/geo catalogs and the real signed registry.
#
# Usage: bash infra/scripts/test_recommend_regions.sh
# Exits 0 and prints "recommend-regions: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RECOMMEND="$SCRIPT_DIR/recommend-regions.sh"
GEO_REAL="$(cd "$SCRIPT_DIR/../geo" && pwd)"
REGISTRY_REAL="$(cd "$SCRIPT_DIR/../.." && pwd)/web/registry.json"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

assert_jq() {
    # assert_jq <file> <jq-expr> <message>
    local file="$1" expr="$2" msg="$3"
    if jq -e "$expr" "$file" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s\n        expr: %s\n' "$msg" "$expr" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

assert_rc0() {
    # assert_rc0 <rc> <message>
    if [ "$1" -eq 0 ]; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s (exit %s, expected 0)\n' "$2" "$1" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

assert_ok() {
    # assert_ok <message> <cmd...>
    local msg="$1"; shift
    if "$@" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s\n' "$msg" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

assert_same() {
    # assert_same <file-a> <file-b> <message>
    if diff -q "$1" "$2" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s\n' "$3" >&2
        diff "$1" "$2" | head -20 >&2
        FAILURES=$((FAILURES + 1))
    fi
}

if [ ! -f "$RECOMMEND" ]; then
    printf 'recommend-regions: FAIL - implementation not found: %s\n' "$RECOMMEND" >&2
    exit 1
fi

# ── Fixture writers ──────────────────────────────────────────
# snap <t> <node_id> <reset:true|false> <geo-json>
snap() {
    jq -cn --argjson t "$1" --arg id "$2" --argjson reset "$3" --argjson geo "$4" \
        '{t:$t, relay_count:1, healthy_count:1,
          per_relay:{($id):{reachable:true,version:"test",active_sessions:0,geo:$geo,reset:$reset}}}'
}

# write_history <out> <snapshots-json-array>
write_history() {
    jq -cn --argjson snaps "$2" '{version:1, generated_at:0, snapshots:$snaps}' > "$1"
}

# write_registry <out> <nodes-json-array>
write_registry() {
    jq -cn --argjson nodes "$2" \
        '{registry: ({schema_version:1, nodes:$nodes} | tojson), signature:"test"}' > "$1"
}

# write_previous <out> <top-id> <n-runs>
write_previous() {
    jq -cn --arg top "$2" --argjson n "$3" \
        '{schema_version:1,
          stability:{top_id:$top, streak:$n, required:3,
                     runs:[range(0;$n) | {t:(900 + (. * 20)), top_id:$top, score:25, margin:25000}]}}' > "$1"
}

# write_synth_regions <dir>
write_synth_regions() {
    mkdir -p "$1"
    cat > "$1/regions.json" <<'JSON'
{
  "schema_version": 1,
  "regions": {
    "hub":  {"label": "Hub",  "lat": 0,  "lon": 0},
    "east": {"label": "East", "lat": 0,  "lon": 60},
    "west": {"label": "West", "lat": 0,  "lon": -60},
    "far":  {"label": "Far",  "lat": 60, "lon": 0}
  },
  "countries": {"HU": "hub", "EA": "east", "WE": "west", "FA": "far"},
  "region_aliases": {"hub": "hub", "east": "east", "west": "west", "far": "far"},
  "relays": {
    "relay-hub-a": {"lat": 30, "lon": 0},
    "relay-hub-b": {"lat": 30, "lon": 1},
    "relay-far":   {"lat": 60, "lon": 30}
  }
}
JSON
}

# write_synth_candidates <dir> <candidates-json-array>
write_synth_candidates() {
    jq -cn --argjson cands "$2" \
        '{schema_version:1,
          params:{alpha:2, window_secs:604800, min_window_sessions:20,
                  min_cell_sessions:3, stability_runs:3, add_margin:0.10,
                  move_margin:0.20, redundancy_weight:0.5, move_coverage_keep:0.9,
                  proximity_ms_floor:15, near_duplicate_ms:25},
          candidates:$cands}' > "$1/candidates.json"
}

# run_rec <history> <previous> <registry> <geo-dir> <out> [extra args...]
run_rec() {
    local h="$1" p="$2" r="$3" g="$4" o="$5"
    shift 5
    bash "$RECOMMEND" --history "$h" --previous "$p" --registry "$r" --geo-dir "$g" --out "$o" "$@" \
        >/dev/null 2>&1
}

SYNTH_NODES='[{"node_id":"relay-hub-a","region":"hub","data_addr":"10.0.0.1:4434"},
              {"node_id":"relay-hub-b","region":"hub","data_addr":"10.0.0.2:4434"},
              {"node_id":"relay-far","region":"far","data_addr":"10.0.0.3:4434"}]'
CAND_EAST='{"id":"cand-east","provider":"test","region":"east","lat":0,"lon":60,"free_tier":true,"viable":true,"note":""}'
CAND_WEST='{"id":"cand-west","provider":"test","region":"west","lat":0,"lon":-60,"free_tier":true,"viable":true,"note":""}'
CAND_RIVAL='{"id":"cand-rival","provider":"test","region":"west","lat":0,"lon":63.063861606035466,"free_tier":true,"viable":true,"note":""}'
CAND_HUB='{"id":"cand-hub","provider":"test","region":"hub","lat":0,"lon":20,"free_tier":true,"viable":true,"note":""}'
CAND_NEAR='{"id":"cand-near","provider":"test","region":"hub","lat":30,"lon":0.2,"free_tier":true,"viable":true,"note":""}'

REG_SYNTH="$TMP/registry-synth.json"
write_registry "$REG_SYNTH" "$SYNTH_NODES"

# geo-add40: cand-east dominates cell east->east (cand-west gains nothing)
GEO_ADD="$TMP/geo-add40"
write_synth_regions "$GEO_ADD"
write_synth_candidates "$GEO_ADD" "[$CAND_EAST,$CAND_WEST]"

# geo-leader5: cand-east leads cand-east2 by ~5% on cell east->east
GEO_LEAD="$TMP/geo-leader5"
write_synth_regions "$GEO_LEAD"
write_synth_candidates "$GEO_LEAD" "[$CAND_EAST,$CAND_RIVAL]"

# geo-move: cand-hub improves the served hub->hub cell; cand-near is a
# near-duplicate of relay-hub-a and must be rejected
GEO_MOVE="$TMP/geo-move"
write_synth_regions "$GEO_MOVE"
write_synth_candidates "$GEO_MOVE" "[$CAND_HUB,$CAND_WEST,$CAND_NEAR]"

# geo-single: exactly one candidate survives, so no meaningful margin exists
GEO_SINGLE="$TMP/geo-single"
write_synth_regions "$GEO_SINGLE"
write_synth_candidates "$GEO_SINGLE" "[$CAND_EAST]"

# 25 reconstructed sessions in one kept cell
H_SYNTH="$TMP/hist-synth.json"
write_history "$H_SYNTH" \
    "[$(snap 1000 relay-hub-a false '{"east-east":25}'),$(snap 1060 relay-hub-a false '{"east-east":50}')]"
# same, but for the hub->hub demand used by the MOVE fixtures
H_MOVE="$TMP/hist-move.json"
write_history "$H_MOVE" \
    "[$(snap 1000 relay-hub-a false '{"hub-hub":25}'),$(snap 1060 relay-hub-a false '{"hub-hub":50}')]"

# ── (1) empty/missing history -> INSUFFICIENT_DATA, exit 0 ───
OUT1="$TMP/out1.json"
run_rec "$TMP/does-not-exist.json" missing "$REGISTRY_REAL" "$GEO_REAL" "$OUT1"
assert_rc0 $? "(1) missing history exits 0"
assert_jq "$OUT1" 'type == "object"' "(1) missing history emits a JSON object"
assert_jq "$OUT1" '.status == "INSUFFICIENT_DATA"' "(1) missing history is INSUFFICIENT_DATA"
assert_jq "$OUT1" '.recommendation.action == "NONE" and .matrix == []' "(1) missing history recommends NONE with an empty matrix"
assert_jq "$OUT1" 'has("schema_version") and has("generated_at") and has("status") and has("window") and has("matrix") and has("existing") and has("ranking") and has("rejected") and has("recommendation") and has("stability") and has("notes")' "(1) full output schema present"

: > "$TMP/empty.json"
run_rec "$TMP/empty.json" missing "$REGISTRY_REAL" "$GEO_REAL" "$TMP/out1b.json"
assert_rc0 $? "(1) empty history file exits 0"
assert_jq "$TMP/out1b.json" '.status == "INSUFFICIENT_DATA"' "(1) empty history is INSUFFICIENT_DATA"

# ── (2) 25 sessions in one kept cell -> OK + ranking ─────────
H_REAL="$TMP/hist-real.json"
write_history "$H_REAL" \
    "[$(snap 1000 relay-fra false '{"na-eu":25}'),$(snap 1060 relay-fra false '{"na-eu":50}')]"
OUT2="$TMP/out2.json"
run_rec "$H_REAL" missing "$REGISTRY_REAL" "$GEO_REAL" "$OUT2"
assert_rc0 $? "(2) real-catalog run exits 0"
assert_jq "$OUT2" '.status == "OK"' "(2) 25 sessions is OK"
assert_jq "$OUT2" '.window.sessions == 25 and .window.snapshots == 2' "(2) window reports the reconstructed total"
assert_jq "$OUT2" '.window.from_t == 1000 and .window.to_t == 1060' "(2) window boundaries"
assert_jq "$OUT2" '.matrix == [{"src":"na","dst":"eu","sessions":25}]' "(2) matrix holds the single kept cell"
assert_jq "$OUT2" '(.ranking | length) > 0' "(2) ranking is non-empty"
assert_jq "$OUT2" '((.existing | length) >= 1) and (.existing[0] | (has("node_id") and has("region") and has("lat") and has("lon")))' "(2) existing relays are located"
assert_jq "$OUT2" '[.ranking[] | has("candidate_id") and has("region") and has("score") and has("coverage_gain") and has("redundancy_gain") and has("margin")] | all' "(2) ranking entries carry the full schema"

# --window-secs override: a 30s window keeps only the last snapshot
run_rec "$H_REAL" missing "$REGISTRY_REAL" "$GEO_REAL" "$TMP/out2b.json" --window-secs 30
assert_rc0 $? "(2) --window-secs run exits 0"
assert_jq "$TMP/out2b.json" '.status == "INSUFFICIENT_DATA" and .window.snapshots == 1' "(2) --window-secs narrows the window"

# ── (3) a 40% leader -> ADD ──────────────────────────────────
OUT3="$TMP/out3.json"
run_rec "$H_SYNTH" missing "$REG_SYNTH" "$GEO_ADD" "$OUT3"
assert_rc0 $? "(3) add40 run exits 0"
assert_jq "$OUT3" '.status == "OK"' "(3) add40 is OK"
assert_jq "$OUT3" '.ranking[0].candidate_id == "cand-east"' "(3) cand-east leads"
assert_jq "$OUT3" '.ranking[0].margin >= 0.40' "(3) leader margin is at least 40%"
assert_jq "$OUT3" '.ranking[0].margin <= 1' "(3) margin is capped at 1 even when rivals score zero"
assert_jq "$OUT3" '.recommendation.action == "ADD" and .recommendation.candidate_id == "cand-east"' "(3) dominant leader recommends ADD"
assert_jq "$OUT3" '.recommendation.remove_node_id == null' "(3) ADD removes nothing"
assert_jq "$OUT3" '.stability.streak == 1' "(3) first run has a one-run streak"

# ── (4) a 5% leader with no history -> NONE ──────────────────
OUT4="$TMP/out4.json"
run_rec "$H_SYNTH" missing "$REG_SYNTH" "$GEO_LEAD" "$OUT4"
assert_rc0 $? "(4) leader5 run exits 0"
assert_jq "$OUT4" '.status == "OK"' "(4) leader5 is OK"
assert_jq "$OUT4" '.ranking[0].candidate_id == "cand-east"' "(4) cand-east leads"
assert_jq "$OUT4" '.ranking[0].margin > 0.04 and .ranking[0].margin < 0.06' "(4) leader margin is about 5%"
assert_jq "$OUT4" '.recommendation.action == "NONE"' "(4) a 5% leader is below add_margin and has no streak"
assert_jq "$OUT4" '.stability.streak == 1' "(4) no prior runs means a one-run streak"

# ── (5) three prior identical runs -> ADD via stability ──────
PREV5="$TMP/prev5.json"
write_previous "$PREV5" cand-east 3
OUT5="$TMP/out5.json"
run_rec "$H_SYNTH" "$PREV5" "$REG_SYNTH" "$GEO_LEAD" "$OUT5"
assert_rc0 $? "(5) stable-leader run exits 0"
assert_jq "$OUT5" '.recommendation.action == "ADD" and .recommendation.candidate_id == "cand-east"' "(5) a 5% leader with 3 stable runs recommends ADD"
assert_jq "$OUT5" '.stability.streak >= 3' "(5) streak reaches stability_runs"
assert_jq "$OUT5" '(.stability.runs | length) == 4' "(5) this run is appended to the prior three"

# ── (6) MOVE requires stability_runs consecutive runs ────────
PREV6A="$TMP/prev6a.json"
write_previous "$PREV6A" cand-hub 1
OUT6A="$TMP/out6a.json"
run_rec "$H_MOVE" "$PREV6A" "$REG_SYNTH" "$GEO_MOVE" "$OUT6A"
assert_rc0 $? "(6) two-run move candidate exits 0"
assert_jq "$OUT6A" '.status == "OK"' "(6) move fixture is OK"
assert_jq "$OUT6A" '.ranking[0].candidate_id == "cand-hub"' "(6) cand-hub leads the served hub region"
assert_jq "$OUT6A" '.rejected == [{"candidate_id":"cand-near","reason":"near_duplicate"}]' "(6) near-duplicate candidate is rejected"
assert_jq "$OUT6A" '.stability.streak == 2' "(6) two consecutive runs so far"
assert_jq "$OUT6A" '.recommendation.action == "NONE"' "(6) MOVE needs three consecutive runs"

PREV6B="$TMP/prev6b.json"
write_previous "$PREV6B" cand-hub 2
OUT6B="$TMP/out6b.json"
run_rec "$H_MOVE" "$PREV6B" "$REG_SYNTH" "$GEO_MOVE" "$OUT6B"
assert_rc0 $? "(6) three-run move candidate exits 0"
assert_jq "$OUT6B" '.stability.streak == 3' "(6) three consecutive runs"
assert_jq "$OUT6B" '.recommendation.action == "MOVE"' "(6) a stable high-margin leader over a served region recommends MOVE"
assert_jq "$OUT6B" '.recommendation.candidate_id == "cand-hub"' "(6) MOVE names the candidate"
assert_jq "$OUT6B" '.recommendation.remove_node_id == "relay-far"' "(6) MOVE names the relay whose removal retains every kept cell"
assert_jq "$OUT6B" '.ranking[0].margin >= 0.20' "(6) MOVE margin clears move_margin"

# ── (7) reset reconstruction uses current as the delta ───────
H_RESET="$TMP/hist-reset.json"
write_history "$H_RESET" \
    "[$(snap 1000 relay-hub-a false '{"east-east":5}'),$(snap 1060 relay-hub-a true '{"east-east":25}')]"
OUT7="$TMP/out7.json"
run_rec "$H_RESET" missing "$REG_SYNTH" "$GEO_ADD" "$OUT7"
assert_rc0 $? "(7) reset run exits 0"
assert_jq "$OUT7" '.status == "OK" and .window.sessions == 25' "(7) reset=true treats current as the delta"

H_NORESET="$TMP/hist-noreset.json"
write_history "$H_NORESET" \
    "[$(snap 1000 relay-hub-a false '{"east-east":5}'),$(snap 1060 relay-hub-a false '{"east-east":25}')]"
OUT7B="$TMP/out7b.json"
run_rec "$H_NORESET" missing "$REG_SYNTH" "$GEO_ADD" "$OUT7B"
assert_jq "$OUT7B" '.status == "OK" and .window.sessions == 20' "(7) reset=false subtracts the previous cumulative count"

# ── (8) garbage history -> valid JSON, exit 0, INSUFFICIENT ──
printf 'not json at all {{{' > "$TMP/garbage.json"
OUT8="$TMP/out8.json"
run_rec "$TMP/garbage.json" missing "$REGISTRY_REAL" "$GEO_REAL" "$OUT8"
assert_rc0 $? "(8) garbage history exits 0"
assert_jq "$OUT8" 'type == "object"' "(8) garbage history emits a JSON object"
assert_jq "$OUT8" '.status == "INSUFFICIENT_DATA" and .recommendation.action == "NONE"' "(8) garbage history is INSUFFICIENT_DATA"

# ── (9) determinism ──────────────────────────────────────────
run_rec "$H_SYNTH" "$PREV5" "$REG_SYNTH" "$GEO_LEAD" "$TMP/det-a.json"
run_rec "$H_SYNTH" "$PREV5" "$REG_SYNTH" "$GEO_LEAD" "$TMP/det-b.json"
jq -S 'del(.generated_at)' "$TMP/det-a.json" > "$TMP/det-a.norm.json"
jq -S 'del(.generated_at)' "$TMP/det-b.json" > "$TMP/det-b.norm.json"
assert_same "$TMP/det-a.norm.json" "$TMP/det-b.norm.json" "(9) identical inputs produce identical output"

# ── (10) catalog integrity ───────────────────────────────────
assert_ok "(10) regions.json: country/alias region keys exist and coordinates are in range" \
    jq -e '
      (.regions | keys) as $R
      | ((.countries // {}) | to_entries | map(.value | IN($R[])) | all)
      and ((.region_aliases // {}) | to_entries | map(.value | IN($R[])) | all)
      and ([.regions[] | ((.lat | type) == "number") and ((.lon | type) == "number")
                        and (.lat >= -90) and (.lat <= 90)
                        and (.lon >= -180) and (.lon <= 180)] | all)
      and ([.relays | to_entries[] | .value
            | ((.lat | type) == "number") and ((.lon | type) == "number")
              and (.lat >= -90) and (.lat <= 90)
              and (.lon >= -180) and (.lon <= 180)] | all)
    ' "$GEO_REAL/regions.json"

assert_ok "(10) candidates.json: candidate region keys exist and coordinates are in range" \
    jq -e --slurpfile reg "$GEO_REAL/regions.json" '
      ($reg[0].regions | keys) as $R
      | ([.candidates[] | .region | IN($R[])] | all)
      and ([.candidates[] | ((.lat | type) == "number") and ((.lon | type) == "number")
                           and (.lat >= -90) and (.lat <= 90)
                           and (.lon >= -180) and (.lon <= 180)] | all)
      and (([.candidates[].id] | length) == ([.candidates[].id] | unique | length))
      and ([.candidates[] | (.free_tier | type) == "boolean"] | all)
    ' "$GEO_REAL/candidates.json"

# ── (11) single candidate: margin is null, no fabricated ADD ─
OUT11="$TMP/out11.json"
run_rec "$H_SYNTH" missing "$REG_SYNTH" "$GEO_SINGLE" "$OUT11"
assert_rc0 $? "(11) single-candidate run exits 0"
assert_jq "$OUT11" '.ranking | length == 1' "(11) exactly one candidate is ranked"
assert_jq "$OUT11" '.ranking[0].margin == null' "(11) a lone candidate has no margin, not a fabricated one"
assert_jq "$OUT11" '.recommendation.action == "NONE"' "(11) a lone candidate cannot fire the margin gate"

# ── (12) a large accumulated history does not fall back ──────
# Regression: the recommender passed the whole history through --argjson, so a
# real accumulated history blew past ARG_MAX and silently emitted the
# INSUFFICIENT_DATA document (window snapshots 0) while small fixtures passed.
LARGE="$TMP/large-history.json"
jq -cn '{version:1,generated_at:0,snapshots:[range(0;60) | {t:(1700000000 + .*3600),relay_count:2,healthy_count:2,interval:{sessions_created:5},totals:{sessions_created:5},per_relay:{"relay-a":{reachable:true,reset:false,geo:{"na-eu":5}},"relay-b":{reachable:true,reset:false,geo:{"eu-eu":4}}}}]}' > "$LARGE"
OUT12="$TMP/out12.json"
run_rec "$LARGE" missing "$REGISTRY_REAL" "$GEO_REAL" "$OUT12"
assert_rc0 $? "(12) large history exits 0"
assert_jq "$OUT12" '.notes | test("fatal parse failure") | not' "(12) large history does not fall back to the minimal document"
assert_jq "$OUT12" '.window.snapshots > 0' "(12) large history yields a non-empty window"

# ── (13) relay necessity is reported for every existing relay ─
assert_jq "$OUT3" '.relay_necessity | length > 0' "(13) necessity lists existing relays"
assert_jq "$OUT3" '(.relay_necessity | length) == (.existing | length)' "(13) one necessity row per existing relay"
assert_jq "$OUT3" '.relay_necessity | all(has("sessions") and has("worst_retention"))' "(13) necessity rows carry sessions and retention"
assert_jq "$OUT3" '.prune_candidates | type == "array"' "(13) prune_candidates is an array"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'recommend-regions: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'recommend-regions: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
