#!/usr/bin/env bash
# Point Formula/lightspeed.rb at a released version and refresh its checksums.
# Run from the repo root, on a machine with the GitHub CLI authenticated.
#
# Usage: bump-homebrew.sh <version>          (for example, 1.5.0)
set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi

repo="${LIGHTSPEED_REPO:-ShibbityShwab/lightspeed}"
tag="v${version}"
formula="${LIGHTSPEED_HOMEBREW_FORMULA:-Formula/lightspeed.rb}"

if [ ! -f "$formula" ]; then
  echo "bump-homebrew: no such formula: $formula" >&2
  exit 1
fi

assets="$(
  gh release view "$tag" --repo "$repo" --json assets \
    --jq '.assets[] | select(.name | test("^lightspeed-client-(x86_64|aarch64)-(apple-darwin|unknown-linux-gnu)\\.tar\\.xz$")) | "\(.name) \(.digest | sub("sha256:"; ""))"'
)"

count="$(printf '%s\n' "$assets" | grep -c . || true)"
if [ "$count" -ne 4 ]; then
  echo "bump-homebrew: expected 4 client tarballs on $tag, found $count" >&2
  exit 1
fi

digest_for() {
  printf '%s\n' "$assets" | awk -v name="$1" '$1 == name { print $2 }'
}

# Update the version and repoint every download URL at the new tag.
sed -i \
  -e "s|^\(  version \)\".*\"|\1\"${version}\"|" \
  -e "s|releases/download/v[0-9][^/]*/|releases/download/${tag}/|g" \
  "$formula"

# Each url line is followed by its sha256 line on the next line.
for name in \
  lightspeed-client-x86_64-apple-darwin.tar.xz \
  lightspeed-client-aarch64-apple-darwin.tar.xz \
  lightspeed-client-x86_64-unknown-linux-gnu.tar.xz \
  lightspeed-client-aarch64-unknown-linux-gnu.tar.xz
do
  digest="$(digest_for "$name")"
  if [ -z "$digest" ]; then
    echo "bump-homebrew: no digest for $name on $tag" >&2
    exit 1
  fi
  sed -i "/${name}/{n;s|sha256 \".*\"|sha256 \"${digest}\"|;}" "$formula"
done

echo "bump-homebrew: $formula set to ${version}"
