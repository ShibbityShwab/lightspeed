#!/usr/bin/env bash
# Point the Chocolatey package at a released version and refresh its download
# URL and checksum. Run from the repo root, on a machine with the GitHub CLI
# authenticated.
#
# Usage: bump-chocolatey.sh <version>          (for example, 1.6.3)
set -euo pipefail

version="${1:-}"
if [ -z "$version" ]; then
  echo "usage: $0 <version>" >&2
  exit 2
fi

repo="${LIGHTSPEED_REPO:-ShibbityShwab/lightspeed}"
tag="v${version}"
base="${LIGHTSPEED_CHOCOLATEY_DIR:-dist/chocolatey}"
nuspec="$base/lightspeed.nuspec"
install="$base/tools/chocolateyInstall.ps1"
asset="lightspeed-client-x86_64-pc-windows-msvc.zip"

for f in "$nuspec" "$install"; do
  if [ ! -f "$f" ]; then
    echo "bump-chocolatey: no such file: $f" >&2
    exit 1
  fi
done

digest="$(
  gh release view "$tag" --repo "$repo" --json assets \
    --jq ".assets[] | select(.name == \"${asset}\") | .digest | sub(\"sha256:\"; \"\")"
)"
if [ -z "$digest" ]; then
  echo "bump-chocolatey: no ${asset} on ${tag}" >&2
  exit 1
fi

sed -i -e "s|<version>[^<]*</version>|<version>${version}</version>|" "$nuspec"
sed -i \
  -e "s|releases/download/v[0-9][^/]*/${asset}|releases/download/${tag}/${asset}|" \
  -e "s|checksum64 *= *'[0-9a-f]*'|checksum64     = '${digest}'|" \
  "$install"

echo "bump-chocolatey: ${nuspec} + ${install} set to ${version}"
