#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - publish dist/aur to the AUR package repository.
#
# dist/aur/PKGBUILD and dist/aur/.SRCINFO are the source of truth for the
# "lightspeed-bin" AUR package. This script copies them into a clone of
# ssh://aur@aur.archlinux.org/lightspeed-bin.git and pushes only when the
# version/release changed. It is the AUR half of the release automation: the
# release workflow's publish-aur job runs it on tag pushes when the
# AUR_SSH_PRIVATE_KEY secret exists.
#
# Safety:
#   - refuses to run when PKGBUILD and .SRCINFO disagree (pkgver/pkgrel, or a
#     source/sha256 count mismatch, or a source URL that does not point at the
#     release matching pkgver) - a half-regenerated .SRCINFO is exactly the
#     AUR validation failure users would hit;
#   - refuses a downgrade (the AUR already has a newer version);
#   - refuses a content change that did not bump pkgrel (AUR packaging rule:
#     bump pkgrel for packaging-only changes, pkgver for a new release);
#   - a clean no-op when the AUR copy is already at the repo version and the
#     content is identical - re-running a release is not an error.
#
# The first AUR push also needs an AUR account with an SSH key (the
# "lightspeed-bin" package name is free); see dist/aur/README.md and
# docs/publishing-channels.md.
#
# CI-only (bash + git). This is a POSIX artifact for a Linux runner; it does
# not run on the Windows dev host. Its hermetic self-test is
# infra/scripts/test_publish_aur.sh (a file:// bare repository, no network,
# no ssh).
#
# Usage:
#   publish-aur.sh [--source-dir DIR] [--repo-url URL] [--branch BRANCH]
#                  [--version VERSION] [--dry-run] [--force]
#
# Environment:
#   AUR_SOURCE_DIR   default for --source-dir (dist/aur)
#   AUR_REPO_URL     default for --repo-url
#                    (ssh://aur@aur.archlinux.org/lightspeed-bin.git)
#   AUR_BRANCH       default for --branch (master)
#   GIT_SSH_COMMAND  honoured by git; the workflow points it at the deployed
#                    key and a pinned known_hosts entry.
#
# Exit codes: 0 = pushed or already current; 1 = failure; 2 = usage error.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SOURCE_DIR="${AUR_SOURCE_DIR:-dist/aur}"
REPO_URL="${AUR_REPO_URL:-ssh://aur@aur.archlinux.org/lightspeed-bin.git}"
BRANCH="${AUR_BRANCH:-master}"
EXPECTED_VERSION=""
DRY_RUN=0
FORCE=0
GIT_NAME="${AUR_GIT_NAME:-github-actions[bot]}"
GIT_EMAIL="${AUR_GIT_EMAIL:-41898282+github-actions[bot]@users.noreply.github.com}"

usage() {
  cat <<'USAGE'
usage: publish-aur.sh [--source-dir DIR] [--repo-url URL] [--branch BRANCH]
                      [--version VERSION] [--dry-run] [--force]

Copies dist/aur/PKGBUILD and dist/aur/.SRCINFO into a clone of the AUR
package repository and pushes when pkgver/pkgrel changed. No-op when the AUR
copy is already current. --version <v> refuses a PKGBUILD that is not at <v>.
--dry-run prints the plan and does not commit or push. --force publishes even
when pkgver/pkgrel did not change (rare; normally bump pkgrel instead).
USAGE
}

die() {
  printf 'publish-aur: %s\n' "$1" >&2
  exit 2
}

fail() {
  printf 'publish-aur: %s\n' "$1" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --source-dir)
      [ "$#" -ge 2 ] || die "--source-dir needs a value"
      SOURCE_DIR="$2"
      shift 2
      ;;
    --repo-url)
      [ "$#" -ge 2 ] || die "--repo-url needs a value"
      REPO_URL="$2"
      shift 2
      ;;
    --branch)
      [ "$#" -ge 2 ] || die "--branch needs a value"
      BRANCH="$2"
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
    --force)
      FORCE=1
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

command -v git >/dev/null 2>&1 || fail "git is not installed"
[ -n "$REPO_URL" ] || die "--repo-url must not be empty"
[ -n "$BRANCH" ] || die "--branch must not be empty"

PKGBUILD="$SOURCE_DIR/PKGBUILD"
SRCINFO="$SOURCE_DIR/.SRCINFO"
[ -f "$PKGBUILD" ] || fail "PKGBUILD not found: $PKGBUILD"
[ -f "$SRCINFO" ] || fail ".SRCINFO not found: $SRCINFO"

# ── read and cross-check the shipped files ────────────────────────────────
pkgver="$(sed -n 's/^pkgver=\(.*\)$/\1/p' "$PKGBUILD" | head -1)"
pkgrel="$(sed -n 's/^pkgrel=\(.*\)$/\1/p' "$PKGBUILD" | head -1)"
[ -n "$pkgver" ] || fail "PKGBUILD has no pkgver= line: $PKGBUILD"
[ -n "$pkgrel" ] || fail "PKGBUILD has no pkgrel= line: $PKGBUILD"

srcinfo_pkgver="$(sed -n 's/^\tpkgver = \(.*\)$/\1/p' "$SRCINFO" | head -1)"
srcinfo_pkgrel="$(sed -n 's/^\tpkgrel = \(.*\)$/\1/p' "$SRCINFO" | head -1)"
if [ "$srcinfo_pkgver" != "$pkgver" ] || [ "$srcinfo_pkgrel" != "$pkgrel" ]; then
  fail ".SRCINFO is out of sync with PKGBUILD: PKGBUILD $pkgver-$pkgrel, .SRCINFO ${srcinfo_pkgver:-<absent>}-${srcinfo_pkgrel:-<absent>}; regenerate it with 'makepkg --printsrcinfo > .SRCINFO'"
fi
if [ -n "$EXPECTED_VERSION" ] && [ "$pkgver" != "$EXPECTED_VERSION" ]; then
  fail "dist/aur is at $pkgver-$pkgrel, expected $EXPECTED_VERSION; bump PKGBUILD and .SRCINFO in the release commit (the AUR package is not published automatically from the tag)"
fi

# Each source_<arch> entry needs its sha256sums_<arch> counterpart, and every
# source URL must point at the release the pkgver names.
arches="$(sed -n 's/^source_\([A-Za-z0-9_]*\) = .*/\1/p' "$SRCINFO" | sort -u)"
[ -n "$arches" ] || fail ".SRCINFO has no source_* entries: $SRCINFO"
for arch in $arches; do
  srcs="$(grep -c "^source_${arch} = " "$SRCINFO" || true)"
  sums="$(grep -c "^sha256sums_${arch} = " "$SRCINFO" || true)"
  if [ "$srcs" != "$sums" ]; then
    fail ".SRCINFO has ${srcs} source_${arch} entries but ${sums} sha256sums_${arch} entries; regenerate it with 'makepkg --printsrcinfo > .SRCINFO'"
  fi
  bad="$(grep "^source_${arch} = " "$SRCINFO" | grep -vc "/releases/download/v${pkgver}/" || true)"
  if [ "$bad" != "0" ]; then
    fail ".SRCINFO source_${arch} has ${bad} URL(s) that do not point at v${pkgver}; refresh the sums and regenerate .SRCINFO"
  fi
done

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT
WORK="$TMP_DIR/lightspeed-bin"

echo "publish-aur: source $SOURCE_DIR version $pkgver-$pkgrel"
echo "publish-aur: remote $REPO_URL (branch $BRANCH)"

if ! git clone --quiet "$REPO_URL" "$WORK" 2> "$TMP_DIR/clone.err"; then
  cat "$TMP_DIR/clone.err" >&2 2>/dev/null || true
  fail "could not clone $REPO_URL; the first push needs an AUR account with an SSH key registered (see dist/aur/README.md) and GIT_SSH_COMMAND pointing at that key"
fi

remote_pkgver=""
remote_pkgrel=""
if [ -f "$WORK/.SRCINFO" ]; then
  remote_pkgver="$(sed -n 's/^\tpkgver = \(.*\)$/\1/p' "$WORK/.SRCINFO" | head -1)"
  remote_pkgrel="$(sed -n 's/^\tpkgrel = \(.*\)$/\1/p' "$WORK/.SRCINFO" | head -1)"
fi

cp "$PKGBUILD" "$WORK/PKGBUILD"
cp "$SRCINFO" "$WORK/.SRCINFO"
git -C "$WORK" add -- PKGBUILD .SRCINFO

if git -C "$WORK" diff --cached --quiet -- PKGBUILD .SRCINFO; then
  if [ -z "$remote_pkgver" ]; then
    fail "the clone of $REPO_URL has no .SRCINFO yet and the copy produced no change; this should not happen"
  fi
  if [ "$remote_pkgver" != "$pkgver" ] || [ "$remote_pkgrel" != "$pkgrel" ]; then
    fail "the AUR copy is at $remote_pkgver-$remote_pkgrel and the repo at $pkgver-$pkgrel, but the files are byte-identical; something is inconsistent"
  fi
  echo "publish-aur: AUR already has lightspeed-bin $pkgver-$pkgrel with identical files; nothing to do"
  exit 0
fi

# Content changed: decide whether this is a legitimate AUR update.
if [ -n "$remote_pkgver" ]; then
  if [ "$remote_pkgver" = "$pkgver" ]; then
    if [ "$remote_pkgrel" = "$pkgrel" ]; then
      if [ "$FORCE" -eq 0 ]; then
        fail "PKGBUILD/.SRCINFO changed but pkgver/pkgrel stayed $pkgver-$pkgrel; bump pkgrel for a packaging-only change (or pass --force to override)"
      fi
    elif [ "$remote_pkgrel" -gt "$pkgrel" ] 2>/dev/null; then
      fail "the AUR copy has $remote_pkgver-$remote_pkgrel, the repo has $pkgver-$pkgrel; refusing to downgrade the AUR package"
    fi
  else
    newer="$(printf '%s\n%s\n' "$pkgver" "$remote_pkgver" | sort -V | tail -1)"
    if [ "$newer" = "$remote_pkgver" ]; then
      fail "the AUR copy has $remote_pkgver, the repo has $pkgver; refusing to downgrade the AUR package"
    fi
  fi
  echo "publish-aur: AUR copy $remote_pkgver-$remote_pkgrel -> $pkgver-$pkgrel"
else
  echo "publish-aur: AUR package is empty; would create lightspeed-bin $pkgver-$pkgrel"
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "publish-aur: dry run - no commit or push was made"
  exit 0
fi

git -C "$WORK" -c "user.name=$GIT_NAME" -c "user.email=$GIT_EMAIL" \
  -c "commit.gpgsign=false" commit --quiet -m "lightspeed-bin $pkgver-$pkgrel"
if ! git -C "$WORK" push --quiet origin "HEAD:refs/heads/$BRANCH" 2> "$TMP_DIR/push.err"; then
  cat "$TMP_DIR/push.err" >&2 2>/dev/null || true
  fail "could not push the AUR package to $REPO_URL; check the SSH key, the known_hosts entry and the AUR account's permissions"
fi

commit_sha="$(git -C "$WORK" rev-parse HEAD)"
echo "publish-aur: pushed lightspeed-bin $pkgver-$pkgrel to $REPO_URL"
echo "publish-aur: commit $commit_sha"
