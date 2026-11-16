#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Flip the control-plane ALPN policy on ONE relay
#
# Stage 3/4 of the ALPN rollout (docs/quic-alpn-rollout.md §4): set
# [network] control_alpn = "off" | "required" in one relay's proxy.toml
# over SSH, verify the edited config with the deployed binary's
# `--check`, restart the service, and health-gate the result. The
# rollout order is client-first; a relay may only be flipped after the
# client release that offers lightspeed-control/1 is in the field, and
# each relay needs its own owner approval.
#
# Fail closed. The relay's /health is read before anything is changed:
# a build older than the stage-2 relay release has no control_alpn field
# there, so the key would be inert and the flip unprovable. Such a relay
# is refused with the instruction to deploy the stage-2 build first.
#
# The config edit is atomic: the new file is written to a temp name in
# the same directory, validated, and renamed over the config; a
# timestamped backup is kept next to it (proxy.toml.bak.YYYYMMDDTHHMMSSZ).
# On any failure after the backup exists -- a failed --check, a failed
# restart, or a bounded /health poll that never reports the requested
# mode -- the backup is restored, the service is restarted, and the
# script exits non-zero with the evidence.
#
# Idempotent: when /health already reports the requested mode the script
# reports and exits 0 without touching the config or restarting.
#
# Usage:
#   ./relay-control-alpn.sh --node relay-lax --mode required --dry-run
#   ./relay-control-alpn.sh --node relay-lax --mode required
#   ./relay-control-alpn.sh --node eu-central --mode off
#   ./relay-control-alpn.sh --node 203.0.113.7 --mode required --ip 203.0.113.7
#
#   --node    node id, region, or IP; resolved from the signed registry
#             (web/registry.json) unless --ip overrides the address
#   --mode    off | required
#   --ip      override the resolved relay address
#   --config  relay config path (default /etc/lightspeed/proxy.toml)
#   --dry-run report the planned change; no file, service, or config change
#
# Environment:
#   LIGHTSPEED_NODES / LIGHTSPEED_REGISTRY_PATH  relay inventory (lib-nodes.sh)
#   DEPLOY_SSH_KEY       default ~/.ssh/lightspeed_deploy
#   DEPLOY_SSH_USER      default root
#   LIGHTSPEED_SSH / LIGHTSPEED_CURL / LIGHTSPEED_JQ  command seams
#   LIGHTSPEED_CONFIG_PATH           default /etc/lightspeed/proxy.toml
#   LIGHTSPEED_PROXY_BINARY          default /opt/lightspeed/current/lightspeed-proxy
#   LIGHTSPEED_REMOTE_SYSTEMCTL      default systemctl (remote restart seam)
#   LIGHTSPEED_HEALTH_URL            default http://<ip>:8080/health
#   LIGHTSPEED_ALPN_HEALTH_TIMEOUT   seconds to wait for /health (default 30)
#   LIGHTSPEED_ALPN_HEALTH_INTERVAL  seconds between polls (default 1)
#
# Sourcing this file defines the functions and changes nothing; the entry
# point is relay_control_alpn_main. The offline self-test
# (test_relay_control_alpn.sh) drives it with ssh/curl/systemctl stubs.
#
# Exit codes: 0 when the requested mode is in effect (including "it
# already was"); non-zero on every refusal and every failure, after
# restoring the previous config and service when a change had started.
# ──────────────────────────────────────────────────────────────

# NOTE: no `set -e` here. The script is sourceable, and enabling errexit in
# the caller's shell would be a side effect. relay_control_alpn_main turns it
# on for the real run.
set -uo pipefail

# ── Configuration ────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

SSH="${LIGHTSPEED_SSH:-ssh}"
CURL="${LIGHTSPEED_CURL:-curl}"
JQ="${LIGHTSPEED_JQ:-jq}"
SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/lightspeed_deploy}"
SSH_USER="${DEPLOY_SSH_USER:-root}"
# shellcheck disable=SC2034  # expanded unquoted (option list) the way deploy.sh does
SSH_OPTS="${LIGHTSPEED_SSH_OPTS:--o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new}"

CONFIG_PATH="${LIGHTSPEED_CONFIG_PATH:-/etc/lightspeed/proxy.toml}"
PROXY_BINARY="${LIGHTSPEED_PROXY_BINARY:-/opt/lightspeed/current/lightspeed-proxy}"
SERVICE_NAME="${LIGHTSPEED_SERVICE_NAME:-lightspeed-proxy}"
REMOTE_SYSTEMCTL="${LIGHTSPEED_REMOTE_SYSTEMCTL:-systemctl}"
HEALTH_PORT="${LIGHTSPEED_HEALTH_PORT:-8080}"
HEALTH_URL_OVERRIDE="${LIGHTSPEED_HEALTH_URL:-}"
HEALTH_TIMEOUT="${LIGHTSPEED_ALPN_HEALTH_TIMEOUT:-30}"
HEALTH_INTERVAL="${LIGHTSPEED_ALPN_HEALTH_INTERVAL:-1}"

# The §7.4 canary gate: any increment of this series is a pre-ALPN client
# that the flip cut off. Printed in every summary so the operator watches
# the right number (docs/quic-alpn-rollout.md §7.4).
ALPN_WATCH_METRIC='lightspeed_control_handshakes_total{outcome="rejected_no_application_protocol"}'

# Args, set by alpn_parse_args.
NODE=""
MODE=""
IP_OVERRIDE=""
DRY_RUN=false

# Resolved target, set by alpn_resolve_target.
ALPN_NODE_ID=""
ALPN_REGION=""
ALPN_IP=""
ALPN_HEALTH_URL=""

# Inventory JSON, set by alpn_load_inventory.
ALPN_INVENTORY_JSON=""

# Preflight state, set by alpn_preflight.
PREVIOUS_MODE=""
HEALTH_VERSION=""

# Last /health observation from the bounded poll (for failure evidence).
ALPN_LAST_MODE=""
# Backup path of the edit in progress, set after the remote edit.
ALPN_BACKUP=""

# ── Logging helpers ──────────────────────────────────────────
log_info() { printf '%s\n' "$*"; }
log_err() { printf '%s\n' "$*" >&2; }
evidence() { printf '  evidence: %s\n' "$*" >&2; }

# ── Usage ────────────────────────────────────────────────────
usage() {
	awk 'NR == 1 { next } /^# ── Configuration/ { exit } { sub(/^# ?/, ""); print }' \
		"${BASH_SOURCE[0]}"
}

# ── Args ─────────────────────────────────────────────────────
alpn_parse_args() {
	NODE=""
	MODE=""
	IP_OVERRIDE=""
	DRY_RUN=false

	while [ $# -gt 0 ]; do
		case "$1" in
		--node)
			[ $# -ge 2 ] || {
				log_err "  --node needs a node id, region, or IP"
				return 1
			}
			NODE="$2"
			shift
			;;
		--node=*) NODE="${1#*=}" ;;
		--mode)
			[ $# -ge 2 ] || {
				log_err "  --mode needs off or required"
				return 1
			}
			MODE="$2"
			shift
			;;
		--mode=*) MODE="${1#*=}" ;;
		--ip)
			[ $# -ge 2 ] || {
				log_err "  --ip needs an address"
				return 1
			}
			IP_OVERRIDE="$2"
			shift
			;;
		--ip=*) IP_OVERRIDE="${1#*=}" ;;
		--config)
			[ $# -ge 2 ] || {
				log_err "  --config needs a path"
				return 1
			}
			CONFIG_PATH="$2"
			shift
			;;
		--config=*) CONFIG_PATH="${1#*=}" ;;
		--dry-run) DRY_RUN=true ;;
		-h | --help)
			usage
			return 2
			;;
		-*)
			log_err "  Unknown option: $1"
			return 1
			;;
		*)
			log_err "  Unexpected argument: $1"
			return 1
			;;
		esac
		shift
	done

	if [ -z "$NODE" ]; then
		log_err "  --node is required (node id, region, or IP)"
		return 1
	fi
	case "$MODE" in
	off | required) ;;
	'')
		log_err "  --mode is required (off or required)"
		return 1
		;;
	*)
		log_err "  Invalid --mode '$MODE': allowed values are off and required"
		return 1
		;;
	esac
	if [ -z "$CONFIG_PATH" ]; then
		log_err "  --config must not be empty"
		return 1
	fi
	return 0
}

# ── Inventory / target resolution ────────────────────────────
# Populate ALPN_INVENTORY_JSON from lib-nodes.sh (signed registry by default,
# LIGHTSPEED_NODES override for self-hosters). Non-zero when no inventory is
# available; --ip still allows an explicit single relay in that case.
alpn_load_inventory() {
	local nodes_json count
	if ! nodes_json="$(lightspeed_resolve_nodes 2>/dev/null)"; then
		return 1
	fi
	count="$(printf '%s' "$nodes_json" | "$JQ" 'length' 2>/dev/null)" || return 1
	case "$count" in
	'' | *[!0-9]*) return 1 ;;
	esac
	[ "$count" -gt 0 ] || return 1
	ALPN_INVENTORY_JSON="$nodes_json"
	return 0
}

# alpn_resolve_target <node> <ip-override>
# Sets ALPN_NODE_ID, ALPN_REGION, ALPN_IP, ALPN_HEALTH_URL. Fails closed on an
# unknown node when no --ip was given.
alpn_resolve_target() {
	local want="$1" ip_override="$2" row available
	ALPN_NODE_ID="$want"
	ALPN_REGION=""
	ALPN_IP=""
	ALPN_HEALTH_URL=""

	if alpn_load_inventory; then
		row="$(printf '%s' "$ALPN_INVENTORY_JSON" | "$JQ" -r --arg want "$want" '
			first(.[]
			| select(.node_id == $want or .region == $want or .ip == $want)
			| [.node_id, .region, .ip, .health_url] | @tsv) // empty' 2>/dev/null)" || true
		if [ -n "$row" ]; then
			IFS=$'\t' read -r ALPN_NODE_ID ALPN_REGION ALPN_IP ALPN_HEALTH_URL <<< "$row"
		fi
	fi

	if [ -n "$ip_override" ]; then
		ALPN_IP="$ip_override"
	fi
	if [ -z "$ALPN_IP" ]; then
		log_err "❌ Unknown relay '$want' (not a node id, region, or IP in the signed registry)."
		if alpn_load_inventory; then
			available="$(printf '%s' "$ALPN_INVENTORY_JSON" | "$JQ" -r \
				'[.[] | .node_id + " (" + .region + ")"] | join(", ")' 2>/dev/null || true)"
			if [ -n "$available" ]; then
				log_err "   Available: $available"
			fi
		else
			log_err "   No inventory available: set LIGHTSPEED_NODES / LIGHTSPEED_REGISTRY_PATH, or pass --ip."
		fi
		return 1
	fi

	if [ -n "$HEALTH_URL_OVERRIDE" ]; then
		ALPN_HEALTH_URL="$HEALTH_URL_OVERRIDE"
	elif [ -n "$ip_override" ] || [ -z "$ALPN_HEALTH_URL" ]; then
		# An --ip override moves the relay endpoint away from the inventory
		# entry, so the inventory's health_url no longer names this relay.
		ALPN_HEALTH_URL="http://${ALPN_IP}:${HEALTH_PORT}/health"
	fi
	return 0
}

# ── /health helpers ──────────────────────────────────────────
alpn_fetch_health() {
	local json
	if ! json="$("$CURL" -sf --max-time 5 "$ALPN_HEALTH_URL" 2>/dev/null)"; then
		return 1
	fi
	printf '%s' "$json"
}

# Preflight: the relay must already run a stage-2 build (control_alpn in
# /health) or the flip cannot be proven and is refused before any change.
alpn_preflight() {
	local json
	if ! json="$(alpn_fetch_health)"; then
		log_err "❌ Cannot read $ALPN_HEALTH_URL — refusing to change anything."
		log_err "   Check the relay is up and its health port is reachable from here."
		return 1
	fi
	PREVIOUS_MODE="$(printf '%s' "$json" | "$JQ" -r '.control_alpn // empty' 2>/dev/null || true)"
	HEALTH_VERSION="$(printf '%s' "$json" | "$JQ" -r '.version // empty' 2>/dev/null || true)"

	case "$PREVIOUS_MODE" in
	off | required) return 0 ;;
	'')
		log_err "❌ Refusing: $ALPN_HEALTH_URL has no control_alpn field."
		log_err "   This relay is not running the stage-2 relay build, so the config key"
		log_err "   would be inert and /health could not prove the flip."
		log_err "   Deploy the stage-2 build first, then re-run this command:"
		log_err "     ./infra/scripts/deploy.sh $ALPN_IP"
		if [ -n "$HEALTH_VERSION" ]; then
			log_err "   (reported version: $HEALTH_VERSION)"
		fi
		return 1
		;;
	*)
		log_err "❌ Refusing: $ALPN_HEALTH_URL reports unknown control_alpn='$PREVIOUS_MODE'."
		log_err "   Expected off or required; this relay build may be newer than this tool."
		return 1
		;;
	esac
}

# Poll /health until it reports <mode>, bounded by HEALTH_TIMEOUT.
alpn_wait_for_mode() {
	local want="$1" deadline json got
	deadline=$(($(date +%s) + HEALTH_TIMEOUT))
	while :; do
		if json="$(alpn_fetch_health 2>/dev/null)"; then
			got="$(printf '%s' "$json" | "$JQ" -r '.control_alpn // empty' 2>/dev/null || true)"
			if [ "$got" = "$want" ]; then
				ALPN_LAST_MODE="$got"
				return 0
			fi
			ALPN_LAST_MODE="${got:-missing} (version ${HEALTH_VERSION:-unknown})"
		else
			ALPN_LAST_MODE="unreachable"
		fi
		if [ "$(date +%s)" -ge "$deadline" ]; then
			return 1
		fi
		sleep "$HEALTH_INTERVAL"
	done
}

# ── Remote plumbing ──────────────────────────────────────────
# Single-quote a value for a remote command line.
alpn_q() { printf '%q' "$1"; }

# Extract one KEY=value line from captured remote output without a pipeline
# (a `| head` here would risk pipefail/SIGPIPE in the caller).
alpn_extract() {
	local text="$1" key="$2" value
	value="${text#*"${key}="}"
	if [ "$value" = "$text" ]; then
		return 0
	fi
	value="${value%%$'\n'*}"
	printf '%s' "$value"
}

# alpn_remote <env-assignment-prefix> <heredoc-file>
# Not used directly; each remote step builds its own ssh call so the exit code
# of the remote shell is the exit code of the step.

# Edit the config on the relay. Prints two machine-readable lines
# (ALPN_PREVIOUS / ALPN_BACKUP) and returns the remote exit code:
#   10 no config      11 no binary       14 backup failed
#   12 TOML edit invalid                 13 deployed binary --check failed
alpn_edit_remote() {
	local remote_cmd
	remote_cmd="MODE=$(alpn_q "$MODE") CONFIG=$(alpn_q "$CONFIG_PATH") BIN=$(alpn_q "$PROXY_BINARY") bash -s"
	# shellcheck disable=SC2086
	"$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ALPN_IP}" "$remote_cmd" <<'ALPN_REMOTE_EDIT'
set -euo pipefail

[ -f "$CONFIG" ] || { printf 'ALPN_ERR=no_config\n' >&2; exit 10; }
[ -x "$BIN" ] || { printf 'ALPN_ERR=no_binary\n' >&2; exit 11; }

previous="$(awk '
	/^[[:space:]]*\[/ { in_network = ($0 ~ /^[[:space:]]*\[[[:space:]]*network[[:space:]]*\][[:space:]]*(#.*)?$/) }
	in_network && /^[[:space:]]*control_alpn[[:space:]]*=/ {
		value = $0
		sub(/^[^=]*=[[:space:]]*/, "", value)
		sub(/[[:space:]]+#.*$/, "", value)
		gsub(/["\047]/, "", value)
		sub(/[[:space:]]+$/, "", value)
		print value
		exit
	}
' "$CONFIG")"

backup="${CONFIG}.bak.$(date -u +%Y%m%dT%H%M%SZ)"
cp -p "$CONFIG" "$backup" || { printf 'ALPN_ERR=backup\n' >&2; exit 14; }
printf 'ALPN_PREVIOUS=%s\n' "$previous"
printf 'ALPN_BACKUP=%s\n' "$backup"

tmp="$(mktemp "${CONFIG}.alpn-tmp.XXXXXX")"
trap 'rm -f "$tmp"' EXIT

# Set exactly one control_alpn under [network]: replace the first active key
# if present, otherwise append it at the end of the [network] section, and
# append a [network] section when the file has none.
awk -v mode="$MODE" '
	function is_header(line) { return line ~ /^[[:space:]]*\[/ }
	BEGIN { in_network = 0; seen_network = 0; wrote = 0 }
	{
		line = $0
		if (is_header(line)) {
			if (in_network && !wrote) { print "control_alpn = \"" mode "\""; wrote = 1 }
			in_network = (line ~ /^[[:space:]]*\[[[:space:]]*network[[:space:]]*\][[:space:]]*(#.*)?$/)
			if (in_network) seen_network = 1
			print line
			next
		}
		if (in_network && line ~ /^[[:space:]]*control_alpn[[:space:]]*=/) {
			if (!wrote) { print "control_alpn = \"" mode "\""; wrote = 1 }
			next
		}
		print line
		next
	}
	END {
		if (in_network && !wrote) print "control_alpn = \"" mode "\""
		if (!seen_network) {
			print ""
			print "[network]"
			print "control_alpn = \"" mode "\""
		}
	}
' "$CONFIG" > "$tmp"

# The edited file must carry exactly one active key, in [network], with the
# requested value. Anything else is a hard failure before activation.
awk -v mode="$MODE" '
	/^[[:space:]]*\[/ { in_network = ($0 ~ /^[[:space:]]*\[[[:space:]]*network[[:space:]]*\][[:space:]]*(#.*)?$/) }
	in_network && /^[[:space:]]*control_alpn[[:space:]]*=/ {
		count++
		value = $0
		sub(/^[^=]*=[[:space:]]*/, "", value)
		sub(/[[:space:]]+#.*$/, "", value)
		gsub(/["\047]/, "", value)
		sub(/[[:space:]]+$/, "", value)
	}
	END {
		if (count != 1) exit 1
		if (value != mode) exit 1
	}
' "$tmp" || { printf 'ALPN_ERR=validate\n' >&2; exit 12; }

# Preserve the config's mode: mktemp creates 0600, but the proxy may run as a
# DynamicUser and must still be able to read the config after the rename.
chmod --reference="$CONFIG" "$tmp" 2>/dev/null ||
	chmod "$(stat -c %a "$CONFIG")" "$tmp" ||
	{ printf 'ALPN_ERR=chmod\n' >&2; exit 15; }

mv -f "$tmp" "$CONFIG"
trap - EXIT

# Verify before restart: the deployed binary must accept the edited config.
"$BIN" --check --config "$CONFIG" || { printf 'ALPN_ERR=check\n' >&2; exit 13; }
exit 0
ALPN_REMOTE_EDIT
}

# Restore the config from <backup> on the relay (exit 20 when it is missing).
alpn_restore_remote() {
	local backup="$1" remote_cmd
	remote_cmd="CONFIG=$(alpn_q "$CONFIG_PATH") BACKUP=$(alpn_q "$backup") bash -s"
	# shellcheck disable=SC2086
	"$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ALPN_IP}" "$remote_cmd" <<'ALPN_REMOTE_RESTORE'
set -euo pipefail
[ -f "$BACKUP" ] || { printf 'ALPN_ERR=no_backup\n' >&2; exit 20; }
cp -p "$BACKUP" "$CONFIG"
exit 0
ALPN_REMOTE_RESTORE
}

# Restart the proxy service on the relay.
alpn_restart_remote() {
	local remote_cmd
	remote_cmd="SERVICE=$(alpn_q "$SERVICE_NAME") SYSTEMCTL=$(alpn_q "$REMOTE_SYSTEMCTL") bash -s"
	# shellcheck disable=SC2086
	"$SSH" $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ALPN_IP}" "$remote_cmd" <<'ALPN_REMOTE_RESTART'
set -euo pipefail
"$SYSTEMCTL" restart "$SERVICE"
exit 0
ALPN_REMOTE_RESTART
}

# ── Rollback after a change was activated ────────────────────
# Restore the backup, restart, and prove the previous mode is back. Best
# effort: every failure is reported with evidence, but the caller exits
# non-zero either way.
alpn_rollback_after_restart() {
	if [ -z "$ALPN_BACKUP" ]; then
		log_err "  ❌ No backup recorded — cannot restore the config by script."
		log_err "     Check $CONFIG_PATH on $ALPN_IP by hand."
		evidence "health=$ALPN_HEALTH_URL config=$CONFIG_PATH"
		return 1
	fi
	if ! alpn_restore_remote "$ALPN_BACKUP"; then
		log_err "  ❌ Restore from $ALPN_BACKUP FAILED — the relay may still serve control_alpn=$MODE."
		log_err "     Restore $CONFIG_PATH from that backup by hand and restart $SERVICE_NAME."
		evidence "health=$ALPN_HEALTH_URL backup=$ALPN_BACKUP"
		return 1
	fi
	log_info "  ↩️  restored $CONFIG_PATH from $ALPN_BACKUP"
	if ! alpn_restart_remote; then
		log_err "  ❌ Rollback restart failed; $SERVICE_NAME may be down on $ALPN_IP."
		evidence "health=$ALPN_HEALTH_URL backup=$ALPN_BACKUP"
		return 1
	fi
	if alpn_wait_for_mode "$PREVIOUS_MODE"; then
		log_info "  ✅ service is back on control_alpn=$PREVIOUS_MODE"
		return 0
	fi
	log_err "  ❌ Rollback restart left /health at '${ALPN_LAST_MODE:-unknown}' (wanted control_alpn=$PREVIOUS_MODE)."
	evidence "health=$ALPN_HEALTH_URL config=$CONFIG_PATH backup=$ALPN_BACKUP"
	return 1
}

# ── Summary ──────────────────────────────────────────────────
alpn_print_summary() {
	local status="${1:-}"
	printf '\n  node:         %s (%s)\n' "$ALPN_NODE_ID" "$ALPN_IP"
	printf '  config:       %s\n' "$CONFIG_PATH"
	printf '  mode:         %s -> %s\n' "${PREVIOUS_MODE:-unknown}" "$MODE"
	printf '  health:       %s\n' "$ALPN_HEALTH_URL"
	printf '  watch metric: %s\n' "$ALPN_WATCH_METRIC"
	if [ -n "$status" ]; then
		printf '  result:       %s\n' "$status"
	fi
	if [ -n "$ALPN_BACKUP" ]; then
		printf '  backup:       %s\n' "$ALPN_BACKUP"
	fi
	printf '  canary gate:  watch the metric above for a full window with at least 200 accepted\n'
	printf '                control handshakes; PASS = 0, FAIL = any increment (plan §7.4).\n'
}

# ── Main ─────────────────────────────────────────────────────
relay_control_alpn_main() {
	set -euo pipefail

	local parse_rc=0
	alpn_parse_args "$@" || parse_rc=$?
	if [ "$parse_rc" -eq 2 ]; then
		return 0
	fi
	if [ "$parse_rc" -ne 0 ]; then
		return 1
	fi

	printf '⚡ LightSpeed relay control_alpn flip\n'
	printf '────────────────────────────────────────\n'
	printf '  requested mode: %s\n' "$MODE"

	if ! command -v "$JQ" >/dev/null 2>&1; then
		log_err "❌ jq is required but not found: $JQ"
		return 1
	fi

	if ! alpn_resolve_target "$NODE" "$IP_OVERRIDE"; then
		return 1
	fi

	# Preflight also proves the relay runs a stage-2 build and tells us the
	# mode currently in effect (idempotency).
	if ! alpn_preflight; then
		return 1
	fi

	if [ "$PREVIOUS_MODE" = "$MODE" ]; then
		alpn_print_summary "already in effect (verified from /health); no config change, no restart"
		return 0
	fi

	if [ "$DRY_RUN" = true ]; then
		alpn_print_summary "dry run: would set [network] control_alpn in $CONFIG_PATH and restart $SERVICE_NAME"
		return 0
	fi

	if [ ! -f "$SSH_KEY" ]; then
		log_err "❌ SSH key not found: $SSH_KEY"
		log_err "   Set DEPLOY_SSH_KEY or place the key at ~/.ssh/lightspeed_deploy"
		return 1
	fi

	log_info ""
	log_info "  editing $CONFIG_PATH on $ALPN_IP (control_alpn: $PREVIOUS_MODE -> $MODE) ..."
	local remote_out="" rc=0
	if remote_out="$(alpn_edit_remote 2>&1)"; then
		rc=0
	else
		rc=$?
	fi
	ALPN_BACKUP="$(alpn_extract "$remote_out" ALPN_BACKUP)"
	local config_previous
	config_previous="$(alpn_extract "$remote_out" ALPN_PREVIOUS)"

	if [ "$rc" -ne 0 ]; then
		log_err "❌ Config edit / pre-restart --check failed on $ALPN_IP (remote exit $rc)."
		if [ -n "$remote_out" ]; then
			printf '%s\n' "$remote_out" | sed 's/^/     /' >&2
		fi
		evidence "health=$ALPN_HEALTH_URL config=$CONFIG_PATH binary=$PROXY_BINARY"
		if [ -n "$ALPN_BACKUP" ]; then
			if alpn_restore_remote "$ALPN_BACKUP"; then
				log_info "  ↩️  restored $CONFIG_PATH from $ALPN_BACKUP (service not restarted)"
			else
				log_err "  ❌ Restore from $ALPN_BACKUP FAILED — manual intervention required."
			fi
		fi
		return 1
	fi
	log_info "  ✅ config updated (previous config mode: ${config_previous:-none}); backup: $ALPN_BACKUP"

	if ! alpn_restart_remote; then
		log_err "❌ Restart of $SERVICE_NAME failed on $ALPN_IP — rolling back."
		alpn_rollback_after_restart || true
		return 1
	fi

	if ! alpn_wait_for_mode "$MODE"; then
		log_err "❌ /health did not report control_alpn='$MODE' within ${HEALTH_TIMEOUT}s (last: ${ALPN_LAST_MODE:-unknown})."
		log_err "   Rolling back to control_alpn=$PREVIOUS_MODE."
		alpn_rollback_after_restart || true
		return 1
	fi

	alpn_print_summary "flipped; /health reports control_alpn=$MODE"
	return 0
}

# ── Entry point ──────────────────────────────────────────────
# Execute only when run directly; sourcing defines functions for the tests.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
	relay_control_alpn_main "$@"
	exit $?
fi
