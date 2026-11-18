use super::*;

/// The custom-game editor's fields, plus the profile it edits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GameEditorDraft {
    /// The user profile being edited, by index into the store's games; `None`
    /// while composing a new one.
    pub(super) editing: Option<usize>,
    /// The schema key. Kept from the profile being edited, so renaming does not
    /// silently orphan a key the user already put on a command line; empty for
    /// a new draft, where the key is the slug of the name.
    pub(super) key: String,
    pub(super) name: String,
    /// The process-names field, as typed (comma- or space-separated).
    pub(super) process_names: String,
    /// The UDP-range field, as typed.
    pub(super) udp_range: String,
    pub(super) dynamic_server: bool,
    pub(super) notes: String,
}

impl GameEditorDraft {
    /// A blank draft: "New custom game…".
    pub(super) fn new_profile() -> Self {
        Self::default()
    }

    /// The draft that edits `profile`.
    pub(super) fn from_profile(index: usize, profile: &UserGameProfile) -> Self {
        Self {
            editing: Some(index),
            key: profile.key.clone(),
            name: profile.name.clone(),
            process_names: profile.process_names.join(", "),
            udp_range: profile
                .udp_ports
                .map(|range| range.to_string())
                .unwrap_or_default(),
            dynamic_server: profile.dynamic_server,
            notes: profile.notes.clone().unwrap_or_default(),
        }
    }

    /// The schema key this draft describes: the edited profile's own key, else
    /// the slug of the name.
    pub(super) fn key(&self) -> Option<String> {
        if !self.key.is_empty() {
            return Some(self.key.clone());
        }
        slugify(self.name.trim())
    }

    /// The `[[games]]` entry this draft describes. Every field goes through the
    /// schema's own parser, so what is written is what the fields hold.
    pub(super) fn to_profile(&self) -> UserGameProfile {
        let notes = self.notes.trim();
        UserGameProfile {
            name: self.name.trim().to_string(),
            key: self.key().unwrap_or_default(),
            process_names: split_process_names(&self.process_names),
            udp_ports: parse_user_port_range(&self.udp_range).ok().flatten(),
            dynamic_server: self.dynamic_server,
            notes: (!notes.is_empty()).then(|| notes.to_string()),
        }
    }
}

/// The editor's UDP-range field, parsed by the schema's own parser so the field
/// and the file can never disagree about what a range is.
pub(super) fn parse_user_port_range(text: &str) -> Result<Option<PortRange>, String> {
    parse_port_range(text)
}

/// The editor's process-names field: image names separated by commas or spaces,
/// blanks dropped, so a trailing comma is not a typo the user has to hunt down.
pub(super) fn split_process_names(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The built-in game that already answers to `key`, if any.
///
/// `detect_game` is crate-private, so the probe is `resolve` - the one public
/// resolution path, whose built-in match arms (keys and aliases) answer first,
/// exactly like the schema's own collision check. A key the user's store
/// already defines is a duplicate of their own, not a built-in claim.
pub(super) fn builtin_claim_of_key(key: &str, user_keys: &[String]) -> Option<String> {
    if user_keys.iter().any(|existing| existing == key) {
        return None;
    }
    lightspeed_client::games::resolve(key)
        .ok()
        .map(|game| game.name().to_string())
}

/// The built-in game that already claims `process`, if any.
///
/// Image names compare case-insensitively, as they do on every platform the
/// scanner runs on; this mirrors the schema's own collision check.
pub(super) fn builtin_claim_of_process(process: &str) -> Option<String> {
    lightspeed_client::games::all_game_keys()
        .into_iter()
        .find_map(|(_, display)| {
            lightspeed_client::games::process_names_for_name(display)
                .iter()
                .any(|known| known.eq_ignore_ascii_case(process))
                .then(|| display.to_string())
        })
}

/// The editor's live verdict on a draft: the schema's save rules, attributed to
/// the field that carries each one, so the offending input is tinted while the
/// user types instead of only when the save is refused.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct DraftVerdict {
    pub(super) name_ok: bool,
    pub(super) udp_ok: bool,
    pub(super) key_ok: bool,
    pub(super) processes_ok: bool,
    pub(super) target_ok: bool,
    /// The built-in game that claims the draft's key, when one does.
    pub(super) key_claimed_by: Option<String>,
    /// The draft's key is already used by another of the user's own games.
    pub(super) key_duplicate: bool,
}

impl DraftVerdict {
    /// Whether the draft can be saved: every field passes its own rule.
    pub(super) fn ok(&self) -> bool {
        self.name_ok && self.udp_ok && self.key_ok && self.processes_ok && self.target_ok
    }
}

/// Mirror the schema's save rules per field.
///
/// The authority stays the client's schema - `ProfileStore::save` re-validates
/// and refuses - but a rule the user cannot see is a rule they cannot satisfy,
/// so everything that can block a save is surfaced on the field that carries it.
/// `user_keys` is the store's current keys; `editing` is the key of the profile
/// being edited, which may keep its own.
pub(super) fn validate_draft(
    draft: &GameEditorDraft,
    user_keys: &[String],
    editing: Option<&str>,
) -> DraftVerdict {
    let processes = split_process_names(&draft.process_names);
    let range = parse_user_port_range(&draft.udp_range);
    let key = draft.key();
    let claimed_by = key
        .as_deref()
        .and_then(|k| builtin_claim_of_key(k, user_keys));
    let duplicate = key
        .as_deref()
        .is_some_and(|k| Some(k) != editing && user_keys.iter().any(|existing| existing == k));
    DraftVerdict {
        name_ok: !draft.name.trim().is_empty(),
        udp_ok: range.is_ok(),
        key_ok: key.as_deref().is_some_and(is_valid_key) && claimed_by.is_none() && !duplicate,
        processes_ok: processes
            .iter()
            .all(|process| builtin_claim_of_process(process).is_none()),
        // At least one of the two targeting fields, as the schema requires.
        target_ok: !processes.is_empty() || matches!(range, Ok(Some(_))),
        key_claimed_by: claimed_by,
        key_duplicate: duplicate,
    }
}

/// The localized reasons a draft cannot be saved, in field order.
///
/// The verdict's flags say *which* field is wrong; the wording is the user's
/// language, so a translated window never shows English rule prose.
pub(super) fn draft_reasons(verdict: &DraftVerdict, key: Option<&str>) -> Vec<String> {
    let mut reasons = Vec::new();
    if !verdict.name_ok {
        reasons.push(i18n::t("game.editor_name_required").into_owned());
    }
    if !verdict.udp_ok {
        reasons.push(i18n::t("game.editor_udp_invalid").into_owned());
    }
    if !verdict.processes_ok {
        reasons.push(i18n::t("game.editor_process_reserved").into_owned());
    }
    if !verdict.key_ok {
        if let Some(builtin) = &verdict.key_claimed_by {
            reasons.push(i18n::t_with(
                "game.editor_key_reserved",
                &[("key", key.unwrap_or_default()), ("name", builtin.as_str())],
            ));
        } else if verdict.key_duplicate {
            reasons.push(i18n::t("game.editor_key_duplicate").into_owned());
        } else {
            reasons.push(i18n::t("game.editor_key_underivable").into_owned());
        }
    }
    if !verdict.target_ok {
        reasons.push(i18n::t("game.editor_target_required").into_owned());
    }
    reasons
}

/// The `[[servers]]` entry the Manual flow's "Save as…" builds from the fields
/// on screen, or the reason it cannot: what is saved is what was typed.
pub(super) fn server_from_fields(
    name: &str,
    address: &str,
    port_range: &str,
) -> Result<UserServer, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(i18n::t("servers.invalid_name").into_owned());
    }
    let address = address.trim();
    if address.parse::<SocketAddrV4>().is_err() {
        return Err(i18n::t("servers.invalid_address").into_owned());
    }
    Ok(UserServer {
        name: name.to_string(),
        address: address.to_string(),
        udp_ports: parse_user_port_range(port_range)?,
    })
}

/// Persist the profiles document, and refresh the cached snapshot.
///
/// `ProfileStore::save` validates, writes, and reloads: the reload is what makes
/// an edit take effect on the next `resolve()` - the next Optimize - without a
/// restart, and the file write is what carries it into the next start.
pub(super) fn save_user_profiles(
    store: &ProfileStore,
    games: &[UserGameProfile],
    servers: &[UserServer],
) -> Result<(), String> {
    store.save(games, servers).map_err(|errors| {
        errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; ")
    })
}
