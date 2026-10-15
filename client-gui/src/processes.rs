//! The Custom game editor's live process picker.
//!
//! The host's process table comes from
//! [`lightspeed_client::processes::list_running_processes`], which is a real
//! enumeration (`CreateToolhelp32Snapshot`, a `/proc` walk, or a `ps` fork) and
//! therefore not something the egui thread may run: opening a sheet must never
//! stall the window on the process table.
//!
//! So enumeration happens on a background thread and is published through a
//! one-shot channel the UI only polls, exactly like relay discovery
//! ([`crate::discovery::spawn_discovery`]) - see `ProcessListState` in
//! `app.rs`. This module owns that spawn plus the pure pieces the picker is
//! built from: the filter, the append-and-dedupe into the typed field, and the
//! path-tail elision that keeps a row inside the sheet's width. The pure pieces
//! are unit-tested here, on every platform.

use std::sync::mpsc::{self, Receiver};

pub use lightspeed_client::processes::ProcessEntry;

/// The longest path tail a picker row keeps, in characters.
///
/// A Windows image path is routinely 80+ characters and would push the row past
/// the sheet's width, so a row shows the last `PATH_TAIL_CHARS` of it, elided
/// with a leading `…`. The image name and the PID stay intact either way, so an
/// elided path never costs the row its identity.
pub const PATH_TAIL_CHARS: usize = 34;

/// Run `job` on a detached worker thread, handing back the single value it
/// produces.
///
/// A one-shot `Receiver` is the GUI's whole contract with background work -
/// discovery, the latency race, the health probe, the RTT re-measurement all
/// publish this way - so the worker only ever touches its own value and the
/// channel, and the caller never blocks on it.
fn spawn_worker<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Receiver<T> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(job());
    });
    rx
}

/// Enumerate the host's processes on a background thread.
///
/// The egui thread only ever holds the returned receiver, so opening the editor
/// cannot block on the enumeration. An empty vector means "the platform would
/// not tell us" - the picker says so and the typed field keeps working.
pub fn spawn_process_list() -> Receiver<Vec<ProcessEntry>> {
    spawn_worker(lightspeed_client::processes::list_running_processes)
}

/// The rows of `entries` whose image name or path contains `query`,
/// case-insensitively. A blank query keeps every row, in the enumeration's
/// order (name, then PID).
pub fn matching<'a>(entries: &'a [ProcessEntry], query: &str) -> Vec<&'a ProcessEntry> {
    let needle = query.trim().to_lowercase();
    entries
        .iter()
        .filter(|entry| {
            needle.is_empty()
                || entry.name.to_lowercase().contains(&needle)
                || entry
                    .path
                    .as_deref()
                    .is_some_and(|path| path.to_lowercase().contains(&needle))
        })
        .collect()
}

/// The process-names field after choosing the row named `name`.
///
/// What the user typed is kept verbatim: the name is appended only when the
/// field does not already name it (case-insensitively, matching how the field
/// is split on save), and the separator the user may have left behind is
/// reused rather than doubled - so `"MyGame.exe, "` becomes
/// `"MyGame.exe, Other.exe"`, never `"MyGame.exe, , Other.exe"`. Choosing a row
/// twice therefore changes nothing.
pub fn append_name(field: &str, name: &str) -> String {
    let already_named = field
        .split(|c: char| c == ',' || c.is_whitespace())
        .any(|existing| existing.eq_ignore_ascii_case(name));
    if already_named {
        return field.to_string();
    }
    let trimmed = field.trim_end_matches(|c: char| c == ',' || c.is_whitespace());
    if trimmed.is_empty() {
        return name.to_string();
    }
    format!("{trimmed}, {name}")
}

/// The tail of `path`, at most `max_chars` characters, elided with a leading
/// `…` when it had to be cut.
///
/// Counted in characters, not bytes, so a non-ASCII path cannot be sliced
/// through the middle of a code point. The cut then lands on a separator when
/// one is in reach, so a row reads `…\bin\win64\Game.exe` rather than a
/// severed folder name.
pub fn tail_path(path: &str, max_chars: usize) -> String {
    let length = path.chars().count();
    if length <= max_chars {
        return path.to_string();
    }
    let tail: String = path.chars().skip(length - max_chars).collect();
    let cut = tail.find(['\\', '/']).map_or(0, |at| at + 1);
    format!("…{}", &tail[cut..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Generous bound: the enumeration is one OS call, but a loaded machine
    /// must not turn a slow snapshot into a test failure.
    const RECV_TIMEOUT: Duration = Duration::from_secs(60);

    fn entry(name: &str, pid: u32, path: Option<&str>) -> ProcessEntry {
        ProcessEntry {
            pid,
            name: name.to_string(),
            path: path.map(str::to_string),
            started_at: None,
        }
    }

    /// The off-thread contract, proven directly: the caller's thread id is not
    /// the one the job ran on, so nothing that goes through `spawn_worker` -
    /// including the process enumeration - can run on the egui thread.
    #[test]
    fn the_enumeration_runs_off_the_calling_thread() {
        let caller = std::thread::current().id();
        let rx = spawn_worker(move || std::thread::current().id());
        let worker = rx
            .recv_timeout(RECV_TIMEOUT)
            .expect("the worker must report back");
        assert_ne!(
            worker, caller,
            "the job ran on the caller's thread, which would block the egui thread"
        );
    }

    /// The wiring end to end: the spawned enumeration really does enumerate, and
    /// a process that is certainly running - this one - is in the result.
    #[test]
    fn the_enumeration_reports_this_process() {
        let rx = spawn_process_list();
        let rows = rx
            .recv_timeout(RECV_TIMEOUT)
            .expect("the enumeration must return");
        let me = std::process::id();
        assert!(
            rows.iter().any(|row| row.pid == me),
            "the enumeration did not list this process (pid {me}): {} rows",
            rows.len()
        );
    }

    #[test]
    fn the_filter_matches_name_and_path_case_insensitively() {
        let rows = vec![
            entry(
                "javaw.exe",
                1,
                Some("C:\\Program Files\\Java\\bin\\javaw.exe"),
            ),
            entry("steam.exe", 2, Some("C:\\Steam\\steam.exe")),
            entry("systemd", 3, None),
        ];

        let names: Vec<&str> = matching(&rows, "JAVAW")
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["javaw.exe"],
            "the name matches case-insensitively"
        );

        let paths: Vec<&str> = matching(&rows, "program files")
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["javaw.exe"],
            "a row matches on its path, not only its name"
        );

        assert!(
            matching(&rows, "nonesuch").is_empty(),
            "a filter nothing matches selects nothing"
        );
    }

    #[test]
    fn a_blank_filter_keeps_every_row_in_order() {
        let rows = vec![
            entry("java", 1, None),
            entry("javaw.exe", 2, None),
            entry("steam.exe", 3, None),
        ];
        for query in ["", "   "] {
            let kept: Vec<&str> = matching(&rows, query)
                .iter()
                .map(|row| row.name.as_str())
                .collect();
            assert_eq!(
                kept,
                vec!["java", "javaw.exe", "steam.exe"],
                "query {query:?}"
            );
        }
    }

    /// The row-choosing rule: keep the typed text, append once, dedupe
    /// case-insensitively, and never double a separator.
    #[test]
    fn choosing_a_process_keeps_the_typed_field_and_dedupes() {
        assert_eq!(append_name("", "MyGame.exe"), "MyGame.exe");
        assert_eq!(append_name("   ", "MyGame.exe"), "MyGame.exe");
        assert_eq!(
            append_name("Handwritten.exe", "MyGame.exe"),
            "Handwritten.exe, MyGame.exe"
        );
        assert_eq!(
            append_name("Handwritten.exe, ", "MyGame.exe"),
            "Handwritten.exe, MyGame.exe",
            "a separator the user left behind is reused, not doubled"
        );
        assert_eq!(
            append_name("MyGame.exe, Handwritten.exe", "MyGame.exe"),
            "MyGame.exe, Handwritten.exe",
            "a name the field already holds is not appended twice"
        );
        assert_eq!(
            append_name("mygame.exe", "MyGame.exe"),
            "mygame.exe",
            "the dedupe compares image names case-insensitively"
        );
        assert_eq!(
            append_name("MyGame.exe,Launcher.exe", "Other.exe"),
            "MyGame.exe,Launcher.exe, Other.exe",
            "what the user typed is kept as typed"
        );
    }

    #[test]
    fn a_long_path_is_trimmed_to_its_tail() {
        let path = "C:\\Program Files\\Some Vendor\\Some Game\\bin\\win64\\SomeGame.exe";
        let tail = tail_path(path, PATH_TAIL_CHARS);
        assert!(tail.starts_with('…'), "a cut path is marked as cut: {tail}");
        assert!(
            tail.ends_with("SomeGame.exe"),
            "the tail keeps the file: {tail}"
        );
        assert!(
            tail.chars().count() <= PATH_TAIL_CHARS + 1,
            "a trimmed path must stay inside the picker's width budget: {tail}"
        );
        assert!(
            tail.starts_with("…Some Game\\bin\\"),
            "the cut lands on a separator, not inside a folder name: {tail}"
        );

        assert_eq!(
            tail_path("C:\\short.exe", PATH_TAIL_CHARS),
            "C:\\short.exe",
            "a path that fits is shown as it is"
        );

        // Character-counted, so a multi-byte path cannot be cut mid-codepoint.
        let wide = "C:\\日本語のゲーム\\とても長いフォルダー名\\ゲーム.exe";
        let tail = tail_path(wide, 10);
        assert!(
            tail.starts_with('…') && tail.ends_with("ゲーム.exe"),
            "{tail}"
        );
    }
}
