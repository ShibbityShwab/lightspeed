#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - self-test for publish-release.sh (the Release workflow's
# idempotent publish step).
#
# Why it exists: publish-release.sh only runs on a tag push, so the
# idempotent-create / retry / permission-verdict branches would otherwise have
# no PR-runnable verification at all - the first execution would be a real
# release. This suite drives the real script with a stateful `gh` stub on
# PATH: no network, no token, no GitHub release. release.yml runs it on every
# event (including pull_request); the Release workflow_dispatch probe runs it
# too.
#
#   (a) a missing release is created (without assets) and the assets are then
#       uploaded with --clobber
#   (b) an existing release is not re-created; assets are uploaded with
#       --clobber and the title/notes/draft metadata is refreshed
#   (c) a create race (release created between the view and the create) falls
#       back to the upload path and still succeeds
#   (d) a persistent 403 on create stops after the bounded attempts and prints
#       the actionable permission verdict (setting name, run URL, RELEASE_TOKEN)
#   (e) a transient 503 on create is retried and then succeeds
#   (f) an existing release whose assets are all present, with a token that
#       cannot upload, warns and exits 0 with writable=false (so the package
#       bumps still run)
#   (g) an existing release with missing assets and a token that cannot upload
#       is a hard permission failure
#   (h) --dry-run on a missing release prints the create plan and mutates nothing
#   (i) --dry-run on an existing release prints the --clobber plan and mutates nothing
#   (j) a pull_request event is refused before any gh call
#   (k) --prerelease is forwarded to gh release create
#   (l) a complete release whose metadata edit is refused warns and exits 0
#
# Usage: bash infra/scripts/test_release_publish.sh
# Exits 0 and prints "release-publish: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PUBLISH="$SCRIPT_DIR/publish-release.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

PASS=0
FAILURES=0

pass() { PASS=$((PASS + 1)); }
fail() {
  printf '  FAIL: %s\n' "$1" >&2
  shift
  for line in "$@"; do printf '        %s\n' "$line" >&2; done
  FAILURES=$((FAILURES + 1))
}

assert_eq() {
  # assert_eq <actual> <expected> <message>
  if [ "$1" = "$2" ]; then pass; else fail "$3" "got:      $1" "expected: $2"; fi
}

assert_contains() {
  # assert_contains <haystack> <needle> <message>
  case "$1" in *"$2"*) pass ;; *) fail "$3" "missing: $2" ;; esac
}

assert_not_contains() {
  case "$1" in *"$2"*) fail "$3" "unexpected: $2" ;; *) pass ;; esac
}

[ -f "$PUBLISH" ] || {
  printf 'release-publish: FAIL - not found: %s\n' "$PUBLISH" >&2
  exit 1
}

# ── stateful gh stub ─────────────────────────────────────────────────────
# State lives in $GH_STUB_STATE (set per case by new_case):
#   release_exists   present once the release exists
#   assets           one published asset name per line
#   create_attempts  counter for 503-then-ok
#   calls.log        every invocation (first argument pair + args)
#   mutations.log    create/upload/edit invocations only
mkdir -p "$TMP/bin"
cat > "$TMP/bin/gh" <<'STUB'
#!/usr/bin/env bash
set -u
STATE="${GH_STUB_STATE:?gh stub: GH_STUB_STATE is not set}"
printf '%s\n' "$*" >> "$STATE/calls.log"
if [ "${1:-}" != "release" ]; then
  echo "stub: unhandled gh command: $*" >&2
  exit 99
fi
sub="${2:-}"
tag="${3:-}"

jq_flag() {
  # jq_flag <args...> -> the expression after --jq, or empty
  local prev="" a
  for a in "$@"; do
    if [ "$prev" = "--jq" ]; then printf '%s' "$a"; return; fi
    prev="$a"
  done
}

release_json() {
  local assets_json='[]'
  if [ -s "$STATE/assets" ]; then
    assets_json="$(jq -R -s 'split("\n") | map(select(length > 0)) | map({name: ., digest: "sha256:0000", size: 1})' "$STATE/assets")"
  fi
  jq -cn --arg tag "$tag" --argjson assets "$assets_json" \
    '{isDraft:false, url:"https://example.test/release", tagName:$tag, assets:$assets}'
}

case "$sub" in
  view)
    if [ ! -f "$STATE/release_exists" ]; then
      echo "release not found" >&2
      exit 1
    fi
    payload="$(release_json)"
    expr="$(jq_flag "$@")"
    if [ -n "$expr" ]; then
      printf '%s' "$payload" | jq -r "$expr"
    else
      printf '%s\n' "$payload"
    fi
    exit 0
    ;;
  create)
    printf 'create\n' >> "$STATE/mutations.log"
    mode="${STUB_CREATE_MODE:-ok}"
    case "$mode" in
      ok)
        touch "$STATE/release_exists"
        echo "https://example.test/release"
        ;;
      exists)
        touch "$STATE/release_exists"
        echo "HTTP 422: Validation Failed (release already exists) (https://api.github.com/repos/x/y/releases)" >&2
        exit 1
        ;;
      403)
        echo "HTTP 403: Resource not accessible by integration (https://api.github.com/repos/x/y/releases)" >&2
        exit 1
        ;;
      503-then-ok)
        n=0
        [ -f "$STATE/create_attempts" ] && n="$(cat "$STATE/create_attempts")"
        n=$((n + 1))
        echo "$n" > "$STATE/create_attempts"
        if [ "$n" -eq 1 ]; then
          echo "HTTP 503: Service Unavailable" >&2
          exit 1
        fi
        touch "$STATE/release_exists"
        echo "https://example.test/release"
        ;;
      *)
        echo "stub: unexpected create mode: $mode" >&2
        exit 99
        ;;
    esac
    exit 0
    ;;
  upload)
    printf 'upload\n' >> "$STATE/mutations.log"
    mode="${STUB_UPLOAD_MODE:-ok}"
    case "$mode" in
      ok)
        for a in "$@"; do
          case "$a" in
            release|upload|--*) continue ;;
            "$tag") continue ;;
          esac
          basename "$a" >> "$STATE/assets"
        done
        if [ -f "$STATE/assets" ]; then
          sort -u "$STATE/assets" -o "$STATE/assets"
        fi
        ;;
      403)
        echo "HTTP 403: Resource not accessible by integration" >&2
        exit 1
        ;;
      *)
        echo "stub: unexpected upload mode: $mode" >&2
        exit 99
        ;;
    esac
    exit 0
    ;;
  edit)
    printf 'edit\n' >> "$STATE/mutations.log"
    mode="${STUB_EDIT_MODE:-ok}"
    case "$mode" in
      ok) touch "$STATE/edited" ;;
      403)
        echo "HTTP 403: Resource not accessible by integration" >&2
        exit 1
        ;;
      *)
        echo "stub: unexpected edit mode: $mode" >&2
        exit 99
        ;;
    esac
    exit 0
    ;;
  *)
    echo "stub: unhandled release subcommand: $sub" >&2
    exit 99
    ;;
esac
STUB
chmod +x "$TMP/bin/gh"

# ── harness ──────────────────────────────────────────────────────────────
STATE=""
ASSETS=""
NOTES=""
OUT=""
RC=0
GEOF=""

new_case() {
  # new_case <name>   -> fresh state dir, assets dir with two fixture files
  local name="$1"
  CASE_DIR="$TMP/case-$name"
  STATE="$CASE_DIR/state"
  ASSETS="$CASE_DIR/artifacts"
  NOTES="$CASE_DIR/notes.txt"
  mkdir -p "$STATE" "$ASSETS"
  : > "$ASSETS/lightspeed-client-x86_64-pc-windows-msvc.zip"
  : > "$ASSETS/dist-manifest.json"
  printf 'release notes for the self-test\n' > "$NOTES"
  export GH_STUB_STATE="$STATE"
  export GITHUB_OUTPUT="$STATE/github_output"
  export GITHUB_REPOSITORY="ShibbityShwab/lightspeed"
  export GITHUB_RUN_ID="43989309532"
  export GITHUB_SERVER_URL="https://github.com"
  unset GITHUB_EVENT_NAME 2>/dev/null || true
  export STUB_CREATE_MODE=ok
  export STUB_UPLOAD_MODE=ok
  export STUB_EDIT_MODE=ok
  : > "$STATE/calls.log"
  : > "$STATE/mutations.log"
}

run_publish() {
  OUT="$(PATH="$TMP/bin:$PATH" RELEASE_PUBLISH_BASE_SLEEP=0 \
    bash "$PUBLISH" "$@" 2>&1)"
  RC=$?
}

call_count() {
  # call_count <fixed string> -> number of gh invocations containing it
  grep -Fc -- "$1" "$STATE/calls.log" 2>/dev/null || true
}

mutation_count() {
  grep -c . "$STATE/mutations.log" 2>/dev/null || true
}

published_output() {
  cat "$STATE/github_output" 2>/dev/null || true
}

# ══════════════════════════════════════════════════════════════
# (a) missing release -> create, then upload with --clobber
# ══════════════════════════════════════════════════════════════
new_case "a-create"
run_publish --tag v9.9.9 --target deadbeef --title "9.9.9 - test" \
  --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(a) a missing release is published"
assert_eq "$(call_count 'release create v9.9.9')" "1" "(a) create ran once"
assert_eq "$(call_count 'release upload v9.9.9')" "1" "(a) the assets were uploaded once"
assert_eq "$(call_count 'release edit')" "0" "(a) a freshly created release needs no edit"
assert_contains "$(cat "$STATE/calls.log")" "--target deadbeef" "(a) create targets the release commit"
assert_contains "$(cat "$STATE/calls.log")" "--title 9.9.9 - test" "(a) create carries the release title"
assert_contains "$(cat "$STATE/calls.log")" "--notes-file $NOTES" "(a) create reads the notes file"
assert_contains "$(cat "$STATE/calls.log")" "lightspeed-client-x86_64-pc-windows-msvc.zip" "(a) the assets are passed to upload"
assert_contains "$(cat "$STATE/calls.log")" "--clobber" "(a) upload uses --clobber"
assert_contains "$(published_output)" "writable=true" "(a) writable=true is reported to the workflow"

# ══════════════════════════════════════════════════════════════
# (b) existing release -> no create, upload with --clobber, edit metadata
# ══════════════════════════════════════════════════════════════
new_case "b-existing"
touch "$STATE/release_exists"
printf '%s\n' lightspeed-client-x86_64-pc-windows-msvc.zip dist-manifest.json > "$STATE/assets"
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(b) an existing release is accepted idempotently"
assert_eq "$(call_count 'release create')" "0" "(b) no second release is created"
assert_eq "$(call_count 'release upload v9.9.9')" "1" "(b) the assets are re-uploaded"
assert_contains "$(cat "$STATE/calls.log")" "release upload v9.9.9" "(b) upload targets the tag"
assert_contains "$(cat "$STATE/calls.log")" "--clobber" "(b) upload uses --clobber"
assert_eq "$(call_count 'release edit v9.9.9')" "1" "(b) existing metadata is refreshed"
assert_contains "$(cat "$STATE/calls.log")" "--draft=false" "(b) a draft release would be published"
assert_contains "$(published_output)" "writable=true" "(b) writable=true is reported"

# ══════════════════════════════════════════════════════════════
# (c) create race (422 already_exists) -> fall back to upload
# ══════════════════════════════════════════════════════════════
new_case "c-race"
export STUB_CREATE_MODE=exists
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(c) a create race is not a failure"
assert_eq "$(call_count 'release create')" "1" "(c) create was attempted once"
assert_eq "$(call_count 'release upload v9.9.9')" "1" "(c) the race fell back to the asset upload"
assert_contains "$OUT" "created concurrently" "(c) the race is reported"
assert_contains "$(published_output)" "writable=true" "(c) the race still ends writable"

# ══════════════════════════════════════════════════════════════
# (d) persistent 403 on create -> bounded attempts + actionable verdict
# ══════════════════════════════════════════════════════════════
new_case "d-403-create"
export STUB_CREATE_MODE=403
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "1" "(d) a persistent 403 fails the step"
assert_eq "$(call_count 'release create')" "4" "(d) create is retried on the bounded budget (4 attempts)"
assert_eq "$(call_count 'release upload')" "0" "(d) no assets are uploaded without a release"
assert_contains "$OUT" "retrying in 0s" "(d) each retry is logged"
assert_contains "$OUT" "::error::" "(d) the verdict is a workflow error annotation"
assert_contains "$OUT" "Workflow permissions" "(d) the verdict names the repository setting"
assert_contains "$OUT" "contents: write" "(d) the verdict names the permission it needs"
assert_contains "$OUT" "RELEASE_TOKEN" "(d) the verdict names the PAT fallback secret"
assert_contains "$OUT" "actions/runs/43989309532" "(d) the verdict links the failing run"
assert_not_contains "$(published_output)" "writable=true" "(d) a failed publish is not reported writable"

# ══════════════════════════════════════════════════════════════
# (e) transient 503 on create -> retried, then succeeds
# ══════════════════════════════════════════════════════════════
new_case "e-503-create"
export STUB_CREATE_MODE=503-then-ok
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(e) a transient 503 is retried and then succeeds"
assert_eq "$(call_count 'release create')" "2" "(e) create ran twice"
assert_contains "$OUT" "transient failure" "(e) the retry is logged"
assert_contains "$(published_output)" "writable=true" "(e) the retried create is writable"

# ══════════════════════════════════════════════════════════════
# (f) existing complete release, token cannot upload -> warn, exit 0
#     (this is the "release published manually, rerun cannot write" case:
#      the package bumps must still run)
# ══════════════════════════════════════════════════════════════
new_case "f-complete-403-upload"
touch "$STATE/release_exists"
printf '%s\n' lightspeed-client-x86_64-pc-windows-msvc.zip dist-manifest.json > "$STATE/assets"
export STUB_UPLOAD_MODE=403
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(f) a complete release is not made red by a read-only token"
assert_contains "$OUT" "already has every expected asset" "(f) the warning explains why it was accepted"
assert_eq "$(call_count 'release upload')" "4" "(f) upload is retried on the bounded budget"
assert_eq "$(call_count 'release edit')" "0" "(f) metadata is not rewritten after the upload failed"
assert_contains "$(published_output)" "writable=false" "(f) writable=false is reported to the workflow"

# ══════════════════════════════════════════════════════════════
# (g) existing release missing assets + token cannot upload -> hard failure
# ══════════════════════════════════════════════════════════════
new_case "g-incomplete-403-upload"
touch "$STATE/release_exists"
printf '%s\n' dist-manifest.json > "$STATE/assets"
export STUB_UPLOAD_MODE=403
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "1" "(g) an incomplete release with a read-only token is a hard failure"
assert_contains "$OUT" "Workflow permissions" "(g) the verdict names the repository setting"
assert_not_contains "$(published_output)" "writable=true" "(g) an incomplete publish is not reported writable"

# ══════════════════════════════════════════════════════════════
# (h) --dry-run performs no mutating gh call (missing release)
# ══════════════════════════════════════════════════════════════
new_case "h-dry-run-missing"
run_publish --dry-run --tag v9.9.9 --assets "$ASSETS"
assert_eq "$RC" "0" "(h) a dry run exits 0"
assert_eq "$(mutation_count)" "0" "(h) a dry run runs no mutating gh command"
assert_eq "$(call_count 'release create')" "0" "(h) a dry run does not create"
assert_eq "$(call_count 'release upload')" "0" "(h) a dry run does not upload"
assert_contains "$OUT" "would run gh release create v9.9.9" "(h) the dry run prints the create plan"
assert_contains "$(published_output)" "writable=false" "(h) a dry run never claims writability"

# ══════════════════════════════════════════════════════════════
# (i) existing release + --dry-run -> upload plan, still no mutation
# ══════════════════════════════════════════════════════════════
new_case "i-dry-run-existing"
touch "$STATE/release_exists"
run_publish --dry-run --tag v9.9.9 --assets "$ASSETS"
assert_eq "$RC" "0" "(i) a dry run on an existing release exits 0"
assert_eq "$(mutation_count)" "0" "(i) a dry run on an existing release mutates nothing"
assert_contains "$OUT" "would run gh release upload v9.9.9" "(i) the dry run prints the upload plan"
assert_contains "$OUT" "--clobber" "(i) the dry run prints --clobber"

# ══════════════════════════════════════════════════════════════
# (j) a pull_request event is refused before any gh call
# ══════════════════════════════════════════════════════════════
new_case "j-pull-request"
export GITHUB_EVENT_NAME=pull_request
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "1" "(j) a pull_request publish is refused"
assert_contains "$OUT" "refusing to publish" "(j) the refusal names the event"
assert_eq "$(mutation_count)" "0" "(j) a refused PR run mutates nothing"
assert_eq "$(call_count 'release view')" "0" "(j) the refusal happens before any gh call"

# ══════════════════════════════════════════════════════════════
# (k) --prerelease is forwarded to create
# ══════════════════════════════════════════════════════════════
new_case "k-prerelease"
run_publish --tag v9.9.9-rc.1 --title "9.9.9-rc.1" --notes-file "$NOTES" --prerelease --assets "$ASSETS"
assert_eq "$RC" "0" "(k) a prerelease tag publishes"
assert_contains "$(cat "$STATE/calls.log")" "--prerelease" "(k) --prerelease is forwarded to gh release create"

# ══════════════════════════════════════════════════════════════
# (l) complete release, metadata edit refused -> warn, exit 0
# ══════════════════════════════════════════════════════════════
new_case "l-complete-403-edit"
touch "$STATE/release_exists"
printf '%s\n' lightspeed-client-x86_64-pc-windows-msvc.zip dist-manifest.json > "$STATE/assets"
export STUB_EDIT_MODE=403
run_publish --tag v9.9.9 --title "9.9.9 - test" --notes-file "$NOTES" --assets "$ASSETS"
assert_eq "$RC" "0" "(l) a complete release is not made red by a refused metadata edit"
assert_contains "$OUT" "metadata could not be updated" "(l) the warning explains the accepted state"
assert_contains "$(published_output)" "writable=false" "(l) writable=false is reported"

# ══════════════════════════════════════════════════════════════
# verdict
# ══════════════════════════════════════════════════════════════
if [ "$FAILURES" -eq 0 ]; then
  printf 'release-publish: all assertions passed\n'
  printf '  (%s checks)\n' "$PASS"
  exit 0
fi

printf 'release-publish: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
