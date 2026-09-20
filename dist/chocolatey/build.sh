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

VERSION="$(grep -oPm1 '(?<=<version>)[^<]+' lightspeed.nuspec)"
OUT="${1:-lightspeed.${VERSION}.nupkg}"

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
