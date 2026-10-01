#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for bump-chocolatey.sh and bump-homebrew.sh
#
# These two scripts run automatically in release.yml on every tag and
# rewrite the version plus every sha256 in the SHIPPED package manifests.
# Nothing else verifies that, so a silent sed miss would publish a
# Chocolatey package or Homebrew formula whose checksum does not match
# its artifact - the failure users see is "hash mismatch" at install
# time, long after the release is out.
#
# Asserted behaviours:
#   bump-chocolatey.sh
#     (a) rewrites <version> in the nuspec
#     (b) rewrites the download URL to the new tag
#     (c) rewrites checksum64 to the released asset's digest
#     (d) a release missing the Windows zip aborts AND leaves files untouched
#     (e) a missing nuspec/install script is a hard error
#   bump-homebrew.sh
#     (f) rewrites the formula version and repoints every download URL
#     (g) writes each of the four platform digests next to its own url
#     (h) a release with the wrong number of client tarballs aborts
#     (i) a release missing one expected tarball aborts
#
# The `gh` binary is stubbed on PATH and honours --jq, so the scripts run
# exactly as they do in CI, with no network and no GitHub credentials.
#
# Usage: bash infra/scripts/test_bump_packages.sh
# Exits 0 and prints "bump-packages: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CHOCO="$SCRIPT_DIR/bump-chocolatey.sh"
BREW="$SCRIPT_DIR/bump-homebrew.sh"

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
	# assert_eq <actual> <expected> <message>
	if [ "$1" = "$2" ]; then
		pass
	else
		fail "$3" "got:      $1" "expected: $2"
	fi
}

assert_contains() {
	# assert_contains <haystack> <needle> <message>
	case "$1" in
	*"$2"*) pass ;;
	*) fail "$3" "missing: $2" ;;
	esac
}

if [ ! -f "$CHOCO" ]; then
	printf 'bump-packages: FAIL - not found: %s\n' "$CHOCO" >&2
	exit 1
fi
if [ ! -f "$BREW" ]; then
	printf 'bump-packages: FAIL - not found: %s\n' "$BREW" >&2
	exit 1
fi

# ── gh stub: honours --jq so callers see what GitHub would return ──
make_stub() {
	# make_stub <dir> <payload-json>
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

# A complete release: all five assets the two scripts care about.
ALL_ASSETS='{"assets":[
{"name":"lightspeed-client-x86_64-pc-windows-msvc.zip","digest":"sha256:aaaa1111"},
{"name":"lightspeed-client-x86_64-apple-darwin.tar.xz","digest":"sha256:bbbb2222"},
{"name":"lightspeed-client-aarch64-apple-darwin.tar.xz","digest":"sha256:cccc3333"},
{"name":"lightspeed-client-x86_64-unknown-linux-gnu.tar.xz","digest":"sha256:dddd4444"},
{"name":"lightspeed-client-aarch64-unknown-linux-gnu.tar.xz","digest":"sha256:eeee5555"}]}'

# Fixture tree: the real manifests, copied so the test never touches them.
make_fixture() {
	# make_fixture <dir>
	local dir="$1"
	mkdir -p "$dir/dist/chocolatey/tools" "$dir/Formula"
	cp "$REPO_ROOT/dist/chocolatey/lightspeed.nuspec" "$dir/dist/chocolatey/"
	cp "$REPO_ROOT/dist/chocolatey/tools/chocolateyInstall.ps1" "$dir/dist/chocolatey/tools/"
	cp "$REPO_ROOT/Formula/lightspeed.rb" "$dir/Formula/"
}

# run_choco <fixture-dir> <stub-dir> <version>  -> sets RC and OUT
run_choco() {
	OUT="$(PATH="$2:$PATH" LIGHTSPEED_CHOCOLATEY_DIR="$1/dist/chocolatey" \
		bash "$CHOCO" "$3" 2>&1)"
	RC=$?
}

run_brew() {
	OUT="$(PATH="$2:$PATH" LIGHTSPEED_HOMEBREW_FORMULA="$1/Formula/lightspeed.rb" \
		bash "$BREW" "$3" 2>&1)"
	RC=$?
}

# ══════════════════════════════════════════════════════════════
# (a)-(c) Chocolatey: a normal bump rewrites version, url, checksum
# ══════════════════════════════════════════════════════════════
F1="$TMP/f1"
make_fixture "$F1"
make_stub "$TMP/stub-ok" "$ALL_ASSETS"
run_choco "$F1" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "0" "(a) a complete release exits 0"
assert_eq "$(grep -o '<version>[^<]*' "$F1/dist/chocolatey/lightspeed.nuspec")" \
	"<version>9.9.9" "(a) nuspec version rewritten"
assert_contains "$(cat "$F1/dist/chocolatey/tools/chocolateyInstall.ps1")" \
	"releases/download/v9.9.9/" "(b) download url repointed at the new tag"
assert_contains "$(cat "$F1/dist/chocolatey/tools/chocolateyInstall.ps1")" \
	"checksum64     = 'aaaa1111'" "(c) checksum64 set to the released digest"
# The version and the checksum must agree, or the package installs a
# mismatched artifact. Assert they moved together, not just that each moved.
assert_eq "$(grep -c 'releases/download/v9.9.9/lightspeed-client-x86_64-pc-windows-msvc.zip' "$F1/dist/chocolatey/tools/chocolateyInstall.ps1")" \
	"1" "(c) url matches the asset the checksum belongs to"

# ══════════════════════════════════════════════════════════════
# (d) A release without the Windows zip must abort and mutate nothing
# ══════════════════════════════════════════════════════════════
F2="$TMP/f2"
make_fixture "$F2"
make_stub "$TMP/stub-nomatch" '{"assets":[{"name":"lightspeed-client-x86_64-apple-darwin.tar.xz","digest":"sha256:bbbb2222"}]}'
BEFORE_VERSION="$(grep -o '<version>[^<]*' "$F2/dist/chocolatey/lightspeed.nuspec")"
BEFORE_SUM="$(grep -o "checksum64 *= *'[^']*'" "$F2/dist/chocolatey/tools/chocolateyInstall.ps1")"
run_choco "$F2" "$TMP/stub-nomatch" "9.9.9"
assert_eq "$RC" "1" "(d) missing windows asset is a hard failure"
assert_eq "$(grep -o '<version>[^<]*' "$F2/dist/chocolatey/lightspeed.nuspec")" \
	"$BEFORE_VERSION" "(d) nuspec left untouched on failure"
assert_eq "$(grep -o "checksum64 *= *'[^']*'" "$F2/dist/chocolatey/tools/chocolateyInstall.ps1")" \
	"$BEFORE_SUM" "(d) install script left untouched on failure"

# ══════════════════════════════════════════════════════════════
# (e) Missing manifest files are a hard error, not a silent pass
# ══════════════════════════════════════════════════════════════
F3="$TMP/f3"
mkdir -p "$F3/dist/chocolatey" "$F3/Formula"
run_choco "$F3" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "1" "(e) missing nuspec/install script exits non-zero"

# ══════════════════════════════════════════════════════════════
# (f)-(g) Homebrew: version, every url, and each digest beside its url
# ══════════════════════════════════════════════════════════════
F4="$TMP/f4"
make_fixture "$F4"
run_brew "$F4" "$TMP/stub-ok" "9.9.9"
assert_eq "$RC" "0" "(f) a complete release exits 0"
assert_eq "$(grep -o 'version "[^"]*"' "$F4/Formula/lightspeed.rb" | head -1)" \
	'version "9.9.9"' "(f) formula version rewritten"
assert_eq "$(grep -c 'releases/download/v9.9.9/' "$F4/Formula/lightspeed.rb")" \
	"4" "(f) all four download urls repointed"
# Each url must be followed by ITS OWN digest.
assert_contains "$(cat "$F4/Formula/lightspeed.rb")" \
	'sha256 "bbbb2222"' "(g) x86_64-apple-darwin digest written"
assert_contains "$(cat "$F4/Formula/lightspeed.rb")" \
	'sha256 "cccc3333"' "(g) aarch64-apple-darwin digest written"
assert_contains "$(cat "$F4/Formula/lightspeed.rb")" \
	'sha256 "dddd4444"' "(g) x86_64-linux digest written"
assert_contains "$(cat "$F4/Formula/lightspeed.rb")" \
	'sha256 "eeee5555"' "(g) aarch64-linux digest written"
# A stale digest must be gone, not merely joined by a new one.
STALE="$(grep -c 'sha256 "bc3ce7d4e207d76fa926458058b1248719fa29c7b985d4e26b7bc025ae752b91"' "$F4/Formula/lightspeed.rb")"
assert_eq "$STALE" "0" "(g) the previous x86_64-apple digest is replaced"

# ══════════════════════════════════════════════════════════════
# (h) Wrong tarball count aborts
# ══════════════════════════════════════════════════════════════
F5="$TMP/f5"
make_fixture "$F5"
make_stub "$TMP/stub-short" '{"assets":[{"name":"lightspeed-client-x86_64-apple-darwin.tar.xz","digest":"sha256:bbbb2222"}]}'
BEFORE_BREW="$(cat "$F5/Formula/lightspeed.rb")"
run_brew "$F5" "$TMP/stub-short" "9.9.9"
assert_eq "$RC" "1" "(h) wrong tarball count is a hard failure"
assert_eq "$(cat "$F5/Formula/lightspeed.rb")" "$BEFORE_BREW" "(h) formula left untouched on failure"

# ══════════════════════════════════════════════════════════════
# (i) One missing tarball out of four still aborts
# ══════════════════════════════════════════════════════════════
F6="$TMP/f6"
make_fixture "$F6"
make_stub "$TMP/stub-partial" '{"assets":[
{"name":"lightspeed-client-x86_64-apple-darwin.tar.xz","digest":"sha256:bbbb2222"},
{"name":"lightspeed-client-aarch64-apple-darwin.tar.xz","digest":"sha256:cccc3333"},
{"name":"lightspeed-client-x86_64-unknown-linux-gnu.tar.xz","digest":"sha256:dddd4444"}]}'
BEFORE_BREW6="$(cat "$F6/Formula/lightspeed.rb")"
run_brew "$F6" "$TMP/stub-partial" "9.9.9"
assert_eq "$RC" "1" "(i) three of four tarballs is a hard failure"
assert_eq "$(cat "$F6/Formula/lightspeed.rb")" "$BEFORE_BREW6" "(i) formula left untouched"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'bump-packages: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'bump-packages: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
