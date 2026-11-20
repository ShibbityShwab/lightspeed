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

/// The one action slot, as a pure function of the gates.
///
/// The *decision* is [`lightspeed_client::app::status::action_slot`] (shared
/// with the CLI); this is only its copy: the three labels plus the disabled
/// case. Copy stays a front-end concern by the boundary rules, so the wording
/// can change without touching the gate arithmetic.
///
/// Returned upper-case because the action slot draws in caps; `to_uppercase`
/// leaves a locale without letter case (ja, ko, zh-Hans) untouched.
pub(super) fn primary_action(
    is_admin: bool,
    boosting: bool,
    connected: bool,
    has_relay: bool,
) -> (String, bool) {
    use lightspeed_client::app::status::{action_slot, ActionSlot};
    match action_slot(is_admin, boosting, connected, has_relay) {
        ActionSlot::StopOptimizing => (i18n::t("action.stop_optimizing").to_uppercase(), true),
        ActionSlot::RestartElevated => (
            i18n::t("action.restart_admin_optimize").to_uppercase(),
            true,
        ),
        ActionSlot::StartReady => (i18n::t("action.optimize_route").to_uppercase(), true),
        ActionSlot::StartBlocked => (i18n::t("action.optimize_route").to_uppercase(), false),
    }
}

/// The two treatments the action slot takes. Accent is the one bright fill;
/// Danger is the quiet destructive control.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionKind {
    Accent,
    Danger,
}

/// The one bright control: a custom-painted button so it never reads as
/// default egui chrome. Accent uses `accent-deep` at rest (white ink on it is
/// AA-safe) and brightens to `accent` on hover; Danger uses `bg-3` with a
/// danger hairline and ink, tinting with `danger-soft` on hover.
pub(super) fn action_button(
    ui: &mut egui::Ui,
    text: &str,
    kind: ActionKind,
    ready: bool,
) -> egui::Response {
    // Shrink to fit rather than wrap: "RESTART AS ADMINISTRATOR TO OPTIMIZE" is
    // wider than a 320 px window at the emphasis size, and a two-line button
    // pushed the rest of the column down. The type scales down until the line
    // fits, with a floor so it stays readable.
    let inner_w = (ui.available_width() - 24.0).max(80.0);
    let mut size = EMPHASIS;
    let measured =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::new(size, semibold()), text_1);
    if measured.size().x > inner_w {
        size = (EMPHASIS * inner_w / measured.size().x).max(9.0);
    }
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ACTION_H),
        egui::Sense::click(),
    );
    let hovered = response.hovered() && ready;

    let (fill, stroke, ink) = match kind {
        ActionKind::Accent => {
            if !ready {
                (bg_3, border_1, text_3)
            } else if hovered {
                (accent, accent, on_accent)
            } else {
                (accent_deep, accent_deep, on_accent)
            }
        }
        ActionKind::Danger => {
            if !ready {
                (bg_3, border_1, text_3)
            } else if hovered {
                (danger_soft, danger, danger)
            } else {
                (bg_3, danger, danger)
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
    if kind == ActionKind::Danger && ready {
        // A painted stop square: U+25A0 lives only in the icon font, so the
        // mark is drawn rather than typed (DESIGN.md rule 1).
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            egui::vec2(10.0, 10.0),
        );
        painter.rect_filled(square, 0.0, danger);
    }
    let galley = painter.layout_no_wrap(text.to_owned(), egui::FontId::new(size, semibold()), ink);
    let pos = rect.center() - galley.size() / 2.0;
    painter.galley(pos, galley, ink);
    response
}

/// One value in the label/value ledger.
pub(super) enum LedgerValue {
    /// Genuine data (addresses, ports, counters): JetBrains Mono.
    Data(String),
    /// A dim prose hint where there is no value to show.
    Hint(Cow<'static, str>),
    /// Data that is working as designed but degraded, in the warning colour.
    Warn(Cow<'static, str>),
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

/// The `Relayed` row: what left this host, and what the relay sent back.
///
/// The starving flag is decided by the shared [`RelayedPair`] rule (one source
/// pair, one decision), never by this formatter.
pub(super) fn relayed_row_text(sent: u64, returned: u64) -> (String, bool) {
    let text = i18n::t_with(
        "route.sent_returned",
        &[
            ("sent", group_digits(sent).as_str()),
            ("returned", group_digits(returned).as_str()),
        ],
    );
    (
        text,
        lightspeed_client::app::dto::RelayedPair { sent, returned }.starving(),
    )
}

/// The ledger's label cell: caption ink, fixed width, left-aligned.
pub(super) fn ledger_label(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(CAPTION),
        text_3,
    );
}

/// A config field's label cell: `label` ink, fixed width at the wide breakpoint.
pub(super) fn field_label(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::new(LABEL, semibold()),
        text_1,
    );
}
