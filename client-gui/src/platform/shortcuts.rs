//! The canonical shortcut keymap (cross-platform plan G2.1; spec W2-4 (d)).
//!
//! One pure table per OS, so the Windows tray accelerators, the macOS menu-bar
//! items and the shell's footer hints can never disagree about a binding. The
//! canonical table is:
//!
//! | Action | Windows / Linux | macOS | Scope |
//! |---|---|---|---|
//! | Quit | `Ctrl+Q` | `⌘Q` | app |
//! | Settings | `Ctrl+,` | `⌘,` | app |
//! | Hide to tray | `Ctrl+H` | `⌘H` | app, only with a real tray |
//! | Close the top sheet | `Esc` | `Esc` | sheet |
//! | Confirm the sheet's primary action | `Enter` | `Enter` | sheet |
//!
//! There is no OS-global hotkey in this wave (the plan forbids one): every
//! binding is scoped to the app or to the open sheet. `Esc`/`Enter` are sheet
//! semantics, not app accelerators; the menu carries the three app chords.
//!
//! Runtime interaction is unverified on macOS/Linux (no hardware); the tables
//! are unit-tested on every host and the Windows tray menu consumes them as its
//! accelerators.

/// The three desktop targets this table distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

/// The OS this binary was compiled for.
pub fn current_os() -> Os {
    if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

/// One modifier of a chord. `Meta` is ⌘ on macOS only.
///
/// The canonical table only uses `Ctrl`/`Meta`; the enum is kept to two
/// variants on purpose so a stray `Shift`-based binding cannot appear without
/// a decision (the plan assigns no `Shift`/`Alt` shortcut).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Meta,
}

/// The non-modifier key of a chord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Q,
    Comma,
    H,
    Escape,
    Enter,
}

/// A modifier list plus one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyChord {
    pub modifiers: &'static [Modifier],
    pub key: Key,
}

impl KeyChord {
    /// A single-modifier chord.
    pub const fn with(modifier: Modifier, key: Key) -> Self {
        Self {
            modifiers: match modifier {
                Modifier::Ctrl => &[Modifier::Ctrl],
                Modifier::Meta => &[Modifier::Meta],
            },
            key,
        }
    }

    /// An unmodified key (`Esc` / `Enter`).
    pub const fn plain(key: Key) -> Self {
        Self {
            modifiers: &[],
            key,
        }
    }

    /// Whether this chord uses the platform's command modifier.
    #[cfg(test)]
    pub fn is_command_chord(&self) -> bool {
        matches!(self.modifiers, [Modifier::Ctrl] | [Modifier::Meta])
    }
}

/// What a shortcut does. The label keys are catalog keys, so a menu item and a
/// footer hint render the same words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutAction {
    Quit,
    Settings,
    HideToTray,
    CloseSheet,
    Confirm,
}

impl ShortcutAction {
    /// The catalog key for this action's menu label.
    ///
    /// `CloseSheet`/`Confirm` are sheet semantics: their wording comes from the
    /// open sheet's own primary action, so they deliberately have no menu label.
    pub fn label_key(self) -> Option<&'static str> {
        match self {
            Self::Quit => Some("action.quit"),
            Self::Settings => Some("settings.title"),
            Self::HideToTray => Some("window.hide_to_tray"),
            Self::CloseSheet | Self::Confirm => None,
        }
    }
}

/// Where a shortcut is valid. `App` bindings may be carried by a menu;
/// `Sheet` bindings are keyboard semantics inside the open sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    App,
    Sheet,
}

/// One row of the canonical table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub action: ShortcutAction,
    pub scope: Scope,
    pub chord: KeyChord,
    /// Hide-to-tray only exists when the platform created a real tray; hiding
    /// with no tray would strand the window.
    pub requires_tray: bool,
    /// The canonical human label for tooltips/hints (`Ctrl+Q`, `⌘,`).
    pub display: &'static str,
}

const WINDOWS_LINUX: &[Shortcut] = &[
    Shortcut {
        action: ShortcutAction::Quit,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Ctrl, Key::Q),
        requires_tray: false,
        display: "Ctrl+Q",
    },
    Shortcut {
        action: ShortcutAction::Settings,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Ctrl, Key::Comma),
        requires_tray: false,
        display: "Ctrl+,",
    },
    Shortcut {
        action: ShortcutAction::HideToTray,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Ctrl, Key::H),
        requires_tray: true,
        display: "Ctrl+H",
    },
    Shortcut {
        action: ShortcutAction::CloseSheet,
        scope: Scope::Sheet,
        chord: KeyChord::plain(Key::Escape),
        requires_tray: false,
        display: "Esc",
    },
    Shortcut {
        action: ShortcutAction::Confirm,
        scope: Scope::Sheet,
        chord: KeyChord::plain(Key::Enter),
        requires_tray: false,
        display: "Enter",
    },
];

const MACOS: &[Shortcut] = &[
    Shortcut {
        action: ShortcutAction::Quit,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Meta, Key::Q),
        requires_tray: false,
        display: "⌘Q",
    },
    Shortcut {
        action: ShortcutAction::Settings,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Meta, Key::Comma),
        requires_tray: false,
        display: "⌘,",
    },
    Shortcut {
        action: ShortcutAction::HideToTray,
        scope: Scope::App,
        chord: KeyChord::with(Modifier::Meta, Key::H),
        requires_tray: true,
        display: "⌘H",
    },
    Shortcut {
        action: ShortcutAction::CloseSheet,
        scope: Scope::Sheet,
        chord: KeyChord::plain(Key::Escape),
        requires_tray: false,
        display: "Esc",
    },
    Shortcut {
        action: ShortcutAction::Confirm,
        scope: Scope::Sheet,
        chord: KeyChord::plain(Key::Enter),
        requires_tray: false,
        display: "Enter",
    },
];

/// The canonical table for `os`.
pub fn keymap(os: Os) -> &'static [Shortcut] {
    match os {
        Os::Windows | Os::Linux => WINDOWS_LINUX,
        Os::Macos => MACOS,
    }
}

/// The binding for one action on `os`, or `None` when the table has no row.
pub fn binding(os: Os, action: ShortcutAction) -> Option<&'static Shortcut> {
    keymap(os).iter().find(|row| row.action == action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_the_same_shape_on_every_os() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            let rows = keymap(os);
            assert_eq!(rows.len(), 5, "{os:?} must carry all five canonical rows");
            // No action appears twice.
            for (index, row) in rows.iter().enumerate() {
                assert!(
                    !rows[index + 1..]
                        .iter()
                        .any(|later| later.action == row.action),
                    "{os:?} repeats {:?}",
                    row.action
                );
                assert!(!row.display.trim().is_empty(), "{os:?} has no hint label");
            }
        }
    }

    #[test]
    fn the_platform_command_modifier_is_canonical() {
        for os in [Os::Windows, Os::Linux] {
            for action in [
                ShortcutAction::Quit,
                ShortcutAction::Settings,
                ShortcutAction::HideToTray,
            ] {
                let row = binding(os, action).expect("app row exists");
                assert_eq!(row.chord.modifiers, [Modifier::Ctrl], "{os:?} {action:?}");
                assert!(!row.chord.modifiers.contains(&Modifier::Meta));
            }
        }
        for action in [
            ShortcutAction::Quit,
            ShortcutAction::Settings,
            ShortcutAction::HideToTray,
        ] {
            let row = binding(Os::Macos, action).expect("macOS app row exists");
            assert_eq!(row.chord.modifiers, [Modifier::Meta], "{action:?}");
            assert!(!row.chord.modifiers.contains(&Modifier::Ctrl));
        }
    }

    #[test]
    fn quit_settings_and_hide_carry_their_exact_keys() {
        let expected = [
            (ShortcutAction::Quit, Key::Q),
            (ShortcutAction::Settings, Key::Comma),
            (ShortcutAction::HideToTray, Key::H),
        ];
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            for (action, key) in expected {
                let row = binding(os, action).expect("app row exists");
                assert_eq!(row.chord.key, key, "{os:?} {action:?}");
                assert_eq!(row.scope, Scope::App, "{os:?} {action:?}");
            }
        }
        assert_eq!(
            binding(Os::Windows, ShortcutAction::Quit).unwrap().display,
            "Ctrl+Q"
        );
        assert_eq!(
            binding(Os::Windows, ShortcutAction::Settings)
                .unwrap()
                .display,
            "Ctrl+,"
        );
        assert_eq!(
            binding(Os::Macos, ShortcutAction::Quit).unwrap().display,
            "⌘Q"
        );
    }

    #[test]
    fn esc_and_enter_are_sheet_semantics_not_app_accelerators() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            for action in [ShortcutAction::CloseSheet, ShortcutAction::Confirm] {
                let row = binding(os, action).expect("sheet row exists");
                assert_eq!(row.scope, Scope::Sheet, "{os:?} {action:?}");
                assert!(
                    row.chord.modifiers.is_empty(),
                    "{action:?} takes no modifier"
                );
                assert!(!row.requires_tray);
            }
            assert_eq!(
                binding(os, ShortcutAction::CloseSheet).unwrap().chord.key,
                Key::Escape
            );
            assert_eq!(
                binding(os, ShortcutAction::Confirm).unwrap().chord.key,
                Key::Enter
            );
        }
    }

    #[test]
    fn hide_to_tray_requires_a_tray_and_a_menu_filters_it_out() {
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            let hide = binding(os, ShortcutAction::HideToTray).expect("row exists");
            assert!(
                hide.requires_tray,
                "{os:?} hide-to-tray must require a tray"
            );
            // The menu rule the macOS menu bar will use (and the Windows tray
            // follows by construction): only app rows that need no tray are
            // offered when the platform has no real tray.
            let without_tray: Vec<_> = keymap(os)
                .iter()
                .filter(|row| row.scope == Scope::App && !row.requires_tray)
                .map(|row| row.action)
                .collect();
            let with_tray: Vec<_> = keymap(os)
                .iter()
                .filter(|row| row.scope == Scope::App)
                .map(|row| row.action)
                .collect();
            assert!(!without_tray.contains(&ShortcutAction::HideToTray));
            assert!(with_tray.contains(&ShortcutAction::HideToTray));
            // Quit and Settings are always available.
            for action in [ShortcutAction::Quit, ShortcutAction::Settings] {
                assert!(with_tray.contains(&action));
                assert!(without_tray.contains(&action));
            }
        }
    }

    #[test]
    fn there_is_no_global_hotkey_in_this_wave() {
        // The plan forbids an OS-global hotkey; the enum has only App/Sheet
        // scopes and every app binding is a modified chord or a sheet key.
        for os in [Os::Windows, Os::Linux, Os::Macos] {
            for row in keymap(os) {
                match row.scope {
                    Scope::App => {
                        assert!(
                            row.chord.is_command_chord(),
                            "{os:?} {:?} is app-wide but has no command modifier",
                            row.action
                        );
                    }
                    Scope::Sheet => {
                        assert!(row.chord.modifiers.is_empty());
                        assert!(matches!(row.chord.key, Key::Escape | Key::Enter));
                    }
                }
            }
        }
    }

    #[test]
    fn the_current_os_matches_the_build() {
        let expected = if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        };
        assert_eq!(current_os(), expected);
    }

    #[test]
    fn every_app_action_has_a_catalogued_label() {
        for action in [
            ShortcutAction::Quit,
            ShortcutAction::Settings,
            ShortcutAction::HideToTray,
        ] {
            let key = action.label_key().expect("app action has a menu label");
            assert!(!key.trim().is_empty());
        }
        // Sheet semantics take their label from the sheet's primary action.
        assert_eq!(ShortcutAction::CloseSheet.label_key(), None);
        assert_eq!(ShortcutAction::Confirm.label_key(), None);
    }
}
