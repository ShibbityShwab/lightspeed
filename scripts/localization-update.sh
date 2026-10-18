#!/usr/bin/env bash
# Re-translate whatever the English copy changed, and fail if it is still stale.
#
# The retired check-only gate told whoever ran CI that the catalogs had drifted,
# and someone had to regenerate and retranslate by hand - which is why eight
# locales still said "22 games" after the count went to 24. This does the work
# first and then runs every existing check, so the gate can only pass in step.
#
# Only strings whose English actually moved are touched; unchanged translations
# are left byte-for-byte alone (scripts/localization-sync.py records the English
# hash of each one).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PY=""
for candidate in python3 py python; do
  if "$candidate" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)' >/dev/null 2>&1; then
    PY="$candidate"
    break
  fi
done
if [[ -z "$PY" ]]; then
  echo "no Python 3.8+ interpreter found (tried python3, py, python)" >&2
  exit 1
fi

echo "== regenerating the website catalog =="
bash scripts/extract-web-strings.sh

echo "== translating new or changed strings =="
"$PY" scripts/localization-sync.py --retranslate --report localization-report.md

echo "== rebuilding the pages the catalogs feed =="
bash scripts/generate-web-locales.sh

echo "== every existing localization check =="
bash scripts/extract-web-strings.sh --check
bash scripts/generate-web-locales.sh --check
"$PY" scripts/localized-docs.py --check
"$PY" scripts/localization-sync.py --check
