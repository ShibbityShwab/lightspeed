use super::*;

use super::a11y::{automation, AccessKitResponse};
use crate::platform::Animations;
use eframe::egui::Color32;

/// What the custom-game editor asked for this frame.
///
/// The sheet cannot mutate the app while the body holds a `&mut` on the draft,
/// so the body records intent and the sheet applies it after the frame. That
/// record is also what the headless render test reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct EditorActions {
    /// "Edit" on one of the user's profiles, by index into the snapshot.
    pub(super) edit: Option<usize>,
    pub(super) save: bool,
    pub(super) delete: bool,
    /// "Refresh" on the process picker: re-read the host's process table.
    pub(super) refresh_processes: bool,
}

/// The custom-game editor's body: the user's profiles, the five fields, the live
/// verdict, the key, and the actions.
///
/// A free function rather than a method so the sheet can be rendered in a test
/// without an engine, a tray, or a config file.
pub(super) fn game_editor_body(
    ui: &mut egui::Ui,
    draft: &mut GameEditorDraft,
    verdict: &DraftVerdict,
    editor_saved: Option<&str>,
    editor_error: Option<&str>,
    user_games: &[UserGameProfile],
    mut picker: ProcessPicker<'_>,
    actions: &mut EditorActions,
) {
    // The user's profiles, each with the way back into it.
    section_label(ui, &i18n::t("game.editor_list"));
    if user_games.is_empty() {
        ui.label(
            egui::RichText::new(i18n::t("game.editor_no_games"))
                .size(BODY)
                .color(text_2),
        );
    }
    for (i, profile) in user_games.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&profile.name).color(text_1));
            ui.label(
                egui::RichText::new(&profile.key)
                    .size(CAPTION)
                    .color(text_3),
            );
            if ui.button(i18n::t("game.editor_edit")).clicked() {
                actions.edit = Some(i);
            }
        });
    }
    ui.add_space(S2);
    hairline(ui);
    ui.add_space(S2);

    editor_field(
        ui,
        &i18n::t("game.editor_name"),
        &mut draft.name,
        i18n::t("game.editor_name_hint"),
        !verdict.name_ok,
        automation::EDITOR_FIELD_NAME,
    );
    editor_field(
        ui,
        &i18n::t("game.editor_process_names"),
        &mut draft.process_names,
        i18n::t("game.editor_process_hint"),
        !verdict.processes_ok,
        automation::EDITOR_FIELD_PROCESSES,
    );
    // The picker sits directly under the field it fills: choosing a row appends
    // its image name there, and the field stays a plain typed field.
    let already = split_process_names(&draft.process_names);
    let chosen = process_picker(
        ui,
        &mut picker,
        |entry| {
            already
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&entry.name))
        },
        &mut actions.refresh_processes,
    );
    if let Some(entry) = chosen {
        draft.process_names = processes::append_name(&draft.process_names, &entry.name);
    }
    editor_field(
        ui,
        &i18n::t("game.editor_udp_range"),
        &mut draft.udp_range,
        i18n::t("game.editor_udp_hint"),
        !verdict.udp_ok,
        automation::EDITOR_FIELD_UDP_RANGE,
    );
    // No TCP field: the interception path is UDP-only, so the schema refuses a
    // `tcp_ports` entry and the editor must not offer what a save would refuse.
    ui.checkbox(
        &mut draft.dynamic_server,
        egui::RichText::new(i18n::t("game.editor_dynamic_server")).size(LABEL),
    )
    .a11y_automation_id(automation::EDITOR_DYNAMIC_SERVER);
    editor_field(
        ui,
        &i18n::t("game.editor_notes"),
        &mut draft.notes,
        i18n::t("game.editor_notes_hint"),
        false,
        automation::EDITOR_FIELD_NOTES,
    );

    // Live validation, named on the field that carries it.
    for reason in draft_reasons(verdict, draft.key().as_deref()) {
        ui.label(egui::RichText::new(reason).size(CAPTION).color(danger));
    }

    // The key, while it is being composed and after it is saved: this is what
    // `--game <key>` takes.
    if let Some(key) = draft.key() {
        ui.label(
            egui::RichText::new(i18n::t_with("game.editor_key", &[("key", key.as_str())]))
                .size(CAPTION)
                .color(if verdict.key_ok { text_2 } else { danger }),
        );
    }
    if let Some(saved) = editor_saved {
        ui.label(egui::RichText::new(saved).size(CAPTION).color(signal));
    }
    if let Some(error) = editor_error {
        ui.label(egui::RichText::new(error).size(CAPTION).color(danger));
    }

    ui.add_space(S2);
    ui.label(
        egui::RichText::new(i18n::t("game.editor_new_hint"))
            .size(CAPTION)
            .color(text_3),
    );
    ui.add_space(S2);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(verdict.ok(), egui::Button::new(i18n::t("game.editor_save")))
            .a11y_automation_id(automation::EDITOR_BUTTON_SAVE)
            .clicked()
        {
            actions.save = true;
        }
        if draft.editing.is_some()
            && ui
                .button(i18n::t("game.editor_delete"))
                .a11y_automation_id(automation::EDITOR_BUTTON_DELETE)
                .clicked()
        {
            actions.delete = true;
        }
    });
}

/// The live process picker: the machine's process table, filtered by a search
/// field, where choosing a row is reported back to the caller.
///
/// The rows are whatever the last background enumeration returned - nothing in
/// here enumerates or blocks, which is what makes it safe to draw from the egui
/// thread. Degrading honestly is the point of the status line: an enumeration
/// that came back empty (or a platform that will not enumerate at all) says so,
/// and the caller's own field (or process target) keeps working without it.
///
/// The same picker serves the editor's process-names field and the app's
/// process target: `highlighted` decides which rows draw as already chosen, and
/// a click on a row is handed back so the caller decides what choosing means.
/// `refresh` is set when the Refresh button is pressed.
pub(super) fn process_picker<'a>(
    ui: &mut egui::Ui,
    picker: &mut ProcessPicker<'a>,
    mut highlighted: impl FnMut(&ProcessEntry) -> bool,
    refresh: &mut bool,
) -> Option<&'a ProcessEntry> {
    ui.add_space(S2);
    section_label(ui, &i18n::t("game.editor_process_picker"));
    if ui
        .button(i18n::t("game.editor_process_refresh"))
        .a11y_automation_id(automation::PICKER_REFRESH)
        .clicked()
    {
        *refresh = true;
    }

    let width = ui.available_width();
    ui.add_sized(
        [width, CONTROL_H],
        egui::TextEdit::singleline(&mut *picker.search)
            .hint_text(i18n::t("game.editor_process_search_hint"))
            .desired_width(width),
    )
    .accessible_name(i18n::t("game.editor_process_search_hint"))
    .a11y_automation_id(automation::PICKER_SEARCH);

    // Copied out of the borrow of `picker` so the chosen row can be handed back
    // with the picker's own lifetime.
    let rows = picker.rows;
    match picker.status {
        PickerStatus::Loading => {
            ui.label(
                egui::RichText::new(i18n::t("game.editor_process_loading"))
                    .size(CAPTION)
                    .color(text_3),
            );
            None
        }
        PickerStatus::Ready => {
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new(i18n::t("game.editor_process_empty"))
                        .size(CAPTION)
                        .color(text_3),
                );
                None
            } else {
                let shown = processes::matching(rows, picker.search);
                if shown.is_empty() {
                    ui.label(
                        egui::RichText::new(i18n::t("game.editor_process_no_match"))
                            .size(CAPTION)
                            .color(text_3),
                    );
                }
                let mut chosen = None;
                for entry in shown {
                    if process_row(ui, entry, highlighted(entry)).clicked() {
                        chosen = Some(entry);
                    }
                }
                chosen
            }
        }
    }
}

/// What one frame of the target front door asks the app to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TargetRowAction {
    /// "Target a process" was pressed: open the picker.
    pub(super) open_picker: bool,
    /// "Clear target" was pressed: back to the built-in Game path.
    pub(super) clear: bool,
}

/// The front door, as a bare widget: the picked process and the way back to the
/// built-in Game path, or the way in to the picker.
///
/// A frame with a target names it - `Target: <name> (PID <n>)` - and offers
/// Clear; a frame without one offers the picker and says what it does. It
/// records intent instead of mutating the app, so the caller applies it after
/// the frame and the widget renders headlessly in a test.
pub(super) fn target_front_door(
    ui: &mut egui::Ui,
    target: Option<&ProcessTarget>,
) -> TargetRowAction {
    let mut action = TargetRowAction::default();
    ui.horizontal_wrapped(|ui| match target {
        Some(target) => {
            ui.label(
                egui::RichText::new(target_summary(target))
                    .size(LABEL)
                    .color(text_1),
            );
            if ui
                .link(i18n::t("game.target_clear"))
                .a11y_automation_id(automation::TARGET_CLEAR)
                .clicked()
            {
                action.clear = true;
            }
        }
        None => {
            if ui
                .button(i18n::t("game.target_process"))
                .a11y_automation_id(automation::TARGET_PROCESS)
                .on_hover_text(i18n::t("game.target_none"))
                .clicked()
            {
                action.open_picker = true;
            }
        }
    });
    action
}
///
/// The whole row is the button - not a small control on it - and because it is
/// a `selectable_label`, hover and the "already in the field" state come from
/// the theme's own widget visuals instead of a hand-rolled fill. Choosing a row
/// whose name the field already carries repaints the same row rather than
/// changing anything: the append dedupes case-insensitively.
pub(super) fn process_row(
    ui: &mut egui::Ui,
    entry: &ProcessEntry,
    in_field: bool,
) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &entry.name,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(BODY, semibold()),
            color: if in_field { text_3 } else { text_1 },
            ..Default::default()
        },
    );
    job.append(
        &format!("  {}", entry.pid),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(CAPTION, egui::FontFamily::Monospace),
            color: text_3,
            ..Default::default()
        },
    );
    if let Some(path) = &entry.path {
        job.append(
            &format!(
                "  {}",
                processes::tail_path(path, processes::PATH_TAIL_CHARS)
            ),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(CAPTION),
                color: text_3,
                ..Default::default()
            },
        );
    }
    ui.selectable_label(in_field, job)
}

/// A single-line field whose ink turns `danger` while its content is invalid,
/// with the existing `proxies.invalid` marker beside it.
///
/// The one implementation of the invalid-input treatment: the Manual flow's
/// port range and every field of the profile editor share it, so "invalid"
/// never looks like two different things.
/// A stacked console editor field: the 11 px label on its own line, then the
/// full sheet width. At 320 px there is no room for inline label+input, so the
/// C editor stacks instead of clipping (UX review C-1 defect 2).
fn editor_field(
    ui: &mut egui::Ui,
    label: &str,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    invalid: bool,
    automation_id: &str,
) {
    tracked_label(
        ui,
        &label.to_uppercase(),
        CAPTION,
        if invalid { danger } else { text_3 },
        TRACK_LABEL,
    );
    let width = ui.available_width();
    // The visible label above the field is a separate painted node; name the
    // field with the same localized words and give automation a stable id.
    ui.add_sized(
        [width, CONTROL_H],
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .text_color(if invalid { danger } else { text_1 }),
    )
    .accessible_name(label)
    .a11y_automation_id(automation_id);
    if invalid {
        ui.label(
            egui::RichText::new(i18n::t("proxies.invalid"))
                .size(CAPTION)
                .color(danger),
        );
    }
    ui.add_space(S1);
}

pub(super) fn invalid_text_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    width: f32,
    invalid: bool,
) -> egui::Response {
    let ink = if invalid { danger } else { text_1 };
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .text_color(ink),
    );
    if invalid {
        ui.label(egui::RichText::new(i18n::t("proxies.invalid")).color(danger));
    }
    response
}

/// Section heading in the config region or a sheet body.
/// A 1px `border-1` hairline: the design's only divider between sections.
pub(super) fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, border_1);
}

/// A section label in the console voice: 11 px mono, uppercase, 0.24em
/// tracking, `dim` ink (token sheet Â§2/Â§4.7); the catalogs stay sentence case.
pub(super) fn section_label(ui: &mut egui::Ui, text: &str) {
    tracked_label(ui, &text.to_uppercase(), CAPTION, text_3, TRACK_SECTION);
    ui.add_space(S1);
}

/// A sheet title in the console voice: mono 12 px, uppercase, 0.24em tracking.
pub(super) fn sheet_title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    // Returned so the sheet can attach its stable automation id to the title.
    let response = ui.label(
        egui::RichText::new(format!("/// {}", text.to_uppercase()))
            .size(TITLE)
            .family(console())
            .color(text_1)
            .extra_letter_spacing(TRACK_SECTION * TITLE),
    );
    ui.add_space(S2);
    response
}

/// The shared frame for every in-window sheet: `bg-0`, a `border-1` hairline,
/// and the sheet radius.
pub(super) fn sheet_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(bg_1)
        .stroke(egui::Stroke::new(1.0, border_2))
        .corner_radius(egui::CornerRadius::same(R_SHEET))
        .inner_margin(egui::Margin::same(S4 as i8))
}

/// The capped width and height a sheet may take, so a pinned sheet never
/// overflows a 320 px window.
pub(super) fn sheet_bounds(ctx: &egui::Context) -> (f32, f32) {
    let screen = ctx.content_rect();
    let max_w = (screen.width() - 24.0 - 2.0 * S4).min(420.0 - 2.0 * S4);
    // Leave room for the title and the Close button inside the sheet itself: the
    // old 120 px reserve plus an unscaled body pushed the last rows past the
    // bottom of the window at 320 px wide.
    let max_h = (screen.height() - 24.0).max(160.0);
    (max_w, max_h)
}

/// The 2 px keyboard-focus ring, painted **outside** the control rect with a
/// radius 2 px larger (`design-system` §6.1 rule 2). `accent_text` reads
/// 7.51–8.11:1 on every plane it can sit on; focus never changes the fill.
pub(super) fn focus_ring(ui: &egui::Ui, response: &egui::Response, radius: u8) {
    if !response.has_focus() {
        return;
    }
    let rect = response.rect.expand(FOCUS_RING_W);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(radius.saturating_add(2)),
        egui::Stroke::new(FOCUS_RING_W, accent_text),
        egui::StrokeKind::Outside,
    );
}

/// One waiting indicator for every wait in the shell.
///
/// Motion policy: a spinner is shown **only** when the caller's
/// [`Animations`] is explicitly `On`; `Off` and `Unknown` render the static
/// text (the WCAG 2.2.2 pause/stop alternative). The signal is the platform's
/// one OS read (W2-4 (i): Windows `SPI_GETCLIENTAREAANIMATION`, macOS/Linux
/// `Unknown` gaps), held on the app for the process lifetime.
pub(super) fn progress_indicator(ui: &mut egui::Ui, text: &str, animations: Animations) -> bool {
    if animations.allows_motion() {
        // The adjacent text carries the meaning; the spinner is decorative.
        ui.add(egui::Spinner::new().size(16.0)).a11y_hidden();
    }
    ui.label(egui::RichText::new(text).size(BODY).color(text_2));
    true
}

/// The notice / warning frame (`design-system` §6.10): a display-only line in
/// the scrolling region, danger or warn tone, `R_INLINE` radius, S2 inner
/// margin, up to the measured four lines. It is never a control.
/// The notice / warning frame (token sheet Â§4.8/Â§4.9): flat fill, 1 px border
/// and a painted 3 px left edge. Danger uses the composited `danger_tint`; the
/// warn tone keeps the inset plane and warn ink.
pub(super) fn notice_frame(
    ui: &mut egui::Ui,
    danger_toned: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let (fill, edge, ink) = if danger_toned {
        (danger_soft, danger, danger)
    } else {
        (bg_2, warn, warn)
    };
    let response = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, edge))
        .corner_radius(egui::CornerRadius::same(R_TAG))
        .inner_margin(egui::Margin {
            left: S3 as i8 + 3,
            right: S3 as i8,
            top: S3 as i8,
            bottom: S3 as i8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(ink);
                add_contents(ui);
            });
        })
        .response;
    let rect = response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 1.0),
            egui::vec2(3.0, (rect.height() - 2.0).max(0.0)),
        ),
        0.0,
        edge,
    );
    response
}
/// A focusable disclosure row: a painted marker (never a typed glyph — the
/// bundled chains carry no `▸`) plus a label. Returns the new open state.
///
/// The row is a real interactive control: a 24 px target, a focus ring, and
/// Space/Enter activation through egui's button semantics.
pub(super) fn disclosure_row(
    ui: &mut egui::Ui,
    label: &str,
    open: &mut bool,
    automation_id: Option<&str>,
) {
    let rect = ui.available_rect_before_wrap();
    let row_h = HIT_MIN;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(rect.width(), row_h), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let response = response.a11y_disclosure(*open);
    let response = match automation_id {
        Some(id) => response.a11y_automation_id(id),
        None => response,
    };
    // A screen reader's Expand/Collapse pattern arrives as an AccessKit action;
    // egui drops those, so the widget owns the handling (and must not treat the
    // same event as a click as well).
    if let Some(requested) = super::a11y::disclosure_action(ui, response.id) {
        *open = requested;
    } else if response.clicked()
        || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
    {
        *open = !*open;
    }
    let ink = if response.hovered() { text_1 } else { text_2 };
    let painter = ui.painter();
    // Painted chevron: 6 px line + 4 px head, mirrored when open.
    let cx = rect.left() + S2;
    let cy = rect.center().y;
    let dir = if *open { 1.0 } else { -1.0 };
    painter.add(egui::Shape::line(
        vec![
            egui::pos2(cx - 3.0, cy - 2.0 * dir),
            egui::pos2(cx + 3.0, cy + 2.0 * dir),
        ],
        egui::Stroke::new(1.5, ink),
    ));
    painter.text(
        egui::pos2(rect.left() + S2 + S3, cy),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(BODY, semibold()),
        ink,
    );
    focus_ring(ui, &response, R_INLINE);
}

// â”€â”€ Direction C console components (text, sections, rows, footer) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Paint text with letter-spacing (egui has no tracking) and return its width.
pub(super) fn paint_tracked(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: &str,
    font: &egui::FontId,
    color: Color32,
    tracking_em: f32,
) -> f32 {
    let tracking = tracking_em * font.size;
    let width = tracked_width(painter, text, font, tracking_em);
    let height = painter
        .layout_no_wrap(text.to_owned(), font.clone(), color)
        .size()
        .y;
    let rect = align.anchor_size(at, egui::vec2(width, height));
    let mut x = rect.left();
    for ch in text.chars() {
        let galley = painter.layout_no_wrap(ch.to_string(), font.clone(), color);
        let advance = galley.size().x;
        painter.galley(egui::pos2(x, rect.top()), galley, color);
        x += advance + tracking;
    }
    width
}

/// The advance width of tracked text (measure only).
pub(super) fn tracked_width(
    painter: &egui::Painter,
    text: &str,
    font: &egui::FontId,
    tracking_em: f32,
) -> f32 {
    let tracking = tracking_em * font.size;
    let mut width = 0.0;
    for (i, ch) in text.chars().enumerate() {
        if i > 0 {
            width += tracking;
        }
        width += painter
            .layout_no_wrap(ch.to_string(), font.clone(), text_1)
            .size()
            .x;
    }
    width
}

/// One line of tracked console text as a laid-out label. Uses egui's own
/// letter-spacing so the full string stays one text shape (screen readers,
/// shape-level tests and copy review can read it).
pub(super) fn tracked_label(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    color: Color32,
    tracking_em: f32,
) {
    ui.label(
        egui::RichText::new(text)
            .size(size)
            .family(console())
            .color(color)
            .extra_letter_spacing(tracking_em * size),
    );
}

/// A section row: `NN NAME â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€ META` (token sheet Â§4.7).
pub(super) fn console_section(
    ui: &mut egui::Ui,
    index: Option<&str>,
    name: Option<&str>,
    meta: Option<&str>,
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SECTION_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let font = egui::FontId::new(SECTION, console());
    let mut x = rect.left();
    if let Some(index) = index {
        let w = paint_tracked(
            painter,
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            index,
            &font,
            ice_dim,
            TRACK_SECTION,
        );
        x += w + S2;
    }
    let mut rule_from = x;
    if let Some(name) = name {
        let name = name.to_uppercase();
        let w = paint_tracked(
            painter,
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &name,
            &font,
            text_3,
            TRACK_SECTION,
        );
        rule_from = x + w + S2;
    }
    let mut rule_to = rect.right();
    if let Some(meta) = meta {
        let meta = meta.to_uppercase();
        let w = tracked_width(painter, &meta, &font, TRACK_META);
        paint_tracked(
            painter,
            egui::pos2(rect.right(), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            &meta,
            &font,
            text_3,
            TRACK_META,
        );
        rule_to = rect.right() - w - S2;
    }
    if rule_to > rule_from + S1 {
        painter.line_segment(
            [
                egui::pos2(rule_from, rect.center().y),
                egui::pos2(rule_to, rect.center().y),
            ],
            egui::Stroke::new(1.0, border_2),
        );
    }
    // A section header is painted, not added: expose the words it draws so a
    // screen reader can orient inside the rail ("01 ROUTE …").
    let spoken = [index, name, meta]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ");
    if !spoken.is_empty() {
        response.a11y_text(spoken);
    }
}

/// Whether a clickable response was activated by pointer or keyboard.
pub(super) fn activated(ui: &egui::Ui, response: &egui::Response) -> bool {
    response.clicked()
        || (response.has_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Space)))
}

/// A flat console text link: 11 px mono, uppercase, `ice` at rest, hover plane
/// behind the row, 24 px target, focus ring.
pub(super) fn console_link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let label = text.to_uppercase();
    let font = egui::FontId::new(CAPTION, console());
    let width = tracked_width(ui.painter(), &label, &font, TRACK_LABEL);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width + 2.0 * S2, HIT_MIN), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(R_TAG), bg_2);
    }
    paint_tracked(
        ui.painter(),
        rect.center(),
        egui::Align2::CENTER_CENTER,
        &label,
        &font,
        if response.hovered() {
            text_1
        } else {
            accent_text
        },
        TRACK_LABEL,
    );
    focus_ring(ui, &response, R_TAG);
    response
}

/// A two-state console toggle: label left, `[ â–  ]` bracket right. No English
/// ON/OFF literal (D-C1): the checked state is painted and exposed through the
/// checkbox role; the label wraps rather than clipping under the bracket.
pub(super) fn console_toggle(ui: &mut egui::Ui, label: &str, checked: bool) -> egui::Response {
    let width = ui.available_width();
    let text_w = (width - 34.0 - 3.0 * S2).max(80.0);
    let font = egui::FontId::new(CAPTION, console());
    let galley = ui
        .painter()
        .layout(label.to_uppercase(), font.clone(), text_1, text_w);
    let height = (galley.size().y + 2.0 * S1).max(CONTROL_H);
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, checked, label));
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(R_TAG), bg_2);
    }
    let painter = ui.painter();
    painter.galley(
        egui::pos2(rect.left() + S2, rect.center().y - galley.size().y / 2.0),
        galley,
        text_1,
    );
    let bracket = egui::Rect::from_min_size(
        egui::pos2(rect.right() - S2 - 34.0, rect.center().y - 10.0),
        egui::vec2(34.0, 20.0),
    );
    painter.rect(
        bracket,
        egui::CornerRadius::same(R_TAG),
        bg_1,
        egui::Stroke::new(1.0, border_control),
        egui::StrokeKind::Inside,
    );
    let mark = egui::Rect::from_center_size(bracket.center(), egui::vec2(8.0, 8.0));
    if checked {
        painter.rect_filled(mark, 0.0, accent_text);
    } else {
        painter.rect_stroke(
            mark,
            0.0,
            egui::Stroke::new(1.0, text_3),
            egui::StrokeKind::Inside,
        );
    }
    focus_ring(ui, &response, R_TAG);
    if activated(ui, &response) {
        response.mark_changed();
    }
    response
}

/// The console footer: evenly spaced flat links with painted ` / ` separators;
/// labels are fitted into their slot by solving the tracking within the 11 px
/// floor. Returns the activated index.
pub(super) fn console_footer(
    ui: &mut egui::Ui,
    labels: &[&str],
    height: f32,
    automation_ids: &[&str],
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let columns = labels.len().max(1) as f32;
    let mut clicked = None;
    let font = egui::FontId::new(CAPTION, console());
    for (i, label) in labels.iter().enumerate() {
        let left = rect.left() + rect.width() * i as f32 / columns;
        let right = rect.left() + rect.width() * (i + 1) as f32 / columns;
        let column = egui::Rect::from_min_max(
            egui::pos2(left, rect.top()),
            egui::pos2(right, rect.bottom()),
        );
        let target = column.shrink2(egui::vec2(S1, 0.0));
        let response = ui.interact(
            target,
            ui.scope_id().with(("console_footer", *label, i)),
            egui::Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, *label));
        let response = match automation_ids.get(i) {
            Some(id) => response.a11y_automation_id(id),
            None => response,
        };
        if response.hovered() {
            ui.painter()
                .rect_filled(target, egui::CornerRadius::same(R_TAG), bg_2);
        }
        let upper = label.to_uppercase();
        let column_avail = (column.width() - 3.0 * S2).max(1.0);
        let base = tracked_width(ui.painter(), &upper, &font, 0.0);
        let fit_tracking = if base >= column_avail {
            0.0
        } else {
            ((column_avail - base) / (font.size * (upper.chars().count().max(1) - 1) as f32))
                .clamp(0.0, TRACK_META)
        };
        paint_tracked(
            ui.painter(),
            column.center(),
            egui::Align2::CENTER_CENTER,
            &upper,
            &font,
            if response.hovered() { text_1 } else { text_3 },
            fit_tracking,
        );
        focus_ring(ui, &response, R_TAG);
        if activated(ui, &response) {
            clicked = Some(i);
        }
        if i + 1 < labels.len() {
            paint_tracked(
                ui.painter(),
                egui::pos2(right, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "/",
                &font,
                text_3,
                0.0,
            );
        }
    }
    clicked
}

/// One console data row: 11 px tracked `dim` label left, right-aligned value in
/// the shipped mono chain (or prose when asked), a 1 px `c.rule` separator.
pub(super) fn console_row(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    value_ink: egui::Color32,
    value_size: f32,
    mono: bool,
    draw_rule: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    let label_font = egui::FontId::new(CAPTION, console());
    paint_tracked(
        painter,
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label.to_uppercase(),
        &label_font,
        text_3,
        TRACK_LABEL,
    );
    let value_font = egui::FontId::new(value_size, if mono { console() } else { prose() });
    paint_tracked(
        painter,
        egui::pos2(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        value,
        &value_font,
        value_ink,
        TRACK_VALUE,
    );
    if draw_rule {
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.bottom() - 0.5),
                egui::pos2(rect.right(), rect.bottom() - 0.5),
            ],
            egui::Stroke::new(1.0, border_1),
        );
    }
    // The pair is painted; the accessible name is the one sentence a reader
    // needs ("RELAY: LAX — Los Angeles").
    response.a11y_text(format!("{label}: {value}"))
}

/// A stacked sentence row: a full-sentence "label" (e.g.
/// `route.cannot_beat_direct`) is prose above its detail instead of a label
/// column (token sheet Â§4.3's variable/stacked model).
pub(super) fn console_sentence_row(
    ui: &mut egui::Ui,
    sentence: &str,
    detail: Option<&str>,
    draw_rule: bool,
) -> egui::Response {
    let paper = ui.painter().clone();
    let width = ui.available_width();
    let font = egui::FontId::new(PROSE, prose());
    let galley = paper.layout(sentence.to_owned(), font.clone(), text_2, width);
    let mut height = galley.size().y + S1;
    let detail_galley = detail.map(|detail| {
        let galley = paper.layout(detail.to_owned(), font, text_3, width);
        height += galley.size().y + S1;
        galley
    });
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, height.max(ROW_H_SENTENCE)),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.galley(egui::pos2(rect.left(), rect.top() + S1), galley, text_2);
    if let Some(detail_galley) = detail_galley {
        painter.galley(
            egui::pos2(rect.left(), rect.bottom() - S1 - detail_galley.size().y),
            detail_galley,
            text_3,
        );
    }
    if draw_rule {
        painter.line_segment(
            [
                egui::pos2(rect.left(), rect.bottom() - 0.5),
                egui::pos2(rect.right(), rect.bottom() - 0.5),
            ],
            egui::Stroke::new(1.0, border_1),
        );
    }
    let spoken = match detail {
        Some(detail) => format!("{sentence} {detail}"),
        None => sentence.to_string(),
    };
    response.a11y_text(spoken)
}

/// A prose sentence in the Inter voice (banner body, notes, error detail).
pub(super) fn prose_label(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.label(
        egui::RichText::new(text)
            .size(size)
            .family(prose())
            .color(color),
    );
}

/// A Direction C editorial block: prose in a 2 px `ice_dim` left rule.
pub(super) fn console_editorial(ui: &mut egui::Ui, text: &str) {
    let width = ui.available_width();
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::new(PROSE, prose()),
        text_2,
        width - 10.0,
    );
    let height = galley.size().y + S2;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.top() + 2.0),
            egui::vec2(2.0, height - 4.0),
        ),
        0.0,
        ice_dim,
    );
    painter.galley(
        egui::pos2(rect.left() + 10.0, rect.top() + S2 / 2.0),
        galley,
        text_2,
    );
    response.a11y_text(text);
}

/// A two-column mini row for the Components table: 11 px label `dim`, 11 px
/// value `ink_2`, 20 px inline rows. When the pair does not fit the sheet body
/// the row **stacks** (label line, wrapped value below) instead of colliding â€”
/// the review's Major 1 fixture is `("Npcap", "not installed (pcap fallback only)")`.
pub(super) fn console_mini_row(ui: &mut egui::Ui, label: &str, value: &str) {
    let width = ui.available_width();
    let font = egui::FontId::new(CAPTION, console());
    let label_up = label.to_uppercase();
    let label_w = tracked_width(ui.painter(), &label_up, &font, TRACK_LABEL);
    let value_w = tracked_width(ui.painter(), value, &font, 0.0);
    if row_fits(label_w, value_w, width) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::hover());
        let painter = ui.painter();
        paint_tracked(
            painter,
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            &label_up,
            &font,
            text_3,
            TRACK_LABEL,
        );
        paint_tracked(
            painter,
            egui::pos2(rect.right(), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            value,
            &font,
            text_2,
            0.0,
        );
        response.a11y_text(format!("{label}: {value}"));
        return;
    }
    let value_galley = ui
        .painter()
        .layout(value.to_owned(), font.clone(), text_2, width);
    let height = 16.0 + S1 + value_galley.size().y + S1;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height.max(20.0)), egui::Sense::hover());
    let painter = ui.painter();
    paint_tracked(
        painter,
        egui::pos2(rect.left(), rect.top() + CAPTION),
        egui::Align2::LEFT_CENTER,
        &label_up,
        &font,
        text_3,
        TRACK_LABEL,
    );
    painter.galley(
        egui::pos2(rect.left(), rect.top() + 16.0 + S1),
        value_galley,
        text_2,
    );
    response.a11y_text(format!("{label}: {value}"));
}

/// Whether a label/value pair fits one console row: the variable row model
/// stacks instead of clipping when it does not (token sheet Â§4.3).
pub(super) fn row_fits(label_w: f32, value_w: f32, width: f32) -> bool {
    label_w + S4 + value_w <= width
}

/// A Direction C inline control row: 11 px uppercase label left, the field
/// right-aligned in the remaining width, row height `CONTROL_H` (Â§4.7).
pub(super) fn console_field_row(
    ui: &mut egui::Ui,
    label: &str,
    add_field: impl FnOnce(&mut egui::Ui),
) {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), CONTROL_H),
        egui::Sense::hover(),
    );
    let label_font = egui::FontId::new(CAPTION, console());
    let label_w = tracked_width(
        ui.painter(),
        &label.to_uppercase(),
        &label_font,
        TRACK_LABEL,
    );
    paint_tracked(
        ui.painter(),
        egui::pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        &label.to_uppercase(),
        &label_font,
        text_3,
        TRACK_LABEL,
    );
    let field = egui::Rect::from_min_max(
        egui::pos2(rect.left() + label_w + S2, rect.top()),
        rect.right_bottom(),
    );
    let field_w = field.width().max(60.0);
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                field.min,
                egui::vec2(field_w, CONTROL_H),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.set_width(field_w);
            add_field(ui);
        },
    );
    // The row's painted label names the control beside it; expose it as its
    // own text node so the association is readable in order.
    response.a11y_text(label.to_uppercase());
}
