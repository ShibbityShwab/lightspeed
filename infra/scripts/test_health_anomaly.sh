#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for health-anomaly.sh
#
# Proves the anomaly monitor fires on the real outage signatures and
# stays silent on a healthy fleet. Fixtures are synthesized from the
# collector history format (collect-metrics.sh): a list of snapshots,
# each with per_relay.<id>.{reachable,version,lifetime.*}.
#
# Detectors covered:
#   (a) zero_relay                  relay up but relayed 0 over the window
#   (b) auth_spike_no_sessions      auth_rejections climb, sessions flat
#   (c) version_lag                 relay behind the fleet majority
#   (d) saved_regression            ping-saved drops vs own baseline
#   (d) negative_saving_spike       negative saving share spikes
#   (e) relay_missing_from_registry
#   (e) relay_health_failed
#
# False-positive guards:
#   * a single zero snapshot is not enough (sustained window required)
#   * an all-idle fleet raises nothing
#   * a newer canary is not "behind"
#   * --no-probe uses history reachability and never flags an unscraped relay
#
# No network: no probe unless --probe is passed; DISCORD_WEBHOOK is
# cleared so nothing is posted. curl is never used by the monitor here.
#
# Usage: bash infra/scripts/test_health_anomaly.sh
# Exits 0 and prints "health-anomaly: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANOMALY="$SCRIPT_DIR/health-anomaly.sh"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# ── Assertion helpers ────────────────────────────────────────
OUT=""
RC=0

assert_rc() {
	# assert_rc <expected-rc> <message>
	if [ "$RC" -eq "$1" ]; then
		PASS=$((PASS + 1))
	else
		printf '  FAIL: %s (rc=%s want %s)\n        output: %s\n' \
			"$2" "$RC" "$1" "$(printf '%s' "$OUT" | head -5 | tr '\n' '|')" >&2
		FAILURES=$((FAILURES + 1))
	fi
}

assert_out() {
	# assert_out <substring> <message>
	if printf '%s' "$OUT" | grep -qF -- "$1"; then
		PASS=$((PASS + 1))
	else
		printf '  FAIL: %s (missing: %s)\n        output: %s\n' \
			"$2" "$1" "$(printf '%s' "$OUT" | head -8 | tr '\n' '|')" >&2
		FAILURES=$((FAILURES + 1))
	fi
}

assert_not_out() {
	# assert_not_out <substring> <message>
	if printf '%s' "$OUT" | grep -qF -- "$1"; then
		printf '  FAIL: %s (unexpected: %s)\n        output: %s\n' \
			"$2" "$1" "$(printf '%s' "$OUT" | head -8 | tr '\n' '|')" >&2
		FAILURES=$((FAILURES + 1))
	else
		PASS=$((PASS + 1))
	fi
}

assert_json() {
	# assert_json <jq-expr> <message>  (evaluated against $OUT as JSON)
	if printf '%s' "$OUT" | jq -e "$1" >/dev/null 2>&1; then
		PASS=$((PASS + 1))
	else
		printf '  FAIL: %s\n        expr: %s\n        output: %s\n' \
			"$2" "$1" "$(printf '%s' "$OUT" | head -8 | tr '\n' '|')" >&2
		FAILURES=$((FAILURES + 1))
	fi
}

# ── Fixture builders ─────────────────────────────────────────
# mkrelay <id> <reachable> <version> <packets> <sessions> <auth> \
#         <saved_sum> <saved_count> <app_neg> <app_count>
mkrelay() {
	jq -cn \
		--arg id "$1" --argjson reach "$2" --arg ver "$3" \
		--argjson p "$4" --argjson s "$5" --argjson a "$6" \
		--argjson ss "$7" --argjson sc "$8" --argjson an "$9" --argjson ac "${10}" '
        {node_id:$id, reachable:$reach, version:$ver, active_sessions:0,
         lifetime:{packets_relayed:$p, sessions_created:$s, drops_auth_rejected:$a,
                   saved_ms_sum:$ss, saved_ms_count:$sc,
                   saved_app_ms_negative_count:$an, saved_app_ms_count:$ac}}'
}

# mksnap <t> <relay-json>...
mksnap() {
	local t="$1"
	shift
	local relays
	relays="$(printf '%s\n' "$@" | jq -s '.')"
	jq -cn --argjson t "$t" --argjson relays "$relays" '
        {t:$t, relay_count:($relays|length),
         healthy_count:([$relays[]|select(.reachable)]|length),
         interval:{}, totals:{},
         per_relay:($relays|map({key:.node_id,value:.})|from_entries)}'
}

# build_history <out> <snapshot-json>...
build_history() {
	local out="$1"
	shift
	printf '%s\n' "$@" | jq -s '{version:1, generated_at:(.[-1].t // 0), snapshots:.}' >"$out"
}

# mkreg <out> <id>...  (registry shape; health_url unused with --no-probe)
mkreg() {
	local out="$1"
	shift
	local nodes="[]"
	local id
	for id in "$@"; do
		nodes="$(printf '%s' "$nodes" | jq -c --arg id "$id" \
			'. + [{node_id:$id,region:"r",data_addr:"127.0.0.1:4434",health_url:"file:///nonexistent",metrics_url:""}]')"
	done
	jq -cn --argjson nodes "$nodes" \
		'{registry:({schema_version:1,nodes:$nodes}|tojson),signature:"test"}' >"$out"
}

# ── Standard healthy peers b, c, d (used by almost every fixture) ──
STD_VER="1.6.11"
std_b() { mkrelay relay-b true "$STD_VER" "$(($1 * 50))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }
std_c() { mkrelay relay-c true "$STD_VER" "$(($1 * 30))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }
std_d() { mkrelay relay-d true "$STD_VER" "$(($1 * 10))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }

# emit8 <out> <relay-a-function>  -> 8 snapshots, window=3 baseline=5
emit8() {
	local out="$1" afn="$2"
	local snaps=()
	local i
	for i in 0 1 2 3 4 5 6 7; do
		snaps+=("$(mksnap "$((1000 + i * 100))" "$($afn "$i")" "$(std_b "$i")" "$(std_c "$i")" "$(std_d "$i")")")
	done
	build_history "$out" "${snaps[@]}"
}

# Relay-a variants --------------------------------------------------
a_healthy() { mkrelay relay-a true "$STD_VER" "$(($1 * 100))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }
# Zero relayed for the whole window WITH clients being rejected: the real
# 1.6.3 outage signature (silent AND turning clients away).
a_zero() { mkrelay relay-a true "$STD_VER" 0 0 "$(($1 * 200))" "$(($1 * 20))" "$(($1 * 4))" 0 0; }
# Zero relayed for the whole window with NO rejections: the relay simply
# received no traffic. Live on 2026-10-01 every relay shared one uptime, so
# this must not be reported as the outage signature.
a_idle() { mkrelay relay-a true "$STD_VER" 0 0 0 "$(($1 * 20))" "$(($1 * 4))" 0 0; }
# Auth rejections climb from snapshot 5 onward, sessions stay flat.
a_authspike() {
	local a=0
	[ "$1" -ge 5 ] && a=$((($1 - 4) * 1700))
	mkrelay relay-a true "$STD_VER" "$(($1 * 100))" 0 "$a" "$(($1 * 20))" "$(($1 * 4))" 0 0
}
# Old version while the rest of the fleet is current.
a_lag() { mkrelay relay-a true "1.6.3" "$(($1 * 100))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }
# Newer than the fleet: must NOT be flagged as behind.
a_canary() { mkrelay relay-a true "1.7.0" "$(($1 * 100))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" 0 "$(($1 * 3))"; }
# Ping-saved collapses in the window (10ms -> 1ms).
a_savedreg() {
	local ss sc
	case "$1" in
	0)
		ss=0
		sc=0
		;;
	1)
		ss=100
		sc=10
		;;
	2)
		ss=200
		sc=20
		;;
	3)
		ss=300
		sc=30
		;;
	4)
		ss=500
		sc=50
		;;
	5)
		ss=500
		sc=50
		;;
	6)
		ss=502
		sc=52
		;;
	7)
		ss=505
		sc=55
		;;
	esac
	mkrelay relay-a true "$STD_VER" "$(($1 * 100))" "$(($1))" "$(($1 * 2))" "$ss" "$sc" 0 0
}
# Negative-saving share spikes in the window (0/0 -> 8/10).
a_negshare() {
	local an=0 ac=0
	[ "$1" -ge 5 ] && {
		an=8
		ac=10
	}
	mkrelay relay-a true "$STD_VER" "$(($1 * 100))" "$(($1))" "$(($1 * 2))" "$(($1 * 20))" "$(($1 * 4))" "$an" "$ac"
}

# mkrelay_abuse <id> <reach> <ver> <relayed> <dropped> <abuse> <sessions>
# The abuse counter lives in `lifetime.drops_abuse_blocked` in the real
# collector output, so a flood fixture has to carry it explicitly.
mkrelay_abuse() {
	jq -cn --arg id "$1" --argjson reach "$2" --arg ver "$3" \
		--argjson p "$4" --argjson d "$5" --argjson ab "$6" --argjson s "$7" '
        {node_id:$id, reachable:$reach, version:$ver, active_sessions:0,
         lifetime:{packets_relayed:$p, packets_dropped:$d,
                   drops_abuse_blocked:$ab, drops_auth_rejected:0,
                   sessions_created:$s,
                   saved_ms_sum:0, saved_ms_count:0,
                   saved_app_ms_negative_count:0, saved_app_ms_count:0}}'
}

# A flood absorbed by the abuse detector: abuse dominates drops, and it
# arrives ALONGSIDE real sessions (the 2026-10-01 relay-fra signature).
a_abuse() {
	local ab=0 sess=0
	[ "$1" -ge 5 ] && {
		ab=$((($1 - 4) * 400000))
		sess=$((($1 - 4) * 12))
	}
	mkrelay_abuse relay-a true "$STD_VER" "$(($1 * 40000))" "$(($1 * 100 + ab))" "$ab" "$sess"
}

# Same flood shape but far too small to matter: a relay with a handful
# of abuse blocks must stay quiet even at 100% of drops.
a_abusetiny() {
	local ab=0
	[ "$1" -ge 5 ] && ab=$((($1 - 4) * 5))
	mkrelay_abuse relay-a true "$STD_VER" "$(($1 * 40000))" "$(($1 * 10 + ab))" "$ab" "$(($1))"
}

# Abuse-heavy drops but real traffic still dominates: must stay quiet
# because abuse is a small share of what the relay dropped.
a_abusemixed() {
	local ab=0
	[ "$1" -ge 5 ] && ab=$((($1 - 4) * 3000))
	mkrelay_abuse relay-a true "$STD_VER" "$(($1 * 40000))" "$(($1 * 30000 + ab))" "$ab" "$(($1 * 5))"
}

# A history whose newest snapshot is recent, so staleness logic in other
# fixtures is not accidentally exercised by wall-clock drift.
# The fixtures below emit tiny t values (1000..1700) on purpose - they are
# synthetic counters, not real epochs - so "now" is pinned just past that
# range. The staleness boundary cases pin their own explicit clocks.
STD_T_BASE=1700

# ── Implementation presence (RED gate) ───────────────────────
# Pin "now" for every run so staleness assertions are deterministic:
# the fixtures' snapshot timestamps live near STD_T_BASE, well inside the
# default 3h limit, so stale_history stays out of the way of tests that
# are about other detectors (and is exercised explicitly below).
LIGHTSPEED_NOW_EPOCH="${LIGHTSPEED_NOW_EPOCH:-$((STD_T_BASE + 100))}"
export LIGHTSPEED_NOW_EPOCH

if [ ! -f "$ANOMALY" ]; then
	printf 'health-anomaly: FAIL - implementation not found: %s\n' "$ANOMALY" >&2
	exit 1
fi

# run_anomaly <history> <registry> [extra args...]
run_anomaly() {
	local hist="$1" reg="$2"
	shift 2
	OUT="$(LIGHTSPEED_NODES='' LIGHTSPEED_REGISTRY_PATH="$reg" DISCORD_WEBHOOK='' \
		bash "$ANOMALY" --history "$hist" --registry "$reg" --no-probe "$@" 2>&1)"
	RC=$?
}

REG4="$TMP/registry-4.json"
mkreg "$REG4" relay-a relay-b relay-c relay-d

# ══════════════════════════════════════════════════════════════
# Healthy fleet: every detector silent, exit 0
# ══════════════════════════════════════════════════════════════
H_HEALTHY="$TMP/healthy.json"
emit8 "$H_HEALTHY" a_healthy
run_anomaly "$H_HEALTHY" "$REG4"
assert_rc 0 "(healthy) exits 0"
assert_out "no anomalies" "(healthy) reports no anomalies"
assert_not_out "zero_relay" "(healthy) no zero_relay"
assert_not_out "auth_spike" "(healthy) no auth spike"
assert_not_out "version_lag" "(healthy) no version lag"
assert_not_out "saved_regression" "(healthy) no saved regression"
assert_not_out "negative_saving" "(healthy) no negative-saving spike"
assert_not_out "relay_health_failed" "(healthy) no health failure"
assert_not_out "missing_from_registry" "(healthy) no missing registry relay"
assert_not_out "abuse_flood" "(healthy) no abuse flood"

# ══════════════════════════════════════════════════════════════
# (a) zero relayed packets while up
# ══════════════════════════════════════════════════════════════
H_ZERO="$TMP/zero.json"
emit8 "$H_ZERO" a_zero
run_anomaly "$H_ZERO" "$REG4"
assert_rc 1 "(a) anomalous run exits 1"
assert_out "zero_relay" "(a) fires zero_relay"
assert_out "0 packets" "(a) carries the evidence"
assert_out "[critical]" "(a) is critical"
assert_out "auth rejection(s) were recorded" "(a) names the rejection that makes it an outage"
assert_out "not proof the relay is faulty" "(a) warns the rejection count is not conclusive"
assert_not_out "auth_spike" "(a) does not invent an auth spike"

# Silence WITHOUT rejections is traffic distribution, not an outage: it must
# be a warning, and it must not claim the outage signature.
H_IDLEONLY="$TMP/zero-idle.json"
emit8 "$H_IDLEONLY" a_idle
run_anomaly "$H_IDLEONLY" "$REG4"
assert_rc 1 "(a2) a silent-but-unused relay still exits 1"
assert_out "idle_relay" "(a2) fires idle_relay"
assert_out "[warning]" "(a2) is a warning, not critical"
assert_out "traffic distribution, not a failure" "(a2) says what it actually is"
assert_not_out "zero_relay" "(a2) does not claim the outage signature"

# FP guard: a single-snapshot window is not sustained enough.
run_anomaly "$H_ZERO" "$REG4" --window 1
assert_rc 0 "(a guard) window=1 is not sustained"
assert_not_out "zero_relay" "(a guard) no zero_relay on a single snapshot"

# FP guard: a fully idle fleet must not be misread as the 1.6.3 outage
# signature. zero_relay means "this relay is silent WHILE the fleet is
# busy"; with nothing flowing anywhere there is no evidence any single
# relay is the broken one, so it stays quiet and fleet_idle - asserted
# separately below - carries that condition instead.
H_IDLE="$TMP/idle.json"
build_history "$H_IDLE" \
	"$(mksnap 1000 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")" \
	"$(mksnap 1100 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")" \
	"$(mksnap 1200 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")"
REG2="$TMP/registry-2.json"
mkreg "$REG2" relay-a relay-b
run_anomaly "$H_IDLE" "$REG2"
assert_not_out "zero_relay:" "(a guard) all-idle fleet raises no zero_relay"
assert_out "fleet_idle" "(a guard) all-idle fleet is reported as fleet_idle instead"

# ══════════════════════════════════════════════════════════════
# (b) auth_rejections spike with sessions flat
# ══════════════════════════════════════════════════════════════
H_AUTH="$TMP/auth.json"
emit8 "$H_AUTH" a_authspike
run_anomaly "$H_AUTH" "$REG4"
assert_rc 1 "(b) anomalous run exits 1"
assert_out "auth_spike_no_sessions" "(b) fires auth spike"
assert_out "auth_rejections +5100 with sessions_created +0" "(b) carries the evidence"
assert_not_out "zero_relay" "(b) packets still flow, so no zero_relay"

# FP guard: a relay with steady scanner noise and flat sessions is not a
# spike, and a fleet with no sessions at all is not the data-only proxy.
H_AUTH_STEADY="$TMP/auth-steady.json"
emit8 "$H_AUTH_STEADY" a_healthy
run_anomaly "$H_AUTH_STEADY" "$REG4"
assert_rc 0 "(b guard) steady auth with sessions exits 0"
assert_not_out "auth_spike" "(b guard) steady traffic is not a spike"

# ══════════════════════════════════════════════════════════════
# (c) version lag vs fleet majority
# ══════════════════════════════════════════════════════════════
H_LAG="$TMP/lag.json"
emit8 "$H_LAG" a_lag
run_anomaly "$H_LAG" "$REG4"
assert_rc 1 "(c) anomalous run exits 1"
assert_out "version_lag" "(c) fires version lag"
assert_out "version 1.6.3 behind fleet 1.6.11" "(c) carries the evidence"
assert_out "[warning]" "(c) is a warning"

# A canary ahead of the fleet is not behind and must not be flagged.
H_CANARY="$TMP/canary.json"
emit8 "$H_CANARY" a_canary
run_anomaly "$H_CANARY" "$REG4"
assert_rc 0 "(c guard) newer canary exits 0"
assert_not_out "version_lag" "(c guard) newer version is not behind"

# ══════════════════════════════════════════════════════════════
# (c2) whole-fleet release lag
# ══════════════════════════════════════════════════════════════
# version_lag only compares relays against EACH OTHER, so a fleet that is
# uniformly behind the latest release raises nothing - every relay agrees with
# its neighbour. That is exactly the "the deploy silently did not happen" case.
# --expect-version (supplied by the workflow from the latest tag) closes it.
run_anomaly "$H_HEALTHY" "$REG4" --expect-version 1.6.99
assert_rc 1 "(c2) a fleet behind the release exits 1"
assert_out "release_lag" "(c2) fires release_lag when the WHOLE fleet is behind"
assert_out "[critical]" "(c2) is critical, not a warning"
assert_out "version_lag cannot see this" "(c2) explains why version_lag missed it"

# Matching the release must stay silent - a detector that only ever fires is useless.
run_anomaly "$H_HEALTHY" "$REG4" --expect-version 1.6.11
assert_rc 0 "(c2 guard) a fleet on the released version exits 0"
assert_not_out "release_lag" "(c2 guard) a fleet on the released version is not flagged"

# Unset (the default) must stay silent so the check is strictly opt-in.
run_anomaly "$H_HEALTHY" "$REG4"
assert_not_out "release_lag" "(c2 guard) no --expect-version means no release check"

# ══════════════════════════════════════════════════════════════
# (d) ping-saved regression and negative-saving share spike
# ══════════════════════════════════════════════════════════════
H_REGRESS="$TMP/regress.json"
emit8 "$H_REGRESS" a_savedreg
run_anomaly "$H_REGRESS" "$REG4"
assert_rc 1 "(d) regression run exits 1"
assert_out "saved_regression" "(d) fires saved regression"
assert_out "relay-a" "(d) names the relay"
assert_not_out "negative_saving" "(d) regression is not a negative share"

H_NEG="$TMP/neg.json"
emit8 "$H_NEG" a_negshare
run_anomaly "$H_NEG" "$REG4"
assert_rc 1 "(d) negative share run exits 1"
assert_out "negative_saving_spike" "(d) fires negative-saving spike"
assert_out "relay-a" "(d) negative share names the relay"

# ══════════════════════════════════════════════════════════════
# (e) registry and health
# ══════════════════════════════════════════════════════════════
# A relay in history but dropped from the registry.
H_MISSING="$TMP/missing.json"
build_history "$H_MISSING" \
	"$(mksnap 1000 "$(mkrelay relay-a true "$STD_VER" 100 1 2 20 4 0 3)" "$(mkrelay relay-b true "$STD_VER" 50 1 2 20 4 0 3)" "$(mkrelay relay-x true "$STD_VER" 10 1 2 20 4 0 3)")" \
	"$(mksnap 1100 "$(mkrelay relay-a true "$STD_VER" 200 2 4 40 8 0 6)" "$(mkrelay relay-b true "$STD_VER" 100 2 4 40 8 0 6)" "$(mkrelay relay-x true "$STD_VER" 20 2 4 40 8 0 6)")" \
	"$(mksnap 1200 "$(mkrelay relay-a true "$STD_VER" 300 3 6 60 12 0 9)" "$(mkrelay relay-b true "$STD_VER" 150 3 6 60 12 0 9)" "$(mkrelay relay-x true "$STD_VER" 30 3 6 60 12 0 9)")"
mkreg "$REG2" relay-a relay-b
run_anomaly "$H_MISSING" "$REG2"
assert_rc 1 "(e) missing registry run exits 1"
assert_out "relay_missing_from_registry" "(e) fires missing from registry"
assert_out "absent from the registry" "(e) carries the evidence"

# A registry relay that stops answering /health. --no-probe reads the
# latest history reachability.
H_DOWN="$TMP/down.json"
build_history "$H_DOWN" \
	"$(mksnap 1000 "$(mkrelay relay-a true "$STD_VER" 100 1 2 20 4 0 3)" "$(mkrelay relay-b true "$STD_VER" 50 1 2 20 4 0 3)")" \
	"$(mksnap 1100 "$(mkrelay relay-a true "$STD_VER" 200 2 4 40 8 0 6)" "$(mkrelay relay-b true "$STD_VER" 100 2 4 40 8 0 6)")" \
	"$(mksnap 1200 "$(mkrelay relay-a false "$STD_VER" 200 2 4 40 8 0 6)" "$(mkrelay relay-b true "$STD_VER" 150 3 6 60 12 0 9)")"
run_anomaly "$H_DOWN" "$REG2"
assert_rc 1 "(e) health failure run exits 1"
assert_out "relay_health_failed" "(e) fires health failure for relay-a"
assert_out "relay-a" "(e) health failure names relay-a"

# ══════════════════════════════════════════════════
# (f) abuse flood absorbed by the detector
# ══════════════════════════════════════════════════
# The signature the liveness gate and detectors a-d cannot see:
# abuse_blocked dominates drops while real sessions keep flowing.
H_ABUSE="$TMP/abuse.json"
emit8 "$H_ABUSE" a_abuse
run_anomaly "$H_ABUSE" "$REG4"
assert_rc 1 "(f) abuse flood exits 1"
assert_out "abuse_flood" "(f) fires abuse_flood"
assert_out "relay-a" "(f) names the flooded relay"
assert_out "% of drops" "(f) reports the drop share"
assert_not_out "zero_relay" "(f) packets still relayed, so no zero_relay"
assert_not_out "auth_spike" "(f) abuse is not an auth spike"

# Guard: a handful of abuse blocks is noise, not a flood.
H_ABUSE_TINY="$TMP/abuse-tiny.json"
emit8 "$H_ABUSE_TINY" a_abusetiny
run_anomaly "$H_ABUSE_TINY" "$REG4"
assert_rc 0 "(f guard) tiny abuse count exits 0"
assert_not_out "abuse_flood" "(f guard) absolute floor suppresses noise"

# Guard: abuse-heavy, but legitimate traffic still dominates the drops.
H_ABUSE_MIXED="$TMP/abuse-mixed.json"
emit8 "$H_ABUSE_MIXED" a_abusemixed
run_anomaly "$H_ABUSE_MIXED" "$REG4"
assert_rc 0 "(f guard) mixed traffic exits 0"
assert_not_out "abuse_flood" "(f guard) share floor suppresses mixed traffic"

# Guard: the full window is required, a single snapshot is not sustained.
run_anomaly "$H_ABUSE" "$REG4" --window 1
assert_rc 0 "(f guard) window=1 is not sustained"
assert_not_out "abuse_flood" "(f guard) no abuse_flood on a single snapshot"

# ═══════════════════════════════════════════
# (g) fleet-wide traffic stop
# ═══════════════════════════════════════════
# Every relay up, every relay relaying nothing, for the whole window.
# This is the condition observed live on 2026-10-01 (all eight relays
# healthy with byte-identical counters for hours) and it previously
# raised NOTHING, because zero_relay requires a busy fleet.
H_FLEETIDLE="$TMP/fleet-idle.json"
build_history "$H_FLEETIDLE" \
	"$(mksnap 1000 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")" \
	"$(mksnap 1100 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")" \
	"$(mksnap 1200 "$(mkrelay relay-a true "$STD_VER" 0 0 0 0 0 0 0)" "$(mkrelay relay-b true "$STD_VER" 0 0 0 0 0 0 0)")"
run_anomaly "$H_FLEETIDLE" "$REG2"
assert_rc 1 "(g) a fleet-wide traffic stop exits 1"
assert_out "fleet_idle" "(g) fires fleet_idle"
assert_out "whole fleet relayed 0 packets" "(g) carries the evidence"
assert_out "[critical]" "(g) is critical"

# A fleet with traffic must not trip it.
run_anomaly "$H_HEALTHY" "$REG4"
assert_not_out "fleet_idle" "(g guard) a busy fleet is not idle"

# Guard: a FROZEN collector must not be read as an idle fleet.
# Observed live 2026-10-01: the collector went 5h stale while its newest
# snapshots kept byte-identical counters, and fleet_idle reported a
# fleet-wide stop that had not happened - live /health showed the relay
# actively relaying. Frozen data looks exactly like an idle fleet, so the
# conclusion requires fresh data.
LIGHTSPEED_NOW_EPOCH=$((STD_T_BASE + 20000)) \
	run_anomaly "$H_FLEETIDLE" "$REG2"
assert_rc 1 "(g2) a stale history still exits 1"
assert_out "stale_history" "(g2) reports the stopped collector"
assert_not_out "fleet_idle" "(g2) frozen data is NOT reported as an idle fleet"

# ═══════════════════════════════════════════
# (h) stale history: the collector stopped publishing
# ═══════════════════════════════════════════
# Every fixture above uses t values near 1700 with "now" pinned
# just past them, so this suite would silently stop meaning anything if
# the staleness check were always-on. These cases pin the boundary.
H_FRESH="$TMP/fresh.json"
emit8 "$H_FRESH" a_healthy

LIGHTSPEED_NOW_EPOCH=$((STD_T_BASE + 10799)) \
	run_anomaly "$H_FRESH" "$REG4"
assert_rc 0 "(g guard) just inside the staleness limit exits 0"
assert_not_out "stale_history" "(g guard) age below the limit is not stale"

LIGHTSPEED_NOW_EPOCH=$((STD_T_BASE + 10801)) \
	run_anomaly "$H_FRESH" "$REG4"
assert_rc 1 "(g) a frozen collector exits 1"
assert_out "stale_history" "(g) fires stale_history"
assert_out "has stopped publishing" "(g) carries the evidence"
assert_out "[warning]" "(g) is a warning"

# --max-staleness 0 disables the check outright, for static fixtures.
LIGHTSPEED_NOW_EPOCH=$((STD_T_BASE + 1000000)) \
	run_anomaly "$H_FRESH" "$REG4" --max-staleness 0
assert_rc 0 "(g guard) --max-staleness 0 disables the check"
assert_not_out "stale_history" "(g guard) disabled check stays silent"

# --no-probe must not flag a registry relay that has no history yet.
H_NOHIST="$TMP/nohist.json"
REG1="$TMP/registry-1.json"
mkreg "$REG1" relay-a
run_anomaly "$H_NOHIST" "$REG1"
assert_rc 0 "(e guard) unscraped registry relay is not a failure"
assert_not_out "relay_health_failed" "(e guard) no health failure without evidence"

# ══════════════════════════════════════════════════════════════
# --json output contract
# ══════════════════════════════════════════════════════════════
run_anomaly "$H_ZERO" "$REG4" --json
assert_rc 1 "(json) anomalous run exits 1"
assert_json '.anomalies | length >= 1' "(json) anomalies array populated"
assert_json '[.anomalies[] | select(.type == "zero_relay" and .relay == "relay-a")] | length == 1' "(json) zero_relay object present"
assert_json '.anomalies | all(.severity == "critical" or .severity == "warning")' "(json) severities are known"

run_anomaly "$H_HEALTHY" "$REG4" --json
assert_rc 0 "(json) healthy run exits 0"
assert_json '.anomalies | length == 0' "(json) no anomalies on healthy fleet"

run_anomaly "$H_ABUSE" "$REG4" --json
assert_rc 1 "(json) abuse run exits 1"
assert_json '[.anomalies[] | select(.type == "abuse_flood" and .relay == "relay-a")] | length == 1' "(json) abuse_flood object present"
assert_json '[.anomalies[] | select(.type == "abuse_flood") | .abuse_share] | all(. >= 0.9)' "(json) abuse share crosses the floor"

# ── Discord path is taken, and a failed post is swallowed ────
# A refused connection must not change the exit code or hide the
# anomaly; the alert is best-effort, exactly like the liveness monitor.
OUT="$(LIGHTSPEED_NODES='' LIGHTSPEED_REGISTRY_PATH="$REG4" \
	DISCORD_WEBHOOK='http://127.0.0.1:9/none' \
	bash "$ANOMALY" --history "$H_ZERO" --registry "$REG4" --no-probe --timeout 2 2>&1)"
RC=$?
assert_rc 1 "(discord) exits 1 when the webhook post fails"
assert_out "zero_relay" "(discord) still reports the anomaly"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'health-anomaly: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'health-anomaly: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
