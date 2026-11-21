# Bundled UI fonts

Ten faces are compiled into the GUI with `include_bytes!` from
`client-gui/src/app/theme.rs`. They are installed before the first frame, and
the three families (`proportional`, `monospace`, `semibold`) chain them with the
tone-carrying face first and the CJK faces ordered by the active locale.

Everything here is **SIL Open Font License 1.1** and redistributable at $0.00.
The licence texts are in this directory: `OFL.txt` (Inter, JetBrains Mono, and
the epaint defaults) and `OFL-NotoSansCJK.txt` (Noto Sans JP/KR/SC). The Noto
licence file is byte-identical to upstream `notofonts/noto-cjk@Sans2.004`
`LICENSE` (SHA-256 `6a73f954…2bf2`).

## Inventory (measured, 2026-11-21)

| file | family / style | role | mapped codepoints | bytes |
|---|---|---|---|---|
| `Inter-Regular.ttf` | Inter Regular 4.001 | proportional head | 230 | 66,912 |
| `Inter-SemiBold.ttf` | Inter SemiBold 4.001 | `semibold` head | 230 | 67,144 |
| `Inter-Bold.ttf` | Inter Bold 4.001 | bundled, no family | 230 | 67,128 |
| `JetBrainsMono-Regular.ttf` | JetBrains Mono 2.211 | monospace head | 229 | 57,800 |
| `NotoSansJP-Regular.otf` | Noto Sans JP 2.004 | ja CJK regular | 405 | 47,776 |
| `NotoSansJP-Bold.otf` | Noto Sans JP Bold 2.004 | ja CJK bold | 405 | 47,952 |
| `NotoSansKR-Regular.otf` | Noto Sans KR 2.004 | ko CJK regular | 395 | 34,168 |
| `NotoSansKR-Bold.otf` | Noto Sans KR Bold 2.004 | ko CJK bold | 395 | 34,532 |
| `NotoSansSC-Regular.otf` | Noto Sans SC 2.004 | zh-Hans CJK regular | 415 | 52,912 |
| `NotoSansSC-Bold.otf` | Noto Sans SC Bold 2.004 | zh-Hans CJK bold | 415 | 53,304 |

The Inter and JetBrains Mono cuts are Latin-only. Inter carries all printable
ASCII and Latin-1 (soft hyphen U+00AD is the one exception; no catalog uses it),
so `de` `ß`, `es` `í`, `fr` `É ê î` and `pt-BR` `ê` draw in Inter. JetBrains
Mono carries all printable ASCII and all of Latin-1. Both add typographic marks
and combining accents; neither has Cyrillic/Greek/CJK. They are unchanged by
Q19; their name-table versions are recorded above because the original cut
command was not.

## The CJK subsets are generated, never hand-edited

The three Noto faces ship as small subsets rather than the ~28 MB full fonts:
the nine locales plus the language picker need 620 distinct non-ASCII
characters, and a subset of the characters the UI can actually emit keeps the
binary lean. **Do not edit an `.otf` by hand** — it has no reproducible
provenance and the coverage/hash gate will reject it.

Regenerate them with the pinned pipeline:

```powershell
# one-time tool (MIT, free): pinned so the output is byte-reproducible
py -3 -m pip install --user fonttools==4.68.0

# re-cut the six faces from the pinned OFL sources (downloads into target/font-in-sources)
py -3 scripts/fonts/subset_gui_fonts.py --write

# verify the shipped assets: hashes, sizes and locale coverage (no deps, no network)
py -3 scripts/fonts/subset_gui_fonts.py --check
```

`subset.json` is the contract produced by `--write`: per-face source URL,
SHA-256 of source and output, byte size, resulting codepoint ranges and the
character policy. `--write` refuses to drop a codepoint a previous cut carried
(the subset is a ratchet), and `--check` fails if an asset no longer matches the
manifest.

Pipeline facts (all pinned in the script and the manifest):

- **Source**: `notofonts/noto-cjk` tag `Sans2.004`, `Sans/SubsetOTF/{JP,KR,SC}`
  (SIL OFL 1.1). Each source file's SHA-256 is pinned; a mismatch aborts.
- **Tool**: fontTools `subset` (MIT). Version is pinned and checked; the output
  is a pure function of source + options.
- **Characters per face**: that region's locale catalog characters + every CJK
  character in the nine `native_name` picker strings + the typed-symbol
  allowlist (`· × — … →`) + a Latin/punctuation safety set (printable ASCII,
  Latin-1 supplement, and `– ‘ ’ “ ” • − ─ ● ✓`, which is what the previous cut
  carried). A character the source does not have is skipped.
- **Options**: no layout-feature glyph closure, CFF hints stripped,
  subroutines inlined, layout tables (`BASE`/`GSUB`/`GPOS`) dropped, name IDs
  `1,2,3,4,6`. Chosen to match the previous cut's encoding and size; epaint
  rasterises outlines and consumes neither CFF hints nor layout tables.
- **Determinism**: the loader disables fontTools' `recalcTimestamp`, so
  `head.modified` keeps the source value instead of the wall clock. Run
  `--write` twice and the files are byte-identical.

`--check` needs neither fontTools nor network, so CI could run it, but it is a
developer/agent gate today; the durable runtime gate is the Rust test
`every_catalog_glyph_is_drawable_in_every_family` plus
`the_language_picker_names_draw_ink_in_every_family` in
`client-gui/src/app/tests.rs`, which reads the real installed `FontDefinitions`.

## History

- **2026-11-21 (WF-081b, Q19)**: regenerated the six CJK faces from the pinned
  source. The previous cut was made ad hoc and had drifted: the picker's `국`,
  `简`, `体` existed in no bundled face (device-dependent system-font fallback),
  and the `ja` and `zh-Hans` faces were missing characters from their *own*
  catalogs (`分`, `前` and `要`, `語`), so those strings were partly drawn with
  another region's glyph forms. Coverage 399/388/411 → 405/395/415 codepoints,
  266,216 → 270,644 bytes (+4,428 B, +1.66%).
