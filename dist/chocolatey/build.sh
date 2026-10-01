#!/usr/bin/env bash
# Build lightspeed.<version>.nupkg from the nuspec + tools.
#
# A .nupkg is an OPC zip, not a plain archive: it must contain the .nuspec plus
# [Content_Types].xml and _rels/.rels at the root. Without those parts NuGet
# rejects the push with "Package does not contain a manifest", which is exactly
# what a hand-zipped nuspec does.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# POSIX extraction: `grep -oP` is a GNU extension and fails on BSD/macOS grep,
# where the README tells users to build from source. sed is portable.
VERSION="$(sed -n 's:.*<version>\([^<]*\)</version>.*:\1:p' lightspeed.nuspec | head -1)"
if [ -z "$VERSION" ]; then
  echo "build.sh: could not read <version> from lightspeed.nuspec" >&2
  exit 1
fi
OUT="${1:-lightspeed.${VERSION}.nupkg}"

# `command -v` is not enough: on Windows a Microsoft Store alias stub can
# satisfy it while executing nothing. Require python3 to actually run.
if ! python3 -c 'import sys, zipfile' >/dev/null 2>&1; then
  echo "build.sh: python3 (with the zipfile module) is required to pack the .nupkg" >&2
  exit 1
fi

python3 - "$OUT" <<'PY'
import sys
import zipfile

out = sys.argv[1]
nuspec = open("lightspeed.nuspec").read()
install = open("tools/chocolateyInstall.ps1").read()

content_types = (
    '<?xml version="1.0" encoding="utf-8"?>\n'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">\n'
    '  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml" />\n'
    '  <Default Extension="nuspec" ContentType="application/octet" />\n'
    '  <Default Extension="ps1" ContentType="application/octet" />\n'
    '</Types>\n'
)
rels = (
    '<?xml version="1.0" encoding="utf-8"?>\n'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">\n'
    '  <Relationship Type="http://schemas.microsoft.com/packaging/2010/07/manifest"'
    ' Target="/lightspeed.nuspec" Id="R0" />\n'
    '</Relationships>\n'
)

with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("lightspeed.nuspec", nuspec)
    z.writestr("[Content_Types].xml", content_types)
    z.writestr("_rels/.rels", rels)
    z.writestr("tools/chocolateyInstall.ps1", install)

print("built", out)
PY
