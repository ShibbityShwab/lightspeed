use super::*;

// ── Pure helpers (no platform dependency) ─────────────────────────────────────

/// Build the font families the UI draws with, for `locale`.
///
/// The bundled Inter is a 230-codepoint subset - no Cyrillic, no Greek, no CJK -
/// so every family needs real fallbacks. `semibold` used to chain only
/// as tofu while the regular text beside it rendered fine through epaint's
/// built-in Ubuntu-Light.
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

    let (cjk_regular, cjk_bold) = cjk_order(locale);
    let builtin = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut proportional = vec!["inter".to_owned()];
    proportional.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    proportional.extend(builtin.iter().cloned());

    let mut monospace = vec!["jbmono".to_owned()];
    monospace.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    monospace.extend(builtin.iter().cloned());

    let mut emphasised = vec!["inter_semibold".to_owned(), "inter".to_owned()];
    emphasised.extend(cjk_bold.iter().map(|name| (*name).to_owned()));
    emphasised.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    emphasised.extend(builtin);

    fonts
        .families
        .insert(egui::FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, monospace);
    fonts.families.insert(semibold(), emphasised);
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
pub(super) fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}

/// Install the application theme. Called once, on the first frame.
///
/// Every colour comes from [`crate::design`]; shadows are off, and the two
/// global hairlines use `border-1`/`border-2` rather than accent arithmetic.
pub(super) fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = bg_1;
    v.window_fill = bg_2;
    v.extreme_bg_color = bg_0;
    v.faint_bg_color = bg_2;
    v.code_bg_color = bg_2;
    v.text_edit_bg_color = Some(bg_0);
    v.window_stroke = egui::Stroke::new(1.0, border_1);
    v.window_corner_radius = egui::CornerRadius::same(R_CONTROL);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.menu_corner_radius = egui::CornerRadius::same(R_CONTROL);
    v.override_text_color = Some(text_1);
    v.weak_text_color = Some(text_3);
    v.hyperlink_color = accent_text;
    v.warn_fg_color = warn;
    v.error_fg_color = danger;
    v.selection.bg_fill = accent_soft;
    v.selection.stroke = egui::Stroke::new(1.0, accent_line);

    let line = egui::Stroke::new(1.0, border_1);
    v.widgets.noninteractive.bg_fill = bg_2;
    v.widgets.noninteractive.weak_bg_fill = bg_2;
    v.widgets.noninteractive.bg_stroke = line;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_3);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.inactive.bg_fill = bg_3;
    v.widgets.inactive.weak_bg_fill = bg_3;
    v.widgets.inactive.bg_stroke = line;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.hovered.bg_fill = bg_4;
    v.widgets.hovered.weak_bg_fill = bg_4;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border_2);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.active.bg_fill = accent_deep;
    v.widgets.active.weak_bg_fill = accent_deep;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, on_accent);
    v.widgets.active.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.open.bg_fill = bg_3;
    v.widgets.open.weak_bg_fill = bg_3;
    v.widgets.open.bg_stroke = line;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.open.corner_radius = egui::CornerRadius::same(R_CONTROL);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(S2, S2);
    style.spacing.button_padding = egui::vec2(S3, S2);
    style.spacing.window_margin = egui::Margin::same(S4 as i8);
    style.spacing.menu_margin = egui::Margin::same(S2 as i8);
    style.spacing.interact_size.y = CONTROL_H;
    style.spacing.icon_width = 14.0;
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(TITLE)),
        (egui::TextStyle::Body, egui::FontId::proportional(BODY)),
        (
            egui::TextStyle::Button,
            egui::FontId::new(LABEL, semibold()),
        ),
        (egui::TextStyle::Small, egui::FontId::proportional(CAPTION)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(VALUE)),
    ]
    .into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

/// The quality ramp for a round trip: good under 60 ms, attention under 120 ms,
/// bad beyond that.
pub(super) fn rtt_colour(rtt_ms: f64) -> egui::Color32 {
    if rtt_ms < 60.0 {
        signal
    } else if rtt_ms < 120.0 {
        warn
    } else {
        danger
    }
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
