#!/usr/bin/env python3
"""Render ``docs/*.md`` into a checkout of the GitHub wiki.

The wiki is a separate git repository, so this is a file generator: it copies the
flat ``docs/*.md`` set as wiki pages, maps ``docs/README.md`` onto the wiki's
``Home.md`` (the wiki front page), and rewrites the two link kinds that cannot
survive the move:

* ``[x](page.md)`` -> ``[x](page)``: wiki pages carry no extension.
* a link to something that is not a docs page - ``infra/README.md``, ``LICENSE``
  - becomes an absolute ``github.com/<repo>/blob/master/...`` URL, since that
  file does not exist in the wiki and 40 such links would otherwise 404.

Usage: sync-wiki.py <repo> <target-dir>
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

LINK = re.compile(r"\]\((?P<target>[^)\s]+)(?P<title>\s+\"[^\"]*\")?\)")
RAW_ASSET = re.compile(r"\.\./web/assets/")
SKIP_SCHEMES = ("http://", "https://", "#", "mailto:", "data:")


def rewrite(text: str, pages: set[str], blob_base: str, raw_base: str) -> str:
    def swap(match: re.Match[str]) -> str:
        target = match.group("target")
        title = match.group("title") or ""
        if target.startswith(SKIP_SCHEMES):
            return match.group(0)
        path, _, fragment = target.partition("#")
        stem = path[:-3] if path.endswith(".md") else path
        stem = stem.lstrip("./")
        if stem in pages:
            return f"]({stem}{('#' + fragment) if fragment else ''}{title})"
        return f"]({blob_base}/{path.lstrip('./')}{title})"

    return RAW_ASSET.sub(f"{raw_base}/web/assets/", LINK.sub(swap, text))


def main() -> int:
    repo, target = sys.argv[1], Path(sys.argv[2])
    docs = Path("docs")
    sources = sorted(docs.glob("*.md"))
    if not sources:
        sys.exit("no docs/*.md found; run this from the repository root")
    if not (docs / "README.md").exists():
        sys.exit("docs/README.md is missing; the wiki would have no Home page")

    pages = {p.stem for p in sources}
    blob_base = f"https://github.com/{repo}/blob/master"
    raw_base = f"https://raw.githubusercontent.com/{repo}/master"

    for source in sources:
        target_page = "Home" if source.stem == "README" else source.stem
        body = rewrite(source.read_text(encoding="utf-8"), pages, blob_base, raw_base)
        (target / f"{target_page}.md").write_text(body, encoding="utf-8")
        print(f"wrote {target_page}.md from {source}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
