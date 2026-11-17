#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for publish-scoop.sh
#
# The Scoop bucket (ShibbityShwab/scoop-bucket) is what
# "scoop install lightspeed" reads. dist/scoop/lightspeed.json is the source
# of truth and bump-scoop.sh keeps it current, but publishing the copy to the
# bucket is a second write through the GitHub contents API, with a separate
# token and a separate failure mode. This suite covers that half with a
# stateful gh stub, so it runs offline with no credentials.
#
# Asserted behaviours:
#   (a) missing GH_TOKEN is a hard failure and makes no API write
#   (b) a missing or invalid local manifest is a hard failure
#   (c) a bucket manifest that is JSON-equivalent but formatted differently
#       (key order, indentation, CRLF) is a clean no-op
#   (d) a stale bucket manifest is updated with the exact local bytes, the
#       current blob sha and a version-named commit message
#   (e) --version refuses to publish a manifest that is not at that version
#   (f) an absent bucket file is created (no sha sent)
#   (g) a refused write names the GH_TOKEN permission requirement
#   (h) an unreadable bucket is a hard failure (never publish blind)
#   (i) --dry-run reports the plan and makes no API write
#   (j) a concurrent writer that already applied the same content is detected
#       after a 409 and is a clean success
#   (k) a one-off 409 is retried and the second write succeeds
#   (l) usage errors exit 2
#
# Usage: bash infra/scripts/test_publish_scoop.sh
# Exits 0 and prints "publish-scoop: all assertions passed" on success.
# Requires: bash, jq, coreutils base64 (CI: ubuntu-latest).
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PUBLISH="$SCRIPT_DIR/publish-scoop.sh"
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

assert_contains() {
	case "$1" in
	*"$2"*) pass ;;
	*) fail "$3" "missing: $2" ;;
	esac
}

assert_no_put() {
	if [ -f "$1/put.log" ] && [ -s "$1/put.log" ]; then
		fail "$2" "a PUT was sent but should not have been"
	else
		pass
	fi
}

assert_put_count() {
	local count=0
	if [ -f "$1/put.log" ]; then
		count="$(grep -c '^content=' "$1/put.log" || true)"
	fi
	assert_eq "$count" "$2" "$3"
}

if [ ! -f "$PUBLISH" ]; then
	printf 'publish-scoop: FAIL - not found: %s\n' "$PUBLISH" >&2
	exit 1
fi
if [ ! -f "$SHIPPED" ]; then
	printf 'publish-scoop: FAIL - shipped manifest not found: %s\n' "$SHIPPED" >&2
	exit 1
fi

# ── the stateful gh stub ─────────────────────────────────────
# It implements only what the script uses: "gh api <path>" for a read and
# "gh api --method PUT <path> -f message= -f content= -f sha=" for a write.
# State lives in a per-scenario directory so assertions can inspect what was
# sent, not just the exit code.
make_gh_stub() {
	local dir="$1"
	mkdir -p "$dir"
	cat > "$dir/gh" <<'STUB'
#!/usr/bin/env bash
set -uo pipefail
STATE="${GH_STUB_STATE:?}"
args=("$@")
method="GET"
path=""
fields=()
i=0
while [ "$i" -lt "${#args[@]}" ]; do
  case "${args[$i]}" in
    --method) i=$((i + 1)); method="${args[$i]}" ;;
    -f) i=$((i + 1)); fields+=("${args[$i]}") ;;
    repos/*) path="${args[$i]}" ;;
  esac
  i=$((i + 1))
done
if [ -z "$path" ]; then
  echo "gh-stub: no repository path in arguments: $*" >&2
  exit 99
fi

write_contents() {
  jq -n --arg sha "$2" --arg content "$1" \
    '{name:"lightspeed.json",path:"bucket/lightspeed.json",sha:$sha,size:1,content:$content,encoding:"base64"}' \
    > "$STATE/contents.json"
}

if [ "$method" = "PUT" ]; then
  # Append (never truncate): the conflict test asserts how many writes were
  # attempted, so each PUT must leave its fields behind.
  content=""
  sha=""
  message=""
  for f in "${fields[@]}"; do
    printf '%s\n' "$f" >> "$STATE/put.log"
    case "$f" in
      content=*) content="${f#content=}" ;;
      sha=*) sha="${f#sha=}" ;;
      message=*) message="${f#message=}" ;;
    esac
  done
  if [ -z "$content" ]; then
    echo "gh-stub: PUT without a content field" >&2
    exit 98
  fi
  if [ -f "$STATE/fail-put" ]; then
    echo "gh: Resource not accessible by integration (HTTP 403)" >&2
    exit 1
  fi
  if [ -f "$STATE/conflict-apply" ]; then
    rm -f "$STATE/conflict-apply"
    write_contents "$content" "stub-blob-concurrent"
    echo "gh: HTTP 409: sha does not match the current file" >&2
    exit 1
  fi
  if [ -f "$STATE/conflict-once" ]; then
    rm -f "$STATE/conflict-once"
    echo "gh: HTTP 409: sha does not match the current file" >&2
    exit 1
  fi
  n=1
  if [ -f "$STATE/sha-counter" ]; then
    n=$(( $(cat "$STATE/sha-counter") + 1 ))
  fi
  printf '%s' "$n" > "$STATE/sha-counter"
  newsha="stub-blob-${n}"
  printf '%s' "$content" > "$STATE/submitted-content.b64"
  printf '%s' "$sha" > "$STATE/submitted-sha.txt"
  printf '%s' "$message" > "$STATE/submitted-message.txt"
  write_contents "$content" "$newsha"
  jq -n --arg sha "$newsha" '{sha:$sha,commit:{sha:("stub-commit-" + $sha)}}'
  exit 0
fi

if [ -f "$STATE/fail-get" ]; then
  echo "gh: Bad credentials (HTTP 401)" >&2
  exit 1
fi
if [ -f "$STATE/absent" ]; then
  echo "gh: Not Found (HTTP 404)" >&2
  exit 1
fi
cat "$STATE/contents.json"
STUB
	chmod +x "$dir/gh"
}

# make_bucket <state-dir> <manifest> [sha]
# Builds the GET payload the GitHub contents API returns for that file.
make_bucket() {
	local state="$1" manifest="$2" sha="${3:-stub-blob-baseline}"
	mkdir -p "$state"
	local b64
	b64="$(base64 < "$manifest" | tr -d '\n')"
	jq -n --arg sha "$sha" --arg content "$b64" \
		'{name:"lightspeed.json",path:"bucket/lightspeed.json",sha:$sha,size:1,content:$content,encoding:"base64"}' \
		> "$state/contents.json"
}

# run_publish <state-dir> <stub-dir> [--args...]  -> sets OUT and RC
run_publish() {
	local state="$1" stub="$2"
	shift 2
	OUT="$(PATH="$stub:$PATH" GH_STUB_STATE="$state" GH_TOKEN="stub-token" \
		LIGHTSPEED_SCOOP_MANIFEST="$FIXTURE" bash "$PUBLISH" "$@" 2>&1)"
	RC=$?
}

# A private copy of the shipped manifest: the tests never touch the real one.
FIXTURE="$TMP/lightspeed.json"
cp "$SHIPPED" "$FIXTURE"

# ── a synthetic bucket payload that is NOT the shipped manifest ──
OLD="$TMP/old.json"
jq '.version = "1.10.3" | .architecture."64bit".hash = "deadbeef"' "$SHIPPED" > "$OLD"

# ══════════════════════════════════════════════════════════════
# (l) usage errors
# ══════════════════════════════════════════════════════════════
STUB="$TMP/stub"
make_gh_stub "$STUB"
OUT="$("$PUBLISH" --bogus 2>&1)"
RC=$?
assert_eq "$RC" "2" "(l) an unknown argument exits 2"
assert_contains "$OUT" "usage:" "(l) an unknown argument prints usage"
OUT="$("$PUBLISH" --bucket-repo "no-slash-repo" 2>&1)"
RC=$?
assert_eq "$RC" "2" "(l) a malformed bucket repo exits 2"
assert_contains "$OUT" "OWNER/REPO" "(l) a malformed bucket repo names the expected shape"

# ══════════════════════════════════════════════════════════════
# (b) missing / invalid local manifest
# ══════════════════════════════════════════════════════════════
S="$TMP/s-missing-manifest"
make_bucket "$S" "$OLD"
OUT="$(PATH="$STUB:$PATH" GH_STUB_STATE="$S" GH_TOKEN="stub-token" \
	LIGHTSPEED_SCOOP_MANIFEST="$TMP/does-not-exist.json" bash "$PUBLISH" 2>&1)"
RC=$?
assert_eq "$RC" "1" "(b) a missing local manifest exits non-zero"
assert_contains "$OUT" "manifest not found" "(b) a missing local manifest is named"
assert_no_put "$S" "(b) a missing local manifest makes no API write"
printf 'not json at all {{' > "$TMP/not-json.json"
OUT="$(PATH="$STUB:$PATH" GH_STUB_STATE="$S" GH_TOKEN="stub-token" \
	LIGHTSPEED_SCOOP_MANIFEST="$TMP/not-json.json" bash "$PUBLISH" 2>&1)"
RC=$?
assert_eq "$RC" "1" "(b) an invalid local manifest exits non-zero"
assert_no_put "$S" "(b) an invalid local manifest makes no API write"

# ══════════════════════════════════════════════════════════════
# (a) missing GH_TOKEN
# ══════════════════════════════════════════════════════════════
S="$TMP/s-no-token"
make_bucket "$S" "$OLD"
OUT="$(PATH="$STUB:$PATH" GH_STUB_STATE="$S" env -u GH_TOKEN \
	LIGHTSPEED_SCOOP_MANIFEST="$FIXTURE" bash "$PUBLISH" 2>&1)"
RC=$?
assert_eq "$RC" "1" "(a) a missing GH_TOKEN exits non-zero"
assert_contains "$OUT" "GH_TOKEN is not set" "(a) a missing GH_TOKEN is named"
assert_no_put "$S" "(a) a missing GH_TOKEN makes no API write"

# ══════════════════════════════════════════════════════════════
# (c) an equivalent but differently-formatted bucket is a clean no-op
#     (the real bucket is served with CRLF and a different layout)
# ══════════════════════════════════════════════════════════════
S="$TMP/s-equivalent"
mkdir -p "$S"
jq -S --indent 2 . "$FIXTURE" | sed 's/$/\r/' > "$TMP/reformatted-crlf.json"
make_bucket "$S" "$TMP/reformatted-crlf.json"
run_publish "$S" "$STUB"
assert_eq "$RC" "0" "(c) a JSON-equivalent bucket manifest exits 0"
assert_contains "$OUT" "already at version" "(c) a current bucket manifest is reported as a no-op"
assert_no_put "$S" "(c) a current bucket manifest makes no API write"

# ══════════════════════════════════════════════════════════════
# (d) a stale bucket manifest is updated with the exact bytes
# ══════════════════════════════════════════════════════════════
S="$TMP/s-stale"
make_bucket "$S" "$OLD" "stub-blob-old"
run_publish "$S" "$STUB" --version "$(jq -r '.version' "$FIXTURE")"
assert_eq "$RC" "0" "(d) a stale bucket manifest is published"
assert_contains "$OUT" "pushed lightspeed" "(d) the publish is reported"
assert_put_count "$S" "1" "(d) exactly one PUT is sent"
assert_eq "$(cat "$S/submitted-sha.txt")" "stub-blob-old" "(d) the PUT carries the current blob sha"
assert_contains "$(cat "$S/submitted-message.txt")" "$(jq -r '.version' "$FIXTURE")" \
	"(d) the commit message names the version"
assert_contains "$(cat "$S/submitted-message.txt")" "scoop" "(d) the commit message is scoped to scoop"
base64 -d < "$S/submitted-content.b64" > "$TMP/submitted.json"
if cmp -s "$TMP/submitted.json" "$FIXTURE"; then
	pass
else
	fail "(d) the PUT content is exactly dist/scoop/lightspeed.json"
fi

# ══════════════════════════════════════════════════════════════
# (e) --version refuses a manifest that is not at that version
# ══════════════════════════════════════════════════════════════
S="$TMP/s-version-mismatch"
make_bucket "$S" "$OLD"
run_publish "$S" "$STUB" --version "9.9.9"
assert_eq "$RC" "1" "(e) a version mismatch exits non-zero"
assert_contains "$OUT" "expected 9.9.9" "(e) a version mismatch names the expected version"
assert_no_put "$S" "(e) a version mismatch makes no API write"

# ══════════════════════════════════════════════════════════════
# (f) an absent bucket file is created without a sha
# ══════════════════════════════════════════════════════════════
S="$TMP/s-absent"
mkdir -p "$S"
touch "$S/absent"
run_publish "$S" "$STUB" --version "$(jq -r '.version' "$FIXTURE")"
assert_eq "$RC" "0" "(f) an absent bucket file is created"
assert_contains "$OUT" "does not exist yet" "(f) the create path is reported"
assert_put_count "$S" "1" "(f) exactly one PUT is sent"
if grep -q '^sha=' "$S/put.log"; then
	fail "(f) a create must not send a sha"
else
	pass
fi

# ══════════════════════════════════════════════════════════════
# (g) a refused write names the token requirement
# ══════════════════════════════════════════════════════════════
S="$TMP/s-auth"
make_bucket "$S" "$OLD"
touch "$S/fail-put"
run_publish "$S" "$STUB"
assert_eq "$RC" "1" "(g) a refused write exits non-zero"
assert_contains "$OUT" "Contents: write" "(g) a refused write names the required permission"
assert_contains "$OUT" "GH_TOKEN" "(g) a refused write names the token"

# ══════════════════════════════════════════════════════════════
# (h) an unreadable bucket is a hard failure
# ══════════════════════════════════════════════════════════════
S="$TMP/s-read-fail"
mkdir -p "$S"
touch "$S/fail-get"
run_publish "$S" "$STUB"
assert_eq "$RC" "1" "(h) an unreadable bucket exits non-zero"
assert_contains "$OUT" "cannot read" "(h) an unreadable bucket is named"
assert_no_put "$S" "(h) an unreadable bucket makes no API write"

# ══════════════════════════════════════════════════════════════
# (i) --dry-run makes no write
# ══════════════════════════════════════════════════════════════
S="$TMP/s-dry-run"
make_bucket "$S" "$OLD"
run_publish "$S" "$STUB" --dry-run
assert_eq "$RC" "0" "(i) --dry-run exits 0"
assert_contains "$OUT" "dry run" "(i) --dry-run says it made no write"
assert_no_put "$S" "(i) --dry-run makes no API write"

# ══════════════════════════════════════════════════════════════
# (j) a concurrent writer that already applied the same content: the 409 is
#     resolved by re-reading, not by failing the release
# ══════════════════════════════════════════════════════════════
S="$TMP/s-conflict-applied"
make_bucket "$S" "$OLD"
touch "$S/conflict-apply"
run_publish "$S" "$STUB"
assert_eq "$RC" "0" "(j) a concurrent same-content write is a clean success"
assert_contains "$OUT" "concurrent update" "(j) the concurrent update is reported"

# ══════════════════════════════════════════════════════════════
# (k) a one-off 409 is retried and the second write succeeds
# ══════════════════════════════════════════════════════════════
S="$TMP/s-conflict-retry"
make_bucket "$S" "$OLD"
touch "$S/conflict-once"
run_publish "$S" "$STUB"
assert_eq "$RC" "0" "(k) a one-off conflict is retried to success"
assert_contains "$OUT" "retrying once" "(k) the retry is logged"
assert_put_count "$S" "2" "(k) two PUTs were sent (conflict, then success)"

# ══════════════════════════════════════════════════════════════
# (f2) the shipped manifest keeps the wiring this script assumes
# ══════════════════════════════════════════════════════════════
assert_eq "$(jq -r '.version // ""' "$SHIPPED" | wc -l | tr -d ' ')" "1" \
	"(f2) the shipped manifest is one JSON document"
assert_eq "$(jq -r '.checkver.github // ""' "$SHIPPED")" \
	"https://github.com/ShibbityShwab/lightspeed" "(f2) checkver still points at the repo"
# shellcheck disable=SC2016  # the manifest's literal $version placeholder
assert_eq "$(jq -r '.autoupdate.architecture."64bit".url // ""' "$SHIPPED")" \
	'https://github.com/ShibbityShwab/lightspeed/releases/download/v$version/lightspeed-client-x86_64-pc-windows-msvc.zip' \
	"(f2) autoupdate url still carries \$version"
assert_contains "$(sed -n '1p' "$PUBLISH")" "/usr/bin/env bash" "(f2) the publish script keeps its bash shebang"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'publish-scoop: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'publish-scoop: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
