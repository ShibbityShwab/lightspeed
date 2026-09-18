//! Owned WinDivert handle with explicit, idempotent teardown.
//!
//! The `windivert` 0.6 wrapper keeps its `handle` field private and its
//! `shutdown`/`close` methods take `&mut self`. That makes teardown impossible
//! while another thread is blocked in `recv(&self)`: the only path left is
//! `Arc::try_unwrap`, which silently no-ops while the receive thread still
//! holds a clone, and the receive thread never releases its clone because
//! `recv` blocks forever. The leaked WFP state then makes the next launch fail
//! with `FWP_E_IN_USE` (0x8032000A).
//!
//! This module owns the raw `HANDLE` directly instead. Every operation takes
//! `&self` (the WinDivert FFI accepts the handle by value), so the handle can
//! live in an `Arc` while `shutdown(&self)` unblocks a concurrent blocking
//! `recv(&self)`. No `&mut` aliasing, no `try_unwrap`, no leak.
//!
//! Windows-only: declared behind
//! `#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]` in
//! `interceptor::mod`, and every dependency it uses is gated with it.

use std::ffi::{c_void, CString};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use windivert_sys::address::WINDIVERT_ADDRESS;
use windivert_sys::{
    WinDivertClose, WinDivertFlags, WinDivertLayer, WinDivertOpen, WinDivertRecv, WinDivertSend,
    WinDivertShutdown, WinDivertShutdownMode,
};
use windows::Win32::Foundation::HANDLE;

/// An owned WinDivert handle with deterministic, idempotent teardown.
///
/// # Concurrency
///
/// `recv`, `send` and `shutdown` may be called concurrently from any thread;
/// the WinDivert driver supports that on a single handle. `shutdown` and
/// `close` are not internally serialised against each other: the owner thread
/// calls `shutdown`, waits for every receive thread to acknowledge
/// ([`crate::interceptor::teardown::TeardownAck`]), and only then calls
/// `close` (or drops the last clone). A `close` that races a `shutdown` could
/// run `WinDivertShutdown` against a handle value Windows has already recycled.
pub struct OwnedHandle {
    raw: HANDLE,
    /// Set (once) when a close has been attempted, so `Drop` never closes
    /// twice. It records the attempt, not success: a failed close leaves it
    /// set so we do not retry against a possibly recycled handle value.
    closed: AtomicBool,
}

// SAFETY: `OwnedHandle` holds the WinDivert handle as an immutable `isize`
// value (`HANDLE`), not a raw pointer, and the only interior state is a
// thread-safe `AtomicBool`. The WinDivert FFI explicitly supports concurrent
// `recv`/`send`/`shutdown`/`close` calls on the same handle from different
// threads, so moving it between threads (`Send`) and sharing references across
// them (`Sync`) introduces no data race. [UB taxonomy category 9]
unsafe impl Send for OwnedHandle {}
// SAFETY: see the `Send` impl above. Every method takes `&self`; the only
// shared mutation is inside the driver, which serialises it internally.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    /// Open a WinDivert handle.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when `filter` contains an
    /// interior NUL byte, and the OS error from `GetLastError` when
    /// `WinDivertOpen` fails (for example `FWP_E_IN_USE`; see
    /// [`is_fwp_in_use`]).
    pub fn open(
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        let filter = CString::new(filter).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "WinDivert filter contains an interior NUL byte",
            )
        })?;

        // SAFETY: [Category 8 - FFI] `filter` is a live NUL-terminated C string
        // owned by the local binding, so the pointer is valid for the whole
        // call; the returned `HANDLE` is checked for validity below before it
        // is stored.
        let raw = unsafe { WinDivertOpen(filter.as_ptr(), layer, priority, flags) };
        if raw.is_invalid() {
            return Err(io::Error::last_os_error());
        }

        Ok(Self {
            raw,
            closed: AtomicBool::new(false),
        })
    }

    /// Receive one packet into `buf`.
    ///
    /// On success returns the number of bytes written and the packet address.
    /// When the handle has been shut down, the returned error carries the
    /// shutdown sentinel (see [`crate::interceptor::teardown::recv_should_break`]).
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] for an empty buffer or a buffer
    /// larger than `u32::MAX` (the FFI length is `u32`), and the OS error from
    /// `GetLastError` when `WinDivertRecv` fails.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, WINDIVERT_ADDRESS)> {
        if buf.is_empty() || buf.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "WinDivert recv buffer length {} is outside 1..={}",
                    buf.len(),
                    u32::MAX
                ),
            ));
        }

        let mut recv_len: u32 = 0;
        let mut addr = WINDIVERT_ADDRESS::default();

        // SAFETY: [Category 8 - FFI] `buf` is a valid exclusive slice, so its
        // pointer is writable for `buf.len()` bytes (just validated to fit in
        // `u32`); `recv_len` and `addr` are stack locals whose pointers are
        // valid and writable for the call. The driver writes at most
        // `buf.len()` bytes and a fully initialized `WINDIVERT_ADDRESS`.
        let ok = unsafe {
            WinDivertRecv(
                self.raw,
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len() as u32,
                &mut recv_len,
                &mut addr,
            )
        };

        if ok.as_bool() {
            Ok((recv_len as usize, addr))
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Send one raw packet with the given address.
    ///
    /// Returns the number of bytes handed to the driver.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] for an empty packet or a packet
    /// larger than `u32::MAX`, and the OS error from `GetLastError` when
    /// `WinDivertSend` fails.
    pub fn send(&self, data: &[u8], addr: &WINDIVERT_ADDRESS) -> io::Result<usize> {
        if data.is_empty() || data.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "WinDivert send packet length {} is outside 1..={}",
                    data.len(),
                    u32::MAX
                ),
            ));
        }

        let mut sent_len: u32 = 0;

        // SAFETY: [Category 8 - FFI] `data` is a valid shared slice whose
        // pointer is readable for `data.len()` bytes (just validated to fit in
        // `u32`); `addr` and `sent_len` are valid references for the call.
        let ok = unsafe {
            WinDivertSend(
                self.raw,
                data.as_ptr().cast::<c_void>(),
                data.len() as u32,
                &mut sent_len,
                addr,
            )
        };

        if ok.as_bool() {
            Ok(sent_len as usize)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Shut down receive and/or send on the handle, unblocking any thread
    /// currently parked in [`recv`](Self::recv) or `send`.
    ///
    /// This is deliberately `&self`: the owner thread can call it while a
    /// receive thread is blocked and still holds its `Arc<OwnedHandle>`.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::Other`] if the handle was already closed
    /// (the FFI is not called on a recycled handle value), and the OS error
    /// from `GetLastError` when `WinDivertShutdown` fails.
    pub fn shutdown(&self, mode: WinDivertShutdownMode) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "WinDivert handle is already closed; refusing to shut it down",
            ));
        }

        // SAFETY: [Category 8 - FFI] `self.raw` is a live handle for the
        // lifetime of `self` (checked above that no close was attempted yet,
        // and the owner contract keeps close after every shutdown), and the
        // mode is a plain `#[repr(u32)]` enum passed by value.
        let ok = unsafe { WinDivertShutdown(self.raw, mode) };

        if ok.as_bool() {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Close the handle, unregistering its WFP filter and callout.
    ///
    /// Takes `&self` on purpose: the owner thread can close without fighting
    /// the borrow checker against the `Arc` shared with the receive threads.
    /// Repeated calls are guarded by an internal [`AtomicBool`], so this is a
    /// no-op after the first attempt and [`Drop`] never double-closes.
    ///
    /// # Errors
    ///
    /// Returns the OS error from `GetLastError` when `WinDivertClose` fails.
    /// A failed close still marks the handle as closed: retrying against a
    /// possibly recycled handle value is worse than leaking a driver handle
    /// that Windows will release at process exit.
    pub fn close(&self) -> io::Result<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        // SAFETY: [Categories 8 - FFI, 12 - double free] The `swap` above
        // makes exactly one thread reach this call, so the handle is closed at
        // most once; at this point it is still live because `self` owns it and
        // `shutdown`/`close` are serialised by the owner thread.
        let ok = unsafe { WinDivertClose(self.raw) };

        if ok.as_bool() {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // Best effort: unblock any straggler parked in recv, then release the
        // filter. Both errors are deliberately ignored here because a `Drop`
        // impl has no channel to report through; callers that need the close
        // result call `close` explicitly.
        let _ = self.shutdown(WinDivertShutdownMode::Both);
        let _ = self.close();
    }
}

/// Returns `true` when `err` is the WFP "handle still in use" failure
/// (`0x8032000A` or `0xC022000A`) that a leaked WinDivert handle produces.
///
/// Callers use this to print an actionable message (reboot or close the stale
/// process) instead of a raw error code.
pub fn is_fwp_in_use(err: &io::Error) -> bool {
    let Some(code) = err.raw_os_error() else {
        return false;
    };
    code == 0x8032_000A_u32 as i32 || code == 0xC022_000A_u32 as i32
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_fwp_in_use_matches_windows_error_codes() {
        assert!(is_fwp_in_use(&io::Error::from_raw_os_error(
            0x8032_000A_u32 as i32
        )));
        assert!(is_fwp_in_use(&io::Error::from_raw_os_error(
            0xC022_000A_u32 as i32
        )));
    }

    #[test]
    fn is_fwp_in_use_rejects_other_errors() {
        assert!(!is_fwp_in_use(&io::Error::from_raw_os_error(5)));
        assert!(!is_fwp_in_use(&io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a WFP error"
        )));
    }
}
