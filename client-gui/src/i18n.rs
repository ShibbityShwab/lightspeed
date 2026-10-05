//! GUI localization.
//!
//! Locale catalogs are TOML files (`client-gui/locales/<tag>.toml`) compiled
//! into the binary at build time, so a shipped installer carries every
//! language it supports and never reads a catalog from disk at startup.
//!
//! Lookup order is exact tag, then the tag's primary language subtag, then
//! `en`. A key missing from a locale therefore renders the English string
//! rather than a raw key, which is the failure mode a half-finished
//! translation deserves.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// The source locale. Every key in any shipped catalog must exist here.
pub const SOURCE_LOCALE: &str = "en";

/// Locales compiled into the binary, as `(tag, catalog)`.
///
/// Adding a language means adding `locales/<tag>.toml` and one `include_str!`
/// line here; `every_shipped_locale_matches_the_source_key_set` fails the build
/// if the two ever drift.
const CATALOGS: &[(&str, &str)] = &[
    (SOURCE_LOCALE, include_str!("../locales/en.toml")),
    ("de", include_str!("../locales/de.toml")),
    ("es", include_str!("../locales/es.toml")),
    ("fr", include_str!("../locales/fr.toml")),
    ("ja", include_str!("../locales/ja.toml")),
    ("ko", include_str!("../locales/ko.toml")),
    ("pt-BR", include_str!("../locales/pt-BR.toml")),
    ("ru", include_str!("../locales/ru.toml")),
    ("zh-Hans", include_str!("../locales/zh-Hans.toml")),
];

/// A parsed locale catalog: flat key -> translated text.
type Catalog = HashMap<String, String>;

fn parse_catalog(text: &str) -> Catalog {
    let table: toml::Table = toml::from_str(text).unwrap_or_else(|e| {
        panic!("embedded locale catalog is not valid TOML: {e}");
    });
    let mut catalog = Catalog::new();
    for (section, value) in table {
        let Some(body) = value.as_table() else {
            continue;
        };
        for (key, value) in body {
            let Some(text) = value.as_str() else {
                continue;
            };
            catalog.insert(format!("{section}.{key}"), text.to_string());
        }
    }
    catalog
}

fn catalogs() -> &'static HashMap<&'static str, Catalog> {
    static CATALOGS_PARSED: OnceLock<HashMap<&'static str, Catalog>> = OnceLock::new();
    CATALOGS_PARSED.get_or_init(|| {
        CATALOGS
            .iter()
            .map(|(tag, text)| (*tag, parse_catalog(text)))
            .collect()
    })
}

/// The pinned language, or `None` when the operating system decides.
fn pinned() -> &'static Mutex<Option<String>> {
    static PINNED: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    PINNED.get_or_init(|| Mutex::new(None))
}

/// Languages whose catalog has been merged into [`active_catalog`]'s cache.
///
/// This holds at most one tag and is the cache key. A language change replaces
/// the entry rather than clearing the cache in place, because clearing from
/// [`set_language`] would take the same lock [`t`] holds, and a reader in
/// another thread would deadlock against it.
fn merged() -> &'static Mutex<Vec<String>> {
    static MERGED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    MERGED.get_or_init(|| Mutex::new(Vec::new()))
}

/// The merged catalog: the active locale layered over the source locale.
///
/// One flat table means [`t`] stays a single lookup on the hot path, while a
/// key missing from a translation still resolves to English. The cache is
/// rebuilt on a language change, which happens once per Settings click.
fn active_catalog() -> &'static Mutex<Catalog> {
    static ACTIVE: OnceLock<Mutex<Catalog>> = OnceLock::new();
    let active = ACTIVE.get_or_init(|| Mutex::new(Catalog::new()));
    let locale = resolve_requested(configured_language().as_deref());
    let mut merged_guard = merged()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Exactly one cached locale, so a change is `!=` rather than a lookup in a
    // growing list; a list would keep rebuilding on every lookup once the user
    // had switched twice.
    if merged_guard.first().map(String::as_str) != Some(locale.as_str()) {
        let catalog = match catalogs().get(locale.as_str()) {
            Some(overlay) => merge_over_source(overlay),
            None => catalogs().get(SOURCE_LOCALE).cloned().unwrap_or_default(),
        };
        let mut guard = active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = catalog;
        *merged_guard = vec![locale];
    }
    drop(merged_guard);
    active
}

/// Layer a translation over the source catalog: shared keys take the
/// translation, keys the translation has not reached yet keep the English
/// string.
fn merge_over_source(overlay: &Catalog) -> Catalog {
    let mut catalog = catalogs().get(SOURCE_LOCALE).cloned().unwrap_or_default();
    catalog.extend(overlay.iter().map(|(k, v)| (k.clone(), v.clone())));
    catalog
}

/// A locale the user can choose in Settings, as shown in the language picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocaleInfo {
    /// BCP-47 tag as stored in `config.toml`.
    pub tag: &'static str,
    /// The language's own name, so the picker is readable when it is open to
    /// someone who does not read the current UI language.
    pub native_name: &'static str,
}

/// Every locale the binary carries, source locale first.
pub const LOCALES: &[LocaleInfo] = &[
    LocaleInfo {
        tag: "en",
        native_name: "English",
    },
    LocaleInfo {
        tag: "de",
        native_name: "Deutsch",
    },
    LocaleInfo {
        tag: "es",
        native_name: "Español",
    },
    LocaleInfo {
        tag: "fr",
        native_name: "Français",
    },
    LocaleInfo {
        tag: "ja",
        native_name: "日本語",
    },
    LocaleInfo {
        tag: "ko",
        native_name: "한국어",
    },
    LocaleInfo {
        tag: "pt-BR",
        native_name: "Português (Brasil)",
    },
    LocaleInfo {
        tag: "ru",
        native_name: "Русский",
    },
    LocaleInfo {
        tag: "zh-Hans",
        native_name: "简体中文",
    },
];

/// Requested locale for this process: the pinned language, else the OS locale.
fn configured_language() -> Option<String> {
    pinned()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .or_else(system_locale)
}

/// Pin the active language, or clear the pin so the operating system decides
/// again. The merged catalog rebuilds on the next lookup.
pub fn set_language(tag: Option<&str>) {
    let mut guard = pinned()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = tag.map(str::to_string);
}

/// Reduce a requested tag to one this binary carries.
///
/// Handles `de_DE`, `de-DE`, `de.UTF-8`, and a bare `de`: any of them select
/// the `de` catalog. An unrecognized language falls back to the source locale
/// so an unsupported OS locale still yields readable English.
fn resolve_requested(requested: Option<&str>) -> String {
    let Some(raw) = requested.filter(|s| !s.trim().is_empty()) else {
        return SOURCE_LOCALE.to_string();
    };
    let normalized = normalize_tag(raw);
    if catalog_exists(&normalized) {
        return normalized;
    }
    let primary = normalized
        .split('-')
        .next()
        .unwrap_or(SOURCE_LOCALE)
        .to_string();
    if catalog_exists(&primary) {
        return primary;
    }
    SOURCE_LOCALE.to_string()
}

/// `C` and `POSIX` are locale names every Unix defines, not languages; they
/// mean "no localization", which the caller turns into the source locale.
fn normalize_tag(raw: &str) -> String {
    let base = raw
        .split(['.', '@'])
        .next()
        .unwrap_or(raw)
        .trim()
        .replace('_', "-");
    if base.eq_ignore_ascii_case("C") || base.eq_ignore_ascii_case("POSIX") {
        return SOURCE_LOCALE.to_string();
    }
    let mut parts = base.split('-').filter(|p| !p.is_empty());
    let Some(primary) = parts.next() else {
        return SOURCE_LOCALE.to_string();
    };
    let mut tag = primary.to_ascii_lowercase();
    for part in parts {
        // Region and script subtags are conventionally cased as `BR` and
        // `Hans`; an all-lowercase tag would miss the `zh-Hans`/`pt-BR`
        // catalogs when the OS reports `zh_hans` or `pt_br`.
        if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            let mut chars = part.chars();
            tag.push('-');
            if let Some(first) = chars.next() {
                tag.extend(first.to_uppercase());
                tag.push_str(&chars.as_str().to_ascii_lowercase());
            }
        } else {
            tag.push('-');
            tag.push_str(&part.to_ascii_uppercase());
        }
    }
    tag
}

fn catalog_exists(tag: &str) -> bool {
    catalogs().contains_key(tag)
}

/// The OS locale.
///
/// A Unix program's language selection is the `LANG`/`LC_ALL` chain; the Win32
/// equivalent is `GetUserDefaultLocaleName`, which would cost a `windows-sys`
/// dependency for one string on a manifest that deliberately keeps
/// Windows-only crates out of the Linux build. Windows does populate `LANG`
/// for MSYS/Cygwin programs, so the two agree in the environment the GUI is
/// built and run from; an unset environment simply resolves to English, which
/// is also what a user who has never touched the language picker gets.
fn system_locale() -> Option<String> {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                return Some(value);
            }
        }
    }
    None
}

/// The locale in use, resolved from the pinned language or the OS locale.
pub fn current() -> String {
    resolve_requested(configured_language().as_deref())
}

/// Translate a key into the active locale.
///
/// A key absent from the active locale falls back to the source catalog; a key
/// absent from both returns the key itself, so a missing string is visible in
/// a screenshot rather than silently blank.
pub fn t(key: &str) -> Cow<'static, str> {
    let catalog = active_catalog()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match catalog.get(key) {
        Some(text) => Cow::Owned(text.clone()),
        None => Cow::Owned(key.to_string()),
    }
}

/// Translate a key and substitute `{name}` placeholders.
///
/// Placeholders carry runtime values (`{count}`, `{ms}`); the catalog keeps
/// the sentence order, so a translator can move a placeholder anywhere in it.
pub fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    let template = t(key);
    let mut out = template.into_owned();
    for (name, value) in args {
        let token = format!("{{{name}}}");
        if out.contains(&token) {
            out = out.replace(&token, value);
        }
    }
    out
}

/// Every key defined in the source catalog, sorted; used by the tests that
/// keep shipped locales in parity with English.
#[cfg(test)]
pub fn source_keys() -> Vec<String> {
    let mut keys: Vec<String> = catalogs()
        .get(SOURCE_LOCALE)
        .map(|c| c.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The active language is process-global, so these tests are serialized.
    /// Without this, a test that pins German races one that pins Japanese and
    /// the failure is a timing accident rather than a defect being reported.
    fn test_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn every_shipped_locale_matches_the_source_key_set() {
        let _guard = test_lock();
        let source = source_keys();
        assert!(!source.is_empty(), "source catalog failed to parse");
        for (tag, _) in CATALOGS {
            if *tag == SOURCE_LOCALE {
                continue;
            }
            let catalog = catalogs().get(tag).expect("catalog parsed");
            let mut keys: Vec<&String> = catalog.keys().collect();
            keys.sort();
            let missing: Vec<&str> = source
                .iter()
                .filter(|key| !catalog.contains_key(key.as_str()))
                .map(String::as_str)
                .collect();
            let extra: Vec<&str> = keys
                .iter()
                .filter(|key| !source.contains(*key))
                .map(|key| key.as_str())
                .collect();
            assert!(missing.is_empty(), "{tag} is missing keys: {missing:?}");
            assert!(extra.is_empty(), "{tag} has unknown keys: {extra:?}");
        }
    }

    #[test]
    fn every_translation_is_non_empty() {
        for (tag, catalog) in catalogs() {
            for (key, text) in catalog {
                assert!(
                    !text.trim().is_empty(),
                    "{tag} has an empty value for {key}"
                );
            }
        }
    }

    #[test]
    fn every_locale_listing_has_a_catalog_and_vice_versa() {
        let listed: Vec<&str> = LOCALES.iter().map(|l| l.tag).collect();
        let compiled: Vec<&str> = CATALOGS.iter().map(|(tag, _)| *tag).collect();
        for tag in &listed {
            assert!(
                compiled.contains(tag),
                "{tag} is listed but not compiled in"
            );
        }
        for tag in &compiled {
            assert!(listed.contains(tag), "{tag} is compiled in but not listed");
        }
    }

    #[test]
    fn placeholders_survive_in_every_translation() {
        // A dropped or renamed `{placeholder}` renders a literal brace in the
        // UI, so parity of the placeholder set is part of a valid translation.
        let source = catalogs().get(SOURCE_LOCALE).expect("source catalog");
        for (tag, catalog) in catalogs() {
            if *tag == SOURCE_LOCALE {
                continue;
            }
            for (key, english) in source {
                let Some(translated) = catalog.get(key) else {
                    continue;
                };
                let expected = placeholders(english);
                let found = placeholders(translated);
                assert_eq!(
                    expected, found,
                    "{tag}.{key} changed the placeholder set: {english:?} -> {translated:?}"
                );
            }
        }
    }

    fn placeholders(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start..].find('}') else {
                break;
            };
            found.push(rest[start + 1..start + end].to_string());
            rest = &rest[start + end + 1..];
        }
        found.sort();
        found
    }

    #[test]
    fn unknown_keys_render_the_key_not_an_empty_string() {
        assert_eq!(t("no.such.key"), "no.such.key");
    }

    #[test]
    fn a_translated_locale_changes_the_rendered_string() {
        let _guard = test_lock();
        set_language(Some("de"));
        assert_eq!(t("state.connected"), "Verbunden");
        assert_eq!(t("action.cancel"), "Abbrechen");
        // Deterministic again for the rest of the suite.
        set_language(None);
    }

    #[test]
    fn switching_languages_takes_effect_without_a_restart() {
        let _guard = test_lock();
        set_language(Some("fr"));
        assert_eq!(t("state.error"), "Erreur");
        set_language(Some("ja"));
        assert_eq!(t("state.error"), "エラー");
        set_language(None);
    }

    /// The shipped catalogs are complete by design, so the fallback is proven
    /// against a synthetic locale instead: an English-only key must still
    /// answer in English when a translation has not landed yet.
    #[test]
    fn a_key_missing_from_a_translation_falls_back_to_english() {
        let _guard = test_lock();
        let mut overlay: Catalog = HashMap::new();
        overlay.insert("state.connected".to_string(), "Verbunden".to_string());
        let merged = merge_over_source(&overlay);
        assert_eq!(
            merged.get("state.connected").map(String::as_str),
            Some("Verbunden")
        );
        assert_eq!(merged.get("state.error").map(String::as_str), Some("Error"));
        assert_eq!(merged.get("no.such.key"), None);
    }

    #[test]
    fn tag_normalization_accepts_platform_spellings() {
        assert_eq!(normalize_tag("de_DE.UTF-8"), "de-DE");
        assert_eq!(normalize_tag("de-DE"), "de-DE");
        assert_eq!(normalize_tag("DE"), "de");
        assert_eq!(normalize_tag("zh_hans"), "zh-Hans");
        assert_eq!(normalize_tag("ZH_HANS"), "zh-Hans");
        assert_eq!(normalize_tag("pt_br"), "pt-BR");
        // `C`/`POSIX` mean "no localization", which is the source locale.
        assert_eq!(normalize_tag("C"), "en");
        assert_eq!(normalize_tag("C.UTF-8"), "en");
        assert_eq!(normalize_tag("POSIX"), "en");
    }

    #[test]
    fn unsupported_languages_fall_back_to_english() {
        assert_eq!(resolve_requested(Some("kl-GL")), "en");
        assert_eq!(resolve_requested(Some("")), "en");
        assert_eq!(resolve_requested(None), "en");
    }

    #[test]
    fn regional_spellings_select_their_base_catalog() {
        assert_eq!(resolve_requested(Some("de_AT.UTF-8")), "de");
        assert_eq!(resolve_requested(Some("fr_CA")), "fr");
        assert_eq!(resolve_requested(Some("pt_BR")), "pt-BR");
        assert_eq!(resolve_requested(Some("zh_hans")), "zh-Hans");
        // No `pt` catalog ships, so European Portuguese falls back to English
        // rather than to Brazilian Portuguese, whose wording is close but not
        // identical in the strings that differ (`Faixa de portas` vs `Gama`).
        assert_eq!(resolve_requested(Some("pt_PT")), "en");
    }
}
