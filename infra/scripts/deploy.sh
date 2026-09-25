#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Canary Deploy of the Proxy to Mesh Nodes
#
# Cross-compiles the proxy binary for Linux x86_64, uploads it to a
# staging path on each node, then runs relay-install.sh over SSH. The
# installer drops the binary into /opt/lightspeed/releases/<version>/,
# repoints /opt/lightspeed/current atomically, restarts systemd, and
# health-gates the result (rolling back on failure).
#
# Fleet deploys are canaried by default: one relay is upgraded and
# verified, and only then are the remaining relays shipped in batches.
# The canary gate is stronger than the installer's /health check:
#   (a) /health reports the new version,
#   (b) UDP 4433 (control) and 4434 (data) are listening, and
#   (c) a real client registration succeeds (--test-control).
# On any failure the rollout stops, the failing relay is reverted to
# the version it was running before this rollout (kept by the
# versioned release layout), and the script exits non-zero with the
# evidence.
#
# Usage:
#   ./deploy.sh                      # Canary, then batches, to all nodes
#   ./deploy.sh --batch-size 2       # Two relays per post-canary batch
#   ./deploy.sh --canary relay-lax   # Pick the canary by node id or region
#   ./deploy.sh --no-canary          # Legacy: install every node in one pass
#   ./deploy.sh relay-lax            # Deploy to a single node (manual)
#   ./deploy.sh --build-only         # Just compile, don't deploy
#
# Environment:
#   LIGHTSPEED_VERSION       Override the release version (default: the built
#                            binary's own version; an override must equal it).
#   LIGHTSPEED_CANARY        Set to 0 to disable the canary pass (legacy).
#   LIGHTSPEED_CANARY_NODE   Node id or region to use as the canary.
#   LIGHTSPEED_BATCH_SIZE    Relays per post-canary batch (default: 1).
#   LIGHTSPEED_PROBE_BIN     Prebuilt client binary for --test-control probes.
#                            Built from source with --features quic when unset.
#   LIGHTSPEED_SSH/SCP/CURL  Override the command seams (used by tests).
#
# Sourcing this file defines the functions and does not run a deploy; the
# test suite (test_deploy_canary.sh) drives the orchestration directly.
#
# Prerequisites:
#   - SSH key at ~/.ssh/lightspeed_deploy (or set DEPLOY_SSH_KEY)
#   - Rust cross-compilation target: rustup target add x86_64-unknown-linux-gnu
#   - Or use cross: cargo install cross
# ──────────────────────────────────────────────────────────────

# NOTE: no `set -e` here. The script is sourceable, and enabling errexit in
# the caller's shell would be a side effect. `lightspeed_deploy_main` turns it
# on for the real run.
set -uo pipefail

# ── Configuration ────────────────────────────────────────────
SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/lightspeed_deploy}"
SSH_USER="${DEPLOY_SSH_USER:-root}"
SSH_OPTS="-o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
BINARY_NAME="lightspeed-proxy"
CLIENT_BINARY_NAME="lightspeed"
REMOTE_STAGING="/tmp/${BINARY_NAME}.staged"
REMOTE_INSTALLER="/tmp/lightspeed-relay-install.sh"
REMOTE_UPDATER="/tmp/lightspeed-relay-updater.sh"

# Command seams. Tests override these (or the functions that use them).
SSH="${LIGHTSPEED_SSH:-ssh}"
SCP="${LIGHTSPEED_SCP:-scp}"
CURL="${LIGHTSPEED_CURL:-curl}"
PYTHON="${LIGHTSPEED_PYTHON:-python3}"

# Control/data plane defaults for the verification gate. Per-node values from
# the inventory take precedence.
CONTROL_PORT="${LIGHTSPEED_CONTROL_PORT:-4433}"
DATA_PORT="${LIGHTSPEED_DATA_PORT:-4434}"
HEALTH_TIMEOUT="${LIGHTSPEED_HEALTH_TIMEOUT:-5}"
POST_INSTALL_SLEEP="${LIGHTSPEED_POST_INSTALL_SLEEP:-2}"
REMOTE_LAYOUT_ROOT="${LIGHTSPEED_LAYOUT_ROOT:-/opt/lightspeed}"
SERVICE_NAME="${LIGHTSPEED_SERVICE_NAME:-lightspeed-proxy}"

# Rollout configuration (CLI flags and env share these globals).
CANARY_ENABLED="${LIGHTSPEED_CANARY:-1}"
CANARY_NODE="${LIGHTSPEED_CANARY_NODE:-}"
BATCH_SIZE="${LIGHTSPEED_BATCH_SIZE:-1}"

# Probe binary (client, --test-control). Resolved/built by main for the canary
# path; may be left empty for the legacy path.
PROBE_BIN="${LIGHTSPEED_PROBE_BIN:-}"

# Evidence accumulated for a stopped rollout and the most recent gate.
ROLLOUT_EVIDENCE=""
VERIFY_EVIDENCE=""
PROBE_LAST_OUTPUT=""

# Inventory globals. Declared here so a sourced shell (and `set -u`) can read
# them before `lightspeed_load_inventory` populates them.
declare -A NODES=()
declare -A REGION_TO_NODE=()
declare -A NODE_DATA_PORT=()
declare -A NODE_CONTROL_PORT=()
NODE_NAMES=()

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RELAY_INSTALLER="$SCRIPT_DIR/relay-install.sh"
RELAY_UPDATER="$SCRIPT_DIR/relay-updater.sh"

# ── Resolve node inventory (fail closed) ─────────────────────
# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

# ── Usage ────────────────────────────────────────────────────
usage() {
	awk 'NR == 1 { next } /^# ── Configuration/ { exit } { sub(/^# ?/, ""); print }' \
		"${BASH_SOURCE[0]}"
}

# ── Logging helpers ──────────────────────────────────────────
log_info() { printf '%s\n' "$*"; }
log_err() { printf '%s\n' "$*" >&2; }
evidence() { printf '  evidence: %s\n' "$*" >&2; }

# ── Args ─────────────────────────────────────────────────────
# Sets TARGET_NODE, BUILD_ONLY, CANARY_ENABLED, CANARY_NODE, BATCH_SIZE.
lightspeed_parse_args() {
	TARGET_NODE=""
	BUILD_ONLY=false

	while [ $# -gt 0 ]; do
		case "$1" in
		--build-only) BUILD_ONLY=true ;;
		--no-canary) CANARY_ENABLED=0 ;;
		--canary)
			[ $# -ge 2 ] || {
				log_err "  --canary needs a node id or region"
				return 1
			}
			CANARY_NODE="$2"
			shift
			;;
		--canary=*) CANARY_NODE="${1#*=}" ;;
		--batch-size)
			[ $# -ge 2 ] || {
				log_err "  --batch-size needs a positive integer"
				return 1
			}
			BATCH_SIZE="$2"
			shift
			;;
		--batch-size=*) BATCH_SIZE="${1#*=}" ;;
		-h | --help)
			usage
			return 2
			;;
		-*)
			log_err "  Unknown option: $1"
			return 1
			;;
		*) TARGET_NODE="$1" ;;
		esac
		shift
	done

	case "$BATCH_SIZE" in
	'' | *[!0-9]*)
		log_err "  Invalid --batch-size '$BATCH_SIZE' (need a positive integer)"
		return 1
		;;
	esac
	if [ "$BATCH_SIZE" -lt 1 ]; then
		log_err "  Invalid --batch-size '$BATCH_SIZE' (need a positive integer)"
		return 1
	fi
	return 0
}

# ── Canary selection ─────────────────────────────────────────
# Echo the node name to canary. Honors CANARY_NODE as a node id or a region,
# otherwise picks the first node in inventory order. Non-zero on an unknown
# canary or an empty fleet.
lightspeed_select_canary() {
	local want="${CANARY_NODE:-}"
	if [ -n "$want" ]; then
		if [ -n "${NODES[$want]:-}" ]; then
			printf '%s' "$want"
			return 0
		fi
		if [ -n "${REGION_TO_NODE[$want]:-}" ]; then
			printf '%s' "${REGION_TO_NODE[$want]}"
			return 0
		fi
		log_err "  Unknown canary '$want' (not a node id or region)"
		return 1
	fi
	if [ "${#NODE_NAMES[@]}" -eq 0 ]; then
		log_err "  No nodes available to choose a canary"
		return 1
	fi
	printf '%s' "${NODE_NAMES[0]}"
}

# ── Batch splitting ──────────────────────────────────────────
# Print the given node names in fixed-size batches, one batch per line,
# preserving inventory order. Non-zero on a non-numeric or zero size.
lightspeed_batch_nodes() {
	local size="$1"
	shift
	case "$size" in
	'' | *[!0-9]*)
		log_err "  Invalid batch size '$size'"
		return 1
		;;
	esac
	if [ "$size" -lt 1 ]; then
		log_err "  Invalid batch size '$size' (must be >= 1)"
		return 1
	fi

	local i=0 line="" node
	for node in "$@"; do
		line="${line:+$line }$node"
		i=$((i + 1))
		if [ $((i % size)) -eq 0 ]; then
			printf '%s\n' "$line"
			line=""
		fi
	done
	[ -n "$line" ] && printf '%s\n' "$line"
	return 0
}

# ── Health / probe helpers ───────────────────────────────────
health_version_of() {
	local ip="$1"
	"$CURL" -sf --max-time "$HEALTH_TIMEOUT" "http://${ip}:8080/health" 2>/dev/null |
		"$PYTHON" -c 'import sys,json; print(json.load(sys.stdin).get("version",""))' 2>/dev/null
}

udp_port_listening() {
	local ip="$1" port="$2" got
	# shellcheck disable=SC2086
	got="$("$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" \
		"ss -lunp 2>/dev/null | grep -q ':${port}' && echo yes || echo no" 2>/dev/null || echo no)"
	[ "$got" = "yes" ]
}

# A real control-plane registration: the same probe the health-monitor
# workflow runs against live relays. The client exits 0 even when it only
# partially connects, so the marker string is the assertion.
probe_control_plane() {
	local ip="$1" dport="${2:-$DATA_PORT}" out attempt
	if [ -z "$PROBE_BIN" ] || [ ! -x "$PROBE_BIN" ]; then
		return 1
	fi
	# A single failed probe is not proof of a broken relay: a relay can be
	# briefly busy or mid-restart, and a false negative here blocks a good
	# deploy. Retry before declaring failure, and keep the last output so the
	# evidence block can show why.
	for attempt in 1 2 3; do
		out="$("$PROBE_BIN" --test-control --proxy "${ip}:${dport}" 2>&1 || true)"
		if printf '%s' "$out" | grep -q "Control plane is working"; then
			return 0
		fi
		PROBE_LAST_OUTPUT="$out"
		[ "$attempt" -lt 3 ] && sleep 5
	done
	return 1
}

# ── Verification gate ────────────────────────────────────────
# verify_node <name> <ip> <expected_version>
#   (a) /health reports the expected version
#   (b) UDP control and data ports are listening
#   (c) a real control-plane registration succeeds
# Sets VERIFY_EVIDENCE and returns non-zero on the first failure.
verify_node() {
	local name="$1" ip="$2" want="$3"
	local dport="${NODE_DATA_PORT[$name]:-$DATA_PORT}"
	local cport="${NODE_CONTROL_PORT[$name]:-$CONTROL_PORT}"
	local hv

	VERIFY_EVIDENCE="node=${name} ip=${ip} expected=${want} data_port=${dport} control_port=${cport}"

	hv="$(health_version_of "$ip" 2>/dev/null || true)"
	if [ "$hv" != "$want" ]; then
		VERIFY_EVIDENCE="${VERIFY_EVIDENCE} health_version=${hv:-unreachable}"
		log_err "  ❌ ${name}: /health version '${hv:-unreachable}' != '${want}'"
		return 1
	fi
	VERIFY_EVIDENCE="${VERIFY_EVIDENCE} health_version=${hv}"

	if ! udp_port_listening "$ip" "$dport"; then
		VERIFY_EVIDENCE="${VERIFY_EVIDENCE} data_port_${dport}=not_listening"
		log_err "  ❌ ${name}: UDP ${dport} (data) is not listening"
		return 1
	fi
	if ! udp_port_listening "$ip" "$cport"; then
		VERIFY_EVIDENCE="${VERIFY_EVIDENCE} control_port_${cport}=not_listening"
		log_err "  ❌ ${name}: UDP ${cport} (control) is not listening"
		return 1
	fi
	VERIFY_EVIDENCE="${VERIFY_EVIDENCE} ports=ok"

	if ! probe_control_plane "$ip" "$dport"; then
		VERIFY_EVIDENCE="${VERIFY_EVIDENCE} control_probe=failed probe_output=$(printf '%s' "$PROBE_LAST_OUTPUT" | tr '\n' '|' | tail -c 300)"
		log_err "  ❌ ${name}: control-plane registration failed"
		return 1
	fi
	VERIFY_EVIDENCE="${VERIFY_EVIDENCE} control_probe=ok"
	return 0
}

# ── Revert a node to its previous release ────────────────────
# revert_node <name> <ip> <previous_version>
# The versioned layout keeps /opt/lightspeed/releases/<version> and the
# `current` symlink. Point `current` back and restart. Never leaves `current`
# dangling: the new symlink is created first and swapped atomically.
revert_node() {
	local name="$1" ip="$2" prev="$3"
	if [ -z "$prev" ] || [ "$prev" = "?" ] || [ "$prev" = "unreachable" ]; then
		log_err "  ⚠️  ${name}: no previous version recorded; cannot revert"
		return 1
	fi

	local root="$REMOTE_LAYOUT_ROOT"
	local remote_cmd
	remote_cmd="set -e
prev='${root}/releases/${prev}'
if [ ! -d \"\$prev\" ]; then echo 'revert: missing release dir' \"\$prev\" >&2; exit 3; fi
tmp='${root}/current.revert.$$'
ln -sfn \"\$prev\" \"\$tmp\"
mv -Tf \"\$tmp\" '${root}/current'
systemctl restart '${SERVICE_NAME}'"

	# shellcheck disable=SC2086
	if "$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" "$remote_cmd"; then
		log_info "  ↩️  ${name} reverted to ${prev}"
		return 0
	fi
	log_err "  ❌ ${name}: revert to ${prev} failed"
	return 1
}

# ── Record and report a stopped rollout ──────────────────────
rollout_stop() {
	ROLLOUT_EVIDENCE="$1"
	printf '\n%s❌ ROLLOUT STOPPED%s\n' "$RED" "$NC" >&2
	evidence "$1"
	return 1
}

# ── Deploy one node (install + installer health gate) ────────
deploy_node() {
	local name="$1"
	local ip="$2"

	printf "  %-12s %-16s " "$name" "$ip"

	# Pre-deploy health check (best effort; also the revert target).
	local pre_health
	pre_health="$(health_version_of "$ip" 2>/dev/null || true)"
	[ -n "$pre_health" ] || pre_health="unreachable"

	# Upload the staged binary and the reusable installer.
	# shellcheck disable=SC2086
	if ! "$SCP" $SSH_OPTS -i "$SSH_KEY" "$BINARY_PATH" \
		"${SSH_USER}@${ip}:${REMOTE_STAGING}" 2>/dev/null; then
		printf '\n'
		log_err "  ❌ ${name}: SCP (binary) failed"
		return 1
	fi

	# shellcheck disable=SC2086
	if ! "$SCP" $SSH_OPTS -i "$SSH_KEY" "$RELAY_INSTALLER" \
		"${SSH_USER}@${ip}:${REMOTE_INSTALLER}" 2>/dev/null; then
		printf '\n'
		log_err "  ❌ ${name}: SCP (installer) failed"
		return 1
	fi

	# Ship the self-updater too: the deploy pipeline is the only place it can
	# be refreshed, since provisioning installs it once. Missing/failed upload
	# is non-fatal.
	local have_updater=0
	if [ -f "$RELAY_UPDATER" ]; then
		# shellcheck disable=SC2086
		if "$SCP" $SSH_OPTS -i "$SSH_KEY" "$RELAY_UPDATER" \
			"${SSH_USER}@${ip}:${REMOTE_UPDATER}" 2>/dev/null; then
			have_updater=1
		else
			log_err "  ⚠️  ${name}: SCP (updater) failed; deploying without it"
		fi
	fi

	# Versioned, health-gated activation with automatic rollback. public_ip
	# gives the proxy exact self-tunnel filtering; the updater is refreshed.
	local installer_cmd="bash ${REMOTE_INSTALLER} --binary ${REMOTE_STAGING} --version '${VERSION}' --public-ip '${ip}'"
	if [ "$have_updater" -eq 1 ]; then
		installer_cmd="$installer_cmd --updater ${REMOTE_UPDATER}"
	fi

	# shellcheck disable=SC2086
	if "$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" "$installer_cmd"; then
		sleep "$POST_INSTALL_SLEEP"
		local post_health
		post_health="$(health_version_of "$ip" 2>/dev/null || true)"
		[ -n "$post_health" ] || post_health="starting..."
		printf '%b✅ OK%b  [%s] → [%s]\n' "$GREEN" "$NC" "$pre_health" "$post_health"

		# The installer health gate cannot see the control plane, so assert
		# the control port here too: a binary without quic still serves
		# /health while rejecting all clients.
		local cport="${NODE_CONTROL_PORT[$name]:-$CONTROL_PORT}"
		if ! udp_port_listening "$ip" "$cport"; then
			log_err "  ❌ ${name}: control plane (UDP ${cport}) is not listening after install"
			return 1
		fi
	else
		printf '\n'
		log_err "  ❌ ${name}: install/health gate failed (installer rolled back)"
		return 1
	fi
}

# ── Canary-then-rest rollout ─────────────────────────────────
# Deploy and verify one canary, then the remaining nodes in batches,
# verifying every node. On any failure stop, revert the failing node to
# the version it ran before this rollout, and return non-zero with
# evidence. Returns 0 only when the whole fleet is on VERSION.
lightspeed_rollout_canary_then_rest() {
	ROLLOUT_EVIDENCE=""

	local canary
	if ! canary="$(lightspeed_select_canary)"; then
		rollout_stop "canary selection failed"
		return 1
	fi
	local canary_ip="${NODES[$canary]}"
	printf '\n%s== Canary: %s (%s) ==%s\n' "$CYAN" "$canary" "$canary_ip" "$NC"

	# Record the running version before touching the canary so we can revert.
	local prev
	prev="$(health_version_of "$canary_ip" 2>/dev/null || true)"
	printf '  previous version: %s\n' "${prev:-unreachable}"

	if ! deploy_node "$canary" "$canary_ip"; then
		# The installer health-gates and rolls back on its own; the canary is
		# still on the previous version. Stop without a second revert.
		rollout_stop "canary install failed on ${canary}; relay-install rolled back"
		return 1
	fi

	if ! verify_node "$canary" "$canary_ip" "$VERSION"; then
		rollout_stop "canary verification failed: ${VERIFY_EVIDENCE}"
		revert_node "$canary" "$canary_ip" "$prev" || true
		return 1
	fi
	printf '  %s✅ canary %s verified%s\n' "$GREEN" "$canary" "$NC"

	# Assemble the remaining nodes in inventory order.
	local -a remaining=()
	local n
	for n in "${NODE_NAMES[@]}"; do
		[ "$n" = "$canary" ] && continue
		remaining+=("$n")
	done
	if [ "${#remaining[@]}" -eq 0 ]; then
		printf '\n%s✅ Fleet updated (canary was the only node)%s\n' "$GREEN" "$NC"
		return 0
	fi

	local batch_no=0 batch line
	# Collect the batches into an array FIRST rather than reading them from a
	# process substitution inside the loop. An ssh command executed inside a
	# `while read` loop consumes the loop's stdin, which silently truncated the
	# rollout after the first batch (the deploy reported success with most of
	# the fleet untouched). Reading the list up front removes that coupling.
	local -a batches=()
	while IFS= read -r line; do
		[ -n "$line" ] && batches+=("$line")
	done < <(lightspeed_batch_nodes "$BATCH_SIZE" "${remaining[@]}")

	for batch in "${batches[@]}"; do
		batch_no=$((batch_no + 1))
		printf '\n%s-- batch %s (size %s): %s%s\n' "$CYAN" "$batch_no" "$BATCH_SIZE" "$batch" "$NC"
		for n in $batch; do
			local nip="${NODES[$n]}"
			local pv
			pv="$(health_version_of "$nip" 2>/dev/null || true)"
			if ! deploy_node "$n" "$nip"; then
				rollout_stop "install failed on ${n} (batch ${batch_no}); rollout stopped"
				return 1
			fi
			if ! verify_node "$n" "$nip" "$VERSION"; then
				rollout_stop "verification failed on ${n} (batch ${batch_no}): ${VERIFY_EVIDENCE}"
				revert_node "$n" "$nip" "$pv" || true
				return 1
			fi
			printf '  %s✅ %s verified%s\n' "$GREEN" "$n" "$NC"
		done
		printf '  %s✅ batch %s verified%s\n' "$GREEN" "$batch_no" "$NC"
	done

	printf '\n%s✅ Fleet updated%s\n' "$GREEN" "$NC"
	return 0
}

# ── Legacy fleet deploy (canary disabled) ────────────────────
lightspeed_deploy_fleet_legacy() {
	local pass=0 fail=0 name
	for name in "${NODE_NAMES[@]}"; do
		if deploy_node "$name" "${NODES[$name]}"; then
			pass=$((pass + 1))
		else
			fail=$((fail + 1))
		fi
	done
	printf '\nDeployed: %s  Failed: %s\n' "$pass" "$fail"
	[ "$fail" -eq 0 ]
}

# ── Single-relay manual deploy ───────────────────────────────
lightspeed_deploy_single() {
	local target="$1" name
	if [ -n "${NODES[$target]:-}" ]; then
		name="$target"
	elif [ -n "${REGION_TO_NODE[$target]:-}" ]; then
		name="${REGION_TO_NODE[$target]}"
	else
		log_err "  Unknown node: $target"
		log_err "  Available: ${NODE_NAMES[*]}"
		return 1
	fi
	deploy_node "$name" "${NODES[$name]}"
}

# ── Build the proxy ──────────────────────────────────────────
lightspeed_build_proxy() {
	log_info ""
	log_info "${CYAN}[1/4] Building release binary...${NC}"

	cd "$PROJECT_ROOT" || return 1

	# The proxy's `default` feature set is empty, so the QUIC control plane
	# (client registration) only exists with `--features quic`. Building
	# without it yields a data-only proxy that auth-rejects every packet.
	# Never drop these flags.
	if command -v cross &>/dev/null; then
		log_info "  Using 'cross' for Linux x86_64 cross-compilation"
		cross build --release --bin "$BINARY_NAME" --features quic --target x86_64-unknown-linux-gnu || return 1
		BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
	elif rustup target list --installed 2>/dev/null | grep -q "x86_64-unknown-linux-gnu"; then
		log_info "  Using cargo with x86_64-unknown-linux-gnu target"
		cargo build --release --bin "$BINARY_NAME" --features quic --target x86_64-unknown-linux-gnu || return 1
		BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
	else
		log_info "  ⚠️  No Linux cross-compilation target available."
		log_info "  Building for current platform (deploy only works if building on Linux)."
		cargo build --release --bin "$BINARY_NAME" --features quic || return 1
		BINARY_PATH="target/release/${BINARY_NAME}"
	fi

	if [ ! -f "$BINARY_PATH" ]; then
		log_err "  ❌ Binary not found at $BINARY_PATH"
		return 1
	fi

	local binary_size
	binary_size="$(du -h "$BINARY_PATH" | cut -f1)"
	log_info "  ${GREEN}✅ Built: $BINARY_PATH ($binary_size)${NC}"

	# Hard gate: the release string only exists in a binary compiled with the
	# quic feature, so a miss means the control plane was compiled out. Search
	# the file directly (a `strings | grep -q` pipeline would false-fail under
	# pipefail when grep exits before strings finishes).
	if ! grep -aqF "QUIC control plane listening" "$BINARY_PATH"; then
		log_err "  ❌ The built proxy has no QUIC control plane (built without --features quic). Refusing to deploy."
		return 1
	fi
	return 0
}

# ── Resolve / build the client probe ─────────────────────────
# The canary gate needs a real client to register against the relay. Prefer
# LIGHTSPEED_PROBE_BIN, then an existing build, then compile one with quic.
lightspeed_resolve_probe() {
	if [ -n "$PROBE_BIN" ]; then
		if [ -x "$PROBE_BIN" ]; then
			return 0
		fi
		log_err "  ❌ LIGHTSPEED_PROBE_BIN is not executable: $PROBE_BIN"
		return 1
	fi

	cd "$PROJECT_ROOT" || return 1
	local target_triple="x86_64-unknown-linux-gnu"
	local cand
	for cand in "target/${target_triple}/release/${CLIENT_BINARY_NAME}" \
		"target/release/${CLIENT_BINARY_NAME}"; do
		if [ -x "$PROJECT_ROOT/$cand" ]; then
			PROBE_BIN="$PROJECT_ROOT/$cand"
			return 0
		fi
	done

	log_info "  Building control-plane probe (${CLIENT_BINARY_NAME} --features quic)..."
	if command -v cross &>/dev/null; then
		cross build --release --bin "$CLIENT_BINARY_NAME" --features quic --target "$target_triple" || return 1
		PROBE_BIN="$PROJECT_ROOT/target/${target_triple}/release/${CLIENT_BINARY_NAME}"
	elif rustup target list --installed 2>/dev/null | grep -q "$target_triple"; then
		cargo build --release --bin "$CLIENT_BINARY_NAME" --features quic --target "$target_triple" || return 1
		PROBE_BIN="$PROJECT_ROOT/target/${target_triple}/release/${CLIENT_BINARY_NAME}"
	else
		cargo build --release --bin "$CLIENT_BINARY_NAME" --features quic || return 1
		PROBE_BIN="$PROJECT_ROOT/target/release/${CLIENT_BINARY_NAME}"
	fi

	if [ ! -x "$PROBE_BIN" ]; then
		log_err "  ❌ Probe build produced no binary at $PROBE_BIN"
		return 1
	fi
	return 0
}

# ── Resolve inventory into globals ───────────────────────────
lightspeed_load_inventory() {
	local nodes_json
	if ! nodes_json="$(lightspeed_resolve_nodes)"; then
		log_err "❌ No proxy node inventory available — refusing to deploy."
		log_err "   Set LIGHTSPEED_NODES or populate $LIGHTSPEED_REGISTRY_PATH."
		return 1
	fi

	if [ "$(printf '%s' "$nodes_json" | jq 'length')" -eq 0 ]; then
		log_err "❌ Proxy node inventory is empty — refusing to deploy."
		log_err "   Set LIGHTSPEED_NODES or populate $LIGHTSPEED_REGISTRY_PATH."
		return 1
	fi

	NODES=()
	REGION_TO_NODE=()
	NODE_NAMES=()
	NODE_DATA_PORT=()
	NODE_CONTROL_PORT=()
	local _name _region _ip _dport _cport
	while IFS=$'\t' read -r _name _region _ip _dport _cport; do
		[ -n "$_name" ] || continue
		NODES["$_name"]="$_ip"
		NODE_NAMES+=("$_name")
		NODE_DATA_PORT["$_name"]="${_dport:-$DATA_PORT}"
		NODE_CONTROL_PORT["$_name"]="${_cport:-$CONTROL_PORT}"
		if [ -n "$_region" ] && [ -z "${REGION_TO_NODE[$_region]:-}" ]; then
			REGION_TO_NODE["$_region"]="$_name"
		fi
	done < <(printf '%s' "$nodes_json" | jq -r \
		'.[] | [.node_id, .region, .ip, (.data_addr | split(":")[1] // ""), (.control_port // "")] | @tsv')
	return 0
}

# ── Main ─────────────────────────────────────────────────────
lightspeed_deploy_main() {
	set -euo pipefail

	# Parse args before any inventory work so --help and bad flags are cheap.
	local parse_rc=0
	lightspeed_parse_args "$@" || parse_rc=$?
	if [ "$parse_rc" -eq 2 ]; then
		return 0
	fi
	if [ "$parse_rc" -ne 0 ]; then
		return 1
	fi

	if ! lightspeed_load_inventory; then
		return 1
	fi

	printf '⚡ LightSpeed Proxy — Proxy Deployment\n'
	printf '━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n'

	if ! lightspeed_build_proxy; then
		return 1
	fi

	if [ "$BUILD_ONLY" = true ]; then
		printf '\n%bBuild complete. Skipping deployment (--build-only).%b\n' "$GREEN" "$NC"
		return 0
	fi

	# ── Step 2: Release version ──────────────────────────────
	# The label must equal the binary's own version: the in-place handoff
	# checks that the requested version matches the new binary's compiled
	# version, and a mismatch makes the handoff refuse it. Deriving the label
	# from the binary also lets a redeploy of the same version no-op instead
	# of reinstalling.
	BINARY_VERSION="$("$BINARY_PATH" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.-]*' | head -1 || true)"
	if [ -n "${LIGHTSPEED_VERSION:-}" ] && [ "$LIGHTSPEED_VERSION" != "$BINARY_VERSION" ]; then
		log_err "❌ LIGHTSPEED_VERSION '$LIGHTSPEED_VERSION' must equal the binary version '$BINARY_VERSION' for in-place handoff"
		return 1
	fi
	VERSION="${LIGHTSPEED_VERSION:-$BINARY_VERSION}"
	if [ -z "$VERSION" ]; then
		log_err "❌ Could not determine the release version from $BINARY_PATH"
		return 1
	fi

	case "$VERSION" in
	*[!A-Za-z0-9._-]* | . | ..)
		log_err "❌ Invalid LIGHTSPEED_VERSION: '$VERSION' (allowed: A-Za-z0-9._-)"
		return 1
		;;
	esac

	if [ ! -f "$RELAY_INSTALLER" ]; then
		log_err "❌ Relay installer not found: $RELAY_INSTALLER"
		return 1
	fi

	printf '  Release version: %s\n' "$VERSION"

	# ── Step 3: Verify SSH ───────────────────────────────────
	printf '\n%b[3/4] Verifying SSH access...%b\n' "$CYAN" "$NC"
	if [ ! -f "$SSH_KEY" ]; then
		log_err "❌ SSH key not found: $SSH_KEY"
		log_err "  Set DEPLOY_SSH_KEY or place key at ~/.ssh/lightspeed_deploy"
		return 1
	fi
	chmod 600 "$SSH_KEY" 2>/dev/null || true

	# ── Step 4: Deploy ───────────────────────────────────────
	printf '\n%b[4/4] Deploying to nodes...%b\n' "$CYAN" "$NC"
	printf '━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n'

	local rc=0
	if [ -n "$TARGET_NODE" ]; then
		lightspeed_deploy_single "$TARGET_NODE" || rc=$?
	elif [ "$CANARY_ENABLED" = "0" ]; then
		printf '%bCanary disabled — legacy fleet pass (%s node(s))%b\n' "$YELLOW" "${#NODE_NAMES[@]}" "$NC"
		lightspeed_deploy_fleet_legacy || rc=$?
	else
		# The canary gate needs a real client to register against the relay.
		if ! lightspeed_resolve_probe; then
			log_err "❌ Cannot run the canary control-plane probe. Refusing to deploy."
			log_err "  Build the client (--features quic) or set LIGHTSPEED_PROBE_BIN."
			return 1
		fi
		lightspeed_rollout_canary_then_rest || rc=$?
	fi

	printf '\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n'
	if [ "$rc" -ne 0 ]; then
		log_err "${RED}⚠️  Rollout did not complete.${NC}"
		if [ -n "$ROLLOUT_EVIDENCE" ]; then
			evidence "$ROLLOUT_EVIDENCE"
		fi
		return 1
	fi
	printf '%b✅ All nodes updated%b\n' "$GREEN" "$NC"
	return 0
}

# ── Entry point ──────────────────────────────────────────────
# Execute only when run directly; sourcing defines functions for the tests.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
	lightspeed_deploy_main "$@"
	exit $?
fi
