//! The window's design tokens — Direction C "Broadcast console".
//!
//! This is the owner-selected `task-101` visual direction. The authority is
//! `docs/design/gui-c-token-sheet.md` (design-system-2, task-102): its palette,
//! type, geometry and component values are implemented verbatim here, and the
//! sheet wins over the static mockups where they differ (§9 of the sheet).
//!
//! Rules the scale carries:
//! - Dark only. Direction C does not implement light mode.
//! - Monospace is the UI voice (`console()` in `app/theme.rs`); Inter carries
//!   prose sentences only (`prose()`). One weight — no faux bold.
//! - Flat: plane separation and 1 px rules only. No shadow, no gradient.
//! - `accent` is the one neon surface (the primary action) and is never ink;
//!   `accent_text` (ice) is the link/focus/data ink.
//! - 11 px is the minimum rendered text size (accessibility floor).
//! - The state word is capped at `STATE` 30 px and must measure ≤276 px in
//!   every locale (the accepted D6 one-line rule; the token sheet §2).
//!
//! Colour names keep the shipped `design.rs` identifiers (`bg_0`…`bg_4`,
//! `text_1`…`text_3`, `accent*`, `signal`, `warn`, `danger`) so the rest of the
//! shell moves onto the console palette without a parallel token system; the
//! sheet's `c.*` names are mapped in this module's doc comments.
#![allow(dead_code, non_upper_case_globals)]

use eframe::egui::Color32;

// ── Planes (§1: canvas → panel → panel_2 → plate) ──────────────────────────

/// `c.bg` — window canvas, the deepest plane.
pub const bg_0: Color32 = Color32::from_rgb(0x05, 0x06, 0x07);
/// `c.panel` — data rail, sheet panel, window fill.
pub const bg_1: Color32 = Color32::from_rgb(0x0A, 0x0C, 0x0E);
/// `c.panel_2` — inset, hover plane, disabled surface.
pub const bg_2: Color32 = Color32::from_rgb(0x0D, 0x11, 0x13);
/// `c.plate` — pressed plane / raised inset (rare).
pub const bg_3: Color32 = Color32::from_rgb(0x11, 0x15, 0x18);
/// Alias of the pressed plane: Direction C has four planes, not five, and the
/// theme's widget state ladder needs a fourth step that stays inside §1.
pub const bg_4: Color32 = bg_3;

// ── Rules and boundaries (§1) ───────────────────────────────────────────────

/// `c.rule` — hairline rule: row separators and panel borders (decorative).
pub const border_1: Color32 = Color32::from_rgb(0x1D, 0x23, 0x29);
/// `c.rule_strong` — section-header rule / hover panel border (decorative).
pub const border_2: Color32 = Color32::from_rgb(0x2B, 0x33, 0x3B);
/// `c.control_line` — the boundary that identifies a control (3.27:1 on panel,
/// WCAG 1.4.11 AA).
pub const border_control: Color32 = Color32::from_rgb(0x58, 0x65, 0x6E);

// ── Ink (§1) ────────────────────────────────────────────────────────────────

/// `c.ink` — primary text and the state word (17.84:1 on panel).
pub const text_1: Color32 = Color32::from_rgb(0xF0, 0xF5, 0xF8);
/// `c.ink_2` — secondary text, row values, prose (9.28:1 on panel).
pub const text_2: Color32 = Color32::from_rgb(0xA9, 0xB4, 0xBD);
/// `c.dim` — muted text, labels, micro-captions at 11 px (5.08:1 on panel).
pub const text_3: Color32 = Color32::from_rgb(0x79, 0x83, 0x8E);
/// `c.disabled_ink` — disabled labels (exempt from AA; kept ≥3:1 UI).
pub const text_disabled: Color32 = Color32::from_rgb(0x6B, 0x75, 0x80);

// ── Data highlight (§1) ─────────────────────────────────────────────────────

/// `c.ice` — data highlight, the LIVE indicator, links, the focus ring.
pub const ice: Color32 = Color32::from_rgb(0x9F, 0xE8, 0xFF);
/// `c.ice_dim` — section index, secondary ice, caret (5.77:1 on panel).
pub const ice_dim: Color32 = Color32::from_rgb(0x5F, 0x93, 0xA2);

// ── Accent — the one neon surface (§1, D-C2: lime is the owner's pick) ──────

/// `c.accent` — the primary CTA fill. Never ink; never a second surface.
pub const accent: Color32 = Color32::from_rgb(0xCD, 0xFF, 0x45);
/// `c.accent_hover` — CTA hover fill.
pub const accent_hover: Color32 = Color32::from_rgb(0xD9, 0xFF, 0x7A);
/// `c.accent_pressed` — CTA pressed fill.
pub const accent_deep: Color32 = Color32::from_rgb(0xB9, 0xEA, 0x2F);
/// `c.accent_ink` — accent as ink if ever needed (kept for the pair table).
pub const accent_ink: Color32 = accent;
/// `c.on_accent` — CTA label ink (16.71:1 on accent).
pub const on_accent: Color32 = Color32::from_rgb(0x0A, 0x0E, 0x02);
/// Accent used as ink: links, focus ring, selected labels (`c.ice`).
pub const accent_text: Color32 = ice;
/// Selected-row fill tint. A UI fill, never text; the only translucent accent
/// use left after the direction's "one neon surface" rule.
pub const accent_soft: Color32 = Color32::from_rgba_unmultiplied_const(159, 232, 255, 28);
/// Accent hairline: focused field / active tab (`rgba(159,232,255,.28)`).
pub const accent_line: Color32 = Color32::from_rgba_unmultiplied_const(159, 232, 255, 71);

// ── Semantic states (§1) ────────────────────────────────────────────────────

/// Connected / optimizing data and the LIVE LED (`c.ice`): Direction C has no
/// green signal token — live data *is* the ice highlight.
pub const signal: Color32 = ice;
/// `c.warn` — transient warning tag.
pub const warn: Color32 = Color32::from_rgb(0xFF, 0xB1, 0x3D);
/// `c.danger` — fault, danger label, the stop action.
pub const danger: Color32 = Color32::from_rgb(0xFF, 0x5C, 0x5C);
/// `c.danger_tint` — banner / error wash (`#160C0D`, the sheet's composited
/// rgba(255,92,92,.07) over `c.bg`). Opaque by design.
pub const danger_soft: Color32 = Color32::from_rgb(0x16, 0x0C, 0x0D);
/// Banner body ink on `danger_soft` (`#FFD8D8`, 14.71:1).
pub const danger_text: Color32 = Color32::from_rgb(0xFF, 0xD8, 0xD8);
/// The one focus-ring colour (`c.focus` = ice).
pub const focus: Color32 = ice;
/// Error-line ink on the danger wash (`c.danger`, 6.35:1 there).
pub const danger_ink: Color32 = danger;

// ── Disabled ────────────────────────────────────────────────────────────────

/// `c.disabled_plane` — disabled fill (`panel_2`).
pub const disabled_plane: Color32 = bg_2;
/// `c.disabled_ink` — disabled label (≥3:1 UI, exempt from text AA).
pub const disabled_ink: Color32 = text_disabled;

// ── Telemetry map (§1/§3) ───────────────────────────────────────────────────

/// `c.map_bg` — telemetry map panel background.
pub const map_bg: Color32 = Color32::from_rgb(0x08, 0x0A, 0x0C);
/// `c.grid` — telemetry graticule (decorative).
pub const grid: Color32 = Color32::from_rgb(0x12, 0x17, 0x1B);
/// `c.grid_land` — telemetry land dots (decorative).
pub const map_land: Color32 = Color32::from_rgb(0x2A, 0x34, 0x3A);
/// `c.grid_fleet` — fleet crosses on the map (3.01:1 vs `map_bg`, UI mark).
pub const map_fleet: Color32 = Color32::from_rgb(0x5E, 0x7C, 0x86);
/// The active relay's halo stroke (`rgba(159,232,255,.45)`).
pub const map_halo: Color32 = Color32::from_rgba_unmultiplied_const(159, 232, 255, 115);
/// The active relay's halo fill (`rgba(159,232,255,.09)`).
pub const map_halo_fill: Color32 = Color32::from_rgba_unmultiplied_const(159, 232, 255, 23);

// ── Utility ─────────────────────────────────────────────────────────────────

/// Backdrop behind in-window sheets (`rgba(2,3,4,.68)`).
pub const scrim: Color32 = Color32::from_rgba_unmultiplied_const(2, 3, 4, 214);

// ── Spacing (§3: canonical S1–S5, D-C3) ─────────────────────────────────────

/// 4 px — label→value gap, tick gap.
pub const S1: f32 = 4.0;
/// 8 px — row padding, map inner pad, bracket pad.
pub const S2: f32 = 8.0;
/// 12 px — section gap, sheet group gap, banner padding.
pub const S3: f32 = 12.0;
/// 16 px — page gutter, sheet inset, panel padding (content width 320−2×S4).
pub const S4: f32 = 16.0;
/// 24 px — sheet bottom pad / large separation only.
pub const S5: f32 = 24.0;

// ── Radii (§3: 2 px tags, 3 px panels; nothing rounder) ─────────────────────

/// `R_TAG` 2 px — brackets, tags, inline marks.
pub const R_TAG: u8 = 2;
/// `R_PANEL` 3 px — panels, CTA, sheet.
pub const R_PANEL: u8 = 3;
/// 2 px — inline marks (alias of `R_TAG`).
pub const R_INLINE: u8 = R_TAG;
/// 2 px — controls (`c.control_line` boxes, combos, fields).
pub const R_CONTROL: u8 = R_TAG;
/// 3 px — the one panel/card style.
pub const R_CARD: u8 = R_PANEL;
/// 3 px — the action button.
pub const R_BUTTON: u8 = R_PANEL;
/// 3 px — in-window sheets.
pub const R_SHEET: u8 = R_PANEL;

// ── Sizes (§3) ──────────────────────────────────────────────────────────────

/// 32 px — the height of a config control (combo, field, toggle row).
pub const CONTROL_H: f32 = 32.0;
/// 48 px — the CTA height (normal).
pub const ACTION_H: f32 = 48.0;
/// 46 px — the CTA height in the compact 420 tier.
pub const ACTION_H_COMPACT: f32 = 46.0;
/// 56 px — the CTA's height when its label wraps to two lines.
pub const ACTION_H_WRAPPED: f32 = 56.0;
/// 30 px — the top brand/status bar (normal).
pub const TOP_H: f32 = 30.0;
/// 28 px — the top brand/status bar (compact).
pub const TOP_H_COMPACT: f32 = 28.0;
/// 38 px — the footer (normal).
pub const FOOTER_H: f32 = 38.0;
/// 34 px — the footer (compact).
pub const FOOTER_H_COMPACT: f32 = 34.0;
/// 146 px — the telemetry map (normal).
pub const MAP_H: f32 = 146.0;
/// 96 px — the telemetry map (compact).
pub const MAP_H_COMPACT: f32 = 96.0;
/// 430 px — the map's maximum width.
pub const MAP_W_MAX: f32 = 430.0;
/// 32 px — the state word's one-line band.
pub const HERO_H: f32 = 32.0;
/// 20 px — a section-header rule row.
pub const SECTION_H: f32 = 20.0;
/// 30 px — minimum data-row height.
pub const ROW_H: f32 = 30.0;
/// 36 px — a stacked sentence row (label + prose value + gap).
pub const ROW_H_SENTENCE: f32 = 36.0;
/// 7 px — the tick ruler under the hero.
pub const RULER_H: f32 = 7.0;
/// 3 px — the painted state edge on the rail (kept; C paints rules instead).
pub const RAIL_EDGE: f32 = 3.0;
/// 24 px — minimum pointer target for any control (WCAG 2.5.8).
pub const HIT_MIN: f32 = 24.0;
/// 2 px — the keyboard-focus ring.
pub const FOCUS_RING_W: f32 = 2.0;
/// 1 px — the focus ring's offset outside the control.
pub const FOCUS_OFFSET: f32 = 1.0;

// ── Type (§2; 11 px floor, state word capped at 30) ────────────────────────

/// 11/16 — the floor: labels, micro-captions, brand, footer, section names.
pub const CAPTION: f32 = 11.0;
/// 11/16 — field and row labels (uppercase at render time).
pub const LABEL: f32 = 11.0;
/// 12/16 — prose sentences (Inter) and the sheet title's body.
pub const BODY: f32 = 12.0;
/// 12.5/18 — the CTA label.
pub const EMPHASIS: f32 = 12.5;
/// 12.5/18 — data values, sheet title.
pub const VALUE: f32 = 12.5;
/// 12/16 — sheet titles.
pub const TITLE: f32 = 12.0;
/// 30/32 — the state word, hard cap (D6 one-line rule).
pub const STATE: f32 = 30.0;
/// 28/32 — the live relay-leg readout (mono, ice).
pub const METRIC: f32 = 28.0;
/// 11/16 — the brand wordmark.
pub const BRAND: f32 = 11.0;
/// 11/16 — section labels and their indices.
pub const SECTION: f32 = 11.0;
/// 12/16 — prose sentences (Inter), the same size as `BODY` for clarity.
pub const PROSE: f32 = 12.0;

// ── Tracking (em, painted by the tracked-text helper) ──────────────────────

/// Brand wordmark tracking (§2: 0.34em).
pub const TRACK_BRAND: f32 = 0.34;
/// Top meta / footer tracking (§2: 0.16em).
pub const TRACK_META: f32 = 0.16;
/// Section label tracking (§2: 0.24em).
pub const TRACK_SECTION: f32 = 0.24;
/// Micro-caption / data-label tracking (§2: 0.19em).
pub const TRACK_LABEL: f32 = 0.19;
/// State word tracking (§2: −0.02em).
pub const TRACK_STATE: f32 = -0.02;
/// Data-value tracking (§2: 0.01em).
pub const TRACK_VALUE: f32 = 0.01;
/// CTA label tracking (§2: 0.2em).
pub const TRACK_CTA: f32 = 0.2;
/// Banner tag tracking (§4.8: 0.24em).
pub const TRACK_TAG: f32 = 0.24;
/// Active fleet code label tracking (§3: 0.14em).
pub const TRACK_FLEET: f32 = 0.14;

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_array` is premultiplied, so an opaque token compares channel-for-
    /// channel against the sheet's hex (`gui-c-token-sheet.md` §1).
    #[test]
    fn opaque_tokens_match_the_token_sheet_hexes() {
        for (token, hex) in [
            (bg_0, [0x05, 0x06, 0x07, 0xFF]),
            (bg_1, [0x0A, 0x0C, 0x0E, 0xFF]),
            (bg_2, [0x0D, 0x11, 0x13, 0xFF]),
            (bg_3, [0x11, 0x15, 0x18, 0xFF]),
            (bg_4, [0x11, 0x15, 0x18, 0xFF]),
            (border_1, [0x1D, 0x23, 0x29, 0xFF]),
            (border_2, [0x2B, 0x33, 0x3B, 0xFF]),
            (border_control, [0x58, 0x65, 0x6E, 0xFF]),
            (text_1, [0xF0, 0xF5, 0xF8, 0xFF]),
            (text_2, [0xA9, 0xB4, 0xBD, 0xFF]),
            (text_3, [0x79, 0x83, 0x8E, 0xFF]),
            (text_disabled, [0x6B, 0x75, 0x80, 0xFF]),
            (ice, [0x9F, 0xE8, 0xFF, 0xFF]),
            (ice_dim, [0x5F, 0x93, 0xA2, 0xFF]),
            (accent, [0xCD, 0xFF, 0x45, 0xFF]),
            (accent_hover, [0xD9, 0xFF, 0x7A, 0xFF]),
            (accent_deep, [0xB9, 0xEA, 0x2F, 0xFF]),
            (on_accent, [0x0A, 0x0E, 0x02, 0xFF]),
            (signal, [0x9F, 0xE8, 0xFF, 0xFF]),
            (warn, [0xFF, 0xB1, 0x3D, 0xFF]),
            (danger, [0xFF, 0x5C, 0x5C, 0xFF]),
            (danger_soft, [0x16, 0x0C, 0x0D, 0xFF]),
            (danger_text, [0xFF, 0xD8, 0xD8, 0xFF]),
            (map_bg, [0x08, 0x0A, 0x0C, 0xFF]),
            (grid, [0x12, 0x17, 0x1B, 0xFF]),
            (map_land, [0x2A, 0x34, 0x3A, 0xFF]),
            (map_fleet, [0x5E, 0x7C, 0x86, 0xFF]),
        ] {
            assert_eq!(token.to_array(), hex, "token drifted from the C sheet hex");
        }
    }

    #[test]
    fn tints_stay_translucent_and_the_danger_wash_is_the_composited_token() {
        for (tint, alpha) in [
            (accent_soft, 28),
            (accent_line, 71),
            (scrim, 214),
            (map_halo, 115),
            (map_halo_fill, 23),
        ] {
            assert_eq!(tint.a(), alpha, "tint alpha drifted");
            assert!(tint.a() < 255, "a tint must not be opaque: {tint:?}");
        }
        // The banner wash is the sheet's *composited* opaque token, not a tint.
        assert_eq!(danger_soft, Color32::from_rgb(0x16, 0x0C, 0x0D));
    }

    #[test]
    fn tints_unmultiply_back_to_their_source() {
        for (tint, base, alpha) in [
            (accent_soft, ice, 28),
            (accent_line, ice, 71),
            (map_halo, ice, 115),
            (map_halo_fill, ice, 23),
        ] {
            assert_eq!(tint.a(), alpha);
            let [r, g, b, _] = base.to_array();
            let authored = Color32::from_rgba_unmultiplied(r, g, b, alpha);
            assert_eq!(
                tint, authored,
                "a tint must be authored from its source rgb at its documented alpha"
            );
        }
        // The sheet scrim is `rgba(2,3,4,.68)` — its own authored ink, not bg_0.
        assert_eq!(
            scrim,
            Color32::from_rgba_unmultiplied(2, 3, 4, 214),
            "the scrim must be authored as rgba(2,3,4,.84) for the m1 sheet dim"
        );
    }

    #[test]
    fn geometry_scale_is_stable() {
        assert_eq!((S1, S2, S3, S4, S5), (4.0, 8.0, 12.0, 16.0, 24.0));
        assert_eq!((R_TAG, R_PANEL), (2, 3));
        assert_eq!(
            (R_INLINE, R_CONTROL, R_CARD, R_BUTTON, R_SHEET),
            (R_TAG, R_TAG, R_PANEL, R_PANEL, R_PANEL)
        );
        assert_eq!(
            (TOP_H, TOP_H_COMPACT, FOOTER_H, FOOTER_H_COMPACT),
            (30.0, 28.0, 38.0, 34.0)
        );
        assert_eq!(
            (CONTROL_H, ACTION_H, ACTION_H_COMPACT, ACTION_H_WRAPPED),
            (32.0, 48.0, 46.0, 56.0)
        );
        assert_eq!((MAP_H, MAP_H_COMPACT, MAP_W_MAX), (146.0, 96.0, 430.0));
        assert_eq!(
            (HERO_H, SECTION_H, ROW_H, ROW_H_SENTENCE, RULER_H),
            (32.0, 20.0, 30.0, 36.0, 7.0)
        );
        assert_eq!((HIT_MIN, FOCUS_RING_W, FOCUS_OFFSET), (24.0, 2.0, 1.0));
    }

    #[test]
    fn type_scale_is_stable_with_the_11px_floor_and_30px_state_cap() {
        assert_eq!(
            (CAPTION, LABEL, BODY, EMPHASIS, VALUE, TITLE, STATE, METRIC),
            (11.0, 11.0, 12.0, 12.5, 12.5, 12.0, 30.0, 28.0)
        );
        // The token sheet's floor: nothing renders below 11 px.
        for role in [
            CAPTION, LABEL, BODY, EMPHASIS, VALUE, TITLE, BRAND, SECTION, PROSE,
        ] {
            assert!(
                role >= 11.0,
                "a type role fell below the 11 px floor: {role}"
            );
        }
        // The state word is hard-capped at 30 px (D6 one-line rule).
        assert_eq!(STATE, 30.0);
        let metric: f32 = METRIC;
        assert!(
            metric <= STATE,
            "the metric must not outgrow the capped state word"
        );
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

    /// The C pair set from `contrast-checks.md` / the token sheet §1, asserted
    /// in the crate so a value edit cannot silently regress AA (the same
    /// discipline the shipped dark canon used).
    #[test]
    fn measured_contrast_meets_the_aa_floors() {
        for (name, ink, plane, floor) in [
            ("ink/panel", text_1, bg_1, 7.0),
            ("ink/canvas", text_1, bg_0, 7.0),
            ("ink_2/panel", text_2, bg_1, 4.5),
            ("ink_2/canvas", text_2, bg_0, 4.5),
            ("dim/panel", text_3, bg_1, 4.5),
            ("dim/canvas", text_3, bg_0, 4.5),
            ("ice/panel", ice, bg_1, 7.0),
            ("ice/canvas", ice, bg_0, 7.0),
            ("ice_dim/panel", ice_dim, bg_1, 4.5),
            ("danger/panel", danger, bg_1, 4.5),
            ("danger/canvas", danger, bg_0, 4.5),
            ("warn/canvas", warn, bg_0, 4.5),
            ("on_accent/accent", on_accent, accent, 4.5),
            ("danger_text/danger_tint", danger_text, danger_soft, 4.5),
            ("danger/danger_tint", danger, danger_soft, 4.5),
        ] {
            let r = ratio(ink, plane);
            assert!(r >= floor, "{name} = {r:.2} < {floor}");
        }

        // Control boundaries and the focus ring (WCAG 1.4.11 / 2.4.13, 3:1).
        for (name, ink, plane) in [
            ("control_line/panel", border_control, bg_1),
            ("control_line/canvas", border_control, bg_0),
            ("focus/panel", focus, bg_1),
            ("focus/canvas", focus, bg_0),
            ("map_fleet/map_bg", map_fleet, map_bg),
        ] {
            let r = ratio(ink, plane);
            assert!(r >= 3.0, "{name} = {r:.2} < 3.0");
        }

        // Disabled ink is exempt from text AA but must stay perceivable (3:1 UI).
        let disabled = ratio(disabled_ink, disabled_plane);
        assert!(disabled >= 3.0, "disabled ink = {disabled:.2} < 3.0");

        // A test that cannot fail is a defect: the retired violet accent fails
        // as ink on the C canvas, proving the pair table is load-bearing.
        let retired_violet = Color32::from_rgb(0x6C, 0x5C, 0xE7);
        assert!(
            ratio(retired_violet, bg_0) < 4.5,
            "the retired accent should not pass as text ink on the C canvas"
        );
    }
}
