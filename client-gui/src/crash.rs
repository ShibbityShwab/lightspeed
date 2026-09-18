//! Crash log + panic hook.
//!
//! The GUI is built with `windows_subsystem = "windows"`, so it has no
//! console: a panic or a returned `Err` is invisible unless we persist it
//! ourselves. [`install_panic_hook`] appends the panic payload, its source
//! location, and a captured backtrace to [`crate::paths::crash_log`] and, on
//! Windows, shows a native message box. Every failure path degrades to a
//! stderr write; the hook itself must never panic.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set while the hook is running so a panic inside the hook cannot recurse.
static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Environment variable that redirects the crash log (diagnostics + tests).
const CRASH_LOG_ENV: &str = "LIGHTSPEED_GUI_CRASH_LOG";

/// Environment variable that suppresses the native crash dialog (headless).
#[cfg(windows)]
const NO_DIALOG_ENV: &str = "LIGHTSPEED_GUI_NO_DIALOG";

/// Install a panic hook that persists the panic before chaining the previous
/// hook. Safe to call first thing in `main`.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if HOOK_ACTIVE.swap(true, Ordering::SeqCst) {
            // Already handling a panic; do not recurse.
            return;
        }
        // A panic inside the hook must not replace the original panic:
        // swallow every error (the body already ignores all write failures).
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let payload = if let Some(text) = info.payload().downcast_ref::<&str>() {
                (*text).to_string()
            } else if let Some(text) = info.payload().downcast_ref::<String>() {
                text.clone()
            } else {
                "non-string panic payload".to_string()
            };
            let location = info
                .location()
                .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()));
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let record = format_panic_record(&payload, location.as_deref(), &backtrace);
            write_record(&record);
            show_dialog(&format!(
                "LightSpeed encountered an error and must close.\n\n{payload}\n\n\
                 A crash log was written to:\n{}",
                preferred_path().display()
            ));
        }));
        HOOK_ACTIVE.store(false, Ordering::SeqCst);
        previous(info);
    }));
}

/// Record a fatal (non-panic) startup error and show it to the user.
///
/// Used by the `main` `Err` wrapper, where the error would otherwise be
/// invisible in a console-less GUI build.
pub fn record_fatal(message: &str) {
    let record = format_fatal_record(message);
    write_record(&record);
    show_dialog(&format!(
        "LightSpeed could not start.\n\n{message}\n\n\
         A crash log was written to:\n{}",
        preferred_path().display()
    ));
}

/// Format one panic record. Pure so it can be unit-tested.
pub fn format_panic_record(payload: &str, location: Option<&str>, backtrace: &str) -> String {
    let location = location.unwrap_or("<unknown location>");
    format!("panic at {location}\n{payload}\n\nbacktrace:\n{backtrace}\n")
}

/// Format one fatal-error record. Pure so it can be unit-tested.
pub fn format_fatal_record(message: &str) -> String {
    format!("fatal error\n{message}\n")
}

/// Preferred crash-log path, overridable for diagnostics and tests.
fn preferred_path() -> PathBuf {
    match std::env::var_os(CRASH_LOG_ENV) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => crate::paths::crash_log(),
    }
}

/// Append a record to the preferred log, then the temp dir, then stderr.
/// Never panics: every failure is ignored and the next sink is tried.
fn write_record(record: &str) {
    if append_to(&preferred_path(), record).is_ok() {
        return;
    }
    let fallback = std::env::temp_dir().join("lightspeed-gui-crash.log");
    if append_to(&fallback, record).is_ok() {
        return;
    }
    let _ = writeln!(std::io::stderr(), "{record}");
}

/// Append one timestamped record to `path`, creating the file/parent as needed.
fn append_to(path: &Path, record: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    writeln!(file, "===== LightSpeed GUI crash @ unix {secs} =====")?;
    writeln!(file, "{record}")?;
    Ok(())
}

/// Show a native error dialog on Windows; a no-op elsewhere (never blocks).
#[cfg(windows)]
fn show_dialog(message: &str) {
    use std::ffi::c_void;

    if std::env::var_os(NO_DIALOG_ENV).is_some() {
        return;
    }

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONERROR: u32 = 0x0000_0010;
    const MB_TOPMOST: u32 = 0x0004_0000;

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let text = wide(message);
    let caption = wide("LightSpeed");
    // SAFETY: both strings are NUL-terminated and outlive the call; a null
    // owner window is valid for a standalone message box.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_TOPMOST,
        );
    }
}

#[cfg(not(windows))]
fn show_dialog(_message: &str) {}

#[cfg(test)]
mod tests {
    use super::{append_to, format_fatal_record, format_panic_record, record_fatal, CRASH_LOG_ENV};

    #[test]
    fn panic_record_includes_payload_location_and_backtrace() {
        let record = format_panic_record("boom", Some("src/main.rs:12:5"), "frame 0");
        assert!(record.contains("boom"));
        assert!(record.contains("src/main.rs:12:5"));
        assert!(record.contains("frame 0"));
    }

    #[test]
    fn panic_record_marks_a_missing_location() {
        let record = format_panic_record("boom", None, "frame 0");
        assert!(record.contains("<unknown location>"));
        assert!(record.contains("boom"));
    }

    #[test]
    fn fatal_record_includes_the_message() {
        let record = format_fatal_record("could not start");
        assert!(record.contains("could not start"));
    }

    #[test]
    fn append_to_writes_a_record_to_a_temp_file() {
        let path =
            std::env::temp_dir().join(format!("lightspeed-crash-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        append_to(&path, "hello crash").expect("write record");
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("hello crash"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn record_fatal_writes_to_the_configured_path_without_panicking() {
        let path =
            std::env::temp_dir().join(format!("lightspeed-fatal-test-{}.log", std::process::id()));
        let _ = std::fs::remove_file(&path);
        std::env::set_var(CRASH_LOG_ENV, &path);
        std::env::set_var("LIGHTSPEED_GUI_NO_DIALOG", "1");
        record_fatal("startup exploded");
        std::env::remove_var(CRASH_LOG_ENV);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(text.contains("startup exploded"));
        let _ = std::fs::remove_file(&path);
    }
}
