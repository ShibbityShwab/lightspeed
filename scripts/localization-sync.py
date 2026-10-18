#!/usr/bin/env python3
"""Keep every machine-assisted translation in step with its English source.

The site, the docs and the GUI catalogs were translated once, by hand, at the
moment a feature landed. That is why eight website locales still said "22 games"
after ARC Raiders took the count to 24: the English changed, the translations did
not, and nothing noticed. This makes that step a mechanism instead of a chore.

Per translated unit (website catalog keys and every ``docs/<page>.<tag>.md``):

- a unit whose English still matches the hash recorded for it is left
  byte-for-byte alone, hand-tuned wording included;
- a unit with no current translation is translated once and its English hash is
  recorded in ``scripts/localization-state.json``;
- a unit whose English moved is reported, and retranslated only with
  ``--retranslate``, because an edited English sentence deserves a look before
  eight languages change under it.

Placeholders (``{0}``, ``{1}`` ...) become single private-use characters before
translation - one character, not a token ending in a digit, because a translator
reliably renumbers anything that looks like a number - and are verified after:
a dropped placeholder would silently remove a link or a command from a page.

The translator is a free, account-less public service, because ``[COST_STUB]``
rules out a paid API and an account-bound key in a CI secret is one more thing
that silently expires. Public instances come and go (one measured here answered
403 to everything), so the list is tried in order and the first that answers is
kept; ``LIGHTSPEED_TRANSLATE_URL`` or ``--endpoint`` points it at a self-hosted
instance when one exists. With none reachable the script still reports what is
stale, which is the part CI needs.

Usage:
  localization-sync.py --check          report drift only (CI); exit 1 on drift
  localization-sync.py                  translate what is new, then report drift
  localization-sync.py --retranslate    also retranslate units whose source changed
  localization-sync.py --only web|docs  restrict to one surface
  localization-sync.py --endpoint URL   prepend a LibreTranslate endpoint
  localization-sync.py --report FILE    write the changed units as markdown
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
STATE_PATH = ROOT / "scripts" / "localization-state.json"
EN_CATALOG = ROOT / "web" / "locales" / "en.json"
LOCALES_DIR = ROOT / "web" / "locales"
DOCS = ROOT / "docs"

DEFAULT_ENDPOINTS = [
    "https://translate.googleapis.com/translate_a/single",
    "https://translate.fedilab.app",
    "https://lingva.ml/api/v1",
]

DOC_TAGS = ["de", "es", "fr", "ja", "ko", "pt-BR", "ru", "zh-Hans"]

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
TAGS_BY_LENGTH = sorted(BANNER, key=len, reverse=True)

PLACEHOLDER = re.compile(r"\{\d+\}")
TOKEN = re.compile("[\ue000-\ue0ff]")
FENCE = re.compile(r"```[a-zA-Z]*\n(.*?)```", re.DOTALL)
EXECUTABLE = re.compile(r"^[./]?[a-z][a-z0-9_.+-]*$")

SKIP_SHORTER_THAN = 3


class Translator:
    def __init__(self, endpoints: list[str]) -> None:
        self.endpoints = [
            url if url.endswith("/translate_a/single") else url.rstrip("/") + "/translate"
            for url in endpoints
        ]
        self.working: str | None = None

    def call(self, url: str, text: str, target: str) -> str:
        if url.endswith("/translate_a/single"):
            return self.call_unofficial(url, text, target)
        payload = json.dumps(
            {"q": text, "source": "en", "target": target, "format": "text"}
        ).encode("utf-8")
        request = urllib.request.Request(
            url,
            data=payload,
            headers={"Content-Type": "application/json", "Accept": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=15) as response:
            body = json.loads(response.read().decode("utf-8"))
        translated = body.get("translatedText")
        if not isinstance(translated, str):
            raise ValueError(f"no translatedText in {body!r}")
        return translated

    def call_unofficial(self, url: str, text: str, target: str) -> str:
        query = urllib.parse.urlencode(
            {"client": "gtx", "sl": "en", "tl": target, "dt": "t", "q": text}
        )
        with urllib.request.urlopen(f"{url}?{query}", timeout=20) as response:
            body = json.loads(response.read().decode("utf-8"))
        pieces = [segment[0] for segment in body[0] if segment and segment[0]]
        if not pieces:
            raise ValueError(f"no translation in {str(body)[:120]}")
        return "".join(pieces)

    def translate(self, text: str, target: str) -> str:
        if not TOKEN.sub("", text).strip() and TOKEN.search(text):
            return text
        if self.working is not None:
            return self.call(self.working, text, target)
        last: Exception | None = None
        for url in self.endpoints:
            try:
                result = self.call(url, text, target)
            except (urllib.error.URLError, TimeoutError, ValueError, OSError) as error:
                last = error
                print(f"  endpoint {url} unavailable: {error}", file=sys.stderr)
                continue
            self.working = url
            return result
        raise urllib.error.URLError(last) if last else ValueError("no translation endpoint")


def source_hash(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:16]


def split_tag(path: Path) -> tuple[str, str] | None:
    stem = path.name[: -len(".md")]
    for tag in TAGS_BY_LENGTH:
        if stem.endswith("." + tag):
            return stem[: -(len(tag) + 1)], tag
    return None


def code_commands(text: str) -> list[str]:
    out = []
    for block in FENCE.findall(text):
        for line in block.splitlines():
            stripped = line.split("#")[0].strip()
            if stripped and stripped.isascii() and EXECUTABLE.match(stripped.split()[0]):
                out.append(stripped)
    return out


def protect(text: str) -> tuple[str, list[str]]:
    slots: list[str] = []

    def swap(match: re.Match[str]) -> str:
        slots.append(match.group(0))
        return chr(0xE000 + len(slots) - 1)

    return PLACEHOLDER.sub(swap, text), slots


def restore(text: str, slots: list[str]) -> str | None:
    found: list[int] = []
    out: list[str] = []
    cursor = 0
    for match in TOKEN.finditer(text):
        index = ord(match.group(0)) - 0xE000
        out.append(text[cursor : match.start()])
        if index < len(slots):
            out.append(slots[index])
            found.append(index)
        cursor = match.end()
    out.append(text[cursor:])
    if sorted(found) != list(range(len(slots))):
        return None
    return "".join(out)


def translate_value(translator: Translator, text: str, target: str) -> str | None:
    protected, slots = protect(text)
    try:
        translated = translator.translate(protected, target)
    except (urllib.error.URLError, TimeoutError, ValueError, OSError) as error:
        print(f"  translation unavailable for {target}: {error}", file=sys.stderr)
        return None
    restored = restore(translated, slots)
    if restored is None:
        print(f"  placeholder lost in {target}: {translated[:80]!r}", file=sys.stderr)
    return restored


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def dump_json(path: Path, data: Any) -> None:
    path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def banner_for(tag: str, english_page: str) -> str:
    return WARNING + "\n> " + BANNER[tag].format(en=english_page + ".md") + "\n"


def insert_banner(text: str, banner: str) -> str:
    lines = text.splitlines(keepends=True)
    for index, line in enumerate(lines):
        if line.startswith("# "):
            return "".join(lines[: index + 1]) + "\n" + banner + "".join(lines[index + 1 :])
    return banner + "\n" + text


def strip_banner(text: str, tag: str, base: str) -> str:
    return text.replace(banner_for(tag, base) + "\n", "", 1)


def sync_web(
    args: argparse.Namespace,
    translator: Translator,
    state: dict[str, Any],
    changed: list[tuple[str, str, str, str]],
) -> list[str]:
    english = load_json(EN_CATALOG)
    strings: dict[str, str] = english.get("strings", {})
    report: list[str] = []
    web_state = state.setdefault("web", {})
    for tag in DOC_TAGS:
        path = LOCALES_DIR / f"{tag}.json"
        if not path.exists():
            report.append(f"web/{tag}: catalog missing")
            continue
        catalog = load_json(path)
        translated: dict[str, str] = catalog.setdefault("strings", {})
        tag_state: dict[str, str] = web_state.setdefault(tag, {})
        for orphan in sorted(key for key in translated if key not in strings):
            report.append(f"web/{tag}: {orphan} stale")
            if not args.check:
                del translated[orphan]
                tag_state.pop(orphan, None)
        dirty = False
        for key, english_text in sorted(strings.items()):
            current = source_hash(english_text)
            have = translated.get(key)
            recorded = tag_state.get(key)
            if have is not None and (recorded == current or recorded is None):
                if recorded is None:
                    tag_state[key] = current
                    dirty = True
                continue
            if have is None:
                report.append(f"web/{tag}: {key} untranslated")
            elif recorded != current:
                if not args.retranslate:
                    report.append(f"web/{tag}: {key} source changed")
                    continue
                report.append(f"web/{tag}: {key} retranslated")
            if args.check or len(english_text.strip()) < SKIP_SHORTER_THAN:
                continue
            result = translate_value(translator, english_text, tag)
            if result is None:
                continue
            translated[key] = result
            tag_state[key] = current
            changed.append((tag, key, english_text, result))
            dirty = True
        if dirty and not args.check:
            dump_json(path, catalog)
    return report


def sync_docs(
    args: argparse.Namespace,
    translator: Translator,
    state: dict[str, Any],
    changed: list[tuple[str, str, str, str]],
) -> list[str]:
    report: list[str] = []
    docs_state: dict[str, str] = state.setdefault("docs", {})
    for path in sorted(DOCS.glob("*.md")):
        pair = split_tag(path)
        if pair is None:
            continue
        base, tag = pair
        english_path = DOCS / f"{base}.md"
        if not english_path.exists():
            report.append(f"{path.name}: no English page")
            continue
        english_text = english_path.read_text(encoding="utf-8")
        current = source_hash(english_text)
        recorded = docs_state.get(path.name)
        if recorded == current:
            continue
        existing = path.read_text(encoding="utf-8")
        same_body = strip_banner(existing, tag, base) == english_text or existing == english_text
        if args.check:
            if recorded is None and not same_body:
                continue
            report.append(f"{path.name}: {'untranslated' if same_body else 'source changed'}")
            continue
        if not same_body and recorded is None:
            docs_state[path.name] = current
            report.append(f"{path.name}: baseline recorded from existing page")
            continue
        if not same_body and not args.retranslate:
            report.append(f"{path.name}: source changed")
            continue
        result = translate_value(translator, english_text, tag)
        if result is None:
            report.append(f"{path.name}: translation unavailable, left as it was")
            continue
        if sorted(code_commands(result)) != sorted(code_commands(english_text)):
            report.append(f"{path.name}: translation altered a command, left as it was")
            continue
        if not result.lstrip().startswith("# "):
            result = insert_banner(result, banner_for(tag, base))
        path.write_text(result, encoding="utf-8")
        docs_state[path.name] = current
        report.append(f"{path.name}: {'retranslated' if not same_body else 'translated'}")
        changed.append((tag, path.name, english_text[:400], result[:400]))
    return report


def write_report(path: Path, changed: list[tuple[str, str, str, str]]) -> None:
    if not changed:
        return
    lines = [
        "# Machine-assisted translations changed by this update",
        "",
        "These were translated because their English source is new or moved. They are",
        "UNREVIEWED. Blocking a pull request on language quality would be review",
        "theatre nobody can perform reliably; this report is how a native speaker can",
        "find the exact strings worth their time.",
        "",
    ]
    for tag, key, english, translated in changed:
        lines += [f"## {tag} · `{key}`", "", f"- EN: {english}", f"- {tag}: {translated}", ""]
    path.write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Keep machine-assisted translations in step with English"
    )
    parser.add_argument("--check", action="store_true", help="report drift only; exit 1 on drift")
    parser.add_argument("--retranslate", action="store_true", help="also retranslate changed sources")
    parser.add_argument("--only", choices=["web", "docs"], default=None)
    parser.add_argument(
        "--endpoint",
        action="append",
        default=None,
        help="LibreTranslate endpoint(s) to try; LIGHTSPEED_TRANSLATE_URL wins",
    )
    parser.add_argument("--report", default=None, help="write the changed units as markdown")
    args = parser.parse_args()

    env = os.environ.get("LIGHTSPEED_TRANSLATE_URL")
    endpoints = args.endpoint or ([env] if env else []) or DEFAULT_ENDPOINTS
    translator = Translator(endpoints)

    state: dict[str, Any] = load_json(STATE_PATH) if STATE_PATH.exists() else {}
    report: list[str] = []
    changed: list[tuple[str, str, str, str]] = []
    if args.only != "docs":
        report += sync_web(args, translator, state, changed)
    if args.only != "web":
        report += sync_docs(args, translator, state, changed)
    if not args.check:
        dump_json(STATE_PATH, state)
    if args.report:
        write_report(Path(args.report), changed)

    for line in report:
        print(line)
    blocking = [line for line in report if "baseline recorded" not in line]
    if blocking:
        print(f"{len(blocking)} localization item(s) need attention", file=sys.stderr)
        return 1 if args.check else 0
    print("localization is in step with English")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
