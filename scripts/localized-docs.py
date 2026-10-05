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

FENCE = re.compile(r"```[a-zA-Z]*\n(.*?)```", re.DOTALL)
# A fenced-block line is a command - something the reader copies and runs - when
# its first token looks like an executable and the line is pure ASCII. That keeps
# the ASCII-art diagrams and prose labels out, which the translations legitimately
# rewrite.
EXECUTABLE = re.compile(r"^[./]?[a-z][a-z0-9_.+-]*$")


def code_commands(path: Path) -> list[str]:
    """The runnable command lines inside a page's fenced code blocks."""
    out = []
    for block in FENCE.findall(path.read_text(encoding="utf-8")):
        for line in block.splitlines():
            stripped = line.split("#")[0].strip()
            if not stripped or not stripped.isascii():
                continue
            if EXECUTABLE.match(stripped.split()[0]):
                out.append(stripped)
    return out


def commands_differ(english: Path, translated: Path) -> list[tuple[str, str]]:
    """Commands the translation lost or altered, as (english, translated) pairs."""
    want = code_commands(english)
    got = set(code_commands(translated))
    return [(c, c) for c in want if c not in got]


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
    root = Path(__file__).resolve().parent.parent
    docs = root / "docs"
    localized = []
    for path in sorted(docs.glob("*.md")):
        name = split_name(path)
        if name is not None:
            localized.append((path, *name))
    missing = []
    broken_commands = []
    for path, base, tag in localized:
        # A command a reader copies and runs must survive translation byte for
        # byte: a mangled install line is silently destructive in a way a clumsy
        # sentence is not, and nothing validated these before.
        altered = commands_differ(docs / f"{base}.md", path)
        if altered:
            broken_commands.append(f"{path}: {'; '.join(a for a, _ in altered[:3])}")
        text = path.read_text(encoding="utf-8")
        if WARNING in text:
            continue
        if check:
            missing.append(str(path))
            continue
        path.write_text(insert(text, banner_for(tag, base)), encoding="utf-8")
        print("banner added:", path)

    if broken_commands:
        sys.exit(
            "translated pages altered a command the reader is meant to run:\n  "
            + "\n  ".join(broken_commands[:8])
        )

    if check and missing:
        sys.exit(
            "localized pages missing the unreviewed-translation banner:\n  "
            + "\n  ".join(missing)
        )
    print(f"{len(localized)} localized page(s) checked")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
