use super::*;

// ── Game list ──────────────────────────────────────────────────────────────

/// One entry in the game selector. Derived from the client crate's canonical
/// `GAME_REGISTRY` so the GUI can never drift from the CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEntry {
    pub key: &'static str,
    pub display: &'static str,
    pub default_port: u16,
}

/// Every supported game, built once from the shared client game registry.
pub fn games() -> &'static [GameEntry] {
    static GAMES: std::sync::OnceLock<Vec<GameEntry>> = std::sync::OnceLock::new();
    GAMES.get_or_init(|| {
        lightspeed_client::games::all_game_keys()
            .into_iter()
            .map(|(key, display)| GameEntry {
                key,
                display,
                default_port: lightspeed_client::games::resolve(key)
                    .map(|game| game.redirect_port())
                    .unwrap_or(0),
            })
            .collect()
    })
}

// ── User games (profiles.toml) ─────────────────────────────────────────────

/// The game the app has pinned: a built-in registry entry, or one of the
/// user's own `[[games]]` profiles.
///
/// Returned by value because a user profile owns its strings: `GameEntry`'s
/// `&'static str` cannot name a key that lives in `profiles.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PickedGame {
    pub(super) key: String,
    pub(super) display: String,
    pub(super) default_port: u16,
}

/// The process-wide profile store the editor reads and writes.
///
/// The same store `games::resolve` consults, so an edit saved here is live for
/// the next Optimize in this process and for the next start from disk.
pub(super) fn profile_store() -> &'static ProfileStore {
    lightspeed_client::profiles::global()
}

/// The loaded user game profiles, as of the store's last reload.
pub(super) fn user_profiles() -> Vec<UserGameProfile> {
    profile_store().snapshot().games.clone()
}

/// The loaded saved servers, as of the store's last reload.
pub(super) fn user_servers() -> Vec<UserServer> {
    profile_store().snapshot().servers.clone()
}

/// The keys of the loaded user game profiles.
pub(super) fn user_game_keys() -> Vec<String> {
    user_profiles()
        .into_iter()
        .map(|profile| profile.key)
        .collect()
}

/// One row of the Game dropdown, in render order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GamePickRow {
    /// "Auto (detect)".
    Auto,
    /// "Manual / Custom…".
    Manual,
    /// A built-in game, by index into [`games`].
    Builtin(usize),
    /// The "My games" group label, drawn only when profiles exist.
    MyGamesHeader,
    /// A user profile, by index into the snapshot the UI holds.
    User(usize),
    /// "New custom game…", always the last row.
    NewCustom,
}

/// The Game dropdown's rows: the two modes, every built-in in registry order,
/// then the "My games" group, then "New custom game…".
///
/// Additive on purpose: the built-ins keep their order and their indices, so a
/// `selected_game` key saved before profiles existed still means the same game,
/// and the user's own list simply appears below them.
pub(super) fn game_pick_rows(builtin_count: usize, user_count: usize) -> Vec<GamePickRow> {
    let mut rows = Vec::with_capacity(builtin_count + user_count + 4);
    rows.push(GamePickRow::Auto);
    rows.push(GamePickRow::Manual);
    rows.extend((0..builtin_count).map(GamePickRow::Builtin));
    if user_count > 0 {
        rows.push(GamePickRow::MyGamesHeader);
        rows.extend((0..user_count).map(GamePickRow::User));
    }
    rows.push(GamePickRow::NewCustom);
    rows
}
