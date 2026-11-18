#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for publish-aur.sh
#
# The AUR package ("lightspeed-bin") is published by cloning
# ssh://aur@aur.archlinux.org/lightspeed-bin.git and pushing PKGBUILD +
# .SRCINFO when the version changed. That path only runs on a tag push with
# an SSH key, so without this suite a broken push (wrong path, stale .SRCINFO,
# downgrade, no-op that is not a no-op) would first execute against the real
# AUR. This suite runs the real script against a file:// bare repository
# standing in for the AUR remote, so it needs no network and no credentials.
#
# Asserted behaviours:
#   (a) first push: creates the package repo, correct content and commit
#   (b) re-running the same version is a clean no-op (no new commit)
#   (c) a version bump pushes and updates PKGBUILD/.SRCINFO
#   (d) a content change without a pkgrel/pkgver bump is refused
#   (e) a pkgrel-only bump is allowed
#   (f) a downgrade is refused
#   (g) a PKGBUILD/.SRCINFO mismatch is refused before any clone/push
#   (h) a source URL that does not match pkgver is refused
#   (i) a source/sha256 count mismatch is refused
#   (j) a missing PKGBUILD is a hard failure
#   (k) --dry-run reports the plan and pushes nothing
#   (l) --version refuses a PKGBUILD that is not at that version
#   (m) the default ssh:// URL works through git's insteadOf rewrite (the
#       workflow relies on the default URL; the test keeps it hermetic)
#   (n) usage errors exit 2
#
# Usage: bash infra/scripts/test_publish_aur.sh
# Exits 0 and prints "publish-aur: all assertions passed" on success.
# Requires: bash, git (CI: ubuntu-latest).
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PUBLISH="$SCRIPT_DIR/publish-aur.sh"
SHIPPED_DIR="$REPO_ROOT/dist/aur"
# The shipped version is whatever dist/aur currently declares, so a release
# bump does not require editing this test. The bump scenarios use the next
# minor above it: newer than what the first push installs, and distinct from
# the 9.9.9 expected-version fixture.
SHIPPED_VERSION="$(sed -n 's/^pkgver=//p' "$SHIPPED_DIR/PKGBUILD" | head -1)"
NEXT_VERSION="$(printf '%s' "$SHIPPED_VERSION" | awk -F. '{printf "%s.%d.0", $1, $2 + 1}')"

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

assert_contains() {
	case "$1" in
	*"$2"*) pass ;;
	*) fail "$3" "missing: $2" ;;
	esac
}

if [ ! -f "$PUBLISH" ]; then
	printf 'publish-aur: FAIL - not found: %s\n' "$PUBLISH" >&2
	exit 1
fi
if ! command -v git >/dev/null 2>&1; then
	printf 'publish-aur: FAIL - git is not installed\n' >&2
	exit 1
fi
for f in PKGBUILD .SRCINFO; do
	if [ ! -f "$SHIPPED_DIR/$f" ]; then
		printf 'publish-aur: FAIL - shipped file not found: %s\n' "$SHIPPED_DIR/$f" >&2
		exit 1
	fi
done

# ── helpers ──────────────────────────────────────────────────
# new_bare <dir>: a bare repository that stands in for the AUR remote.
new_bare() {
	git init --bare --quiet "$1"
	# Pin the branch the publish script pushes to, whatever the host's
	# init.defaultBranch is.
	git -C "$1" symbolic-ref HEAD refs/heads/master
}

# make_source <dir> [version] [pkgrel]: a copy of the shipped dist/aur files.
make_source() {
	local dir="$1" version="${2:-}" pkgrel="${3:-}"
	mkdir -p "$dir"
	cp "$SHIPPED_DIR/PKGBUILD" "$SHIPPED_DIR/.SRCINFO" "$dir/"
	if [ -n "$version" ]; then
		sed -i "s/^pkgver=.*/pkgver=${version}/" "$dir/PKGBUILD"
		sed -i "s/^\tpkgver = .*/\tpkgver = ${version}/" "$dir/.SRCINFO"
		# The source URLs carry the release tag literally in .SRCINFO (the
		# PKGBUILD uses ${pkgver}); keep them at the version under test.
		sed -i "s|/releases/download/v[0-9][0-9.]*/|/releases/download/v${version}/|" "$dir/.SRCINFO"
	fi
	if [ -n "$pkgrel" ]; then
		sed -i "s/^pkgrel=.*/pkgrel=${pkgrel}/" "$dir/PKGBUILD"
		sed -i "s/^\tpkgrel = .*/\tpkgrel = ${pkgrel}/" "$dir/.SRCINFO"
	fi
}

# run_publish <source-dir> <repo-url> [args...]  -> sets OUT and RC
run_publish() {
	local src="$1" url="$2"
	shift 2
	OUT="$(bash "$PUBLISH" --source-dir "$src" --repo-url "$url" "$@" 2>&1)"
	RC=$?
	# A failing path is usually asserted by message; print the publisher's own
	# output for the first failing scenario so a CI-only environment difference
	# (git version, file:// protocol policy) is diagnosable from the job log.
	if [ "$RC" -ne 0 ] && [ -z "${AUR_TEST_QUIET:-}" ]; then
		printf '  --- publisher output (rc=%s) ---\n%s\n' "$RC" "$OUT" >&2
	fi
}

bare_head() {
	git -C "$1" rev-parse --verify -q refs/heads/master 2>/dev/null || echo ""
}
bare_show() {
	git -C "$1" show "refs/heads/master:$2" 2>/dev/null || true
}
bare_msg() {
	git -C "$1" log -1 --format=%s refs/heads/master 2>/dev/null || true
}
bare_author() {
	git -C "$1" log -1 --format='%an <%ae>' refs/heads/master 2>/dev/null || true
}

BARE="$TMP/aur.git"
new_bare "$BARE"
EMPTY_HEAD="$(bare_head "$BARE")"

# ══════════════════════════════════════════════════════════════
# (a) first push publishes the shipped dist/aur files, as a bot commit
# ══════════════════════════════════════════════════════════════
run_publish "$SHIPPED_DIR" "file://$BARE"
assert_eq "$RC" "0" "(a) the first push exits 0"
assert_contains "$OUT" "pushed lightspeed-bin" "(a) the push is reported"
assert_eq "$EMPTY_HEAD" "" "(a) the bare repo started empty"
HEAD1="$(bare_head "$BARE")"
if [ -n "$HEAD1" ]; then pass; else fail "(a) the first push created refs/heads/master"; fi
assert_contains "$(bare_show "$BARE" PKGBUILD)" "pkgver=$SHIPPED_VERSION" "(a) PKGBUILD landed at the shipped version"
assert_contains "$(bare_show "$BARE" .SRCINFO)" "pkgver = $SHIPPED_VERSION" "(a) .SRCINFO landed at the shipped version"
assert_eq "$(bare_msg "$BARE")" "lightspeed-bin ${SHIPPED_VERSION}-1" "(a) the commit message names the version-release"
assert_eq "$(bare_author "$BARE")" "github-actions[bot] <41898282+github-actions[bot]@users.noreply.github.com>" \
	"(a) the commit is authored as the actions bot"
bare_show "$BARE" PKGBUILD > "$TMP/remote-PKGBUILD"
if cmp -s "$TMP/remote-PKGBUILD" "$SHIPPED_DIR/PKGBUILD"; then
	pass
else
	fail "(a) the pushed PKGBUILD is byte-identical to dist/aur/PKGBUILD"
fi
bare_show "$BARE" .SRCINFO > "$TMP/remote-SRCINFO"
if cmp -s "$TMP/remote-SRCINFO" "$SHIPPED_DIR/.SRCINFO"; then
	pass
else
	fail "(a) the pushed .SRCINFO is byte-identical to dist/aur/.SRCINFO"
fi

# ══════════════════════════════════════════════════════════════
# (b) re-running the same version is a clean no-op
# ══════════════════════════════════════════════════════════════
run_publish "$SHIPPED_DIR" "file://$BARE"
assert_eq "$RC" "0" "(b) a re-run exits 0"
assert_contains "$OUT" "nothing to do" "(b) a re-run reports the no-op"
assert_eq "$(bare_head "$BARE")" "$HEAD1" "(b) a re-run made no new commit"

# ══════════════════════════════════════════════════════════════
# (c) a version bump pushes
# ══════════════════════════════════════════════════════════════
SRC_1111="$TMP/src-1.11.1"
make_source "$SRC_1111" "$NEXT_VERSION" "1"
run_publish "$SRC_1111" "file://$BARE"
assert_eq "$RC" "0" "(c) a version bump exits 0"
assert_contains "$OUT" "pushed lightspeed-bin ${NEXT_VERSION}-1" "(c) the new version is reported"
assert_eq "$(bare_msg "$BARE")" "lightspeed-bin ${NEXT_VERSION}-1" "(c) the commit message uses the new version"
assert_contains "$(bare_show "$BARE" PKGBUILD)" "pkgver=$NEXT_VERSION" "(c) PKGBUILD was updated"
assert_contains "$(bare_show "$BARE" .SRCINFO)" "pkgver = $NEXT_VERSION" "(c) .SRCINFO was updated"
HEAD2="$(bare_head "$BARE")"
if [ -n "$HEAD2" ] && [ "$HEAD2" != "$HEAD1" ]; then pass; else fail "(c) the bump created a new commit"; fi

# ══════════════════════════════════════════════════════════════
# (d) a content change without a pkgrel/pkgver bump is refused
# ══════════════════════════════════════════════════════════════
SRC_SAME="$TMP/src-same"
make_source "$SRC_SAME" "$NEXT_VERSION" "1"
printf '\n# packaging tweak without a pkgrel bump\n' >> "$SRC_SAME/PKGBUILD"
run_publish "$SRC_SAME" "file://$BARE"
assert_eq "$RC" "1" "(d) an unbumped content change exits non-zero"
assert_contains "$OUT" "bump pkgrel" "(d) the refusal says to bump pkgrel"
assert_eq "$(bare_head "$BARE")" "$HEAD2" "(d) the refusal pushed nothing"

# ══════════════════════════════════════════════════════════════
# (e) a pkgrel-only bump is allowed
# ══════════════════════════════════════════════════════════════
SRC_REL="$TMP/src-rel"
make_source "$SRC_REL" "$NEXT_VERSION" "2"
printf '\n# packaging tweak with a pkgrel bump\n' >> "$SRC_REL/PKGBUILD"
run_publish "$SRC_REL" "file://$BARE"
assert_eq "$RC" "0" "(e) a pkgrel-only bump exits 0"
assert_eq "$(bare_msg "$BARE")" "lightspeed-bin ${NEXT_VERSION}-2" "(e) the commit message uses the new pkgrel"
HEAD3="$(bare_head "$BARE")"

# ══════════════════════════════════════════════════════════════
# (f) a downgrade is refused
# ══════════════════════════════════════════════════════════════
SRC_OLD="$TMP/src-old"
make_source "$SRC_OLD" "0.9.9" "1"
run_publish "$SRC_OLD" "file://$BARE"
assert_eq "$RC" "1" "(f) a downgrade exits non-zero"
assert_contains "$OUT" "refusing to downgrade" "(f) the refusal says downgrade"
assert_eq "$(bare_head "$BARE")" "$HEAD3" "(f) the downgrade pushed nothing"

# ══════════════════════════════════════════════════════════════
# (g) a PKGBUILD/.SRCINFO mismatch is refused before any git work
# ══════════════════════════════════════════════════════════════
SRC_MISMATCH="$TMP/src-mismatch"
make_source "$SRC_MISMATCH" "$NEXT_VERSION" "1"
sed -i 's/^\tpkgver = .*/\tpkgver = 1.10.0/' "$SRC_MISMATCH/.SRCINFO"
run_publish "$SRC_MISMATCH" "file://$BARE"
assert_eq "$RC" "1" "(g) a .SRCINFO mismatch exits non-zero"
assert_contains "$OUT" ".SRCINFO is out of sync" "(g) the mismatch is named"
assert_eq "$(bare_head "$BARE")" "$HEAD3" "(g) the mismatch pushed nothing"

# ══════════════════════════════════════════════════════════════
# (h) a source URL that does not match pkgver is refused
# ══════════════════════════════════════════════════════════════
SRC_URL="$TMP/src-url"
make_source "$SRC_URL" "$NEXT_VERSION" "1"
sed -i "s|/download/v${NEXT_VERSION}/|/download/v1.10.0/|" "$SRC_URL/.SRCINFO"
run_publish "$SRC_URL" "file://$BARE"
assert_eq "$RC" "1" "(h) a stale source URL exits non-zero"
assert_contains "$OUT" "do not point at v${NEXT_VERSION}" "(h) the stale source URL is named"

# ══════════════════════════════════════════════════════════════
# (i) a source/sha256 count mismatch is refused
# ══════════════════════════════════════════════════════════════
SRC_SUMS="$TMP/src-sums"
make_source "$SRC_SUMS" "$NEXT_VERSION" "1"
# Remove exactly one sha256sums_x86_64 line (the first match). The pattern is
# unanchored on purpose: awk implementations differ on \t in a regex, and the
# string only appears on the sha256sums_x86_64 lines.
awk 'BEGIN{done=0} /sha256sums_x86_64 = /{if (!done) {done=1; next}} {print}' \
	"$SRC_SUMS/.SRCINFO" > "$SRC_SUMS/.SRCINFO.tmp"
mv "$SRC_SUMS/.SRCINFO.tmp" "$SRC_SUMS/.SRCINFO"
run_publish "$SRC_SUMS" "file://$BARE"
assert_eq "$RC" "1" "(i) a source/sha256 count mismatch exits non-zero"
assert_contains "$OUT" "sha256sums_x86_64" "(i) the mismatch names the arch"

# ══════════════════════════════════════════════════════════════
# (j) a missing PKGBUILD is a hard failure
# ══════════════════════════════════════════════════════════════
SRC_EMPTY="$TMP/src-empty"
mkdir -p "$SRC_EMPTY"
run_publish "$SRC_EMPTY" "file://$BARE"
assert_eq "$RC" "1" "(j) a missing PKGBUILD exits non-zero"
assert_contains "$OUT" "PKGBUILD not found" "(j) the missing PKGBUILD is named"

# ══════════════════════════════════════════════════════════════
# (k) --dry-run pushes nothing
# ══════════════════════════════════════════════════════════════
SRC_NEXT="$TMP/src-next"
make_source "$SRC_NEXT" "$NEXT_VERSION" "3"
run_publish "$SRC_NEXT" "file://$BARE" --dry-run
assert_eq "$RC" "0" "(k) --dry-run exits 0"
assert_contains "$OUT" "dry run" "(k) --dry-run says nothing was pushed"
assert_eq "$(bare_head "$BARE")" "$HEAD3" "(k) --dry-run made no commit"

# ══════════════════════════════════════════════════════════════
# (l) --version refuses a PKGBUILD that is not at that version
# ══════════════════════════════════════════════════════════════
run_publish "$SHIPPED_DIR" "file://$BARE" --version "9.9.9"
assert_eq "$RC" "1" "(l) a version mismatch exits non-zero"
assert_contains "$OUT" "expected 9.9.9" "(l) the expected version is named"
assert_eq "$(bare_head "$BARE")" "$HEAD3" "(l) the mismatch pushed nothing"

# ══════════════════════════════════════════════════════════════
# (m) the default ssh:// URL is used when --repo-url is absent: git's
#     insteadOf rewrite keeps the test hermetic while exercising the exact
#     URL string the workflow relies on.
# ══════════════════════════════════════════════════════════════
BARE2="$TMP/aur-ssh.git"
new_bare "$BARE2"
CFG="$TMP/gitconfig"
cat > "$CFG" <<CFG
[url "file://$BARE2"]
	insteadOf = ssh://aur@aur.archlinux.org/lightspeed-bin.git
CFG
OUT="$(GIT_CONFIG_GLOBAL="$CFG" GIT_CONFIG_NOSYSTEM=1 bash "$PUBLISH" --source-dir "$SHIPPED_DIR" 2>&1)"
RC=$?
assert_eq "$RC" "0" "(m) the default ssh:// URL publish path exits 0"
assert_contains "$OUT" "ssh://aur@aur.archlinux.org/lightspeed-bin.git" "(m) the default URL is reported"
assert_contains "$(bare_show "$BARE2" PKGBUILD)" "pkgver=$SHIPPED_VERSION" "(m) the default-URL path pushed the package"

# ══════════════════════════════════════════════════════════════
# (n) usage errors
# ══════════════════════════════════════════════════════════════
OUT="$(bash "$PUBLISH" --bogus 2>&1)"
RC=$?
assert_eq "$RC" "2" "(n) an unknown argument exits 2"
assert_contains "$OUT" "usage:" "(n) an unknown argument prints usage"
OUT="$(bash "$PUBLISH" --repo-url "" 2>&1)"
RC=$?
assert_eq "$RC" "2" "(n) an empty --repo-url exits 2"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'publish-aur: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'publish-aur: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
