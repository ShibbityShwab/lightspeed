use super::a11y::{automation, AccessKitResponse};
use super::*;

// The counter/state rules moved to the shared crate at W2-1 S3a. Re-exported so
// existing call sites and tests address the same names; the widgets and copy
// below are presentation and stay.
#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::status::{
    history_delta, mode_counter, returned_count, routed_count, session_delta,
};

/// Parse a user-supplied port range string.
///
/// Accepted formats:
///  - `"28015-28999"` → `(28015, 28999)`
///  - `"28015"`       → `(28015, 28015)`
///  - `""`            → `None`  (blank → use default)
pub(super) fn parse_custom_port_range(s: &str) -> Option<(u16, u16)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((lo_s, hi_s)) = s.split_once('-') {
        let lo = lo_s.trim().parse::<u16>().ok()?;
        let hi = hi_s.trim().parse::<u16>().ok()?;
        if lo <= hi {
            Some((lo, hi))
        } else {
            None
        }
    } else {
        let p = s.parse::<u16>().ok()?;
        Some((p, p))
    }
}

/// The two treatments the action slot takes. Accent is the one bright fill;
/// Danger is the quiet destructive control.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionKind {
    Accent,
    Danger,
}

/// The one bright control — Direction C's single neon surface (token sheet
/// §4.5). Accent: lime fill (`c.accent`, hover `accent_hover`, pressed
/// `accent_pressed`), dark `on_accent` label, uppercase tracked mono, a painted
/// `▶` mark; Danger: transparent fill, 1 px danger outline, danger label and a
/// painted 8×8 stop square. Disabled/loading use the disabled plane/ink and no
/// accent. Focus paints the 2 px `c.focus` ring outside the rect and never
/// changes the fill. The control registers an AccessKit role/name through
/// `widget_info`.
///
/// **Wrap, never shrink**: a label whose tracked width exceeds the inner width
/// falls back to a wrapped two-line layout in `ACTION_H_WRAPPED`; the label is
/// never drawn below `EMPHASIS` and never ellipsised.
pub(super) fn action_button(
    ui: &mut egui::Ui,
    text: &str,
    kind: ActionKind,
    ready: bool,
    in_flight: bool,
    compact: bool,
) -> egui::Response {
    let font = egui::FontId::new(EMPHASIS, console());
    let tracking = TRACK_CTA;
    let mark_w = if kind == ActionKind::Danger { 8.0 } else { 5.0 };
    let mark = egui::vec2(mark_w, if kind == ActionKind::Danger { 8.0 } else { 6.0 });
    let label_w = tracked_width(ui.painter(), text, &font, tracking);
    let single_w = label_w + S2 + mark.x;
    let wrapped = single_w + 2.0 * S3 > ui.available_width();
    let height = if wrapped {
        ACTION_H_WRAPPED
    } else if compact {
        ACTION_H_COMPACT
    } else {
        ACTION_H
    };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ready && !in_flight, text));
    let response = response.a11y_automation_id(automation::ACTION_PRIMARY);
    let hovered = response.hovered() && ready && !in_flight;
    let pressed = response.is_pointer_button_down_on() && ready && !in_flight;

    let (fill, stroke, ink): (egui::Color32, egui::Color32, egui::Color32) = match kind {
        ActionKind::Accent => {
            if !ready || in_flight {
                (disabled_plane, disabled_plane, disabled_ink)
            } else if pressed {
                (accent_deep, accent_deep, on_accent)
            } else if hovered {
                (accent_hover, accent_hover, on_accent)
            } else {
                (accent, accent, on_accent)
            }
        }
        ActionKind::Danger => {
            if !ready || in_flight {
                (disabled_plane, disabled_plane, disabled_ink)
            } else if pressed {
                (bg_3, danger, danger)
            } else if hovered {
                (bg_2, danger, danger)
            } else {
                (egui::Color32::TRANSPARENT, danger, danger)
            }
        }
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(R_BUTTON),
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );

    if wrapped {
        // Wrap, never shrink: the group is vertically centred and the mark
        // stays aligned with the first line.
        let galley = painter.layout(
            text.to_owned(),
            font.clone(),
            ink,
            (ui.available_width() - 2.0 * S3 - mark.x - S2).max(60.0),
        );
        let group = egui::vec2(galley.size().x + mark.x + S2, galley.size().y);
        let top_left = rect.center() - group / 2.0;
        paint_action_mark(
            painter,
            kind,
            egui::pos2(
                top_left.x,
                top_left.y + galley.size().y / 2.0 - mark.y / 2.0,
            ),
            mark,
            ink,
            fill,
        );
        painter.galley(
            egui::pos2(top_left.x + mark.x + S2, top_left.y),
            galley,
            ink,
        );
    } else {
        let top_left = egui::pos2(
            rect.center().x - single_w / 2.0,
            rect.center().y - mark.y / 2.0,
        );
        paint_action_mark(painter, kind, top_left, mark, ink, fill);
        paint_tracked(
            painter,
            egui::pos2(top_left.x + mark.x + S2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            &font,
            ink,
            tracking,
        );
    }
    focus_ring(ui, &response, R_BUTTON);
    response
}

/// The CTA's painted mark: a `▶` triangle for the accent action, a stop square
/// for the danger action (no typed glyph — the bundled chains do not carry them).
fn paint_action_mark(
    painter: &egui::Painter,
    kind: ActionKind,
    top_left: egui::Pos2,
    mark: egui::Vec2,
    ink: egui::Color32,
    fill: egui::Color32,
) {
    match kind {
        ActionKind::Danger => {
            painter.rect_filled(
                egui::Rect::from_min_size(top_left, mark),
                0.0,
                if ink == disabled_ink { ink } else { danger },
            );
        }
        ActionKind::Accent => {
            // 55 % on-accent over the current lime fill (sheet 4.5).
            let _ = fill;
            let triangle_ink = egui::Color32::from_rgba_unmultiplied(0x0A, 0x0E, 0x02, 140);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    top_left,
                    egui::pos2(top_left.x, top_left.y + mark.y),
                    egui::pos2(top_left.x + mark.x, top_left.y + mark.y / 2.0),
                ],
                triangle_ink,
                egui::Stroke::NONE,
            ));
        }
    }
}
/// Whether the Game row should explain that nothing is selected yet, and which
/// key to say it with.
///
/// Only auto mode can be waiting on a game. A pinned choice is the user's own
/// and is already named in the dropdown, so captioning it "No game detected" -
/// the state the live audit caught, under a `Game: ROB RUN` picker - contradicts
/// the row it sits beneath. `None` means draw nothing.
pub(super) fn game_caption(
    auto: bool,
    manual: bool,
    detected: Option<&str>,
) -> Option<&'static str> {
    if detected.is_some() || manual || !auto {
        None
    } else {
        Some("game.none_detected_hint")
    }
}
