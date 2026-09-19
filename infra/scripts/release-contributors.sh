#!/usr/bin/env bash
# List the human contributors who changed files in a commit range, for the
# `### Community` section of a release's changelog entry. Bots and the
# placeholder identity used by release tooling are excluded. Prints one
# `Name <email>` per line, sorted and de-duplicated.
#
# Usage: release-contributors.sh [<from-ref>] [<to-ref>]
#   Defaults to the previous tag..HEAD.
set -euo pipefail

from="${1:-}"
to="${2:-}"

if [ -z "$from" ]; then
  from="$(git describe --tags --abbrev=0 HEAD^ 2>/dev/null || true)"
fi
if [ -z "$to" ]; then
  to="HEAD"
fi

range="${from:+$from..}$to"

git log --format='%aN <%aE>' "$range" 2>/dev/null \
  | grep -viE '\[bot\]|dependabot|github-actions' \
  | grep -vF 'test <test@example.com>' \
  | sort -u || true
