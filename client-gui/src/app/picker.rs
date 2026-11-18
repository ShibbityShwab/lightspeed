use super::*;

/// What the picker is showing this frame: rows from the last enumeration, or a
/// load still in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PickerStatus {
    Loading,
    Ready,
}

/// The picker's live inputs for one frame, borrowed from the app.
pub(super) struct ProcessPicker<'a> {
    /// The rows the last enumeration produced.
    pub(super) rows: &'a [ProcessEntry],
    pub(super) status: PickerStatus,
    /// The filter field, editable in place.
    pub(super) search: &'a mut String,
}

/// What the picker knows about the last enumeration ({status} of
/// [`ProcessListState`]).
/// Resolve the game a start should use.
///
/// In Auto mode this is whatever detection last found, not the index the picker
/// was built with. The index is fixed at construction, so a GUI launched before
/// the game falls back to the first entry (Fortnite), and the zero-click start
/// then failed with "Fortnite is not running" while ARC Raiders was on screen.
pub(super) fn pick_game_entry(
    game_auto: bool,
    auto_key: Option<&str>,
    selected_idx: usize,
) -> PickedGame {
    let games = games();
    if game_auto {
        if let Some(key) = auto_key {
            if let Some(entry) = games
                .iter()
                .find(|entry| entry.key.eq_ignore_ascii_case(key))
            {
                return PickedGame {
                    key: entry.key.to_string(),
                    display: entry.display.to_string(),
                    default_port: entry.default_port,
                };
            }
        }
    }
    let idx = selected_idx.min(games.len().saturating_sub(1));
    let entry = &games[idx];
    PickedGame {
        key: entry.key.to_string(),
        display: entry.display.to_string(),
        default_port: entry.default_port,
    }
}

pub(super) fn picker_status(state: &ProcessListState) -> PickerStatus {
    match state {
        ProcessListState::Done => PickerStatus::Ready,
        // A load is either running or about to be started by the sheet's own
        // open handler, so the honest thing on screen is the spinner's line.
        ProcessListState::InFlight(_) | ProcessListState::Idle => PickerStatus::Loading,
    }
}

/// The process target a picker row stands for: the image name - the identity
/// that survives a relaunch - plus the PID the row was running under. The start
/// path re-resolves the name against the live process table, so the PID is what
/// the status surface shows, never what interception binds to.
pub(super) fn target_for_pick(entry: &ProcessEntry) -> ProcessTarget {
    ProcessTarget {
        name: Some(entry.name.clone()),
        pid: Some(entry.pid),
    }
}

/// The status-surface line for a picked process: `Target: <name> (PID <n>)`,
/// naming the process exactly as the start path describes it.
pub(super) fn target_summary(target: &ProcessTarget) -> String {
    let described = target.describe();
    i18n::t_with("game.target_set", &[("target", described.as_str())])
}

/// Apply one frame of the target picker to the app's target.
///
/// A chosen row wins; an explicit Clear drops the target so the built-in Game
/// path decides again; a frame with neither leaves the target alone.
pub(super) fn apply_target_choice(
    current: Option<ProcessTarget>,
    picked: Option<ProcessTarget>,
    clear: bool,
) -> Option<ProcessTarget> {
    if clear {
        None
    } else {
        picked.or(current)
    }
}
