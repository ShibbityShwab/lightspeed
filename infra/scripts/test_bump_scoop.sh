#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for bump-scoop.sh
#
# The Scoop bucket (ShibbityShwab/scoop-bucket) is what `scoop install
# lightspeed` reads, and it sat at 1.6.3 for eleven releases because the
# bucket was the only copy of the manifest and nothing in this repo
# updated it. This suite covers the in-repo source of truth and the script
# that bumps it, so the same rot cannot recur silently.
#
# Asserted behaviours:
#   (a) a complete release rewrites version, url and hash together
#   (b) the url points at the tag the hash was taken from
#   (c) a release missing the Windows asset aborts and mutates nothing
#   (d) a manifest that is not valid JSON is a hard error
#   (e) a manifest without a version, or without architecture.64bit, aborts
#   (f) the manifest keeps autoupdate/checkver so `scoop update` still works
#
# The `gh` binary is stubbed on PATH and honours --jq, so the script runs
# exactly as in CI, with no network and no GitHub credentials.
#
# Usage: bash infra/scripts/test_bump_scoop.sh
# Exits 0 and prints "bump-scoop: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUMP="$SCRIPT_DIR/bump-scoop.sh"
SHIPPED="$REPO_ROOT/dist/scoop/lightspeed.json"

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

assert_eq() {
	if [ "$1" = "$2" ]; then
		pass
	else
		fail "$3" "got:      $1" "expected: $2"
	fi
}

if [ ! -f "$BUMP" ]; then
	printf 'bump-scoop: FAIL - not found: %s\n' "$BUMP" >&2
	exit 1
fi
if [ ! -f "$SHIPPED" ]; then
	printf 'bump-scoop: FAIL - shipped manifest not found: %s\n' "$SHIPPED" >&2
	exit 1
fi

make_stub() {
	local dir="$1" payload="$2"
	mkdir -p "$dir"
	cat > "$dir/gh" <<STUB
#!/usr/bin/env bash
JQEXPR=""
args=("\$@")
for ((i=0;i<\${#args[@]};i++)); do
  [ "\${args[\$i]}" = "--jq" ] && JQEXPR="\${args[\$((i+1))]}"
done
PAYLOAD='$payload'
if [ -n "\$JQEXPR" ]; then printf '%s' "\$PAYLOAD" | jq -r "\$JQEXPR"; else printf '%s' "\$PAYLOAD"; fi
STUB
	chmod +x "$dir/gh"
}

# run_bump <manifest-path> <stub-dir> <version>  -> sets RC and OUT
run_bump() {
	OUT="$(PATH="$2:$PATH" LIGHTSPEED_SCOOP_MANIFEST="$1" \
		bash "$BUMP" "$3" 2>&1)"
	RC=$?
}

ALL_ASSETS='{"assets":[{"name":"lightspeed-client-x86_64-pc-windows-msvc.zip","digest":"sha256:aaaa1111"}]}'
NO_ASSETS='{"assets":[{"name":"lightspeed-client-x86_64-apple-darwin.tar.xz","digest":"sha256:bbbb2222"}]}'

# ══════════════════════════════════════════════════════════════
# (f) the shipped manifest is valid and keeps its autoupdate wiring
# ══════════════════════════════════════════════════════════════
if jq -e . "$SHIPPED" >/dev/null 2>&1; then
	pass
else
	fail "(f) the shipped manifest is valid JSON"
fi
assert_eq "$(jq -r '.checkver.github // ""' "$SHIPPED")" \
	"https://github.com/ShibbityShwab/lightspeed" "(f) checkver still points at the repo"
assert_eq "$(jq -r '.autoupdate.architecture."64bit".url // ""' "$SHIPPED")" \
	'https://github.com/ShibbityShwab/lightspeed/releases/download/v$version/lightspeed-client-x86_64-pc-windows-msvc.zip' \
	"(f) autoupdate url still carries \$version"
assert_eq "$(jq -r '.bin // ""' "$SHIPPED")" "lightspeed.exe" "(f) the bin entry is unchanged"

# ══════════════════════════════════════════════════════════════
# (a)-(b) a complete release rewrites version, url and hash together
# ══════════════════════════════════════════════════════════════
F1="$TMP/f1.json"
cp "$SHIPPED" "$F1"
make_stub "$TMP/stub-ok" "$ALL_ASSETS"
run_bump "$F1" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "0" "(a) a complete release exits 0"
assert_eq "$(jq -r '.version' "$F1")" "9.9.9" "(a) version rewritten"
assert_eq "$(jq -r '.architecture."64bit".hash' "$F1")" "aaaa1111" "(a) hash set to the released digest"
assert_eq "$(jq -r '.architecture."64bit".url' "$F1")" \
	"https://github.com/ShibbityShwab/lightspeed/releases/download/v9.9.9/lightspeed-client-x86_64-pc-windows-msvc.zip" \
	"(b) url repointed at the tag the hash came from"

# ══════════════════════════════════════════════════════════════
# (b2) re-running the same version must leave the manifest byte-identical
# ══════════════════════════════════════════════════════════════
# The release job commits only when `git diff -- dist/scoop` is non-empty. A
# rewrite through jq re-indents and normalises the hand-written layout, so the
# file would churn on EVERY tag and the "already up to date" path would never
# fire. Found by running the script against the manifest it had just written.
SAME_BEFORE="$(cat "$F1")"
run_bump "$F1" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "0" "(b2) an already-current manifest exits 0"
assert_eq "$(cat "$F1")" "$SAME_BEFORE" "(b2) an already-current manifest is left byte-identical"

# ══════════════════════════════════════════════════════════════
# (c) a release without the Windows asset aborts and mutates nothing
# ══════════════════════════════════════════════════════════════
F2="$TMP/f2.json"
cp "$SHIPPED" "$F2"
BEFORE="$(cat "$F2")"
make_stub "$TMP/stub-noasset" "$NO_ASSETS"
run_bump "$F2" "$TMP/stub-noasset" "9.9.9"
assert_eq "$RC" "1" "(c) missing windows asset is a hard failure"
assert_eq "$(cat "$F2")" "$BEFORE" "(c) manifest left untouched on failure"

# ══════════════════════════════════════════════════════════════
# (d) an unparseable manifest is a hard error
# ══════════════════════════════════════════════════════════════
F3="$TMP/f3.json"
printf 'not json at all {{' > "$F3"
run_bump "$F3" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "1" "(d) invalid JSON manifest exits non-zero"
assert_eq "$(cat "$F3")" "not json at all {{" "(d) invalid manifest left untouched"

# ══════════════════════════════════════════════════════════════
# (e) a manifest missing version / architecture.64bit aborts
# ══════════════════════════════════════════════════════════════
F4="$TMP/f4.json"
printf '{"bin":"lightspeed.exe"}' > "$F4"
run_bump "$F4" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "1" "(e) manifest without a version exits non-zero"

F5="$TMP/f5.json"
printf '{"version":"1.0.0","bin":"lightspeed.exe"}' > "$F5"
run_bump "$F5" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "1" "(e) manifest without architecture.64bit exits non-zero"
assert_eq "$(jq -r '.version' "$F5")" "1.0.0" "(e) manifest left untouched"

# A missing manifest path is also an error, not a silent success.
run_bump "$TMP/does-not-exist.json" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "1" "(e) missing manifest exits non-zero"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'bump-scoop: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'bump-scoop: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
