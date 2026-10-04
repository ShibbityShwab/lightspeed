//! The window's design tokens: the site's full `:root` scale, in one module.
//!
//! `web/styles.css` is the product's canonical visual language, so this module
//! adopts the site's token names verbatim (`bg-0` becomes `bg_0`) and its values
//! exactly, so the two files diff cleanly. `client-gui/src/app.rs` (`mod theme`)
//! still carries the older, partial palette; the layout migration moves onto
//! these names in a later change, which is why every item here is (for now)
//! unreferenced - hence the module-scoped allowances.
//!
//! Rules the scale carries:
//! - Alpha tints stay `rgba`, never a hex with the alpha baked in. They are built
//!   with `Color32::from_rgba_unmultiplied_const`, the `const`-context form of
//!   `Color32::from_rgba_unmultiplied` (both delegate to the same rounding), so a
//!   tint can live in a `const`.
//! - `accent` is a fill, never ink; accent-as-ink is always `accent_text`.
//! - Semantics are fixed: signal = good, warn = attention, danger = bad,
//!   accent = action and identity, info = neutral fact.
#![allow(dead_code, non_upper_case_globals)]

use eframe::egui::Color32;

// ── Planes ───────────────────────────────────────────────────────────────────

/// Deepest plane: sunken fields, plots, and the backdrop behind sheets.
pub const bg_0: Color32 = Color32::from_rgb(0x07, 0x07, 0x0F);
/// The window fill - the app's default background.
pub const bg_1: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x1A);
/// Raised surface: cards, the state rail, sheets.
pub const bg_2: Color32 = Color32::from_rgb(0x10, 0x10, 0x24);
/// Hovered / active rows and quiet filled controls.
pub const bg_3: Color32 = Color32::from_rgb(0x16, 0x16, 0x2E);
/// Pressed rows, the globe's graticule band, the highest surface.
pub const bg_4: Color32 = Color32::from_rgb(0x1D, 0x1D, 0x3A);

// ── Hairlines ────────────────────────────────────────────────────────────────

/// Every hairline: card edges, dividers, plot axes.
pub const border_1: Color32 = Color32::from_rgb(0x23, 0x23, 0x3F);
/// Hover / focus hairline and the scrollbar thumb.
pub const border_2: Color32 = Color32::from_rgb(0x2F, 0x2F, 0x52);

// ── Ink ──────────────────────────────────────────────────────────────────────

/// Primary text: labels and values.
pub const text_1: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xFA);
/// Prose: help text, the rail's second line.
pub const text_2: Color32 = Color32::from_rgb(0xB4, 0xB4, 0xCC);
/// Tertiary ink: units, captions, footnotes, disabled text.
pub const text_3: Color32 = Color32::from_rgb(0x8A, 0x8A, 0xA8);

// ── Accent ───────────────────────────────────────────────────────────────────

/// Action fill (the single bright control) and the globe's route arc.
pub const accent: Color32 = Color32::from_rgb(0x6C, 0x5C, 0xE7);
/// Hover fill of the action, and focus glow.
pub const accent_strong: Color32 = Color32::from_rgb(0x7D, 0x6F, 0xF0);
/// Pressed fill of the action.
pub const accent_deep: Color32 = Color32::from_rgb(0x5A, 0x4B, 0xD4);
/// Accent used as ink: links, selected labels, focus ring.
pub const accent_text: Color32 = Color32::from_rgb(0xA2, 0x9B, 0xFE);
/// Selected-row tint, and the rail edge on CONNECTED (`rgba(108, 92, 231, 0.12)`).
pub const accent_soft: Color32 = Color32::from_rgba_unmultiplied_const(108, 92, 231, 31);
/// Accent hairline: a focused field, an active tab (`rgba(108, 92, 231, 0.28)`).
pub const accent_line: Color32 = Color32::from_rgba_unmultiplied_const(108, 92, 231, 71);

// ── Semantic states ──────────────────────────────────────────────────────────

/// Good / healthy: the BOOSTING state, a good quality RTT, live counters.
pub const signal: Color32 = Color32::from_rgb(0x00, 0xD6, 0x8F);
/// BOOSTING chip / edge tint (`rgba(0, 214, 143, 0.12)`).
pub const signal_soft: Color32 = Color32::from_rgba_unmultiplied_const(0, 214, 143, 31);
/// Neutral fact: discovery in flight, "measuring".
pub const info: Color32 = Color32::from_rgb(0x00, 0xCE, 0xC9);
/// Info tint (`rgba(0, 206, 201, 0.12)`).
pub const info_soft: Color32 = Color32::from_rgba_unmultiplied_const(0, 206, 201, 31);
/// Attention: degraded RTT, relay-refresh failure, the game-server marker.
pub const warn: Color32 = Color32::from_rgb(0xFD, 0xCB, 0x6E);
/// Warn tint (`rgba(253, 203, 110, 0.12)`).
pub const warn_soft: Color32 = Color32::from_rgba_unmultiplied_const(253, 203, 110, 31);
/// Bad / destructive: errors and the STOP BOOST ink.
pub const danger: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
/// Error tint, and the STOP hover fill (`rgba(255, 107, 107, 0.12)`).
pub const danger_soft: Color32 = Color32::from_rgba_unmultiplied_const(255, 107, 107, 31);

// ── Utility ──────────────────────────────────────────────────────────────────

/// Ink placed on `accent`; replaces the raw `Color32::WHITE`.
pub const on_accent: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Backdrop behind in-window sheets (`rgba(7, 7, 15, 0.72)`).
pub const scrim: Color32 = Color32::from_rgba_unmultiplied_const(7, 7, 15, 184);
/// The globe's graticule and plot gridlines (`rgba(242, 242, 250, 0.06)`).
pub const grid: Color32 = Color32::from_rgba_unmultiplied_const(242, 242, 250, 15);

// ── Spacing ──────────────────────────────────────────────────────────────────

/// 4 px - the tightest gap (icon to label).
pub const S1: f32 = 4.0;
/// 8 px - default gap between related controls.
pub const S2: f32 = 8.0;
/// 12 px - card gutter, button padding.
pub const S3: f32 = 12.0;
/// 16 px - gap between content groups.
pub const S4: f32 = 16.0;
/// 24 px - separation between major regions.
pub const S5: f32 = 24.0;

// ── Radii ────────────────────────────────────────────────────────────────────
//
// The retired `mod theme` values were 6 (inline) and 10 (card); the site's
// `--r-*` scale replaces them so radius and surface stop being one-per-widget.

/// 4 px (`--r-xs`): inline marks, chip corners, the disclosure marker.
pub const R_INLINE: u8 = 4;
/// 8 px (`--r-sm`): text fields, combos, tiles, the globe frame.
pub const R_CONTROL: u8 = 8;
/// 8 px: the one card style.
pub const R_CARD: u8 = 8;
/// 12 px (`--r-md`): the action button.
pub const R_BUTTON: u8 = 12;
/// 16 px (`--r-lg`): in-window sheets.
pub const R_SHEET: u8 = 16;

// ── Sizes ────────────────────────────────────────────────────────────────────

/// 40 px: the height of a config control (combo, field).
pub const CONTROL_H: f32 = 40.0;
/// 48 px: the height of the one action button.
pub const ACTION_H: f32 = 48.0;
/// 64 px: the RTT sparkline's height.
pub const PLOT_H: f32 = 64.0;
/// 132 px: the globe at default width.
pub const GLOBE_LG: f32 = 132.0;
/// 104 px: the globe at narrow width.
pub const GLOBE_SM: f32 = 104.0;
/// 72 px: the globe in the short-height breakpoint.
pub const GLOBE_XS: f32 = 72.0;
/// 24 px: one ledger row's allocation.
pub const ROW_H: f32 = 24.0;
/// 96 px at 440 px width: the ledger's label column (full width when narrow).
pub const LEDGER_LABEL_W: f32 = 96.0;

// ── Type scale ───────────────────────────────────────────────────────────────
//
// px, matching the site's rem scale at a 16px root, so a token has the same
// size on the landing page and in the window. Inter Regular / SemiBold for
// text, JetBrains Mono Regular for data; weight is binary inside Inter, so
// emphasis is carried by size, colour and alignment rather than a fake weight.

/// 11/16 - units (`ms`), footnotes, tooltips, disabled ink.
pub const CAPTION: f32 = 11.0;
/// 12/16 - field and section labels, sentence case.
pub const LABEL: f32 = 12.0;
/// 13/19 - prose: help text, the rail's second line.
pub const BODY: f32 = 13.0;
/// 15/19 - the action button and other emphasis (the site's `--fs-base`).
pub const EMPHASIS: f32 = 15.0;
/// 17/24 - JetBrains Mono: addresses, ports, node ids, counters.
pub const VALUE: f32 = 17.0;
/// 20/26 - sheet titles ("Proxy Manager", "Update").
pub const TITLE: f32 = 20.0;
/// 30/34 - the state word in the rail.
pub const STATE: f32 = 30.0;
/// 30/34 - the live round trip in the rail (JetBrains Mono).
pub const METRIC: f32 = 30.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_array` is premultiplied, so an opaque token compares channel-for-
    /// channel against the site's hex.
    #[test]
    fn opaque_tokens_match_the_site_hexes() {
        for (token, hex) in [
            (bg_0, [0x07, 0x07, 0x0F, 0xFF]),
            (bg_1, [0x0A, 0x0A, 0x1A, 0xFF]),
            (bg_2, [0x10, 0x10, 0x24, 0xFF]),
            (bg_3, [0x16, 0x16, 0x2E, 0xFF]),
            (bg_4, [0x1D, 0x1D, 0x3A, 0xFF]),
            (border_1, [0x23, 0x23, 0x3F, 0xFF]),
            (border_2, [0x2F, 0x2F, 0x52, 0xFF]),
            (text_1, [0xF2, 0xF2, 0xFA, 0xFF]),
            (text_2, [0xB4, 0xB4, 0xCC, 0xFF]),
            (text_3, [0x8A, 0x8A, 0xA8, 0xFF]),
            (accent, [0x6C, 0x5C, 0xE7, 0xFF]),
            (accent_strong, [0x7D, 0x6F, 0xF0, 0xFF]),
            (accent_deep, [0x5A, 0x4B, 0xD4, 0xFF]),
            (accent_text, [0xA2, 0x9B, 0xFE, 0xFF]),
            (signal, [0x00, 0xD6, 0x8F, 0xFF]),
            (info, [0x00, 0xCE, 0xC9, 0xFF]),
            (warn, [0xFD, 0xCB, 0x6E, 0xFF]),
            (danger, [0xFF, 0x6B, 0x6B, 0xFF]),
            (on_accent, [0xFF, 0xFF, 0xFF, 0xFF]),
        ] {
            assert_eq!(token.to_array(), hex, "token drifted from the site hex");
        }
    }

    #[test]
    fn tints_stay_translucent() {
        for tint in [
            accent_soft,
            accent_line,
            signal_soft,
            info_soft,
            warn_soft,
            danger_soft,
            scrim,
            grid,
        ] {
            assert!(tint.a() < 255, "a tint must not be opaque: {tint:?}");
        }
    }

    #[test]
    fn tints_unmultiply_back_to_their_source() {
        // A tint must be the exact 8-bit premultiplication of its documented
        // source at its documented alpha. Comparing the unmultiplied round
        // trip is lossy (an 8-bit alpha can drift the channels by more than
        // one step), so assert the authored premultiplied value directly.
        for (tint, base, alpha) in [
            (accent_soft, accent, 31),
            (accent_line, accent, 71),
            (signal_soft, signal, 31),
            (info_soft, info, 31),
            (warn_soft, warn, 31),
            (danger_soft, danger, 31),
            (scrim, bg_0, 184),
            (grid, text_1, 15),
        ] {
            assert_eq!(tint.a(), alpha);
            let [r, g, b, _] = base.to_array();
            let authored = Color32::from_rgba_unmultiplied(r, g, b, alpha);
            assert_eq!(
                tint, authored,
                "a tint must be authored from its source rgb at its documented alpha"
            );
        }
    }

    #[test]
    fn geometry_scale_is_stable() {
        assert_eq!((S1, S2, S3, S4, S5), (4.0, 8.0, 12.0, 16.0, 24.0));
        assert_eq!(
            (R_INLINE, R_CONTROL, R_CARD, R_BUTTON, R_SHEET),
            (4, 8, 8, 12, 16)
        );
        assert_eq!((CONTROL_H, ACTION_H, PLOT_H), (40.0, 48.0, 64.0));
        assert_eq!((GLOBE_XS, GLOBE_SM, GLOBE_LG), (72.0, 104.0, 132.0));
    }

    #[test]
    fn type_scale_is_stable() {
        assert_eq!(
            (CAPTION, LABEL, BODY, EMPHASIS, VALUE, TITLE, STATE, METRIC),
            (11.0, 12.0, 13.0, 15.0, 17.0, 20.0, 30.0, 30.0)
        );
        // Mono is never smaller than the value size; captions are Inter-only.
        assert_eq!(STATE, METRIC);
    }
}
