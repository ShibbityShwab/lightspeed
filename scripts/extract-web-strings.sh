#!/usr/bin/env bash
# Extract the website's visible copy into a keyed catalog.
#
# The site is hand-written static HTML with no build step, so localization is
# done by generating per-locale pages from the English source rather than by
# editing every string in place. Keys are positional (`idx.NNN`) because the
# HTML has no stable ids on most text nodes; that makes the catalog immune to
# reordering prose within a page but sensitive to inserting new text nodes,
# which is why the generator verifies the key count against the source before
# writing anything.
#
# Usage: extract-web-strings.sh [--check]
#   (no args)  write web/locales/en.json and a key list
#   --check    fail if web/locales/en.json is out of date with web/index.html
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CHECK=0
if [[ "${1:-}" == "--check" ]]; then
  CHECK=1
fi

# CI is Linux (python3); this host is Windows, where python3 is the Microsoft
# Store stub while the real interpreter answers to `py`. Resolve once so the
# same script runs on both instead of failing on whichever name is absent.
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

"$PY" - "$CHECK" "$ROOT" <<'PY'
import html
import json
import pathlib
import re
import sys

check = sys.argv[1] == "1"
root = pathlib.Path(sys.argv[2])
source = root / "web" / "index.html"
out_path = root / "web" / "locales" / "en.json"

text = source.read_text(encoding="utf-8")

# A container holding block children is not a leaf string; its text is
# captured from those children instead, and copying it here would both
# duplicate the copy and flatten the markup the page depends on. A container of
# pure inline content (`<div><svg/> Ready</div>`) is a leaf and is captured.
INLINE_ONLY = re.compile(
    r"<(svg|img|use|path|br|wbr|i|b|u|sup|sub|abbr|input)\b[^>]*/?>",
    re.IGNORECASE,
)

# Tags whose direct text children are copy a reader sees. Deliberately excludes
# script/style (code, not prose) and anything inside them.
TAGS = (
    "h1", "h2", "h3", "h4", "p", "li", "button", "summary", "figcaption",
    "span", "td", "th", "label", "option", "title", "small", "strong", "em",
    "a", "dt", "dd", "div", "legend", "caption", "mark", "time",
)
# Elements that are deliberately NOT localized: package-manager commands,
# terminal one-liners and the protocol/CLI syntax around them. Translating an
# `install` command would break copy-paste, which is the whole point of it.
stripped = re.sub(r"<(script|style)\b.*?</\1>", "", text, flags=re.DOTALL | re.IGNORECASE)

# Character ranges covered by a skip element, so a leaf inside <code> is skipped
# while a paragraph that merely *contains* inline <code> is still captured.
skip_ranges = []
for element in re.finditer(
    r"<(code|pre|kbd|samp|script|style|svg)\b[^>]*>.*?</\1>",
    stripped,
    flags=re.DOTALL | re.IGNORECASE,
):
    skip_ranges.append((element.start(), element.end()))


def inside_skip(start: int) -> bool:
    return any(lo <= start < hi for lo, hi in skip_ranges)


# `\b` after the tag name is load-bearing: without it `<p` also matches `<path`,
# `<a` matches `<article`, `<th` matches `<thead` and `<li` matches `<link`. A
# bogus match also swallows the next real one of that name, because scanning
# resumes after it - a `<path>` read as `<p>` ran to the following `</p>` and hid
# the privacy-notice paragraph entirely.
tag_res = [
    (
        tag,
        re.compile(
            r"<" + tag + r"\b(?P<attrs>[^>]*)>(?P<body>.*?)</" + tag + r">",
            re.DOTALL | re.IGNORECASE,
        ),
    )
    for tag in TAGS
]

# Attributes that are read aloud or shown in place of missing text.
ATTR_RE = re.compile(
    r'(?P<attr>aria-label|placeholder|title|alt)\s*=\s*"(?P<value>[^"]{3,})"',
    re.IGNORECASE,
)

# A container whose body holds an inline element the skip rules would otherwise
# make it stand down for is captured as a *block* unit instead: its prose becomes
# the translatable value with each inline element replaced by {N}, and the
# element's own markup is restored around the translation at render time. Without
# this, any paragraph containing a link or inline <code> stayed English.
INLINE_ELEMENT = re.compile(
    r"<(?P<tag>a|code|strong|em|b|i|span|small)\b[^>]*>(?P<inner>.*?)</(?P=tag)>"
    r"|<(?P<void>br|wbr)\b[^>]*/?>",
    re.DOTALL | re.IGNORECASE,
)
# The block-children list minus `a`: a container with a real block child is still
# not a leaf, but one whose only "block" child is a link is.
BLOCK_WITHOUT_LINK = (
    r"<(div|section|article|ul|ol|table|thead|tbody|tr|nav|header|footer|form"
    r"|select|figure|details|dl|h1|h2|h3|h4|p|li|dt|dd)\b"
)
LINK_OR_CODE = re.compile(r"<(?:a|code)\b", re.IGNORECASE)


catalog = {}
order = []
markup = {}
blocks = {}


def clean(body: str) -> str:
    """Reduce a tag body to its visible text, keeping inline markup readable."""
    no_tags = re.sub(r"<[^>]+>", "", body)
    collapsed = re.sub(r"\s+", " ", no_tags).strip()
    return html.unescape(collapsed)


def block_value(body: str):
    """A container's prose with every inline element replaced by {N}.

    Returns the value and the ordered elements, so the renderer can restore each
    one's markup around the translated sentence instead of flattening it.
    """
    elements = list(INLINE_ELEMENT.finditer(body))
    pieces = []
    cursor = 0
    for index, element in enumerate(elements):
        pieces.append(body[cursor : element.start()])
        pieces.append("{%d}" % index)
        cursor = element.end()
    pieces.append(body[cursor:])
    value = clean("".join(pieces))
    return value, elements


matches = []
for tag, pattern in tag_res:
    for match in pattern.finditer(stripped):
        matches.append((match.start(), tag, match))
matches.sort(key=lambda item: item[0])

# The `\b` in tag_res is load-bearing; this asserts the invariant rather than
# trusting the pattern, because a prefix match also swallows the next real match
# of that name and so hides whole paragraphs from extraction.
prefix_hits = sorted(
    {
        tag
        for _pos, tag, match in matches
        if match.group("attrs")[:1] not in ("", " ", "/", chr(9), chr(10), chr(13))
    }
)
if prefix_hits:
    sys.exit(
        "tag match without a name boundary, so a longer element was matched as this one: "
        + ", ".join("<" + tag for tag in prefix_hits)
    )

for index, (_pos, _tag, match) in enumerate(matches):
    if inside_skip(match.start()):
        continue
    body = match.group("body")
    # An element wrapping a code block is a command the reader copies verbatim,
    # not prose: localizing it would break the paste. This catches the case the
    # skip ranges miss, where `<code>` is a *child* rather than the whole leaf.
    child_code = re.search(r"<(code|pre|kbd|samp)\b", body, re.IGNORECASE)
    own_code = re.match(r"\s*<(code|pre|kbd|samp)\b", body, re.IGNORECASE)
    if own_code:
        continue
    # A container holding block children is not a leaf string; its text is
    # captured from those children instead, and copying it here would both
    # duplicate the copy and flatten the markup the page depends on.
    has_block = re.search(
        r"<(div|section|article|ul|ol|table|thead|tbody|tr|nav|header|footer|form"
        r"|select|figure|details|dl|h1|h2|h3|h4|p|li|dt|dd|a)\b",
        body,
        re.IGNORECASE,
    )
    if has_block and (re.search(BLOCK_WITHOUT_LINK, body, re.IGNORECASE) or not LINK_OR_CODE.search(body)):
        continue
    if child_code or LINK_OR_CODE.search(body):
        # Prose that merely carries inline markup: the value keeps {N} where each
        # element was, so the translation is a whole sentence and the element's
        # markup (a link, an inline command) is restored around it at render.
        value, _elements = block_value(body)
        if len(value) < 3 or not re.search(r"\{\d+\}", value):
            continue
        key = f"blk.{len(blocks):03d}"
        blocks[key] = value
        markup[key] = re.sub(r"\s+", " ", body).strip()
        order.append(key)
        continue
    value = clean(body)
    if len(value) < 3:
        continue
    key = f"idx.{index:03d}"
    catalog[key] = value
    # Keep the markup-bearing original so the generator can reinsert a
    # translation with its inline tags (`<a>`, `<strong>`, `<code>`) intact.
    markup[key] = re.sub(r"\s+", " ", body).strip()
    order.append(key)

attr_catalog = {}
for index, match in enumerate(ATTR_RE.finditer(stripped)):
    value = html.unescape(match.group("value")).strip()
    if len(value) < 3:
        continue
    key = f"attr.{index:03d}"
    attr_catalog[key] = value

catalog.update(attr_catalog)
catalog.update(blocks)

payload = {
    "generated_from": "web/index.html",
    "note": "Source locale. Keys are positional; regenerate with scripts/extract-web-strings.sh",
    "strings": catalog,
    "markup": markup,
    "block_placeholders": True,
    "skipped_elements": ["code", "pre", "kbd", "samp", "svg", "script", "style"],
}

rendered = json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True) + "\n"

if check:
    if not out_path.exists():
        sys.exit(f"{out_path} is missing; run scripts/extract-web-strings.sh")
    if out_path.read_text(encoding="utf-8") != rendered:
        sys.exit(f"{out_path} is out of date with web/index.html; re-run the extractor")
    print(f"web catalog up to date ({len(catalog)} keys)")
    sys.exit(0)

out_path.parent.mkdir(parents=True, exist_ok=True)
out_path.write_text(rendered, encoding="utf-8")
print(f"wrote {out_path} with {len(catalog)} keys")
PY
