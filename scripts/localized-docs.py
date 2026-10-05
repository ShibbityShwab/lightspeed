#!/usr/bin/env python3
"""Put the "machine-assisted, unreviewed" warning on every translated doc page.

``docs/<page>.<tag>.md`` siblings are machine-assisted and no native speaker has
read them, but only ``docs/LANGUAGES.md`` said so - a reader who lands straight
on ``docs/faq.ja.md`` from a search result saw a confident assertion about
anti-cheat behaviour with nothing to indicate it was unreviewed. The banner goes
under each page's own title and links back to the English page, which stays
authoritative.

Usage: localized-docs.py [--check]
  (no args)  insert the banner into any localized page missing it
  --check    fail listing the pages that are missing it
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

WARNING = "> [!WARNING]"
BANNER = {
    "de": "Maschinell unterstützte Übersetzung, nicht von Muttersprachlern geprüft. "
          "Maßgeblich ist die [deutsche Fassung]({en}).",
    "es": "Traducción asistida por máquina, no revisada por un hablante nativo. "
          "La [versión en inglés]({en}) es la autoritativa.",
    "fr": "Traduction assistée par machine, non relue par un locuteur natif. "
          "La [version anglaise]({en}) fait foi.",
    "ja": "機械翻訳によるもので、ネイティブによる確認は行われていません。"
          "正式な内容は[英語版]({en})を参照してください。",
    "ko": "기계 번역이며 원어민의 검수를 받지 않았습니다. "
          "[영어 원문]({en})이 기준입니다.",
    "pt-BR": "Tradução assistida por máquina, não revisada por um falante nativo. "
             "A [versão em inglês]({en}) é a autoritativa.",
    "ru": "Машинный перевод, не проверенный носителем языка. "
          "Основным является [английский текст]({en}).",
    "zh-Hans": "机器翻译，未经母语者审核。以[英文版]({en})为准。",
}
TAGS = sorted(BANNER, key=len, reverse=True)


def split_name(path: Path) -> tuple[str, str] | None:
    """``docs/faq.ja.md`` -> ("faq", "ja"); ``docs/faq.md`` -> None."""
    stem = path.name[: -len(".md")]
    for tag in TAGS:
        if stem.endswith("." + tag):
            return stem[: -(len(tag) + 1)], tag
    return None


def banner_for(tag: str, english_page: str) -> str:
    return WARNING + "\n> " + BANNER[tag].format(en=english_page + ".md") + "\n"


def insert(text: str, banner: str) -> str:
    lines = text.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith("# "):
            return "".join(lines[: index + 1]) + "\n" + banner + "".join(lines[index + 1 :])
    return banner + "\n" + text


def main() -> int:
    check = "--check" in sys.argv
    docs = Path("docs")
    localized = []
    for path in sorted(docs.glob("*.md")):
        name = split_name(path)
        if name is not None:
            localized.append((path, *name))
    missing = []
    for path, base, tag in localized:
        text = path.read_text(encoding="utf-8")
        if WARNING in text:
            continue
        if check:
            missing.append(str(path))
            continue
        path.write_text(insert(text, banner_for(tag, base)), encoding="utf-8")
        print("banner added:", path)

    if check and missing:
        sys.exit(
            "localized pages missing the unreviewed-translation banner:\n  "
            + "\n  ".join(missing)
        )
    print(f"{len(localized)} localized page(s) checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
