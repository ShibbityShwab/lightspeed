#!/usr/bin/env bash
# Extract one release section from CHANGELOG.md, for use as the GitHub
# release body. Prints the `## [<version>]` section (heading through the
# line before the next `## [` heading, or EOF), or nothing when the section
# is absent. GitHub already lists the release artifacts, so the body should
# be the changelog and nothing else.
#
# Usage: release-notes.sh <version> [changelog-path]
set -euo pipefail

version="${1:-}"
changelog="${2:-CHANGELOG.md}"

if [ -z "$version" ]; then
  echo "usage: $0 <version> [changelog-path]" >&2
  exit 2
fi

if [ ! -f "$changelog" ]; then
  echo "release-notes: no such changelog: $changelog" >&2
  exit 2
fi

# Match the heading by literal string prefix (index), not a regex, so a version
# containing regex metacharacters (the dots in 1.5.0) is matched exactly.
awk -v pat="## [$version]" '
  index($0, pat) == 1 { found = 1 }
  found && /^## \[/ && index($0, pat) != 1 { exit }
  found { print }
' "$changelog"
