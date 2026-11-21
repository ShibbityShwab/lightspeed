use super::*;

// ── Pure helpers (no platform dependency) ─────────────────────────────────────

/// Build the font families the UI draws with, for `locale`.
///
/// Direction C's voice is monospace (`FontFamily::Monospace`), with Inter
/// (`FontFamily::Proportional`) only for prose sentences. The bundled
/// JetBrains Mono is Latin-only (229 codepoints: no Cyrillic, no CJK), so the
/// monospace chain ends with the locale-ordered CJK faces and epaint's built-in
/// Ubuntu-Light — the documented fallback of `docs/design/gui-c-token-sheet.md`
/// §2. RU therefore draws Cyrillic in Ubuntu-Light (proportional) and CJK text
/// in Noto at the same size; ASCII digits/addresses always stay JetBrains Mono,
/// so tabular numerals and data alignment hold in every locale. No tofu: every
/// catalog codepoint resolves through one of these faces (the coverage gate).
///
/// `locale` chooses only the ORDER of the three CJK faces. Noto's Japanese,
/// Korean and Simplified-Chinese subsets each carry the shared Han repertoire in
/// their own regional forms - 127 of the Chinese catalog's 172 Han characters
/// also exist in the Japanese face - so a fixed order would draw Chinese text
/// with Japanese glyphs.
pub(crate) fn font_definitions(locale: Option<&str>) -> egui::FontDefinitions {
    use std::sync::Arc;

    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "inter",
            include_bytes!("../../assets/fonts/Inter-Regular.ttf") as &[u8],
        ),
        (
            "inter_semibold",
            include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
        ),
        (
            "jbmono",
            include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
        ),
        (
            "cjk_jp",
            include_bytes!("../../assets/fonts/NotoSansJP-Regular.otf"),
        ),
        (
            "cjk_kr",
            include_bytes!("../../assets/fonts/NotoSansKR-Regular.otf"),
        ),
        (
            "cjk_sc",
            include_bytes!("../../assets/fonts/NotoSansSC-Regular.otf"),
        ),
        (
            "cjk_jp_bold",
            include_bytes!("../../assets/fonts/NotoSansJP-Bold.otf"),
        ),
        (
            "cjk_kr_bold",
            include_bytes!("../../assets/fonts/NotoSansKR-Bold.otf"),
        ),
        (
            "cjk_sc_bold",
            include_bytes!("../../assets/fonts/NotoSansSC-Bold.otf"),
        ),
    ] {
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
    }

    let (cjk_regular, _cjk_bold) = cjk_order(locale);
    let builtin = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    // Prose: Inter first, then the CJK faces, then the built-in Latin/Cyrillic
    // fallback (Ubuntu-Light). Inter is a Latin subset only.
    let mut proportional = vec!["inter".to_owned()];
    proportional.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    proportional.extend(builtin.iter().cloned());

    // The console voice: JetBrains Mono (Latin + digits) → locale-ordered CJK
    // → the built-in Ubuntu-Light for Cyrillic and any remaining marks.
    let mut monospace = vec!["jbmono".to_owned()];
    monospace.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    monospace.extend(builtin);

    fonts
        .families
        .insert(egui::FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, monospace);
    fonts
}

/// The CJK faces for a locale, own-regional-form first, as (regular, bold).
pub(super) fn cjk_order(locale: Option<&str>) -> ([&'static str; 3], [&'static str; 3]) {
    match locale {
        Some("ko") => (
            ["cjk_kr", "cjk_jp", "cjk_sc"],
            ["cjk_kr_bold", "cjk_jp_bold", "cjk_sc_bold"],
        ),
        Some("zh-Hans") => (
            ["cjk_sc", "cjk_jp", "cjk_kr"],
            ["cjk_sc_bold", "cjk_jp_bold", "cjk_kr_bold"],
        ),
        _ => (
            ["cjk_jp", "cjk_kr", "cjk_sc"],
            ["cjk_jp_bold", "cjk_kr_bold", "cjk_sc_bold"],
        ),
    }
}

/// Install the bundled fonts.
///
/// Must run BEFORE the first frame, which is why `main.rs` calls it from the
/// eframe creation context rather than from `apply_theme`: `set_fonts` only
/// takes effect on the next frame, and the UI reaches for the named family
/// immediately, so binding them late panics with "FontFamily::Name(..) is not
/// bound to any fonts".
pub(crate) fn install_fonts(ctx: &egui::Context, locale: Option<&str>) {
    ctx.set_fonts(font_definitions(locale));
}

/// Direction C's one weight: monospace carries emphasis too (the sheet forbids
/// faux bold). Kept as a named helper so the shell's `.family(semibold())` call
/// sites keep compiling and now draw in the console voice.
pub(super) fn semibold() -> egui::FontFamily {
    egui::FontFamily::Monospace
}

/// The console/UI voice (JetBrains Mono + fallbacks). Alias of the monospace
/// family so there is exactly one installed chain.
pub(super) fn console() -> egui::FontFamily {
    egui::FontFamily::Monospace
}

/// The prose voice: Inter Regular (+ CJK/Ubuntu fallbacks) for sentences.
pub(super) fn prose() -> egui::FontFamily {
    egui::FontFamily::Proportional
}

/// Install the application theme. Called once, on the first frame.
///
/// Direction C: near-black planes, flat elevation (1 px rules only, no shadow),
/// 2–3 px radii, control boundaries at `border_control` (AA 1.4.11), ice for
/// links/data/focus and the single lime accent reserved for the CTA.
pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = bg_1;
    v.window_fill = bg_1;
    v.extreme_bg_color = bg_0;
    v.faint_bg_color = bg_2;
    v.code_bg_color = bg_0;
    v.text_edit_bg_color = Some(bg_0);
    v.window_stroke = egui::Stroke::new(1.0, border_2);
    v.window_corner_radius = egui::CornerRadius::same(R_PANEL);
    // Flat: the only depth cue is the 1 px rule (token sheet §3).
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.menu_corner_radius = egui::CornerRadius::same(R_TAG);
    v.override_text_color = Some(text_1);
    v.weak_text_color = Some(text_3);
    v.hyperlink_color = accent_text;
    v.warn_fg_color = warn;
    v.error_fg_color = danger;
    // Selected rows use the inset plane + an ice hairline, not a tint; the
    // sheet keeps translucent colour off text and chrome.
    v.selection.bg_fill = bg_2;
    v.selection.stroke = egui::Stroke::new(1.0, accent_text);
    v.text_cursor.stroke = egui::Stroke::new(1.0, accent_text);

    let rule = egui::Stroke::new(1.0, border_1);
    // A control's identifying boundary is `border_control` (3:1 on every plane,
    // WCAG 1.4.11); decorative hairlines stay `border_1`.
    let control_line = egui::Stroke::new(1.0, border_control);
    v.widgets.noninteractive.bg_fill = bg_1;
    v.widgets.noninteractive.weak_bg_fill = bg_1;
    v.widgets.noninteractive.bg_stroke = rule;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_3);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(R_TAG);

    // Buttons and controls are flat plates: the fill matches the panel so a
    // plain button reads as a text control, while the `control_line` boundary
    // keeps a combo/field perceivable as a control (1.4.11).
    v.widgets.inactive.bg_fill = bg_1;
    v.widgets.inactive.weak_bg_fill = bg_1;
    v.widgets.inactive.bg_stroke = control_line;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text_2);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(R_TAG);

    v.widgets.hovered.bg_fill = bg_2;
    v.widgets.hovered.weak_bg_fill = bg_2;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border_2);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(R_TAG);

    v.widgets.active.bg_fill = bg_3;
    v.widgets.active.weak_bg_fill = bg_3;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, border_2);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.active.corner_radius = egui::CornerRadius::same(R_TAG);

    v.widgets.open.bg_fill = bg_2;
    v.widgets.open.weak_bg_fill = bg_2;
    v.widgets.open.bg_stroke = control_line;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.open.corner_radius = egui::CornerRadius::same(R_TAG);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(S2, S2);
    style.spacing.button_padding = egui::vec2(S2, S1);
    style.spacing.window_margin = egui::Margin::same(S4 as i8);
    style.spacing.menu_margin = egui::Margin::same(S2 as i8);
    style.spacing.interact_size.y = CONTROL_H;
    style.spacing.icon_width = 12.0;
    style.spacing.scroll.bar_width = 6.0;
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(TITLE, console()),
        ),
        (egui::TextStyle::Body, egui::FontId::new(BODY, console())),
        (egui::TextStyle::Button, egui::FontId::new(LABEL, console())),
        (
            egui::TextStyle::Small,
            egui::FontId::new(CAPTION, console()),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(VALUE, console()),
        ),
    ]
    .into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

pub(super) fn parse_server_addr(s: &str) -> Option<SocketAddrV4> {
    if s.is_empty() {
        return None;
    }
    s.parse::<SocketAddrV4>().ok()
}

pub(super) fn connect_instruction(game_key: &str, local_port: u16) -> String {
    let port = local_port.to_string();
    match game_key {
        "rust" => i18n::t_with("connect.rust", &[("port", port.as_str())]),
        "cs2" => i18n::t_with("connect.cs2", &[("port", port.as_str())]),
        "dota2" => i18n::t_with("connect.dota2", &[("port", port.as_str())]),
        _ => i18n::t_with("connect.generic", &[("port", port.as_str())]),
    }
}
