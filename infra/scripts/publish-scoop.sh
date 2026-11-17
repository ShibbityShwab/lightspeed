#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - publish the Scoop manifest to the public bucket.
#
# dist/scoop/lightspeed.json in this repository is the source of truth; the
# public bucket (ShibbityShwab/scoop-bucket) is a published copy that
# "scoop install lightspeed" reads. The bucket used to hold the only copy and
# nothing updated it, so it sat at 1.6.3 while eleven releases went out.
# bump-scoop.sh keeps the in-repo manifest current on every tag; this script is
# the second half - it copies that manifest into the bucket through the GitHub
# contents API.
#
# The release workflow's publish-scoop job runs this on a tag push when the
# SCOOP_TOKEN secret exists (a token with "Contents: write" on
# ShibbityShwab/scoop-bucket). Without the secret the job logs a notice and
# skips; the script itself fails loudly on a broken publish.
#
# Behaviour:
#   - no-op (exit 0) when the bucket already holds an equivalent manifest,
#     comparing parsed JSON so whitespace/line endings/key order do not cause a
#     churn commit on every run;
#   - --version <v> refuses to publish a manifest that is not at <v>, so a tag
#     release can never silently push a stale version (for example when the
#     bump-scoop job has not committed it to master yet, or was skipped);
#   - creates the file when it does not exist, updates it with the current blob
#     sha otherwise, and retries once on a write conflict;
#   - --dry-run prints what would happen without making an API write.
#
# CI-only (bash + jq + gh). This is a POSIX artifact for the Linux runner; it
# does not run on the Windows dev host. Its hermetic self-test is
# infra/scripts/test_publish_scoop.sh (stateful gh stub, no network).
#
# Usage:
#   publish-scoop.sh [--manifest PATH] [--bucket-repo OWNER/REPO]
#                    [--bucket-path PATH] [--version VERSION] [--dry-run]
#
# Environment:
#   GH_TOKEN  required: token with "Contents: write" on the bucket repository.
#             The release workflow sets it from the SCOOP_TOKEN secret.
#   LIGHTSPEED_SCOOP_MANIFEST  default for --manifest
#   SCOOP_BUCKET_REPO          default for --bucket-repo
#
# Exit codes: 0 = bucket current or updated; 1 = failure; 2 = usage error.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

MANIFEST="${LIGHTSPEED_SCOOP_MANIFEST:-dist/scoop/lightspeed.json}"
BUCKET_REPO="${SCOOP_BUCKET_REPO:-ShibbityShwab/scoop-bucket}"
BUCKET_PATH="${SCOOP_BUCKET_PATH:-bucket/lightspeed.json}"
EXPECTED_VERSION=""
DRY_RUN=0

usage() {
  cat <<'USAGE'
usage: publish-scoop.sh [--manifest PATH] [--bucket-repo OWNER/REPO]
                        [--bucket-path PATH] [--version VERSION] [--dry-run]

Copies dist/scoop/lightspeed.json into the public Scoop bucket through the
GitHub contents API. No-op when the bucket already holds an equivalent
manifest. --version <v> refuses to publish a manifest that is not at <v>.
--dry-run prints the plan and makes no API write.
USAGE
}

die() {
  printf 'publish-scoop: %s\n' "$1" >&2
  exit 2
}

fail() {
  printf 'publish-scoop: %s\n' "$1" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --manifest)
      [ "$#" -ge 2 ] || die "--manifest needs a value"
      MANIFEST="$2"
      shift 2
      ;;
    --bucket-repo)
      [ "$#" -ge 2 ] || die "--bucket-repo needs a value"
      BUCKET_REPO="$2"
      shift 2
      ;;
    --bucket-path)
      [ "$#" -ge 2 ] || die "--bucket-path needs a value"
      BUCKET_PATH="$2"
      shift 2
      ;;
    --version)
      [ "$#" -ge 2 ] || die "--version needs a value"
      EXPECTED_VERSION="$2"
      shift 2
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

command -v gh >/dev/null 2>&1 || fail "gh is not installed"
command -v jq >/dev/null 2>&1 || fail "jq is not installed"
[ -n "${GH_TOKEN:-}" ] || fail "GH_TOKEN is not set; it needs \"Contents: write\" on ${BUCKET_REPO} (the release workflow passes SCOOP_TOKEN). Never commit a token"
[ -n "$BUCKET_REPO" ] || die "--bucket-repo must not be empty"
[ -n "$BUCKET_PATH" ] || die "--bucket-path must not be empty"
case "$BUCKET_REPO" in
  */*) ;;
  *) die "--bucket-repo must look like OWNER/REPO (got: $BUCKET_REPO)" ;;
esac
[ -f "$MANIFEST" ] || fail "manifest not found: $MANIFEST"
jq -e . "$MANIFEST" >/dev/null 2>&1 || fail "manifest is not valid JSON: $MANIFEST"

local_version="$(jq -r '.version // ""' "$MANIFEST")"
[ -n "$local_version" ] || fail "manifest has no version field: $MANIFEST"
if [ -n "$EXPECTED_VERSION" ] && [ "$local_version" != "$EXPECTED_VERSION" ]; then
  fail "$MANIFEST is at $local_version, expected $EXPECTED_VERSION; bump-scoop has not committed that version to master yet, so the bucket was not touched"
fi

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# gh_error_kind <stderr-file> -> reads a gh error and says what it means.
gh_error_kind() {
  if grep -qiE 'HTTP 404|Not Found' "$1"; then
    printf 'missing'
  elif grep -qiE 'HTTP 409|HTTP 422|does not match|sha' "$1"; then
    printf 'conflict'
  elif grep -qiE 'HTTP 401|HTTP 403|Bad credentials|Resource not accessible' "$1"; then
    printf 'auth'
  else
    printf 'other'
  fi
}

# fetch_bucket: 0 = file exists (API JSON in $TMP_DIR/current.json),
#               1 = file absent (404), 2 = cannot read it (auth/network/etc).
fetch_bucket() {
  local rc=0
  gh api "repos/${BUCKET_REPO}/contents/${BUCKET_PATH}" > "$TMP_DIR/current.json" 2> "$TMP_DIR/fetch.err" || rc=$?
  if [ "$rc" -eq 0 ]; then
    return 0
  fi
  case "$(gh_error_kind "$TMP_DIR/fetch.err")" in
    missing) return 1 ;;
    auth)
      printf 'publish-scoop: cannot read %s/%s: GH_TOKEN was refused (it needs "Contents: write" on that repository).\n' \
        "$BUCKET_REPO" "$BUCKET_PATH" >&2
      ;;
    *)
      printf 'publish-scoop: cannot read %s/%s (gh exit %s).\n' \
        "$BUCKET_REPO" "$BUCKET_PATH" "$rc" >&2
      ;;
  esac
  cat "$TMP_DIR/fetch.err" >&2 2>/dev/null || true
  return 2
}

# bucket_matches_local: 0 when the fetched file parses to the same JSON as the
# local manifest (formatting, key order and CRLF do not matter).
bucket_matches_local() {
  local content_b64 current_canon local_canon
  content_b64="$(jq -r '.content // ""' "$TMP_DIR/current.json" 2>/dev/null || true)"
  [ -n "$content_b64" ] || return 1
  printf '%s' "$content_b64" | tr -d '\r\n' | base64 -d > "$TMP_DIR/bucket.json" 2>/dev/null || return 1
  current_canon="$(jq -S . "$TMP_DIR/bucket.json" 2>/dev/null || true)"
  local_canon="$(jq -S . "$MANIFEST")"
  [ -n "$current_canon" ] || return 1
  [ "$current_canon" = "$local_canon" ]
}

state=0
fetch_bucket || state=$?
[ "$state" -ne 2 ] || fail "could not read the bucket manifest; refusing to publish blind"

if [ "$state" -eq 0 ] && bucket_matches_local; then
  echo "publish-scoop: ${BUCKET_REPO}/${BUCKET_PATH} is already at version ${local_version}; nothing to do"
  exit 0
fi

if [ "$state" -eq 1 ]; then
  echo "publish-scoop: ${BUCKET_REPO}/${BUCKET_PATH} does not exist yet; would create it at version ${local_version}"
else
  echo "publish-scoop: ${BUCKET_REPO}/${BUCKET_PATH} will be updated to version ${local_version}"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "publish-scoop: dry run - no GitHub API write was made"
  exit 0
fi

MESSAGE="chore(scoop): sync lightspeed ${local_version}"
CONTENT_B64="$(base64 < "$MANIFEST" | tr -d '\r\n')"
[ -n "$CONTENT_B64" ] || fail "could not base64-encode $MANIFEST"

attempt=1
while [ "$attempt" -le 2 ]; do
  bucket_sha=""
  if [ "$state" -eq 0 ]; then
    bucket_sha="$(jq -r '.sha // ""' "$TMP_DIR/current.json" 2>/dev/null || true)"
  fi
  put_args=(api --method PUT "repos/${BUCKET_REPO}/contents/${BUCKET_PATH}"
    -f "message=${MESSAGE}" -f "content=${CONTENT_B64}")
  if [ -n "$bucket_sha" ]; then
    put_args+=(-f "sha=${bucket_sha}")
  fi
  put_rc=0
  put_out="$(gh "${put_args[@]}" 2> "$TMP_DIR/put.err")" || put_rc=$?
  if [ "$put_rc" -eq 0 ]; then
    commit_sha="$(printf '%s' "$put_out" | jq -r '.commit.sha // ""' 2>/dev/null || true)"
    echo "publish-scoop: pushed lightspeed ${local_version} to ${BUCKET_REPO}/${BUCKET_PATH}"
    if [ -n "$commit_sha" ]; then
      echo "publish-scoop: commit ${commit_sha}"
    fi
    exit 0
  fi
  kind="$(gh_error_kind "$TMP_DIR/put.err")"
  if [ "$attempt" -eq 1 ] && { [ "$kind" = "conflict" ] || [ "$kind" = "missing" ]; }; then
    echo "publish-scoop: the bucket changed while publishing ($kind); re-reading and retrying once" >&2
    state=0
    fetch_bucket || state=$?
    if [ "$state" -eq 2 ]; then
      fail "could not re-read the bucket manifest after a write conflict"
    fi
    if [ "$state" -eq 0 ] && bucket_matches_local; then
      echo "publish-scoop: ${BUCKET_REPO}/${BUCKET_PATH} is already at version ${local_version} (concurrent update); nothing else to do"
      exit 0
    fi
    attempt=$((attempt + 1))
    continue
  fi
  if [ "$kind" = "auth" ]; then
    fail "the GitHub API refused the write (HTTP 401/403): GH_TOKEN needs \"Contents: write\" on ${BUCKET_REPO}"
  fi
  printf 'publish-scoop: failed to publish %s (gh exit %s):\n' "$MANIFEST" "$put_rc" >&2
  cat "$TMP_DIR/put.err" >&2 2>/dev/null || true
  exit 1
done

fail "failed to publish ${MANIFEST} after retrying a write conflict"
