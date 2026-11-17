#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - publish one GitHub Release, idempotently.
#
# release.yml calls this after cargo-dist has built the artifacts. On
# 2026-11-16 the generated `gh release create` line failed with
# `HTTP 403: Resource not accessible by integration` on POST /releases. The
# release had to be published by hand, and the package-bump jobs were skipped
# because they required `needs.host.result == 'success'`. This script is the
# publish half of that fix; `release-state` in release.yml is the other half.
#
# The 2026-11-17 token probe (run 43989309532, throwaway branch) proved the
# Actions token CAN create a release under the current repository config, so
# the 2026-11-16 403 was environmental/transient (the same window hit an
# artifact-egress 503). The design is therefore "assume publish works, but
# survive a repeat": retry, then explain, never a bare 403.
#
# What it guarantees:
#   (a) Idempotent. If the release for <tag> already exists, the assets are
#       uploaded with --clobber instead of failing. If the API reports a create
#       race (someone else created it), it falls back to the upload path.
#   (b) Bounded retry. HTTP 5xx/429, timeouts, connection resets, the "Egress
#       is over the account limit" 503 and a 403 are retried on the same
#       bounded budget - RELEASE_PUBLISH_ATTEMPTS attempts (default 4, i.e.
#       three retries) with a 5 s / 15 s / 30 s backoff - and every attempt is
#       logged.
#   (c) An actionable permission verdict. A persistent 403 names the exact
#       settings to check (repository default workflow permissions, the
#       workflow's `permissions:` block, an org/App cap, a ruleset), the run
#       URL, and the optional RELEASE_TOKEN secret.
#   (d) Complete-release tolerance. If the release already has every expected
#       asset and the token cannot rewrite them, it warns and exits 0, so the
#       package bumps (which only read the release) still run.
#   (e) --dry-run. Reads the release state and prints the plan; never calls a
#       mutating `gh` command. The read-only workflow_dispatch probe uses it.
#   (f) Never publishes from a pull_request event, even if called by mistake.
#
# Exit code: 0 when the release exists with the expected assets at the end of
# the run (or the run was a dry run); non-zero otherwise.
#
# This is a POSIX/CI artifact (bash). It does not run on the Windows dev host;
# infra/scripts/test_release_publish.sh verifies it in CI.
#
# Usage:
#   publish-release.sh --tag <tag> [--target <commit>] [--title <title>]
#                      [--notes-file <file>] [--prerelease] [--dry-run]
#                      [--assets <dir>]...
#
# Environment:
#   GH_TOKEN                     required unless --dry-run (gh reads it)
#   RELEASE_PUBLISH_ATTEMPTS     total attempts incl. the first (default 4)
#   RELEASE_PUBLISH_BASE_SLEEP   first backoff in seconds (default 5; then
#                                3x and 6x it - 5 s, 15 s, 30 s)
#   GITHUB_EVENT_NAME            "pull_request" is refused
#   GITHUB_REPOSITORY            used in messages
#   GITHUB_RUN_ID                used in the permission verdict's run URL
#   GITHUB_OUTPUT                receives writable=true|false when set
# ──────────────────────────────────────────────────────────────
set -euo pipefail

TAG=""
TARGET=""
TITLE=""
NOTES_FILE=""
PRERELEASE=0
DRY_RUN=0
ASSET_DIRS=()

usage() {
  cat <<'USAGE'
usage: publish-release.sh --tag <tag> [--target <commit>] [--title <title>]
                          [--notes-file <file>] [--prerelease] [--dry-run]
                          [--assets <dir>]...

Publishes (or updates) the GitHub Release for <tag>. Idempotent: an existing
release gets its assets uploaded with --clobber. Retries a transient 5xx/403 on
a bounded budget and fails with an actionable permission message on a
persistent 403. Use --dry-run to print the plan without running a mutating gh
command.
USAGE
}

die() {
  printf 'publish-release: %s\n' "$1" >&2
  exit 2
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --tag)
      [ "$#" -ge 2 ] || die "--tag needs a value"
      TAG="$2"
      shift 2
      ;;
    --target)
      [ "$#" -ge 2 ] || die "--target needs a value"
      TARGET="$2"
      shift 2
      ;;
    --title)
      [ "$#" -ge 2 ] || die "--title needs a value"
      TITLE="$2"
      shift 2
      ;;
    --notes-file)
      [ "$#" -ge 2 ] || die "--notes-file needs a value"
      NOTES_FILE="$2"
      shift 2
      ;;
    --assets)
      [ "$#" -ge 2 ] || die "--assets needs a value"
      ASSET_DIRS+=("$2")
      shift 2
      ;;
    --prerelease)
      PRERELEASE=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      die "unknown argument: $1"
      ;;
  esac
done

[ -n "$TAG" ] || die "--tag is required"
case "$TAG" in
  *[[:space:]]*) die "tag must not contain whitespace: $TAG" ;;
esac
if [ -n "$NOTES_FILE" ] && [ ! -f "$NOTES_FILE" ]; then
  die "--notes-file does not exist: $NOTES_FILE"
fi

ATTEMPTS="${RELEASE_PUBLISH_ATTEMPTS:-4}"
BASE_SLEEP="${RELEASE_PUBLISH_BASE_SLEEP:-5}"
case "$ATTEMPTS" in ''|*[!0-9]*) ATTEMPTS=4 ;; esac
case "$BASE_SLEEP" in ''|*[!0-9]*) BASE_SLEEP=5 ;; esac
[ "$ATTEMPTS" -ge 1 ] || ATTEMPTS=4

# A pull request must never publish, whatever the caller believes.
if [ "${GITHUB_EVENT_NAME:-}" = "pull_request" ] && [ "$DRY_RUN" -eq 0 ]; then
  printf '::error::publish-release: refusing to publish a release on a pull_request event.\n' >&2
  exit 1
fi

# Collect the assets to upload. --assets may be given more than once.
FILES=()
if [ "${#ASSET_DIRS[@]}" -gt 0 ]; then
  for dir in "${ASSET_DIRS[@]}"; do
    [ -d "$dir" ] || die "--assets directory does not exist: $dir"
    shopt -s nullglob
    for f in "$dir"/*; do
      [ -f "$f" ] || continue
      FILES+=("$f")
    done
    shopt -u nullglob
  done
fi

EXPECTED_NAMES=()
if [ "${#FILES[@]}" -gt 0 ]; then
  for f in "${FILES[@]}"; do
    EXPECTED_NAMES+=("$(basename "$f")")
  done
fi

REPO="${GITHUB_REPOSITORY:-}"
RUN_URL=""
if [ -n "${GITHUB_RUN_ID:-}" ] && [ -n "$REPO" ]; then
  RUN_URL="${GITHUB_SERVER_URL:-https://github.com}/${REPO}/actions/runs/${GITHUB_RUN_ID}"
fi

# ── helpers ──────────────────────────────────────────────────────────────

classify_error() {
  # classify_error <output> -> exists | permission | transient | other
  local out="$1"
  if grep -Eqi 'already_exists|already exists' <<< "$out"; then
    printf 'exists'
    return
  fi
  if grep -Eqi 'Resource not accessible by integration|HTTP 403|"status": ?"403"' <<< "$out"; then
    printf 'permission'
    return
  fi
  if grep -Eqi 'HTTP (429|500|502|503|504)|"status": ?"(429|500|502|503|504)"|timed out|timeout|connection reset|ECONNRESET|TLS handshake|Bad Gateway|Service Unavailable|Internal Server Error|secondary rate limit|Egress is over the account limit' <<< "$out"; then
    printf 'transient'
    return
  fi
  printf 'other'
}

retry_sleep_seconds() {
  # retry_sleep_seconds <retry-number 1..3> -> 1x, 3x, 6x the base
  case "$1" in
    1) printf '%s' "$BASE_SLEEP" ;;
    2) printf '%s' "$((BASE_SLEEP * 3))" ;;
    *) printf '%s' "$((BASE_SLEEP * 6))" ;;
  esac
}

LAST_OUT=""
gh_retry() {
  # gh_retry <description> <gh args...>
  # 0 success, 1 persistent permission failure, 2 another failure.
  # Transient and permission failures share the bounded attempt budget.
  local desc="$1"
  shift
  local attempt=1 class retry_no sleep_s
  while :; do
    if LAST_OUT="$("$@" 2>&1)"; then
      if [ -n "$LAST_OUT" ]; then printf '%s\n' "$LAST_OUT"; fi
      return 0
    fi
    class="$(classify_error "$LAST_OUT")"
    if { [ "$class" = "transient" ] || [ "$class" = "permission" ]; } && [ "$attempt" -lt "$ATTEMPTS" ]; then
      retry_no=$attempt
      sleep_s="$(retry_sleep_seconds "$retry_no")"
      printf 'publish-release: %s: %s failure (attempt %s/%s), retrying in %ss\n' \
        "$desc" "$class" "$attempt" "$ATTEMPTS" "$sleep_s" >&2
      sleep "$sleep_s"
      attempt=$((attempt + 1))
      continue
    fi
    printf '%s\n' "$LAST_OUT" >&2
    if [ "$class" = "permission" ]; then
      return 1
    fi
    return 2
  done
}

write_output() {
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    printf '%s\n' "$1" >> "$GITHUB_OUTPUT"
  fi
}

permission_verdict() {
  # permission_verdict <action> <failing-output>
  local action="$1"
  local out="$2"
  local repo_label="${REPO:-this repository}"
  printf '::error::publish-release: could not %s the GitHub Release for %s: the token was refused with HTTP 403 on every attempt.\n' "$action" "$TAG" >&2
  if [ -n "$RUN_URL" ]; then
    printf '::error::Run: %s\n' "$RUN_URL" >&2
  fi
  printf '::error::The GITHUB_TOKEN cannot write repository contents for %s. Check, in this order:\n' "$repo_label" >&2
  printf '::error::1. Settings -> Actions -> General -> Workflow permissions -> "Read and write permissions", then re-run this workflow.\n' >&2
  printf '::error::2. This workflow must keep its top-level permissions: { contents: write } (release.yml has it; a job-level override with contents: read re-breaks publish).\n' >&2
  printf '::error::3. In an organisation repository the GitHub Actions App installation must be allowed to write contents; an org policy that caps Actions tokens at read cannot be overridden from the workflow.\n' >&2
  printf '::error::4. A repository ruleset must not block release or tag creation.\n' >&2
  printf '::error::If the token is capped anyway (a credential decision for the owner): add a fine-grained PAT with "Contents: read and write" on this repository as the RELEASE_TOKEN repository secret - release.yml prefers it for the publish step when set. Never commit a token.\n' >&2
  printf '::error::Last API error: %s\n' "$(printf '%s' "$out" | tr '\n' ' ' | cut -c1-500)" >&2
  exit 1
}

release_state() {
  # 0 = release exists (VIEW_JSON set), 1 = not found, 2 = could not tell.
  if ! command -v gh >/dev/null 2>&1; then
    VIEW_ERR="gh is not installed"
    return 2
  fi
  local out
  if out="$(gh release view "$TAG" --json isDraft,url,tagName,assets 2>&1)"; then
    VIEW_JSON="$out"
    return 0
  fi
  VIEW_ERR="$out"
  case "$out" in
    *"release not found"*|*"HTTP 404"*|*not\ found*) return 1 ;;
  esac
  return 2
}

release_is_complete() {
  # Every expected asset name is already on the release.
  [ "${#EXPECTED_NAMES[@]}" -gt 0 ] || return 1
  local names name missing=0
  if ! names="$(gh release view "$TAG" --json assets --jq '.assets[].name' 2>/dev/null)"; then
    return 1
  fi
  for name in "${EXPECTED_NAMES[@]}"; do
    if ! grep -Fxq -- "$name" <<< "$names"; then
      missing=$((missing + 1))
    fi
  done
  [ "$missing" -eq 0 ]
}

VIEW_JSON=""
VIEW_ERR=""

echo "publish-release: tag=$TAG assets=${#FILES[@]} dry_run=$DRY_RUN repo=${REPO:-<from gh>}"

state=0
release_state || state=$?
case "$state" in
  0) echo "publish-release: release $TAG already exists" ;;
  1) echo "publish-release: release $TAG does not exist yet" ;;
  *) echo "publish-release: could not read release $TAG: $(printf '%s' "$VIEW_ERR" | head -1)" ;;
esac

if [ "$DRY_RUN" -eq 1 ]; then
  echo "publish-release: dry run - no mutating gh command will be run"
  if [ "$state" -eq 0 ]; then
    echo "dry run: would run gh release upload $TAG <${#FILES[@]} assets> --clobber"
    echo "dry run: would run gh release edit $TAG --title '...' --notes-file '...' --draft=false"
  else
    echo "dry run: would run gh release create $TAG --target '${TARGET:-<default>}' --title '...' --notes-file '...'"
    echo "dry run: then would run gh release upload $TAG <${#FILES[@]} assets> --clobber"
  fi
  write_output "writable=false"
  exit 0
fi

CREATED=0
if [ "$state" -ne 0 ]; then
  create_args=(release create "$TAG")
  if [ -n "$TARGET" ]; then create_args+=(--target "$TARGET"); fi
  if [ -n "$TITLE" ]; then create_args+=(--title "$TITLE"); fi
  if [ -n "$NOTES_FILE" ]; then
    create_args+=(--notes-file "$NOTES_FILE")
  else
    create_args+=(--notes "")
  fi
  if [ "$PRERELEASE" -eq 1 ]; then create_args+=(--prerelease); fi
  if gh_retry "create release $TAG" gh "${create_args[@]}"; then
    echo "publish-release: created release $TAG"
    CREATED=1
  else
    rc=$?
    if [ "$rc" -eq 1 ]; then
      permission_verdict "create" "$LAST_OUT"
    fi
    if [ "$(classify_error "$LAST_OUT")" = "exists" ] || release_state; then
      echo "publish-release: release $TAG exists after the create failed; treating it as created concurrently and switching to the asset upload"
    else
      printf 'publish-release: failed to create release %s\n' "$TAG" >&2
      exit 1
    fi
  fi
fi

if [ "${#FILES[@]}" -gt 0 ]; then
  if gh_retry "upload assets to $TAG" gh release upload "$TAG" "${FILES[@]}" --clobber; then
    echo "publish-release: uploaded ${#FILES[@]} asset(s) to $TAG"
  else
    rc=$?
    if [ "$rc" -eq 1 ] && release_is_complete; then
      printf '::warning::publish-release: release %s already has every expected asset; the token cannot rewrite them (contents: write missing), so the published assets are left as they are.\n' "$TAG" >&2
      write_output "writable=false"
      exit 0
    fi
    if [ "$rc" -eq 1 ]; then
      permission_verdict "upload assets for" "$LAST_OUT"
    fi
    printf 'publish-release: failed to upload assets to %s\n' "$TAG" >&2
    exit 1
  fi
fi

if [ "$CREATED" -eq 0 ]; then
  edit_args=(release edit "$TAG")
  if [ -n "$TITLE" ]; then edit_args+=(--title "$TITLE"); fi
  if [ -n "$NOTES_FILE" ]; then
    edit_args+=(--notes-file "$NOTES_FILE")
  fi
  if [ "$PRERELEASE" -eq 1 ]; then
    edit_args+=(--prerelease)
  else
    edit_args+=(--prerelease=false)
  fi
  edit_args+=(--draft=false)
  if gh_retry "update release metadata for $TAG" gh "${edit_args[@]}"; then
    echo "publish-release: release $TAG metadata updated (title/notes, draft=false)"
  else
    rc=$?
    if [ "$rc" -eq 1 ] && release_is_complete; then
      printf '::warning::publish-release: release %s is complete but its metadata could not be updated (contents: write missing); leaving it as published.\n' "$TAG" >&2
      write_output "writable=false"
      exit 0
    fi
    if [ "$rc" -eq 1 ]; then
      permission_verdict "update" "$LAST_OUT"
    fi
    printf 'publish-release: failed to update release metadata for %s\n' "$TAG" >&2
    exit 1
  fi
fi

write_output "writable=true"
echo "publish-release: release $TAG is published with ${#FILES[@]} expected asset(s)"
