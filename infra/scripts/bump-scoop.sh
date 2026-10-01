#!/usr/bin/env bash
# Point dist/scoop/lightspeed.json at a released version and refresh its hash.
# Run from the repo root, on a machine with the GitHub CLI authenticated.
#
# Usage: bump-scoop.sh <version>          (for example, 1.6.14)
#
# The manifest in this repo is the source of truth; the public bucket
# (ShibbityShwab/scoop-bucket) is a published copy. Before this script the
# bucket was the only copy and nothing updated it, so it sat at 1.6.3 for
# eleven releases while `scoop install lightspeed` handed users that binary.
set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi

repo="${LIGHTSPEED_REPO:-ShibbityShwab/lightspeed}"
tag="v${version}"
manifest="${LIGHTSPEED_SCOOP_MANIFEST:-dist/scoop/lightspeed.json}"
asset="lightspeed-client-x86_64-pc-windows-msvc.zip"

if [ ! -f "$manifest" ]; then
  echo "bump-scoop: no such manifest: $manifest" >&2
  exit 1
fi

digest="$(
  gh release view "$tag" --repo "$repo" --json assets \
    --jq ".assets[] | select(.name == \"${asset}\") | .digest | sub(\"sha256:\"; \"\")"
)"
if [ -z "$digest" ]; then
  echo "bump-scoop: no ${asset} on ${tag}" >&2
  exit 1
fi

json="$(jq -c . "$manifest" 2>/dev/null)" || {
  echo "bump-scoop: manifest is not valid JSON: $manifest" >&2
  exit 1
}

old_version="$(printf '%s' "$json" | jq -r '.version // ""')"
if [ -z "$old_version" ]; then
  echo "bump-scoop: manifest has no version field: $manifest" >&2
  exit 1
fi

if ! printf '%s' "$json" | jq -e '.architecture."64bit" | type == "object"' >/dev/null 2>&1; then
  echo "bump-scoop: manifest has no architecture.64bit object: $manifest" >&2
  exit 1
fi

updated="$(printf '%s' "$json" | jq \
  --arg v "$version" \
  --arg tag "$tag" \
  --arg asset "$asset" \
  --arg hash "$digest" '
    .version = $v
    | .architecture."64bit".url =
        ("https://github.com/ShibbityShwab/lightspeed/releases/download/" + $tag + "/" + $asset)
    | .architecture."64bit".hash = $hash
  ')"

if [ -z "$updated" ]; then
  echo "bump-scoop: failed to update the manifest" >&2
  exit 1
fi

printf '%s\n' "$updated" > "$manifest"
echo "bump-scoop: ${manifest} set to ${version}"
