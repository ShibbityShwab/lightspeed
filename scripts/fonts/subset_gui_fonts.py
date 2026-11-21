"""Regenerate the bundled CJK UI-font subsets from pinned OFL sources.

LightSpeed bundles three Noto Sans CJK faces (Japanese, Korean, Simplified
Chinese - regular and bold). They are license-clean subset copies of the
upstream Noto CJK *language-specific* OTFs: shipping the full faces would add
~28 MB to the binary for ~1,500 characters of UI text, so the assets are cut to
the characters the nine locale catalogs and the language picker can actually
produce.

Why this script exists: Q19 in the GUI-refresh close-out. The previous cut was
made ad hoc and the language picker's own names (``한국어``, ``简体中文``) were
never covered - ``국``, ``简`` and ``体`` had no bundled face, so the picker drew
them through device-dependent system-font fallback (or tofu). Regenerating a
subset by hand is not reproducible and a hand-edited binary has no provenance,
so this script replaces that with a pinned, deterministic pipeline:

  source  notofonts/noto-cjk tag ``Sans2.004`` (SIL OFL 1.1), URL + SHA-256 pinned
  tool    fontTools ``subset`` (MIT); pin the version printed by ``--version``
  policy  each face = its regional catalog characters
                      + every CJK character in the nine ``native_name`` strings
                      + the typed-symbol allowlist
                      + a Latin/punctuation safety set
  output  byte-identical on every run with the same fontTools version

Usage (Windows host, from the repo root):

    py -3 scripts/fonts/subset_gui_fonts.py --check
        Verify the shipped assets against ``client-gui/assets/fonts/subset.json``
        (SHA-256, size, coverage of the locale catalogs) and verify the
        coverage policy. Needs no fontTools and no network: CI-able.

    py -3 -m pip install --user fonttools==4.68.0
    py -3 scripts/fonts/subset_gui_fonts.py --write --sources <dir>
        Re-cut the six faces from source files already in ``<dir>`` (or omit
        ``--sources`` to download the pinned URLs into
        ``target/font-in-sources``). Rewrites the assets and the manifest.

    py -3 scripts/fonts/subset_gui_fonts.py --write --dry-run
        Print what would change without writing.

The manifest is the contract: ``--write`` refuses to drop a codepoint that a
previous manifest carried (the subset is a ratchet), and ``--check`` fails if
an asset's hash no longer matches it. Never hand-edit a bundled face.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import sys
import tomllib
import urllib.request

REPO = pathlib.Path(__file__).resolve().parents[2]
FONT_DIR = REPO / "client-gui" / "assets" / "fonts"
LOCALE_DIR = REPO / "client-gui" / "locales"
I18N = REPO / "client-gui" / "src" / "i18n.rs"
MANIFEST = FONT_DIR / "subset.json"

TOOL_NAME = "fonttools"
TOOL_VERSION = "4.68.0"
TOOL_LICENSE = "MIT"
TOOL_HOMEPAGE = "https://github.com/fonttools/fonttools"

SOURCE_REPO = "notofonts/noto-cjk"
SOURCE_TAG = "Sans2.004"
SOURCE_BASE = f"https://raw.githubusercontent.com/{SOURCE_REPO}/{SOURCE_TAG}/Sans/SubsetOTF"
SOURCE_LICENSE = "OFL-1.1"
SOURCE_LICENSE_FILE = "OFL-NotoSansCJK.txt"
SOURCE_LICENSE_SHA256 = "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2"

# The six bundled faces: (asset name, region, source sha256). The hashes pin the
# exact upstream bytes; `--write` refuses a source that does not match.
FACES = [
    ("NotoSansJP-Regular.otf", "JP", "dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073"),
    ("NotoSansJP-Bold.otf", "JP", "1b0edfb500b73a4fa8a4fcaae1bbbd403994e08e73e3e0da37e70d3853f42c5f"),
    ("NotoSansKR-Regular.otf", "KR", "69975a0ac8472717870aefeab0a4d52739308d90856b9955313b2ad5e0148d68"),
    ("NotoSansKR-Bold.otf", "KR", "5a6ceb287ed2fc6cfc6213144ebea68cbd94b20fc9eb873d8486493bf02d9bda"),
    ("NotoSansSC-Regular.otf", "SC", "faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9"),
    ("NotoSansSC-Bold.otf", "SC", "c6cb5a93abaa9edc8ee7463b7ebb7f42d618d40e6ed2f7a5371c97b0b64767c0"),
]

REGION_LOCALE = {"JP": "ja", "KR": "ko", "SC": "zh-Hans"}

# The typed non-ASCII marks the design system allows in the window
# (`docs/design/design-system-2026.md` §8.3). The rest are painted shapes.
TYPED_MARKS = "\u00b7\u00d7\u2014\u2026\u2192"  # · × — … →

# The Latin/punctuation safety set the previous cut carried and that dynamic
# strings (relay cities, server names, OS-provided names) can still reach.
SAFETY_RANGES = [
    (0x0020, 0x007E),  # printable ASCII
    (0x00A0, 0x00FF),  # Latin-1 supplement
]
SAFETY_CHARS = "\u2013\u2018\u2019\u201c\u201d\u2022\u2212\u2500\u25cf\u2713"  # – ‘ ’ “ ” • − ─ ● ✓

# The subset options. `layout_features=[]` + the dropped layout tables keep the
# glyph closure exactly the requested codepoints (no stylistic alternates);
# `hinting=False` matches the previous cut (epaint rasterises outlines and does
# not consume CFF hints); `desubroutinize` matches its CFF encoding; name IDs
# 1,2,3,4,6 drop the copyright/version records to save bytes (the licence file
# beside the assets is the licence record).
SUBSET_OPTIONS = {
    "layout_features": [],
    "hinting": False,
    "desubroutinize": True,
    "name_IDs": [1, 2, 3, 4, 6],
    "drop_tables": ["JSTF", "DSIG", "EBDT", "EBLC", "EBSC", "PCLT", "LTSH", "Feat", "Glat", "Gloc", "Silf", "Sill", "BASE", "GSUB", "GPOS"],
}

CJK_RANGES = [
    (0x2E80, 0x2EFF),  # CJK radicals
    (0x3000, 0x303F),  # CJK punctuation
    (0x3040, 0x30FF),  # kana
    (0x3400, 0x4DBF),  # CJK ext A
    (0x4E00, 0x9FFF),  # CJK unified ideographs
    (0xAC00, 0xD7A3),  # hangul syllables
    (0xF900, 0xFAFF),  # CJK compatibility ideographs
]


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def is_cjk(ch: str) -> bool:
    cp = ord(ch)
    return any(lo <= cp <= hi for lo, hi in CJK_RANGES)


def safety_set() -> set[str]:
    chars = set(SAFETY_CHARS) | set(TYPED_MARKS)
    for lo, hi in SAFETY_RANGES:
        chars.update(chr(cp) for cp in range(lo, hi + 1))
    return chars


def catalog_chars() -> dict[str, set[str]]:
    """Every character each shipped locale catalog can draw (keys and values)."""
    catalogs: dict[str, set[str]] = {}
    for path in sorted(LOCALE_DIR.glob("*.toml")):
        chars: set[str] = set()

        def walk(node: object) -> None:
            if isinstance(node, dict):
                for key, value in node.items():
                    chars.update(key)
                    walk(value)
            elif isinstance(node, list):
                for value in node:
                    walk(value)
            elif isinstance(node, str):
                chars.update(node)

        walk(tomllib.loads(path.read_text(encoding="utf-8")))
        catalogs[path.stem] = chars
    return catalogs


def native_name_chars() -> set[str]:
    """The language picker's own names, straight out of `i18n.rs`."""
    text = I18N.read_text(encoding="utf-8")
    names = re.findall(r'native_name:\s*"([^"]+)"', text)
    if len(names) != 9:
        raise SystemExit(f"expected 9 native_name strings in {I18N}, found {len(names)}")
    chars: set[str] = set()
    for name in names:
        chars.update(name)
    return chars


def derived_codepoints(region: str) -> tuple[list[int], dict[str, int]]:
    """The characters a face must carry, and the policy's contribution counts.

    A character the pinned source does not carry is skipped by the subsetter, so
    this is a requested set, not a guarantee - `--write` reports the result.
    """
    catalogs = catalog_chars()
    native = native_name_chars()
    safety = safety_set()
    region_chars = catalogs[REGION_LOCALE[region]]
    native_cjk = {ch for ch in native if is_cjk(ch)}
    # Cros-region CJK is carried too: the picker can draw any native name under
    # any locale, so the first CJK face in that locale's chain must have it.
    requested = region_chars | native_cjk | safety
    breakdown = {
        "regional_catalog": len(region_chars),
        "native_name_cjk": len(native_cjk),
        "safety": len(safety),
        "union": len(requested),
    }
    return sorted(ord(ch) for ch in requested), breakdown


def ranges_string(codepoints: list[int]) -> str:
    """Compact stable encoding: U+0020-007E,U+00A0-00FF,U+4E00,..."""
    parts: list[str] = []
    cps = sorted(set(codepoints))
    start = prev = cps[0]
    for cp in cps[1:] + [None]:  # type: ignore[list-item]
        if cp is not None and cp == prev + 1:
            prev = cp
            continue
        parts.append(f"U+{start:04X}" if start == prev else f"U+{start:04X}-{prev:04X}")
        if cp is not None:
            start = prev = cp
    return ",".join(parts)


def parse_ranges(text: str) -> set[int]:
    out: set[int] = set()
    for part in text.split(","):
        part = part.strip()
        if not part:
            continue
        body = part.removeprefix("U+")
        if "-" in body:
            lo, hi = body.split("-")
            out.update(range(int(lo, 16), int(hi, 16) + 1))
        else:
            out.add(int(body, 16))
    return out


def must_have_fonttools() -> dict:
    try:
        import fontTools
        from fontTools import subset
        from fontTools.ttLib import TTFont
    except ImportError as exc:  # pragma: no cover - environment guard
        raise SystemExit(
            f"--write needs fontTools ({TOOL_LICENSE}, free): {exc}\n"
            f"  py -3 -m pip install --user {TOOL_NAME}=={TOOL_VERSION}\n"
            f"--check needs no fontTools."
        ) from exc
    if fontTools.version != TOOL_VERSION:
        raise SystemExit(
            f"fontTools {fontTools.version} is installed but the manifest was built "
            f"with {TOOL_VERSION}; pin it so the output stays reproducible:\n"
            f"  py -3 -m pip install --user {TOOL_NAME}=={TOOL_VERSION}"
        )
    return {"subset": subset, "TTFont": TTFont}


def subset_face(source_bytes: bytes, codepoints: list[int], deps: dict) -> bytes:
    import io

    font = deps["TTFont"](io.BytesIO(source_bytes))
    # `TTFont` loaded from a byte stream defaults `recalcTimestamp` on, which
    # would stamp `head.modified` with the wall-clock time on every save - the
    # one thing that made two otherwise-identical runs differ. The pyftsubset
    # CLI reads from a path and therefore does not do this; match it so the
    # output is a pure function of (source, options).
    font.recalcTimestamp = False
    options = deps["subset"].Options()
    for key, value in SUBSET_OPTIONS.items():
        setattr(options, key, value)
    subsetter = deps["subset"].Subsetter(options=options)
    subsetter.populate(unicodes=codepoints)
    subsetter.subset(font)
    out = io.BytesIO()
    font.save(out)
    return out.getvalue()


def output_cmap(face_bytes: bytes, deps: dict) -> set[int]:
    import io

    font = deps["TTFont"](io.BytesIO(face_bytes))
    return set(font.getBestCmap())


def read_manifest() -> dict:
    if not MANIFEST.exists():
        return {"schema": 1, "faces": {}}
    return json.loads(MANIFEST.read_text(encoding="utf-8"))


def check() -> int:
    """Verify the shipped assets and the coverage policy. No deps, no network."""
    failures: list[str] = []
    manifest = read_manifest()
    faces = manifest.get("faces", {})
    if manifest.get("tool", {}).get("version") != TOOL_VERSION:
        failures.append(f"manifest tool version is {manifest.get('tool', {}).get('version')!r}, script expects {TOOL_VERSION}")
    if manifest.get("source", {}).get("tag") != SOURCE_TAG:
        failures.append(f"manifest source tag is {manifest.get('source', {}).get('tag')!r}, script expects {SOURCE_TAG}")
    licence = FONT_DIR / SOURCE_LICENSE_FILE
    if not licence.exists() or sha256(licence.read_bytes()) != SOURCE_LICENSE_SHA256:
        failures.append(f"{licence} is missing or is not the pinned upstream {SOURCE_LICENSE} text")

    catalogs = catalog_chars()
    native_cjk = {ch for ch in native_name_chars() if is_cjk(ch)}
    carried: dict[str, set[int]] = {}
    for asset, region, _sha in FACES:
        path = FONT_DIR / asset
        entry = faces.get(asset)
        if not path.exists():
            failures.append(f"{asset}: missing from {FONT_DIR}")
            continue
        data = path.read_bytes()
        if entry is None:
            failures.append(f"{asset}: not registered in {MANIFEST.name}")
            continue
        if sha256(data) != entry.get("sha256"):
            failures.append(f"{asset}: sha256 does not match the manifest (hand-edited or stale binary?)")
        if len(data) != entry.get("bytes"):
            failures.append(f"{asset}: size {len(data)} does not match the manifest {entry.get('bytes')}")
        carried[region] = parse_ranges(entry.get("codepoints", ""))

    # Each locale's own CJK catalog text must be drawable by the face that owns
    # the locale; the picker names only have to be drawable by the *chain*, and a
    # regional source may legitimately lack another region's characters (a
    # Japanese cut has no Hangul).
    for region, locale in REGION_LOCALE.items():
        have = carried.get(region, set())
        missing = sorted({ord(ch) for ch in catalogs[locale] if is_cjk(ch)} - have)
        if missing:
            failures.append(
                f"{region} face does not cover {len(missing)} CJK character(s) its locale catalog uses: "
                + "".join(chr(cp) for cp in missing[:20])
            )
    chain: set[int] = set().union(*carried.values()) if carried else set()
    missing_names = sorted({ord(ch) for ch in native_cjk} - chain)
    if missing_names:
        failures.append(
            f"the CJK chain does not cover {len(missing_names)} picker name character(s): "
            + "".join(chr(cp) for cp in missing_names)
        )

    if failures:
        print("font-subset check: FAIL")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    total = sum((FONT_DIR / asset).stat().st_size for asset, _r, _s in FACES)
    print(
        f"font-subset check: PASS - {len(FACES)} CJK faces, {total:,} bytes, "
        f"pinned {SOURCE_REPO}@{SOURCE_TAG} + {TOOL_NAME} {TOOL_VERSION}"
    )
    return 0


def write(args: argparse.Namespace) -> int:
    deps = must_have_fonttools()
    manifest = read_manifest()
    previous = {
        name: parse_ranges(entry.get("codepoints", ""))
        for name, entry in manifest.get("faces", {}).items()
    }
    cache = pathlib.Path(args.sources) if args.sources else REPO / "target" / "font-in-sources"
    cache.mkdir(parents=True, exist_ok=True)

    new_faces: dict[str, dict] = {}
    changed = 0
    total_before = 0
    total_after = 0
    for asset, region, source_sha in FACES:
        source_name = f"NotoSans{region}-{asset.split('-')[1]}"  # NotoSansJP-Regular.otf
        source_path = cache / source_name
        if source_path.exists():
            source_bytes = source_path.read_bytes()
            if sha256(source_bytes) != source_sha:
                raise SystemExit(f"{source_path}: sha256 is not the pinned upstream file; delete it and re-run")
        else:
            url = f"{SOURCE_BASE}/{region}/{source_name}"
            print(f"downloading {url}")
            with urllib.request.urlopen(url) as response:  # noqa: S310 - pinned https URL
                source_bytes = response.read()
            if sha256(source_bytes) != source_sha:
                raise SystemExit(f"{url}: downloaded sha256 is not the pinned value; refusing to subset")
            source_path.write_bytes(source_bytes)

        codepoints, breakdown = derived_codepoints(region)
        out = subset_face(source_bytes, codepoints, deps)
        got = output_cmap(out, deps)

        # Ratchet: never drop a character a previous cut carried.
        dropped = sorted(previous.get(asset, set()) - got)
        if dropped:
            raise SystemExit(
                f"{asset}: the new cut would drop {len(dropped)} codepoint(s) the previous cut carried: "
                + "".join(chr(cp) for cp in dropped[:20])
            )

        path = FONT_DIR / asset
        before = path.stat().st_size if path.exists() else 0
        total_before += before
        total_after += len(out)
        if args.dry_run:
            print(f"  {asset:26} {before:8,}B -> {len(out):8,}B  cmap {len(got):4} (+{len(got - previous.get(asset, set()))})")
        elif path.read_bytes() != out:
            path.write_bytes(out)
            changed += 1
            print(f"  {asset:26} {before:8,}B -> {len(out):8,}B  cmap {len(got):4} (+{len(got - previous.get(asset, set()))})")
        else:
            print(f"  {asset:26} unchanged  {len(out):8,}B  cmap {len(got):4}")

        new_faces[asset] = {
            "region": region,
            "locale": REGION_LOCALE[region],
            "source": f"{region}/{source_name}",
            "source_sha256": source_sha,
            "sha256": sha256(out),
            "bytes": len(out),
            "codepoints": ranges_string(sorted(got)),
            "policy": breakdown,
        }

    manifest = {
        "schema": 1,
        "tool": {
            "name": TOOL_NAME,
            "version": TOOL_VERSION,
            "license": TOOL_LICENSE,
            "homepage": TOOL_HOMEPAGE,
        },
        "source": {
            "repo": SOURCE_REPO,
            "tag": SOURCE_TAG,
            "base_url": SOURCE_BASE,
            "license": SOURCE_LICENSE,
            "license_file": SOURCE_LICENSE_FILE,
            "license_sha256": SOURCE_LICENSE_SHA256,
        },
        "options": SUBSET_OPTIONS,
        "policy": {
            "regional_catalog": "client-gui/locales/<locale>.toml characters",
            "native_name_cjk": "CJK characters in client-gui/src/i18n.rs native_name strings",
            "typed_marks": TYPED_MARKS,
            "safety_ranges": [f"U+{lo:04X}-U+{hi:04X}" for lo, hi in SAFETY_RANGES],
            "safety_chars": SAFETY_CHARS,
            "regenerate": "py -3 scripts/fonts/subset_gui_fonts.py --write",
        },
        "faces": new_faces,
    }
    rendered = json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"
    if args.dry_run:
        print(f"dry run: {changed} face(s) would change, {len(rendered)}-byte manifest, "
              f"{total_before:,}B -> {total_after:,}B")
        return 0
    MANIFEST.write_text(rendered, encoding="utf-8", newline="\n")
    print(f"wrote {changed} face(s); CJK assets {total_before:,}B -> {total_after:,}B; manifest {MANIFEST.name}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="verify shipped assets against the manifest (no deps)")
    mode.add_argument("--write", action="store_true", help="re-cut the assets from pinned sources")
    parser.add_argument("--sources", help="directory holding the six pinned source files (default: download into target/font-in-sources)")
    parser.add_argument("--dry-run", action="store_true", help="with --write: report, write nothing")
    args = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    return check() if args.check else write(args)


if __name__ == "__main__":
    sys.exit(main())
