use super::*;

use crate::platform::Animations;

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

    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_name"));
        invalid_text_field(
            ui,
            &mut draft.name,
            i18n::t("game.editor_name_hint"),
            220.0,
            !verdict.name_ok,
        );
    });
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_process_names"));
        invalid_text_field(
            ui,
            &mut draft.process_names,
            i18n::t("game.editor_process_hint"),
            260.0,
            !verdict.processes_ok,
        );
    });
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
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_udp_range"));
        invalid_text_field(
            ui,
            &mut draft.udp_range,
            i18n::t("game.editor_udp_hint"),
            200.0,
            !verdict.udp_ok,
        );
    });
    // No TCP field: the interception path is UDP-only, so the schema refuses a
    // `tcp_ports` entry and the editor must not offer what a save would refuse.
    ui.checkbox(
        &mut draft.dynamic_server,
        egui::RichText::new(i18n::t("game.editor_dynamic_server")).size(LABEL),
    );
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_notes"));
        ui.add(
            egui::TextEdit::singleline(&mut draft.notes)
                .hint_text(i18n::t("game.editor_notes_hint"))
                .desired_width(260.0),
        );
    });

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
            .clicked()
        {
            actions.save = true;
        }
        if draft.editing.is_some() && ui.button(i18n::t("game.editor_delete")).clicked() {
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
    ui.horizontal(|ui| {
        section_label(ui, &i18n::t("game.editor_process_picker"));
        if ui.button(i18n::t("game.editor_process_refresh")).clicked() {
            *refresh = true;
        }
    });

    let width = ui.available_width();
    ui.add(
        egui::TextEdit::singleline(&mut *picker.search)
            .hint_text(i18n::t("game.editor_process_search_hint"))
            .desired_width(width),
    );

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
            if ui.link(i18n::t("game.target_clear")).clicked() {
                action.clear = true;
            }
        }
        None => {
            if ui
                .button(i18n::t("game.target_process"))
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

/// A config row: the label always sits above its control (mobile-only 320 px;
/// the retired wide breakpoint's side-by-side label column is gone), so the
/// control always keeps the full 288 px tap width.
pub(super) fn field_row(
    ui: &mut egui::Ui,
    _narrow: bool,
    label: &str,
    add_field: impl FnOnce(&mut egui::Ui),
) {
    ui.label(
        egui::RichText::new(label)
            .size(LABEL)
            .family(semibold())
            .color(text_1),
    );
    ui.add_space(S1);
    add_field(ui);
}

/// Section heading in the config region or a sheet body.
/// A 1px `border-1` hairline: the design's only divider between sections.
pub(super) fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, border_1);
}

pub(super) fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(LABEL)
            .family(semibold())
            .color(text_1),
    );
    ui.add_space(S1);
}

/// A sheet title (`title` type).
pub(super) fn sheet_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(TITLE)
            .family(semibold())
            .color(text_1),
    );
    ui.add_space(S2);
}

/// The shared frame for every in-window sheet: `bg-0`, a `border-1` hairline,
/// and the sheet radius.
pub(super) fn sheet_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(bg_0)
        .stroke(egui::Stroke::new(1.0, border_1))
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
        ui.add(egui::Spinner::new().size(16.0));
    }
    ui.label(egui::RichText::new(text).size(BODY).color(text_2));
    true
}

/// The notice / warning frame (`design-system` §6.10): a display-only line in
/// the scrolling region, danger or warn tone, `R_INLINE` radius, S2 inner
/// margin, up to the measured four lines. It is never a control.
pub(super) fn notice_frame(
    ui: &mut egui::Ui,
    danger_toned: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let (fill, ink) = if danger_toned {
        (danger_soft, danger)
    } else {
        // The warn-tone notice fill is the same 12 % tint pattern; the design
        // system keeps `warn` as ink and the card plane under the tint.
        (accent_soft, warn)
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, ink))
        .corner_radius(egui::CornerRadius::same(R_INLINE))
        .inner_margin(egui::Margin::same(S2 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(ink);
                add_contents(ui);
            });
        });
}

/// A focusable disclosure row: a painted marker (never a typed glyph — the
/// bundled chains carry no `▸`) plus a label. Returns the new open state.
///
/// The row is a real interactive control: a 24 px target, a focus ring, and
/// Space/Enter activation through egui's button semantics.
pub(super) fn disclosure_row(ui: &mut egui::Ui, label: &str, open: &mut bool) -> bool {
    let rect = ui.available_rect_before_wrap();
    let row_h = HIT_MIN;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(rect.width(), row_h), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, *open, label));
    if response.clicked() || response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
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
    response.clicked()
}

/// Group a counter's thousands with commas, so `12412` reads `12,412`.
pub(super) fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}
