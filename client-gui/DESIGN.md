# LightSpeed GUI — design decisions

Scope: `client-gui` (eframe/egui 0.36, Windows tray + status window).

## Pattern stack

| Spatial problem | Pattern | Decision |
|---|---|---|
| Shell regions fixed, only the middle scrolls | [`scroll-body-shell`](https://github.com/changeroa/StyleGallery/blob/main/patterns/viewport-shell/scroll-body-shell.md) | Header and footer are `auto`; the config body is `minmax(0, 1fr)` with `min-block-size: 0` and owns the scroll. In egui: header + footer render outside `ScrollArea`, the config renders inside it. |
| Consistent vertical rhythm | [`stack`](https://github.com/changeroa/StyleGallery/blob/main/patterns/stacking/stack.md) | One `gap` token between siblings. No per-call magic numbers. |
| In-line grouping that may wrap | [`cluster`](https://github.com/changeroa/StyleGallery/blob/main/patterns/in-line-grouping/cluster.md) | The relay list wraps as one cluster instead of stretching. |
| Primary vs secondary actions in one row | [`split-nav`](https://github.com/changeroa/StyleGallery/blob/main/patterns/in-line-grouping/split-nav.md) | Footer separates the destructive/secondary actions from the primary. |

Load-bearing constraint carried from the pattern: **the body must be the only element that
scrolls, and it must be allowed to shrink below its content height** (`min-block-size: 0`).
Without that the shell grows and the footer is pushed off-screen.

## Why the previous revision looked unstyled

egui was never themed. `Style`, `Spacing` and `Visuals` were left at their defaults, so every
widget — button, checkbox, combo box, scrollbar, collapsing header — drew in stock egui's light
palette and default metrics, inside hand-rolled panels. Colours were declared per-const but the
widgets themselves were untouched. The fix is one `apply_theme` call that owns all of it.

## Tokens

One source of truth is `mod theme` in `client-gui/src/app.rs`.

Type scale (px) — six steps, no ad-hoc sizes:

| Token | Size | Use |
|---|---|---|
| `CAPTION` | 11 | footnotes, tooltips |
| `LABEL` | 12 | field labels, secondary metadata |
| `BODY` | 13 | default text |
| `EMPHASIS` | 15 | values, card titles |
| `HEADING` | 20 | section headings |
| `DISPLAY` | 34 | the single state readout |

Spacing — 4pt grid, six steps: `4 8 12 16 24 32`.

Colour — neutral surfaces, one accent, three semantic states:

| Token | Value | Role |
|---|---|---|
| `BG` | `#0B0B0F` | window background |
| `SURFACE` | `#15151D` | cards |
| `SURFACE_RAISED` | `#1D1D28` | hovered / active rows |
| `BORDER` | `#272736` | 1px hairlines, used instead of heavy fills |
| `TEXT` | `#ECECF2` | primary text |
| `TEXT_DIM` | `#8B8BA3` | secondary text |
| `ACCENT` | `#6C5CE7` | brand purple: primary action, focus |
| `OK` | `#34D399` | connected / healthy |
| `WARN` | `#FBBF24` | degraded / attention |
| `BAD` | `#F87171` | failed / blocked |

Radii: `6` inline controls, `10` cards, `12` primary button.

## Rules

1. **No emoji as icons.** Emoji render differently per platform and read as placeholder art.
   States and actions are named in words; the only glyphs are geometric (`■`, `▸`) where a
   mark genuinely helps.
2. **Borders carry structure, not fills.** A card is a hairline plus a near-background surface;
   it is never a bright block. Bright fill is reserved for the one primary action.
3. **Exactly one primary action on screen.** Everything else is a secondary or a link.
4. **Text has at most three levels**: primary, dim, and an accent/semantic colour.
5. **The state readout is the largest thing in the window.** It answers the only question the
   window exists for.
6. Secondary and destructive actions are visually quieter than configuration fields.
