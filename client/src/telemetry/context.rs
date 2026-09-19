//! Session-scoped telemetry context and locale-derived country detection.

/// Session-scoped values attached to every telemetry report.
///
/// Built once at startup (after game selection) and threaded through both the
/// periodic flush task and the shutdown flush so every report carries the real
/// active game and a locale-derived country instead of placeholders.
///
/// This struct is never serialised — only its fields are copied into the
/// [`TelemetryReport`](lightspeed_protocol::TelemetryReport) at flush time, so
/// it adds no PII to the wire format.
#[derive(Clone, Debug)]
pub struct TelemetryContext {
    /// Wire game id for the active game (`0` / `UNKNOWN` when none selected).
    pub game_id: u8,
    /// Two-letter ISO 3166-1 alpha-2 country derived from the OS locale, or an
    /// empty string when the locale exposes no usable territory.
    pub country: String,
}

/// Extract a two-letter territory code from a BCP-47-style locale string.
///
/// Takes the first subtag after the language (the segment following the first
/// `-` or `_`), keeps only ASCII alphabetic characters, uppercases it, and
/// returns it only when exactly two letters remain. Anything else yields an
/// empty string. This is deliberately not a full locale parser: it exists to
/// surface the common `en-US` / `th_TH` territory without pulling in ICU data.
///
/// ```text
/// "en-US"    -> "US"
/// "th_TH"    -> "TH"
/// "en"       -> ""
/// ""         -> ""
/// "en-us-x"  -> "US"
/// ```
pub fn normalize_locale_territory(locale: &str) -> String {
    let rest = match locale.find(['-', '_']) {
        Some(idx) => &locale[idx + 1..],
        None => return String::new(),
    };
    let territory: String = rest
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if territory.len() == 2 {
        territory
    } else {
        String::new()
    }
}

/// Detect the user's country from the operating system locale.
///
/// Reads [`sys_locale::get_locale`] and normalises it with
/// [`normalize_locale_territory`]. Returns an empty string when the OS reports
/// no locale or the locale exposes no two-letter territory. Only the OS locale
/// is consulted — no IP address or other identifying signal is used.
pub fn detect_country() -> String {
    sys_locale::get_locale()
        .map(|locale| normalize_locale_territory(&locale))
        .unwrap_or_default()
}
