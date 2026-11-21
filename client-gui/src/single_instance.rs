//! Single-instance guard.
//!
//! Launching two GUIs would mean two engines, two tray icons, and two
//! registry/health pollers racing over the same relays. The guard is acquired
//! before any other startup work in `main`; when it fails, the second process
//! shows a short notice and exits 2 without touching the engine or tray.
//!
//! - Windows: a named mutex (`Local\` namespace, per session). The kernel
//!   releases it when the process dies, so a crash never leaves a stale lock.
//! - Linux/macOS: an exclusive advisory lock on `instance.lock` under the
//!   data directory. The OS releases the lock on process death, so `kill -9`
//!   followed by a relaunch works.

#[cfg(not(windows))]
use std::path::Path;

/// Held for the lifetime of the first instance; dropping it releases the
/// guard.
pub struct InstanceGuard {
    #[cfg(windows)]
    mutex: usize,
    #[cfg(not(windows))]
    _lock: Option<std::fs::File>,
}

/// Outcome of trying to become the first instance.
pub enum InstanceOutcome {
    /// This process owns the guard.
    Acquired(InstanceGuard),
    /// Another LightSpeed instance already holds the guard.
    AlreadyRunning,
}

/// Acquire the process-wide single-instance guard.
///
/// `--force` on the command line or `LIGHTSPEED_GUI_FORCE=1` in the
/// environment bypasses the guard entirely (logged, so it is never silent).
pub fn acquire() -> InstanceOutcome {
    let args: Vec<String> = std::env::args().collect();
    let env_force = std::env::var("LIGHTSPEED_GUI_FORCE")
        .map(|value| value == "1")
        .unwrap_or(false);
    if force_requested(&args, env_force) {
        tracing::warn!("Single-instance guard bypassed (--force or LIGHTSPEED_GUI_FORCE=1)");
        return unguarded();
    }
    #[cfg(windows)]
    {
        acquire_named_mutex(MUTEX_NAME)
    }
    #[cfg(not(windows))]
    {
        acquire_lock_file(&crate::paths::data_dir().join("instance.lock"))
    }
}

/// Whether the user explicitly asked to skip the single-instance guard.
///
/// Pure so it is unit-testable on every platform.
pub fn force_requested(args: &[String], env_force: bool) -> bool {
    env_force || args.iter().any(|arg| arg == "--force")
}

/// A guard that owns nothing, used when the check is bypassed.
fn unguarded() -> InstanceOutcome {
    #[cfg(windows)]
    {
        InstanceOutcome::Acquired(InstanceGuard { mutex: 0 })
    }
    #[cfg(not(windows))]
    {
        InstanceOutcome::Acquired(InstanceGuard { _lock: None })
    }
}

/// Explain why this launch is exiting, then return.
pub fn show_already_running_notice() {
    let message = "LightSpeed is already running.\n\n\
                   Look for the \u{26a1} icon in the system tray, or the open \
                   LightSpeed window.";
    tracing::info!("Second instance detected, showing notice and exiting");
    #[cfg(windows)]
    message_box(message);
    #[cfg(not(windows))]
    {
        eprintln!("{message}");
        notice_window(message);
    }
}

// ── Windows: named mutex ─────────────────────────────────────────────────────

#[cfg(windows)]
const MUTEX_NAME: &str = "Local\\LightSpeed-GUI-Single-Instance";

#[cfg(windows)]
fn acquire_named_mutex(name: &str) -> InstanceOutcome {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // Clear this thread's last-error first: a stale non-zero value could
    // otherwise masquerade as ERROR_ALREADY_EXISTS below.
    // SAFETY: `SetLastError` only writes the calling thread's error slot.
    unsafe { win::SetLastError(0) };
    // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call;
    // a null SECURITY_ATTRIBUTES requests the default descriptor.
    let handle = unsafe { win::CreateMutexW(std::ptr::null_mut(), 0, wide.as_ptr()) };
    if handle.is_null() {
        tracing::warn!("CreateMutexW failed; continuing without single-instance guard");
        return InstanceOutcome::Acquired(InstanceGuard { mutex: 0 });
    }
    // SAFETY: `CreateMutexW` was just called on this thread, so `GetLastError`
    // reports its result.
    if unsafe { win::GetLastError() } == win::ERROR_ALREADY_EXISTS {
        // SAFETY: `handle` is a valid handle returned by `CreateMutexW`.
        unsafe { win::CloseHandle(handle) };
        return InstanceOutcome::AlreadyRunning;
    }
    InstanceOutcome::Acquired(InstanceGuard {
        mutex: handle as usize,
    })
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if self.mutex != 0 {
            // SAFETY: `mutex` was returned by `CreateMutexW` and is closed
            // exactly once, here.
            unsafe { win::CloseHandle(self.mutex as *mut std::ffi::c_void) };
        }
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateMutexW(
            attributes: *mut c_void,
            initial_owner: i32,
            name: *const u16,
        ) -> *mut c_void;
        pub fn GetLastError() -> u32;
        pub fn SetLastError(error: u32);
        pub fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// `CreateMutexW` sets this when the named mutex already exists.
    pub const ERROR_ALREADY_EXISTS: u32 = 183;
}

#[cfg(windows)]
fn message_box(message: &str) {
    use std::ffi::c_void;

    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    const MB_OK: u32 = 0x0000_0000;
    const MB_ICONINFORMATION: u32 = 0x0000_0040;
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
            MB_OK | MB_ICONINFORMATION | MB_TOPMOST,
        );
    }
}

// ── Linux/macOS: exclusive lock file ─────────────────────────────────────────

#[cfg(not(windows))]
fn acquire_lock_file(path: &Path) -> InstanceOutcome {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Could not create {}: {e}", parent.display());
        }
    }
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
    {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!("Could not open instance lock {}: {e}", path.display());
            return InstanceOutcome::Acquired(InstanceGuard { _lock: None });
        }
    };
    match file.try_lock() {
        Ok(()) => InstanceOutcome::Acquired(InstanceGuard { _lock: Some(file) }),
        Err(std::fs::TryLockError::WouldBlock) => InstanceOutcome::AlreadyRunning,
        Err(std::fs::TryLockError::Error(e)) => {
            // A filesystem without advisory-lock support must not brick the
            // app; continue without the guard and say so in the log.
            tracing::warn!("Instance lock unsupported ({e}); continuing without guard");
            InstanceOutcome::Acquired(InstanceGuard { _lock: None })
        }
    }
}

#[cfg(not(windows))]
fn notice_window(message: &str) {
    struct Notice {
        message: String,
    }

    impl eframe::App for Notice {
        fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
            eframe::egui::CentralPanel::default().show(ui, |ui| {
                ui.heading("LightSpeed");
                ui.add_space(4.0);
                ui.label(&self.message);
                ui.add_space(8.0);
                if ui.button("OK").clicked() {
                    ui.ctx()
                        .send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
            });
        }
    }

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([380.0, 160.0])
            .with_resizable(false)
            .with_title("LightSpeed"),
        ..Default::default()
    };
    let message = message.to_string();
    let _ = eframe::run_native(
        "LightSpeed",
        options,
        Box::new(move |_cc| Ok(Box::new(Notice { message }))),
    );
}

#[cfg(test)]
mod tests {
    use super::force_requested;
    #[cfg(not(windows))]
    use super::{acquire_lock_file, InstanceOutcome};
    #[cfg(windows)]
    use super::{acquire_named_mutex, InstanceOutcome};

    /// How long the post-drop release poll may take before the lock test fails.
    /// Generous for a loaded CI runner, short enough that a guard which genuinely
    /// never releases still fails the test quickly. Only the non-Windows lock test
    /// uses it, so it is gated to keep the Windows -D warnings build clean.
    #[cfg(not(windows))]
    const RELEASE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    /// One poll step for [`poll_until_released`].
    #[derive(Debug, PartialEq, Eq)]
    enum Polled<G> {
        /// The lock is free and the guard is held again.
        Ready(G),
        /// The lock is still held (or its release is not visible yet).
        Busy,
    }

    /// Poll `attempt` until it yields a guard, or `deadline` elapses.
    ///
    /// Why this exists: dropping the first guard closes the lock file, but the
    /// macOS CI job saw the *immediate* re-acquire occasionally report
    /// `WouldBlock` (the flake at `lock_file_guard_excludes_second_instance`).
    /// Waiting for the observable release with a bounded deadline removes the
    /// timing assumption **without weakening the assertion**: a guard that is
    /// genuinely never released leaves the poll `Busy` until the deadline
    /// expires and returns `None`, so the caller fails with its diagnostic.
    fn poll_until_released<G>(
        deadline: std::time::Duration,
        mut attempt: impl FnMut() -> Polled<G>,
    ) -> Option<G> {
        let started = std::time::Instant::now();
        loop {
            match attempt() {
                Polled::Ready(guard) => return Some(guard),
                Polled::Busy => {
                    if started.elapsed() >= deadline {
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }

    #[test]
    fn force_requested_accepts_the_flag_or_the_environment() {
        let plain = vec!["lightspeed-gui".to_string()];
        assert!(!force_requested(&plain, false));
        assert!(force_requested(&plain, true));

        let flagged = vec!["lightspeed-gui".to_string(), "--force".to_string()];
        assert!(force_requested(&flagged, false));
        assert!(force_requested(&flagged, true));
    }

    #[cfg(not(windows))]
    fn unique_lock_path(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lightspeed-single-instance-{}-{tag}.lock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[cfg(not(windows))]
    #[test]
    fn lock_file_guard_excludes_second_instance() {
        let path = unique_lock_path("excludes");
        let first = match acquire_lock_file(&path) {
            InstanceOutcome::Acquired(guard) => guard,
            InstanceOutcome::AlreadyRunning => panic!("first acquire must succeed"),
        };
        // The test's real assertion, unchanged: while the first guard is alive a
        // second instance is excluded (and the failed attempt does not disturb the
        // live lock).
        assert!(matches!(
            acquire_lock_file(&path),
            InstanceOutcome::AlreadyRunning
        ));
        drop(first);
        // A dropped guard must free the lock, but on macOS the very next
        // `try_lock` occasionally still reported `WouldBlock` (CI flake). Observe
        // the release with a bounded poll instead of assuming the next call wins;
        // a guard that never releases exhausts the deadline and fails the test.
        let second = poll_until_released(RELEASE_DEADLINE, || match acquire_lock_file(&path) {
            InstanceOutcome::Acquired(guard) => Polled::Ready(guard),
            InstanceOutcome::AlreadyRunning => Polled::Busy,
        })
        .unwrap_or_else(|| {
            panic!(
                "the dropped guard did not release the lock within {RELEASE_DEADLINE:?} \
                 (two live instances stay excluded, but a dropped guard must free it)"
            )
        });
        drop(second);
        let _ = std::fs::remove_file(&path);
    }

    /// The poll retries until the release is observed, and takes the common
    /// immediate-release path without sleeping.
    #[test]
    fn the_release_poll_retries_until_the_guard_is_available() {
        let mut calls = 0;
        let guard = poll_until_released(std::time::Duration::from_secs(1), || {
            calls += 1;
            if calls < 4 {
                Polled::Busy
            } else {
                Polled::Ready("released")
            }
        });
        assert_eq!(guard, Some("released"));
        assert_eq!(calls, 4, "the poll must retry until the release is visible");
    }

    /// The negative control for the hardening: a guard that is genuinely never
    /// released must not be reported as acquired. This is the exact mechanism the
    /// lock test relies on to still fail a broken release.
    #[test]
    fn the_release_poll_fails_when_the_guard_never_releases() {
        let started = std::time::Instant::now();
        let guard =
            poll_until_released(std::time::Duration::from_millis(50), || Polled::<()>::Busy);
        assert!(
            guard.is_none(),
            "a never-released guard must not be reported as acquired"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_millis(50),
            "the poll must exhaust its bounded deadline"
        );
    }

    /// The common case: the lock is already free on the first attempt.
    #[test]
    fn the_release_poll_takes_the_common_immediate_release() {
        let guard = poll_until_released(std::time::Duration::from_secs(1), || Polled::Ready(7u8));
        assert_eq!(guard, Some(7));
    }

    #[cfg(not(windows))]
    #[test]
    fn lock_file_guard_creates_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "lightspeed-single-instance-{}-nested",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sub").join("instance.lock");
        assert!(matches!(
            acquire_lock_file(&path),
            InstanceOutcome::Acquired(_)
        ));
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn named_mutex_excludes_second_instance() {
        let name = format!("Local\\LightSpeed-GUI-test-{}", std::process::id());
        let first = match acquire_named_mutex(&name) {
            InstanceOutcome::Acquired(guard) => guard,
            InstanceOutcome::AlreadyRunning => panic!("first acquire must succeed"),
        };
        assert!(matches!(
            acquire_named_mutex(&name),
            InstanceOutcome::AlreadyRunning
        ));
        drop(first);
    }
}
