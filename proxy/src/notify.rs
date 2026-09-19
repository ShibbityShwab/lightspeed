//! systemd readiness and watchdog notification.
//!
//! Implements the `sd_notify(3)` protocol directly over a Unix datagram
//! socket, with no extra crate dependency. `NOTIFY_SOCKET` may be an absolute
//! filesystem path or the Linux abstract form prefixed with `@`.
//!
//! Every function is a silent no-op when `NOTIFY_SOCKET` is unset or empty,
//! and no send failure ever blocks or panics the caller. On non-Unix targets
//! the entire module degrades to no-ops so Windows/macOS builds stay green.

use std::time::Duration;

/// systemd `WatchdogSec` expressed in microseconds, via `WATCHDOG_USEC`.
///
/// Returns half the configured interval, clamped to a floor of one second so
/// an aggressive watchdog never turns into a busy loop.
pub fn watchdog_interval() -> Option<Duration> {
    let raw = std::env::var("WATCHDOG_USEC").ok()?;
    let usec: u64 = raw.trim().parse().ok()?;
    if usec == 0 {
        return None;
    }
    Some(Duration::from_micros(usec / 2).max(Duration::from_secs(1)))
}

// ── Unix implementation ─────────────────────────────────────────────

#[cfg(unix)]
mod imp {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::UnixDatagram;

    /// Read `NOTIFY_SOCKET`, treating unset or empty as "notification off".
    pub(super) fn socket_path() -> Option<String> {
        std::env::var("NOTIFY_SOCKET")
            .ok()
            .filter(|value| !value.is_empty())
    }

    /// Map the raw `NOTIFY_SOCKET` value to the bytes systemd expects.
    ///
    /// The abstract form `@name` fills the leading address byte with a NUL
    /// (`\0name`); a filesystem path is returned byte-for-byte.
    pub(super) fn target_bytes(raw: &str) -> Vec<u8> {
        let mut bytes = raw.as_bytes().to_vec();
        if bytes.first() == Some(&b'@') {
            bytes[0] = 0;
        }
        bytes
    }

    /// Send one datagram. Never panics, never blocks, silently drops errors.
    pub(super) fn send(payload: &[u8]) {
        let Some(raw) = socket_path() else {
            return;
        };
        let Ok(socket) = UnixDatagram::unbound() else {
            return;
        };
        let target = target_bytes(&raw);
        let _ = send_to(&socket, &target, payload);
    }

    #[cfg(target_os = "linux")]
    fn send_to(socket: &UnixDatagram, target: &[u8], payload: &[u8]) -> std::io::Result<()> {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::SocketAddr;

        if target.first() == Some(&0) {
            let addr = SocketAddr::from_abstract_name(&target[1..])?;
            socket.send_to_addr(payload, &addr).map(|_| ())
        } else {
            let path = std::path::Path::new(OsStr::from_bytes(target));
            socket.send_to(payload, path).map(|_| ())
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn send_to(socket: &UnixDatagram, target: &[u8], payload: &[u8]) -> std::io::Result<()> {
        // The abstract namespace is Linux-only; a `@`-prefixed name has no
        // destination here, so the notification is silently skipped.
        if target.first() == Some(&0) {
            return Ok(());
        }
        let path = std::path::Path::new(OsStr::from_bytes(target));
        socket.send_to(payload, path).map(|_| ())
    }
}

// ── Public API ──────────────────────────────────────────────────────

/// Whether systemd notification is active for this process.
pub fn is_enabled() -> bool {
    #[cfg(unix)]
    {
        imp::socket_path().is_some()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Announce that the service has finished starting and is ready to serve.
pub fn ready() {
    #[cfg(unix)]
    imp::send(b"READY=1");
}

/// Reset the systemd watchdog timer (`WATCHDOG=1`).
pub fn watchdog() {
    #[cfg(unix)]
    imp::send(b"WATCHDOG=1");
}

/// Announce that the service has begun shutting down.
pub fn stopping() {
    #[cfg(unix)]
    imp::send(b"STOPPING=1");
}

/// Attach a human-readable status line to subsequent notifications.
pub fn status(msg: &str) {
    #[cfg(unix)]
    {
        let sanitized: String = msg
            .chars()
            .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
            .collect();
        let mut payload = String::with_capacity("STATUS=".len() + sanitized.len());
        payload.push_str("STATUS=");
        payload.push_str(&sanitized);
        imp::send(payload.as_bytes());
    }
    #[cfg(not(unix))]
    {
        let _ = msg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NOTIFY_SOCKET`/`WATCHDOG_USEC` are process-global, so env-mutating
    /// tests serialize on this lock to stay deterministic under the parallel
    /// test harness (mirrors `client/src/session.rs`).
    static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn env_test_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(unix)]
    #[test]
    fn abstract_target_maps_to_leading_nul_and_path_is_unchanged() {
        assert_eq!(imp::target_bytes("@lightspeed"), b"\0lightspeed");
        assert_eq!(
            imp::target_bytes("/run/systemd/notify"),
            b"/run/systemd/notify"
        );
        assert_eq!(imp::target_bytes(""), b"");
    }

    #[test]
    fn disabled_without_notify_socket_and_never_panics() {
        let _guard = env_test_guard();
        std::env::remove_var("NOTIFY_SOCKET");
        assert!(!is_enabled());
        ready();
        watchdog();
        stopping();
        status("ignored when disabled");
        assert!(!is_enabled());
    }

    #[cfg(unix)]
    #[test]
    fn sends_ready_and_watchdog_payloads_to_the_notify_socket() {
        use std::os::unix::net::UnixDatagram;
        use std::time::{SystemTime, UNIX_EPOCH};

        let _guard = env_test_guard();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lightspeed-notify-{}-{unique}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let receiver = UnixDatagram::bind(&path).expect("bind notify receiver");
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .expect("set read timeout");

        std::env::set_var("NOTIFY_SOCKET", &path);
        assert!(is_enabled());

        ready();
        let mut buf = [0u8; 64];
        let n = receiver.recv(&mut buf).expect("READY datagram");
        assert_eq!(&buf[..n], b"READY=1");

        watchdog();
        let n = receiver.recv(&mut buf).expect("WATCHDOG datagram");
        assert_eq!(&buf[..n], b"WATCHDOG=1");

        status("node=test region=eu");
        let n = receiver.recv(&mut buf).expect("STATUS datagram");
        assert_eq!(&buf[..n], b"STATUS=node=test region=eu");

        std::env::remove_var("NOTIFY_SOCKET");
        let _ = std::fs::remove_file(&path);
    }
}
