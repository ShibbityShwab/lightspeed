# LightSpeed brand assets

The LightSpeed mark is a minimal geometric "L" whose foot strikes a lightning
point. It ships in two treatments:

- **Monochrome mark** for documentation and any surface that needs a plain
  black or white glyph.
- **Hybrid tile** for app and web iconography: the same L in the site's purple
  to teal gradient on a dark rounded tile.

## Files

| File | Use |
|------|-----|
| `lightspeed-mark.svg` | Monochrome mark, black, transparent. For light backgrounds. |
| `lightspeed-mark-inverse.svg` | Monochrome mark, white, transparent. For dark backgrounds. |
| `lightspeed-mark-current.svg` | Monochrome mark using `currentColor`, for CSS themed surfaces. |
| `lightspeed-logo.svg` | Horizontal lockup: mark plus outlined "LightSpeed" wordmark, black. |
| `lightspeed-logo-inverse.svg` | Horizontal lockup, white. |
| `lightspeed-tile.svg` | Hybrid tile (dark rounded tile plus gradient L). Source for every app and web icon. |
| `icon-256.png`, `icon-512.png` | Raster app icons rendered from `lightspeed-tile.svg`. The GUI embeds `icon-256.png` as its window and header icon. |

Consumers:

- `README.md` uses `lightspeed-logo.svg` and `lightspeed-logo-inverse.svg`
  through a `prefers-color-scheme` `<picture>`.
- `CONTRIBUTING.md`, `docs/*.md`, and the infra READMEs use the monochrome mark
  inline in their H1.
- The website uses the hybrid tile in `web/assets/favicon.svg`, the inline
  `#i-logo` symbol in `web/index.html`, and `web/assets/og-image.svg`.

## Palette

| Token | Value |
|-------|-------|
| Tile top | `#1a1a38` |
| Tile bottom | `#0a0a1a` |
| Tile border | `#2f2f52` |
| Mark gradient start | `#a29bfe` |
| Mark gradient end | `#00cec9` |
| Tile glow | `#6c5ce7` at 45% |

## Regenerating

The wordmark is outlined from Manrope ExtraBold (SIL Open Font License), so the
lockups carry no font dependency and do not need regenerating.

Rasters are produced from the SVGs with `rsvg-convert`:

```bash
cd web/assets/brand
rsvg-convert -w 256 -h 256 lightspeed-tile.svg -o icon-256.png
rsvg-convert -w 512 -h 512 lightspeed-tile.svg -o icon-512.png
```

```bash
cd web/assets
rsvg-convert -w 180 -h 180 favicon.svg -o apple-touch-icon.png
rsvg-convert -w 1200 -h 630 og-image.svg -o og-image.png
```

## SVG sprite note

The website's inline icon sprite defines the tile gradients at the sprite root
and keeps the sprite at zero size (`position: absolute; width: 0; height: 0`)
rather than `display: none`. A gradient referenced from inside a `<symbol>`
does not resolve through `<use>` when the sprite is `display: none`, which
renders the logo as an empty tile.
