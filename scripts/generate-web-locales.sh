#!/usr/bin/env bash
# Generate per-locale website pages from web/index.html plus the locale catalogs.
#
# The site has no build step: `web/**` is published verbatim by the Pages
# workflow, so localization has to produce real files rather than resolve strings
# at request time. Each locale gets `web/<tag>/index.html`, rewritten from the
# English source by replacing the exact text the extractor recorded.
#
# Replacement is positional and verified: the generator re-extracts the source,
# then requires every catalog key to be known before writing, so a catalog that
# has drifted from web/index.html fails here instead of shipping a half-English
# page.
#
# Usage: generate-web-locales.sh [--check]
#   (no args)  write web/<tag>/index.html for every locale that has a catalog
#   --check    fail if generated pages are missing or stale
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CHECK=0
if [[ "${1:-}" == "--check" ]]; then
  CHECK=1
fi

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
source_path = root / "web" / "index.html"
locales_dir = root / "web" / "locales"

source = source_path.read_text(encoding="utf-8")
english = json.loads((locales_dir / "en.json").read_text(encoding="utf-8"))

# The generator runs the extractor's own leaf discovery on the source, so the
# units it replaces are exactly the units the extractor recorded.
TAGS = (
    "h1", "h2", "h3", "h4", "p", "li", "button", "summary", "figcaption",
    "span", "td", "th", "label", "option", "title", "small", "strong", "em",
    "a", "dt", "dd", "div", "legend", "caption", "mark", "time",
)
SKIP = ("code", "pre", "kbd", "samp", "svg", "script", "style")

stripped = re.sub(r"<(script|style)\b.*?</\1>", "", source, flags=re.DOTALL | re.IGNORECASE)

# Offsets must be valid in the SAME string that receives the replacements, so the
# blanked copy keeps every removed character's length: `<script>` bodies become
# spaces rather than disappearing, and match positions stay true in `source`.
blanked = []
pos = 0
for removed in re.finditer(r"<(script|style)\b.*?</\1>", source, flags=re.DOTALL | re.IGNORECASE):
    blanked.append(source[pos : removed.start()])
    blanked.append(re.sub(r"[^\n]", " ", removed.group(0)))
    pos = removed.end()
blanked.append(source[pos:])
stripped = "".join(blanked)
assert len(stripped) == len(source), "the blanked copy must preserve every offset"

skip_ranges = [
    (m.start(), m.end())
    for m in re.finditer(
        r"<(code|pre|kbd|samp|svg)\b[^>]*>.*?</\1>", stripped, flags=re.DOTALL | re.IGNORECASE
    )
]


def inside_skip(pos: int) -> bool:
    return any(lo <= pos < hi for lo, hi in skip_ranges)


matches = []
for tag in TAGS:
    pattern = re.compile(r"<" + tag + r"(?P<attrs>[^>]*)>(?P<body>.*?)</" + tag + r">", re.DOTALL | re.IGNORECASE)
    for m in pattern.finditer(stripped):
        matches.append((m.start(), tag, m))
matches.sort(key=lambda item: item[0])

INLINE_ELEMENT = re.compile(
    r"<(?P<tag>a|code|strong|em|b|i|span|small)\b[^>]*>(?P<inner>.*?)</(?P=tag)>"
    r"|<(?P<void>br|wbr)\b[^>]*/?>",
    re.DOTALL | re.IGNORECASE,
)
BLOCK_WITHOUT_LINK = (
    r"<(div|section|article|ul|ol|table|thead|tbody|tr|nav|header|footer|form"
    r"|select|figure|details|dl|h1|h2|h3|h4|p|li|dt|dd)\b"
)
BLOCK_OR_LINK = (
    r"<(div|section|article|ul|ol|table|thead|tbody|tr|nav|header|footer|form"
    r"|select|figure|details|dl|h1|h2|h3|h4|p|li|dt|dd|a)\b"
)
LINK_OR_CODE = re.compile(r"<(?:a|code)\b", re.IGNORECASE)


PLACEHOLDER = re.compile(r"\{(\d+)\}")


def block_value(body):
    """A container's prose with each inline element replaced by {N}."""
    elements = list(INLINE_ELEMENT.finditer(body))
    pieces = []
    cursor = 0
    for index, element in enumerate(elements):
        pieces.append(body[cursor : element.start()])
        pieces.append("{%d}" % index)
        cursor = element.end()
    pieces.append(body[cursor:])
    text = html.unescape(re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", "".join(pieces))).strip())
    return text, elements


units = []
blocks = []
for index, (_pos, _tag, match) in enumerate(matches):
    if inside_skip(match.start()):
        continue
    body = match.group("body")
    if re.match(r"\s*<(code|pre|kbd|samp)\b", body, re.IGNORECASE):
        continue
    child_code = re.search(r"<(code|pre|kbd|samp)\b", body, re.IGNORECASE)
    has_block = re.search(BLOCK_OR_LINK, body, re.IGNORECASE)
    if has_block and (
        re.search(BLOCK_WITHOUT_LINK, body, re.IGNORECASE) or not LINK_OR_CODE.search(body)
    ):
        continue
    if child_code or LINK_OR_CODE.search(body):
        value, _elements = block_value(body)
        if len(value) < 3 or not re.search(r"\{\d+\}", value):
            continue
        blocks.append((f"blk.{len(blocks):03d}", match, value))
        continue
    value = html.unescape(re.sub(r"\s+", " ", re.sub(r"<[^>]+>", "", body)).strip())
    if len(value) < 3:
        continue
    units.append((f"idx.{index:03d}", match, value))

known = {key for key, _m, _v in units}
block_known = {key for key, _m, _v in blocks}
blocks_documented = {k for k in english["strings"] if k.startswith("blk.")}
if block_known != blocks_documented:
    sys.exit(
        "the English catalog's block keys do not match web/index.html; re-run "
        f"scripts/extract-web-strings.sh (missing={sorted(blocks_documented - block_known)[:5]} "
        f"extra={sorted(block_known - blocks_documented)[:5]})"
    )
documented = {k for k in english["strings"] if k.startswith("idx.")}
if known != documented:
    missing = sorted(documented - known)
    extra = sorted(known - documented)
    sys.exit(
        "the English catalog does not match web/index.html; re-run "
        f"scripts/extract-web-strings.sh (missing={missing[:5]} extra={extra[:5]})"
    )

attr_pattern = re.compile(r'(?P<attr>aria-label|placeholder|title|alt)\s*=\s*"(?P<value>[^"]{3,})"', re.IGNORECASE)
attr_units = []
for index, match in enumerate(attr_pattern.finditer(stripped)):
    value = html.unescape(match.group("value")).strip()
    if len(value) < 3:
        continue
    attr_units.append((f"attr.{index:03d}", match, value))

attr_documented = {k for k in english["strings"] if k.startswith("attr.")}
attr_known = {key for key, _m, _v in attr_units}
if attr_known != attr_documented:
    sys.exit(
        "the English catalog's attribute keys do not match web/index.html; re-run "
        f"scripts/extract-web-strings.sh (missing={sorted(attr_documented - attr_known)[:5]})"
    )

def render_block(match, translated, inner_translation):
    """Rebuild a block unit's inner HTML from its translated sentence.

    The value holds `{N}` where each inline element stood, so the prose translates
    as one sentence; here the element's own markup is restored around it, with an
    anchor's label taken from its own unit's translation. `<code>` has no unit by
    design (a command must stay byte-identical), so it is restored verbatim.
    """
    body_start, body_end = match.span("body")
    elements = list(INLINE_ELEMENT.finditer(stripped[body_start:body_end]))
    restored = {}
    for index, element in enumerate(elements):
        start = body_start + element.start()
        end = body_start + element.end()
        if element.group("void"):
            restored[index] = source[start:end]
            continue
        inner_start = body_start + element.start("inner")
        inner_end = body_start + element.end("inner")
        inner_html = source[inner_start:inner_end]
        nested = inner_translation.get((inner_start, inner_end))
        if nested is not None:
            inner_html = html.escape(nested)
        restored[index] = source[start:inner_start] + inner_html + source[inner_end:end]

    pieces = re.split(r"\{(\d+)\}", translated)
    out = []
    for index, piece in enumerate(pieces):
        if index % 2 == 0:
            out.append(html.escape(piece))
        else:
            out.append(restored.get(int(piece), ""))
    return "".join(out)


def render(tag, catalog):
    """Return the source with each recorded unit replaced by its translation."""
    out = source
    # Replace from the end so earlier offsets stay valid.
    replacements = []

    # Element units NEST: `<td><strong>$0/forever</strong></td>` yields a unit for
    # the cell AND one for the strong, 96 such pairs on this page. Applying both
    # corrupts the markup, because the outer span's offsets were measured against
    # the source and the inner replacement has since changed the length inside
    # it - so the outer replacement eats into a closing tag ("$0/para siemprerong>").
    # Only one unit per overlapping region may be applied:
    #   - an enclosing unit whose text is identical to the unit inside it adds
    #     nothing (the inner one renders the same words and KEEPS the markup,
    #     so bold stays bold), so the enclosing one stands down;
    #   - otherwise the enclosing one wins, because it carries the whole phrase
    #     and the inner text is only a fragment - keeping the inner one would
    #     leave the rest of the sentence in English.
    elements = []
    inner_translation = {
        match.span("body"): catalog[key]
        for key, match, english_text in units
        if catalog.get(key) is not None and catalog.get(key) != english_text
    }
    for key, match, english_text in units + blocks:
        translated = catalog.get(key)
        if translated is None or translated == english_text:
            continue
        if key.startswith("blk."):
            rewritten = render_block(match, translated, inner_translation)
        else:
            body = match.group("body")
            leading = re.match(r"\s*", body).group(0)
            trailing = re.search(r"\s*$", body).group(0)
            rewritten = leading + html.escape(translated) + trailing
        elements.append(
            {
                "key": key,
                "whole": match.span(),
                "span": match.span("body"),
                "text": english_text,
                "rewritten": rewritten,
            }
        )

    superseded = set()
    for inner in elements:
        for outer in elements:
            if inner is outer:
                continue
            if outer["whole"][0] <= inner["whole"][0] and inner["whole"][1] <= outer["whole"][1]:
                if inner["text"] == outer["text"]:
                    superseded.add(outer["key"])

    for element in sorted(
        (e for e in elements if e["key"] not in superseded),
        key=lambda e: (e["span"][0], -e["span"][1]),
    ):
        start, end = element["span"]
        if any(start < other_end and other_start < end for other_start, other_end, _ in replacements):
            continue  # overlaps a unit already accepted
        replacements.append((start, end, element["rewritten"]))

    for key, match, english_text in attr_units:
        translated = catalog.get(key)
        if translated is None or translated == english_text:
            continue
        start, end = match.span("value")
        assert not any(
            start < other_end and other_start < end for other_start, other_end, _ in replacements
        ), "an attribute replacement overlaps an element replacement"
        replacements.append((start, end, html.escape(translated, quote=True)))

    for start, end, text in sorted(replacements, key=lambda r: r[0], reverse=True):
        out = out[:start] + text + out[end:]

    # Point the document language and its alternates at the truth.
    out = out.replace('<html lang="en">', f'<html lang="{tag}">', 1)
    out = out.replace(
        "<head>",
        "<head>\n" + alternates_block(tag, catalog),
        1,
    )
    # The switcher sits beside the nav, which is where a reader looking for a
    # language expects to find it on an otherwise single-language page.
    nav_anchor = '<a href="#download" class="btn btn-sm btn-primary">'
    if nav_anchor in out:
        index = out.index(nav_anchor)
        out = out[:index] + language_switcher(tag) + out[index:]
    return localize_urls(out)


RELATIVE_URL = re.compile(
    r'(?P<prefix>\b(?:href|src|poster|action|data-src)\s*=\s*")(?P<url>[^"]*)"',
    re.IGNORECASE,
)


def is_rootless_url(url):
    """True for a URL that resolves against the page's own directory."""
    if not url or url.startswith(("#", "/", "..", "?")):
        return False
    return not re.match(r"[a-zA-Z][a-zA-Z0-9+.-]*:", url)  # scheme://, mailto:, data:


def localize_urls(markup):
    """Point root-relative references at the site root from a locale subdirectory.

    The site is published from `web/`, so a page written to `web/<tag>/index.html`
    that says `href="styles.css"` asks the browser for `/lightspeed/<tag>/styles.css`,
    which does not exist: the localized pages were served with no stylesheet, no
    icon and no script at all, while the English page looked fine. Each relative
    URL gains the `../` that makes it resolve to the shared file at the root.
    """

    def swap(match):
        url = match.group("url")
        if not is_rootless_url(url):
            return match.group(0)
        return match.group("prefix") + "../" + url + '"'

    return RELATIVE_URL.sub(swap, markup)


def unrooted_urls(markup):
    """Relative URLs left unrooted - every one of them is a 404 in production."""
    return [m.group("url") for m in RELATIVE_URL.finditer(markup) if is_rootless_url(m.group("url"))]


def stray_text_gt(markup):
    """Count '>' characters that are not part of a tag.

    A tag fragment left behind by a bad replacement (``...siemprerong>``) shows
    up here as text carrying a '>', which the source does not have. The check is
    on the output rather than on the replacement plan because it is the artifact
    that ships.
    """
    count = 0
    i = 0
    while True:
        opening = markup.find("<", i)
        if opening == -1:
            return count + markup.count(">", i)
        count += markup.count(">", i, opening)
        closing = markup.find(">", opening)
        if closing == -1:
            return count
        i = closing + 1


SOURCE_TEXT_GT = stray_text_gt(source)


def render_english(source):
    """The English page, carrying the two locale links the generated pages have.

    English is the source, so this output is NOT the file the extractor's keys
    point at - it goes to web/index.en.html, which the Pages workflow promotes
    over web/index.html at deploy time. Two additions, both required for
    discovery: the full hreflang alternate set, and the same no-JavaScript
    switcher the translated pages carry. Without them a reader (or a crawler)
    landing on `/lightspeed/` sees a single-language site and cannot reach any
    of the eight translated pages that already exist.
    """
    out = source.replace(
        "</head>",
        alternates_block("en", {}) + "\n</head>",
        1,
    )
    nav_anchor = '<a href="#download" class="btn btn-sm btn-primary">'
    if nav_anchor not in out:
        sys.exit("nav anchor not found in web/index.html; the switcher anchors to it")
    index = out.index(nav_anchor)
    return out[:index] + language_switcher("en") + out[index:]


LOCALE_NAMES = {
    "en": "English",
    "de": "Deutsch",
    "es": "Español",
    "fr": "Français",
    "ja": "日本語",
    "ko": "한국어",
    "pt-BR": "Português (Brasil)",
    "ru": "Русский",
    "zh-Hans": "简体中文",
}


class LanguageControl(str):
    """The switcher markup, rendered as a plain string."""


def language_switcher(current):
    """The language picker: a native select plus a no-JavaScript fallback.

    The select navigates from an inline `onchange`, so it needs JavaScript. The
    `<noscript>` list carries the same destinations as plain links for a reader
    whose browser blocks scripts. Only locales that actually have a generated
    page appear in either, so neither can offer a 404.
    """
    options = []
    links = []
    for info in locale_registry:
        tag = info["tag"]
        name = LOCALE_NAMES.get(tag, tag)
        selected = " selected" if tag == current else ""
        href = page_url(tag)
        options.append(
            f'<option value="{html.escape(href, quote=True)}"{selected}>{html.escape(name)}</option>'
        )
        aria = ' aria-current="page"' if tag == current else ""
        links.append(f'<a href="{html.escape(href, quote=True)}"{aria}>{html.escape(name)}</a>')
    label = html.escape(LOCALE_NAMES.get(current, current))
    return (
        '<noscript><style>.lang-switch{display:none}</style></noscript>'
        '<label class="lang-switch">'
        f'<span class="lang-switch-label">{label}</span>'
        '<select aria-label="Change language" onchange="if(this.value)location.href=this.value">'
        + "".join(options)
        + "</select></label>"
        + '<noscript><span class="lang-fallback">'
        + "".join(links)
        + "</span></noscript>"
    )


def alternates_block(current, _catalog):
    lines = []
    for info in locale_registry:
        tag = info["tag"]
        if tag == current:
            continue
        lines.append(
            f'  <link rel="alternate" hreflang="{tag}" href="{page_url(tag)}">'
        )
    lines.append(f'  <link rel="alternate" hreflang="x-default" href="{page_url("en")}">')
    return "\n".join(lines)


SITE_ORIGIN = "https://shibbityshwab.github.io/lightspeed"


def page_url(tag):
    """Absolute URL of a locale's page.

    hreflang requires absolute, crawlable URLs; an empty or relative href is a
    silent search-engine error, which is why these are built from the published
    origin rather than the repository's directory layout.
    """
    return f"{SITE_ORIGIN}/" if tag == "en" else f"{SITE_ORIGIN}/{tag}/"


def sitemap_xml():
    """The sitemap, built from the registry that generates the pages.

    Every locale page advertises its siblings through hreflang, but the sitemap
    listed only the root and the route visualizer, so the eight translated pages
    were absent from the file search engines actually read. Deriving it here keeps
    the two in step when a locale is added or removed.
    """
    urls = [SITE_ORIGIN + "/", SITE_ORIGIN + "/route-visualizer/"]
    urls += [
        page_url(info["tag"]) for info in locale_registry if info["tag"] != "en"
    ]
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">',
    ]
    for url in urls:
        lines += ["  <url>", f"    <loc>{url}</loc>", "  </url>"]
    lines.append("</urlset>")
    return "\n".join(lines) + "\n"


locale_registry = []
for path in sorted(locales_dir.glob("*.json")):
    if path.name == "en.json":
        locale_registry.append({"tag": "en", "path": path})
        continue
    locale_registry.append({"tag": path.stem, "path": path})

written = []
for info in locale_registry:
    tag = info["tag"]
    if tag == "en":
        continue
    catalog = json.loads(info["path"].read_text(encoding="utf-8"))
    target_html = render(tag, catalog.get("strings", {}))
    # A block value is a sentence with {N} where its links and inline commands
    # were. Losing or renumbering one silently drops a link or a command out of
    # the rendered page, so the placeholder set must survive translation exactly.
    for key, _match, english_value in blocks:
        translated = catalog.get(key)
        if translated is None or translated == english_value:
            continue
        if PLACEHOLDER.findall(translated) != PLACEHOLDER.findall(english_value):
            sys.exit(
                f"{tag}: {key} changed its placeholders. It must keep "
                f"{PLACEHOLDER.findall(english_value)} exactly: {translated[:90]!r}"
            )
    strays = stray_text_gt(target_html) - SOURCE_TEXT_GT
    if strays:
        sys.exit(
            f"{tag}: the rendered page left {strays} tag fragment(s) in the text "
            "(a replacement overlapped another); see the nesting note in render()"
        )
    unrooted = unrooted_urls(target_html)
    if unrooted:
        sys.exit(
            f"{tag}: {len(unrooted)} relative URL(s) would resolve under /{tag}/ and 404: "
            f"{sorted(set(unrooted))}"
        )
    target = root / "web" / tag / "index.html"
    if check:
        if not target.exists():
            sys.exit(f"{target} is missing; run scripts/generate-web-locales.sh")
        if target.read_text(encoding="utf-8") != target_html:
            sys.exit(f"{target} is stale; re-run scripts/generate-web-locales.sh")
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(target_html, encoding="utf-8")
        written.append(str(target.relative_to(root)))

# The English page is the source, so its localized build is written separately
# (web/index.en.html) rather than by rewriting the file the extractor's keys
# point at. The Pages workflow promotes it over web/index.html at deploy time.
english_target = root / "web" / "index.en.html"
english_html = render_english(source)
if check:
    if not english_target.exists():
        sys.exit(f"{english_target} is missing; run scripts/generate-web-locales.sh")
    if english_target.read_text(encoding="utf-8") != english_html:
        sys.exit(f"{english_target} is stale; re-run scripts/generate-web-locales.sh")
else:
    # newline="" keeps the generated page on the source's own LF endings: this
    # file is compared byte-for-byte by --check, and write_text would otherwise
    # translate \n to \r\n on Windows and fail the same check on Linux.
    english_target.write_text(english_html, encoding="utf-8", newline="")
    written.append(str(english_target.relative_to(root)))

sitemap_target = root / "web" / "sitemap.xml"
sitemap = sitemap_xml()
if check:
    if sitemap_target.read_text(encoding="utf-8") != sitemap:
        sys.exit(f"{sitemap_target} is stale; re-run scripts/generate-web-locales.sh")
else:
    sitemap_target.write_text(sitemap, encoding="utf-8", newline="")
    written.append(str(sitemap_target.relative_to(root)))

if check:
    print(f"{len(locale_registry) - 1} locale page(s) up to date")
else:
    for path in written:
        print(f"wrote {path}")
PY
