//! The Windows tray presentation: one pure mapping from [`TrayState`] to icon
//! colour, tooltip and menu enablement (spec W2-4 (c), Q14).
//!
//! The fifth state lives here: a session whose relay is not returning the
//! game's traffic is [`TrayState::RelayStarved`] — amber-orange, with the short
//! `tray.starved` sentence and the Connect/Disconnect items reflecting a live
//! session. It is **never** the green optimizing icon: the colour table and the
//! tests below pin that distinction, and the state itself comes from the shared
//! core selector (`status::tray_state_for`) whose ordering already puts
//! starvation before optimization.
//!
//! The macOS/Linux tray backends are stubs, so this module is Windows-only
//! today. It is pure data: [`WindowsTray`](crate::platform::windows::WindowsTray)
//! applies it; the tests here pin what the user sees.

use crate::app::TrayState;
use crate::i18n;

/// One row of the tray context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrayMenuItem {
    pub action: TrayMenuAction,
    pub enabled: bool,
}

/// What a tray menu row does. The label is a catalog key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayMenuAction {
    Show,
    Connect,
    Disconnect,
    Quit,
}

impl TrayMenuAction {
    /// The catalog key for this row's label.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Show => "tray.show",
            Self::Connect => "tray.connect",
            Self::Disconnect => "tray.disconnect",
            Self::Quit => "tray.quit",
        }
    }

    /// The localized label for this row.
    pub fn label(self) -> String {
        i18n::t(self.label_key()).into_owned()
    }
}

/// Everything the tray icon shows for one state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayPresentation {
    /// The state-colour square on the icon, `(r, g, b)`.
    pub icon_rgb: (u8, u8, u8),
    /// The catalog key for the tooltip's state phrase. `tray.starved` is the
    /// short form of the starvation warning; every other state reuses the
    /// already-translated `state.*` word.
    pub tooltip_key: &'static str,
    /// The four menu rows in display order.
    pub items: [TrayMenuItem; 4],
}

impl TrayPresentation {
    /// The localized tooltip. `rtt_ms` is only meaningful for a connected (or
    /// optimizing) session; it is otherwise ignored.
    pub fn tooltip(&self, rtt_ms: f64) -> String {
        let phrase = i18n::t(self.tooltip_key);
        if rtt_ms > 0.0 && matches!(self.tooltip_key, "state.connected" | "state.optimizing") {
            return format!("\u{26a1} LightSpeed \u{2014} {phrase} \u{00b7} RTT {rtt_ms:.0}ms");
        }
        format!("\u{26a1} LightSpeed \u{2014} {phrase}")
    }
}

/// The one presentation mapping. Every state has a distinct colour square so a
/// starving session can never be mistaken for a healthy optimizing one.
pub fn presentation(state: TrayState) -> TrayPresentation {
    let (icon_rgb, tooltip_key) = match state {
        TrayState::Disconnected => ((160, 160, 160), "state.disconnected"),
        TrayState::Connected => ((255, 200, 60), "state.connected"),
        TrayState::Optimizing => ((80, 210, 120), "state.optimizing"),
        // Amber-orange, deliberately distinct from the red error square: the
        // client is running, but the relay is not returning the game's traffic.
        TrayState::RelayStarved => ((255, 150, 50), "tray.starved"),
        TrayState::Error => ((220, 80, 80), "state.error"),
    };
    let session_running = matches!(
        state,
        TrayState::Connected | TrayState::Optimizing | TrayState::RelayStarved
    );
    TrayPresentation {
        icon_rgb,
        tooltip_key,
        items: [
            TrayMenuItem {
                action: TrayMenuAction::Show,
                enabled: true,
            },
            // Connect is an offer, Disconnect is the escape hatch: exactly one
            // of them is meaningful for the current state, and a starving
            // session still needs Disconnect.
            TrayMenuItem {
                action: TrayMenuAction::Connect,
                enabled: !session_running,
            },
            TrayMenuItem {
                action: TrayMenuAction::Disconnect,
                enabled: session_running,
            },
            TrayMenuItem {
                action: TrayMenuAction::Quit,
                enabled: true,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states() -> [TrayState; 5] {
        [
            TrayState::Disconnected,
            TrayState::Connected,
            TrayState::Optimizing,
            TrayState::RelayStarved,
            TrayState::Error,
        ]
    }

    #[test]
    fn the_fifth_state_is_its_own_presentation() {
        let starved = presentation(TrayState::RelayStarved);
        let optimizing = presentation(TrayState::Optimizing);
        assert_eq!(starved.tooltip_key, "tray.starved");
        assert_ne!(
            starved.icon_rgb, optimizing.icon_rgb,
            "a starving relay may never paint the green optimizing square"
        );
        assert_ne!(starved.tooltip_key, optimizing.tooltip_key);
        // The two non-idle UI states still allow the session escape hatch.
        for state in [TrayState::Optimizing, TrayState::RelayStarved] {
            let menu = presentation(state);
            assert!(
                !menu
                    .items
                    .iter()
                    .any(|item| item.action == TrayMenuAction::Connect && item.enabled),
                "{state:?}"
            );
            assert!(menu
                .items
                .iter()
                .any(|item| item.action == TrayMenuAction::Disconnect && item.enabled));
        }
    }

    #[test]
    fn every_state_has_a_distinct_colour_and_tooltip() {
        let mut seen_colours = Vec::new();
        let mut seen_tooltips = Vec::new();
        for state in states() {
            let menu = presentation(state);
            assert!(
                !seen_colours.contains(&menu.icon_rgb),
                "{state:?} reuses a colour"
            );
            assert!(
                !seen_tooltips.contains(&menu.tooltip_key),
                "{state:?} reuses a tooltip"
            );
            seen_colours.push(menu.icon_rgb);
            seen_tooltips.push(menu.tooltip_key);
        }
    }

    #[test]
    fn connect_and_disconnect_are_never_both_enabled() {
        for state in states() {
            let menu = presentation(state);
            let connect = menu
                .items
                .iter()
                .find(|item| item.action == TrayMenuAction::Connect)
                .expect("connect row");
            let disconnect = menu
                .items
                .iter()
                .find(|item| item.action == TrayMenuAction::Disconnect)
                .expect("disconnect row");
            assert_ne!(
                connect.enabled, disconnect.enabled,
                "{state:?}: exactly one of Connect/Disconnect must be offered"
            );
            // Show and Quit are always available.
            for action in [TrayMenuAction::Show, TrayMenuAction::Quit] {
                assert!(menu
                    .items
                    .iter()
                    .any(|item| item.action == action && item.enabled));
            }
        }
    }

    #[test]
    fn the_menu_order_is_show_connect_disconnect_quit() {
        let menu = presentation(TrayState::Disconnected);
        let actions: Vec<_> = menu.items.iter().map(|item| item.action).collect();
        assert_eq!(
            actions,
            [
                TrayMenuAction::Show,
                TrayMenuAction::Connect,
                TrayMenuAction::Disconnect,
                TrayMenuAction::Quit
            ]
        );
    }

    #[test]
    fn the_tooltip_is_localized_copy_and_never_a_raw_key() {
        let _guard = i18n::test_lock();
        i18n::set_language(Some("en"));
        for state in states() {
            let menu = presentation(state);
            let text = menu.tooltip(0.0);
            assert!(text.starts_with("\u{26a1} LightSpeed \u{2014} "), "{text}");
            assert!(
                !text.contains("state.") && !text.contains("tray."),
                "{state:?} rendered a raw key: {text}"
            );
            // A catalogued label exists for every menu row too.
            for item in menu.items {
                let label = item.action.label();
                assert!(!label.is_empty());
                assert_ne!(label.as_str(), item.action.label_key());
            }
        }
        i18n::set_language(None);
    }

    #[test]
    fn the_connected_tooltip_carries_the_rtt_and_others_ignore_it() {
        let _guard = i18n::test_lock();
        i18n::set_language(Some("en"));
        let connected = presentation(TrayState::Connected).tooltip(42.4);
        assert!(connected.contains("RTT 42ms"), "{connected}");
        let starving = presentation(TrayState::RelayStarved).tooltip(42.4);
        assert!(
            !starving.contains("RTT"),
            "a starved session must not advertise a healthy-looking RTT: {starving}"
        );
        assert!(starving.to_lowercase().contains("relay"));
        i18n::set_language(None);
    }

    #[test]
    fn every_presentation_label_key_exists_in_the_source_catalog() {
        let _guard = i18n::test_lock();
        i18n::set_language(Some("en"));
        let source = i18n::source_keys();
        for state in states() {
            let menu = presentation(state);
            let keys: Vec<&str> = std::iter::once(menu.tooltip_key)
                .chain(menu.items.iter().map(|item| item.action.label_key()))
                .collect();
            for key in keys {
                assert!(
                    source.iter().any(|existing| existing == key),
                    "{key} is missing from en.toml"
                );
                assert_ne!(i18n::t(key).as_ref(), key, "{key} has no English value");
            }
        }
        i18n::set_language(None);
    }
}
