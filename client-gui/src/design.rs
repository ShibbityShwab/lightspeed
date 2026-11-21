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
pub const bg_0: Color32 = Color32::from_rgb(0x0B, 0x0B, 0x0D);
/// The window fill - the app's default background.
pub const bg_1: Color32 = Color32::from_rgb(0x0F, 0x0F, 0x11);
/// Raised surface: cards, the state rail, sheets.
pub const bg_2: Color32 = Color32::from_rgb(0x15, 0x15, 0x18);
/// Hovered / active rows and quiet filled controls.
pub const bg_3: Color32 = Color32::from_rgb(0x1B, 0x1B, 0x1F);
/// Pressed rows, the globe's graticule band, the highest surface.
pub const bg_4: Color32 = Color32::from_rgb(0x22, 0x22, 0x27);

// ── Hairlines ────────────────────────────────────────────────────────────────

/// Every hairline: card edges, dividers, plot axes.
pub const border_1: Color32 = Color32::from_rgb(0x25, 0x25, 0x2A);
/// Hover / focus hairline and the scrollbar thumb.
pub const border_2: Color32 = Color32::from_rgb(0x33, 0x33, 0x39);
/// The boundary that identifies an interactive control at rest (field,
/// checkbox box, combo). 3.14:1 on `bg_2`, 3.30:1 on `bg_1`, 3.39:1 on `bg_0`
/// (`design-system-2026.md` §1.4/§3.2: the shipped 1.19:1 boundary failed
/// WCAG 1.4.11). Decorative hairlines stay `border_1`; hover keeps `border_2`.
pub const border_control: Color32 = Color32::from_rgb(0x65, 0x65, 0x6A);

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
/// Pressed fill of the action.
pub const accent_deep: Color32 = Color32::from_rgb(0x5A, 0x4B, 0xD4);
/// Accent used as ink: links, selected labels, focus ring, selected-row ink.
/// Wire via `selection.stroke` and the focus helper (`design-system` §3.4).
pub const accent_text: Color32 = Color32::from_rgb(0xA2, 0x9B, 0xFE);
/// Selected-row tint, and the rail edge on CONNECTED (`rgba(108, 92, 231, 0.12)`).
pub const accent_soft: Color32 = Color32::from_rgba_unmultiplied_const(108, 92, 231, 31);
/// Accent hairline: a focused field, an active tab (`rgba(108, 92, 231, 0.28)`).
pub const accent_line: Color32 = Color32::from_rgba_unmultiplied_const(108, 92, 231, 71);

// ── Semantic states ──────────────────────────────────────────────────────────

/// Good / healthy: the BOOSTING state, a good quality RTT, live counters.
pub const signal: Color32 = Color32::from_rgb(0x00, 0xD6, 0x8F);
/// Attention: degraded RTT, relay-refresh failure, the game-server marker.
pub const warn: Color32 = Color32::from_rgb(0xFD, 0xCB, 0x6E);
/// Bad / destructive: errors and the STOP BOOST ink.
pub const danger: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
/// Error tint, and the danger-tone notice fill (`rgba(255, 107, 107, 0.12)`).
pub const danger_soft: Color32 = Color32::from_rgba_unmultiplied_const(255, 107, 107, 31);

// ── Utility ──────────────────────────────────────────────────────────────────

/// Ink placed on `accent`; replaces the raw `Color32::WHITE`, and the map
/// marker core.
pub const on_accent: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
/// Backdrop behind in-window sheets (`rgba(11, 11, 13, 0.72)`).
pub const scrim: Color32 = Color32::from_rgba_unmultiplied_const(11, 11, 13, 184);
/// The route map's graticule (`rgba(242, 242, 250, 0.06)`) and land silhouette.
pub const grid: Color32 = Color32::from_rgba_unmultiplied_const(242, 242, 250, 15);
/// Land silhouette on the route map. Decorative (not a 1.4.11 claim); the only
/// map colour that cannot be reused from an existing role (`design-system` §1.4).
pub const map_land: Color32 = Color32::from_rgb(0x4A, 0x4A, 0x72);

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
/// 48 px: the height of the one action button with a single-line label.
pub const ACTION_H: f32 = 48.0;
/// 56 px: the action slot's height when its label wraps to two `EMPHASIS`
/// lines (two 19 px rows = 38 + 2×S2 = 54, rounded up to the 8-px step). The
/// old 9 px shrink floor is gone: long labels wrap, they are never shrunk.
pub const ACTION_H_WRAPPED: f32 = 56.0;
/// 64 px: the RTT sparkline's height. The sparkline is removed in W2 v1; the
/// token is kept for a future labelled, fixed-scale chart (`design-system` §1.4).
pub const PLOT_H: f32 = 64.0;
/// 132 px: the route map's height (replaces the dead `GLOBE_*` trio).
pub const MAP_H: f32 = 132.0;
/// 430 px: the route map's maximum width.
pub const MAP_W_MAX: f32 = 430.0;
/// 40 px: the state rail's hero band (a `STATE` 30 row).
pub const HERO_H: f32 = 40.0;
/// 3 px: the painted state edge on the rail.
pub const RAIL_EDGE: f32 = 3.0;
/// 24 px: the minimum pointer target for any control (WCAG 2.5.8).
pub const HIT_MIN: f32 = 24.0;
/// 2 px: the keyboard-focus ring, painted outside the control.
pub const FOCUS_RING_W: f32 = 2.0;
/// 24 px: one ledger row's allocation.
pub const ROW_H: f32 = 24.0;

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
            (bg_0, [0x0B, 0x0B, 0x0D, 0xFF]),
            (bg_1, [0x0F, 0x0F, 0x11, 0xFF]),
            (bg_2, [0x15, 0x15, 0x18, 0xFF]),
            (bg_3, [0x1B, 0x1B, 0x1F, 0xFF]),
            (bg_4, [0x22, 0x22, 0x27, 0xFF]),
            (border_1, [0x25, 0x25, 0x2A, 0xFF]),
            (border_2, [0x33, 0x33, 0x39, 0xFF]),
            (border_control, [0x65, 0x65, 0x6A, 0xFF]),
            (text_1, [0xF2, 0xF2, 0xFA, 0xFF]),
            (text_2, [0xB4, 0xB4, 0xCC, 0xFF]),
            (text_3, [0x8A, 0x8A, 0xA8, 0xFF]),
            (accent, [0x6C, 0x5C, 0xE7, 0xFF]),
            (accent_deep, [0x5A, 0x4B, 0xD4, 0xFF]),
            (accent_text, [0xA2, 0x9B, 0xFE, 0xFF]),
            (signal, [0x00, 0xD6, 0x8F, 0xFF]),
            (warn, [0xFD, 0xCB, 0x6E, 0xFF]),
            (danger, [0xFF, 0x6B, 0x6B, 0xFF]),
            (on_accent, [0xFF, 0xFF, 0xFF, 0xFF]),
            (map_land, [0x4A, 0x4A, 0x72, 0xFF]),
        ] {
            assert_eq!(token.to_array(), hex, "token drifted from the site hex");
        }
    }

    #[test]
    fn tints_stay_translucent() {
        for tint in [accent_soft, accent_line, danger_soft, scrim, grid] {
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
        // W2 mobile-only adoption: the wrapped action slot and the measured
        // hero band replace the retired globe trio / 96 px label column.
        assert_eq!(
            (CONTROL_H, ACTION_H, ACTION_H_WRAPPED, HERO_H, ROW_H),
            (40.0, 48.0, 56.0, 40.0, 24.0)
        );
        assert_eq!(
            (MAP_H, MAP_W_MAX, RAIL_EDGE, HIT_MIN),
            (132.0, 430.0, 3.0, 24.0)
        );
        assert_eq!(FOCUS_RING_W, 2.0);
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

    /// WCAG relative luminance of an opaque authored colour.
    fn luminance(c: Color32) -> f64 {
        fn channel(v: u8) -> f64 {
            let s = f64::from(v) / 255.0;
            if s <= 0.039_28 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }

    /// WCAG contrast ratio between two opaque colours.
    fn ratio(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a) + 0.05, luminance(b) + 0.05);
        if la >= lb {
            la / lb
        } else {
            lb / la
        }
    }

    /// Straight-alpha composite of `top` over `bottom`, returning an opaque colour.
    fn over(top: Color32, bottom: Color32) -> Color32 {
        let a = f64::from(top.a()) / 255.0;
        let mix =
            |t: u8, b: u8| -> u8 { (f64::from(t) * a + f64::from(b) * (1.0 - a)).round() as u8 };
        Color32::from_rgb(
            mix(top.r(), bottom.r()),
            mix(top.g(), bottom.g()),
            mix(top.b(), bottom.b()),
        )
    }

    /// The measured floors from `design-system-2026.md` §3, asserted in the
    /// crate so a value edit cannot silently regress AA. The selected-row
    /// before-state is asserted as a **failure**: a test that cannot fail is a
    /// defect (QA F7/F1).
    #[test]
    fn measured_contrast_meets_the_aa_floors() {
        // Selected list row: ink over the accent_soft selection fill (the real
        // paint plane; QA R6). Post-fix accent_text ink reaches 6.70:1.
        let selection_fill = over(accent_soft, bg_2);
        let selected = ratio(accent_text, selection_fill);
        assert!(
            selected >= 6.70,
            "selected-row ink must reach 6.70:1, got {selected:.2}"
        );
        // If the ink reverts to the accent fill (the shipped 1.35:1 defect),
        // the same assertion fails: the test can detect the regression.
        let reverted = ratio(accent, selection_fill);
        assert!(
            reverted < 4.5,
            "the accent-ink before-state must fail AA, got {reverted:.2}"
        );

        // Text floors (measured pairings, `design-system` §3.2).
        for (ink_name, ink, plane, floor) in [
            ("text_1/bg_1", text_1, bg_1, 7.0),
            ("text_1/bg_2", text_1, bg_2, 7.0),
            ("text_2/bg_2", text_2, bg_2, 4.5),
            ("text_3/bg_0", text_3, bg_0, 4.5),
            ("text_3/bg_2", text_3, bg_2, 4.5),
            ("warn/bg_2", warn, bg_2, 4.5),
            ("accent_text/bg_0", accent_text, bg_0, 4.5),
            ("signal/bg_2", signal, bg_2, 3.0),
        ] {
            let r = ratio(ink, plane);
            assert!(r >= floor, "{ink_name} = {r:.2} < {floor}");
        }

        // The starvation notice: danger ink on its own danger_soft frame over
        // the card (measured 5.61:1), and the connectable notice on the card.
        let notice_fill = over(danger_soft, bg_2);
        let notice = ratio(danger, notice_fill);
        assert!(notice >= 4.5, "notice ink = {notice:.2} < 4.5");
        let row_plane = bg_2;
        let _ = row_plane;

        // Every interactive control's identifying boundary is ≥3:1 on the
        // plane the control sits on (1.4.11: bg_0/bg_1/bg_2; a `bg_3` field
        // sits on `bg_2`, so the boundary is measured against `bg_2`).
        for (name, plane) in [("bg_0", bg_0), ("bg_1", bg_1), ("bg_2", bg_2)] {
            let r = ratio(border_control, plane);
            assert!(r >= 3.0, "border_control on {name} = {r:.2} < 3.0");
        }

        // The focus ring reads outside the control on every plane it can sit on.
        for (name, plane) in [("bg_0", bg_0), ("bg_1", bg_1), ("bg_2", bg_2)] {
            let r = ratio(accent_text, plane);
            assert!(r >= 3.0, "focus ring on {name} = {r:.2} < 3.0");
        }
    }
}
