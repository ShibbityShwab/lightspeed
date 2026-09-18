//! Shared teardown helpers for the blocking WinDivert receive loops.
//!
//! This module is platform-neutral on purpose: it contains no Windows types, so
//! it compiles and runs its unit tests on every target. Both WinDivert backends
//! (`interceptor::windows` and `capture::windivert_redirect`) use these helpers
//! to decide when a blocked `recv` loop must exit and to wait for every owner
//! thread to acknowledge that it has stopped touching the driver handle.
//!
//! # Why a receive loop needs a break predicate
//!
//! `WinDivertRecv` blocks in the kernel until a matching packet arrives or the
//! handle is shut down. After `WinDivertShutdown`, the blocked call returns
//! `FALSE` and `GetLastError()` reports [`ERROR_NO_DATA`] (232), WinDivert's
//! "the handle was shut down" sentinel. Aborted I/O (995) is the other terminal
//! code, seen when the underlying device handle is closed underneath a pending
//! receive. Every *other* error while the loop is still meant to run (for
//! example `ERROR_TIMEOUT`, 1460) must keep the loop alive so the caller can
//! retry with its own backoff.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// `GetLastError()` value returned by `WinDivertRecv` after `WinDivertShutdown`.
pub const ERROR_NO_DATA: i32 = 232;

/// `GetLastError()` value for aborted or cancelled I/O (995).
const ERROR_OPERATION_ABORTED: i32 = 995;

/// Upper bound on a single `recv_timeout` slice inside [`TeardownAck::wait`].
///
/// Clamping keeps `wait` free of the `Instant + Duration` overflow panic for
/// absurd caller timeouts while still honouring the caller's total `timeout`:
/// a chunk timeout only re-checks the deadline, it never ends the wait early.
const MAX_WAIT_CHUNK: Duration = Duration::from_secs(24 * 60 * 60);

/// Returns `true` when a blocked `recv` loop must stop retrying and exit.
///
/// The loop breaks when either:
///
/// - `running` is `false` (the caller asked it to stop), or
/// - the OS error is [`ERROR_NO_DATA`] (WinDivert's shutdown sentinel), or
/// - the OS error is `ERROR_OPERATION_ABORTED` (995, aborted I/O).
///
/// Any other error while `running` is `true` (a receive timeout, a short
/// buffer, ...) is transient from the loop's point of view: it must not break,
/// so the caller can back off and retry.
pub const fn recv_should_break(running: bool, raw_os_error: i32) -> bool {
    if !running {
        return true;
    }
    raw_os_error == ERROR_NO_DATA || raw_os_error == ERROR_OPERATION_ABORTED
}

/// Bounded acknowledgement that every owner thread released its handle.
///
/// Each owner thread (the blocking `recv` loop, the inject loop, ...) holds a
/// clone of the [`Sender`] returned by [`TeardownAck::new`] and sends `()` once
/// it can no longer touch the WinDivert handle. The owner of the handle calls
/// [`TeardownAck::wait`] *after* `WinDivertShutdown` and closes the handle only
/// if every thread acknowledged. That ordering is what makes teardown
/// deterministic instead of an `Arc::try_unwrap` guess.
pub struct TeardownAck {
    rx: Receiver<()>,
    expected: usize,
}

impl TeardownAck {
    /// Create an acknowledgement channel expecting `expected` sends.
    ///
    /// Clone the returned sender into every owner thread. A clone that is
    /// dropped without sending counts as a missing acknowledgement (see
    /// [`TeardownAck::wait`]).
    pub fn new(expected: usize) -> (Sender<()>, Self) {
        let (tx, rx) = mpsc::channel();
        (tx, Self { rx, expected })
    }

    /// Number of acknowledgements [`TeardownAck::wait`] requires.
    pub const fn expected(&self) -> usize {
        self.expected
    }

    /// Wait until all `expected` acknowledgements arrive or `timeout` elapses.
    ///
    /// Returns `true` only when every acknowledgement arrived in time. Returns
    /// `false` on timeout, and also when the channel disconnects before the
    /// required count is reached: a sender dropped without sending is a failure
    /// to acknowledge, not an implicit acknowledgement. The fail-closed choice
    /// matters because "the thread is gone" is a weaker promise than "the
    /// thread stopped touching the handle", and callers must not close the
    /// handle on the weaker promise.
    ///
    /// `wait` drains the channel and is intended to be called once; a second
    /// call starts its count from zero.
    pub fn wait(&self, timeout: Duration) -> bool {
        let start = Instant::now();
        let mut acked = 0usize;

        while acked < self.expected {
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                return false;
            };
            match self.rx.recv_timeout(remaining.min(MAX_WAIT_CHUNK)) {
                Ok(()) => acked += 1,
                // A chunk timeout only means "re-check the total deadline".
                Err(RecvTimeoutError::Timeout) => {}
                // Every sender dropped before the required count was reached.
                Err(RecvTimeoutError::Disconnected) => return false,
            }
        }
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Short enough to keep the suite fast, long enough that a queued ack is
    /// always observed on a loaded CI machine.
    const SHORT: Duration = Duration::from_millis(30);

    #[test]
    fn recv_should_break_when_stop_requested() {
        assert!(recv_should_break(false, 0));
        // Even a transient error breaks once the loop was told to stop.
        assert!(recv_should_break(false, 1460));
    }

    #[test]
    fn recv_should_break_on_shutdown_sentinel() {
        assert!(recv_should_break(true, ERROR_NO_DATA));
    }

    #[test]
    fn recv_should_break_on_operation_aborted() {
        assert!(recv_should_break(true, 995));
    }

    #[test]
    fn recv_should_not_break_on_transient_error_while_running() {
        // ERROR_TIMEOUT (1460) and friends: retry, do not exit.
        assert!(!recv_should_break(true, 1460));
        assert!(!recv_should_break(true, 0));
        assert!(!recv_should_break(true, 1234));
    }

    #[test]
    fn teardown_ack_true_when_all_senders_ack() {
        let (tx, ack) = TeardownAck::new(2);
        tx.send(()).unwrap();
        tx.clone().send(()).unwrap();

        assert!(ack.wait(SHORT));
        assert_eq!(ack.expected(), 2);
    }

    #[test]
    fn teardown_ack_waits_for_threads_to_ack() {
        let (tx, ack) = TeardownAck::new(2);
        let handles: Vec<std::thread::JoinHandle<()>> = (0..2)
            .map(|_| {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(());
                })
            })
            .collect();
        drop(tx);

        assert!(ack.wait(Duration::from_millis(500)));
        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn teardown_ack_false_on_timeout() {
        let (tx, ack) = TeardownAck::new(1);
        assert!(!ack.wait(SHORT));
        drop(tx);
    }

    #[test]
    fn teardown_ack_false_when_sender_dropped_without_acking() {
        let (tx, ack) = TeardownAck::new(1);
        drop(tx);
        assert!(!ack.wait(SHORT));
    }

    #[test]
    fn teardown_ack_false_on_partial_ack() {
        let (tx, ack) = TeardownAck::new(2);
        tx.send(()).unwrap();
        assert!(!ack.wait(SHORT));
    }

    #[test]
    fn teardown_ack_zero_expected_succeeds_immediately() {
        let (tx, ack) = TeardownAck::new(0);
        assert_eq!(ack.expected(), 0);
        assert!(ack.wait(SHORT));
        drop(tx);
    }
}
