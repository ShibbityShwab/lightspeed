//! The off-thread game-presence watcher.
//!
//! `LightSpeedApp::poll_game_presence` used to run the whole detection chain -
//! `lightspeed_client::games::auto_detect`, which spawns `tasklist /FO CSV /NH`
//! and a `powershell Get-CimInstance Win32_Process` query - on the egui thread
//! whenever its 5 s timer fired. Measured at 0.43-0.47 s on the development host
//! and 2.5-3.4 s on the EDR-heavy machine in issue #178, it blocked the window
//! for the whole scan (the reporter's 1-3 s freezes on window move and click).
//!
//! Detection now runs on one dedicated worker thread, in the same shape the
//! process picker already uses ([`crate::processes`]): the worker owns the 5 s
//! cadence, runs exactly one scan at a time and publishes each result on a
//! channel; the egui thread only polls that channel and applies the result
//! (`LightSpeedApp::poll_game_presence`). The worker is stopped and joined from
//! `LightSpeedApp::shutdown_engine`, so it cannot outlive the app.
//!
//! Everything that decides *whether* a scan may run and *when* it starts is pure
//! and unit-tested here; the worker loop is a thin shell around those pieces.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use super::*;

/// How often to scan the process table for a game appearing or exiting.
///
/// Slower than a relay probe on purpose: starting a game is a minutes-scale
/// event, and the scan walks every process on the machine.
const GAME_PRESENCE_INTERVAL: Duration = Duration::from_secs(5);

/// Whether the Game field's current mode allows a presence scan at all.
///
/// Auto is the only scanning mode. "Manual / Custom…" hands routing to the
/// server the user typed, and a picked process target is an explicit choice the
/// automatic scan must not override - neither mode may start the engine from a
/// detection result. The UI also drops any result that predates such a switch,
/// so leaving Auto can never be followed by an auto-start.
pub(super) fn presence_scan_allowed(
    game_auto: bool,
    game_manual: bool,
    has_process_target: bool,
) -> bool {
    game_auto && !game_manual && !has_process_target
}

/// The fixed-rate scan schedule the watcher follows.
///
/// Start-to-start, not finish-to-start: a scan that finishes quickly keeps the
/// 5 s rhythm exactly as before, and one that overruns its slot does not queue
/// a burst to catch up - the next scan starts at once, as a single scan, and the
/// schedule re-bases on it. So the cadence never drifts from scan duration, and
/// no two scans are ever in flight.
pub(super) struct PresenceSchedule {
    /// The instant the next scan may start.
    next_at: Instant,
}

impl PresenceSchedule {
    /// A schedule whose first scan is due immediately.
    pub(super) fn due_now() -> Self {
        Self {
            next_at: Instant::now(),
        }
    }

    /// A schedule pinned to an explicit deadline (tests only).
    #[cfg(test)]
    fn at(next_at: Instant) -> Self {
        Self { next_at }
    }

    /// How long to wait before the next scan may start; zero when it is due.
    pub(super) fn wait(&self, now: Instant) -> Duration {
        self.next_at.saturating_duration_since(now)
    }

    /// A scan that started at `started` has run: schedule the next one a full
    /// interval after that start, whether or not it overran.
    pub(super) fn started(&mut self, started: Instant, interval: Duration) {
        self.next_at = started + interval;
    }

    /// The deadline passed with no scan (the mode did not allow one): look again
    /// one interval later instead of spinning on a past deadline.
    pub(super) fn skipped(&mut self, now: Instant, interval: Duration) {
        self.next_at = now + interval;
    }

    /// Make the next scan due at once - used when a control message arrives, so
    /// turning Auto back on scans immediately rather than waiting out the rest
    /// of the previous interval.
    pub(super) fn wake(&mut self) {
        self.next_at = Instant::now();
    }
}

/// A message from the egui thread to the watcher.
enum PresenceCommand {
    /// Auto mode is on or off for this frame. Manual/Custom and a picked process
    /// target both turn it off.
    SetEnabled(bool),
    /// The app is going away: stop the loop and join.
    Stop,
}

/// The UI-side handle to the single game-presence worker thread.
///
/// The same one-shot-channel shape as [`crate::processes::spawn_process_list`],
/// except this worker is long-lived because it owns the cadence: the UI only
/// sends mode changes and drains results.
pub(super) struct GamePresence {
    /// Finished scans. The worker sends one per completed scan; the UI applies
    /// only the newest, because a scan describes "now" and a backlog would act
    /// on the past.
    updates: Receiver<Option<String>>,
    /// Mode and lifecycle messages. A send also wakes a sleeping worker.
    control: Sender<PresenceCommand>,
    /// The worker, joined by [`Self::shutdown`].
    handle: Option<JoinHandle<()>>,
    /// The last state sent, so a per-frame poll does not flood the channel.
    enabled: bool,
}

impl GamePresence {
    /// Start the watcher with the real detection function.
    pub(super) fn spawn() -> Self {
        Self::spawn_with(GAME_PRESENCE_INTERVAL, try_auto_detect_game)
    }

    /// A watcher with no worker: it never reports a game and never spawns a
    /// process. Used by the offline renderer (`--a11y-dump` and the
    /// accessibility tests), where a real scan would be a side effect.
    pub(super) fn idle() -> Self {
        let (updates_tx, updates) = mpsc::channel();
        // Dropped immediately: `latest()` then reports "no new result", which is
        // exactly a worker that has not finished a scan.
        drop(updates_tx);
        let (control, control_rx) = mpsc::channel();
        drop(control_rx);
        Self {
            updates,
            control,
            handle: None,
            enabled: false,
        }
    }

    /// Start a watcher around `detect`, on a fixed `interval`.
    ///
    /// Parameterised so the unit tests can drive the loop with a short interval
    /// and an instrumented detection closure instead of spawning `tasklist`.
    fn spawn_with(
        interval: Duration,
        detect: impl Fn() -> Option<String> + Send + 'static,
    ) -> Self {
        let (updates_tx, updates) = mpsc::channel();
        let (control, control_rx) = mpsc::channel();
        let handle = std::thread::spawn(move || run(updates_tx, control_rx, interval, detect));
        Self {
            updates,
            control,
            handle: Some(handle),
            enabled: false,
        }
    }

    /// Enable or disable scanning. Sends only on a change, so calling this every
    /// frame is free.
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        let _ = self.control.send(PresenceCommand::SetEnabled(enabled));
    }

    /// The newest finished scan, or `None` if nothing landed since the last
    /// call. Any older result is discarded: only the latest presence matters.
    pub(super) fn latest(&self) -> Option<Option<String>> {
        let mut newest = self.updates.try_recv().ok()?;
        while let Ok(next) = self.updates.try_recv() {
            newest = next;
        }
        Some(newest)
    }

    /// Drop every undrained result. Called when scanning is disabled, so a scan
    /// that started before the user left Auto can never auto-start the engine
    /// afterwards.
    pub(super) fn discard_pending(&self) {
        while self.updates.try_recv().is_ok() {}
    }

    /// Stop the worker and wait for it to finish. Idempotent, and also run by
    /// [`Drop`] as a safety net.
    pub(super) fn shutdown(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        let _ = self.control.send(PresenceCommand::Stop);
        // A scan already in progress cannot be cancelled mid-call, so the join
        // is bounded by one detection pass (sub-second here, a few seconds on
        // the slow machine the worker exists for) - and nothing is left running
        // once the app goes away, which is the point.
        if handle.join().is_err() {
            tracing::warn!("game-presence worker panicked");
        }
    }
}

impl Drop for GamePresence {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The watcher loop: sleep until the next scan is due or a message arrives, scan
/// only while enabled, and publish each result.
fn run(
    updates: Sender<Option<String>>,
    control: Receiver<PresenceCommand>,
    interval: Duration,
    detect: impl Fn() -> Option<String>,
) {
    let mut enabled = false;
    let mut schedule = PresenceSchedule::due_now();
    loop {
        let wait = schedule.wait(Instant::now());
        match control.recv_timeout(wait) {
            Ok(PresenceCommand::Stop) => break,
            Ok(PresenceCommand::SetEnabled(state)) => {
                enabled = state;
                schedule.wake();
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The UI is gone; there is nobody left to scan for.
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if !enabled {
            schedule.skipped(Instant::now(), interval);
            continue;
        }
        let started = Instant::now();
        let detected = detect();
        schedule.started(started, interval);
        if updates.send(detected).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Generous bound: a slow machine must not turn a worker round trip into a
    /// test failure.
    const RECV_TIMEOUT: Duration = Duration::from_secs(30);

    /// Wait up to [`RECV_TIMEOUT`] for `predicate`, sleeping between polls.
    fn wait_for(predicate: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + RECV_TIMEOUT;
        while Instant::now() < deadline {
            if predicate() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        predicate()
    }

    /// The manual-skip rule: only Auto may scan, and a picked process target
    /// disables even Auto.
    #[test]
    fn only_auto_mode_scans() {
        assert!(
            presence_scan_allowed(true, false, false),
            "Auto is the scanning mode"
        );
        assert!(
            !presence_scan_allowed(false, true, false),
            "Manual / Custom must never scan: it hands routing to the typed server"
        );
        assert!(
            !presence_scan_allowed(true, true, false),
            "a mode that is both Auto and Manual is not Auto"
        );
        assert!(
            !presence_scan_allowed(false, false, false),
            "a pinned built-in game or one of the user's profiles must not scan"
        );
        assert!(
            !presence_scan_allowed(true, false, true),
            "a picked process target is the user's explicit choice; never fight it"
        );
    }

    /// The cadence: fixed-rate, and a slow scan neither drifts the schedule nor
    /// queues a catch-up burst.
    #[test]
    fn the_schedule_keeps_a_fixed_rate_and_never_bursts_after_a_slow_scan() {
        let interval = Duration::from_secs(5);
        let base = Instant::now();

        let mut schedule = PresenceSchedule::at(base);
        assert_eq!(
            schedule.wait(base - Duration::from_secs(1)),
            Duration::from_secs(1),
            "before the deadline it waits exactly the remaining time"
        );
        assert_eq!(
            schedule.wait(base),
            Duration::ZERO,
            "at the deadline the scan is due and waits for nothing"
        );
        assert_eq!(
            schedule.wait(base + Duration::from_secs(3)),
            Duration::ZERO,
            "a past deadline is not waited for again"
        );

        schedule.started(base, interval);
        assert_eq!(
            schedule.wait(base + Duration::from_secs(1)),
            Duration::from_secs(4),
            "a quick scan keeps the 5 s start-to-start rhythm"
        );
        assert_eq!(
            schedule.wait(base + Duration::from_secs(5)),
            Duration::ZERO,
            "the next scan is due five seconds after the previous start"
        );

        // A scan started at base+5 and did not finish until base+12: its slot
        // is over, so the next scan starts at once as one scan ...
        schedule.started(base + Duration::from_secs(5), interval);
        assert_eq!(
            schedule.wait(base + Duration::from_secs(12)),
            Duration::ZERO,
            "an overrun scan starts the next scan immediately, not a burst"
        );
        // ... and the schedule re-bases on the late scan instead of trying to
        // catch up on missed slots.
        schedule.started(base + Duration::from_secs(12), interval);
        assert_eq!(
            schedule.wait(base + Duration::from_secs(12)),
            Duration::from_secs(5),
            "the next deadline is measured from the scan that actually ran"
        );

        // A wake makes the next scan due at once (a mode switch back to Auto).
        schedule.wake();
        assert_eq!(schedule.wait(Instant::now()), Duration::ZERO);
    }

    /// The off-thread contract, proven directly: the scan does not run on the
    /// thread that polls the app, so nothing that goes through the watcher can
    /// block the egui thread.
    #[test]
    fn the_scan_runs_off_the_calling_thread() {
        let caller = std::thread::current().id();
        let (tx, rx) = mpsc::channel();
        let mut presence = GamePresence::spawn_with(Duration::from_millis(10), move || {
            let _ = tx.send(std::thread::current().id());
            None
        });
        presence.set_enabled(true);
        let worker = rx
            .recv_timeout(RECV_TIMEOUT)
            .expect("the worker must report a finished scan");
        assert_ne!(
            worker, caller,
            "the scan ran on the caller's thread, which would block the egui thread"
        );
        presence.shutdown();
    }

    /// One scan in flight, always: scans slower than the interval must not
    /// overlap and must not queue duplicates.
    #[test]
    fn at_most_one_scan_runs_at_a_time() {
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let scans = Arc::new(AtomicUsize::new(0));

        let mut presence = {
            let in_flight = Arc::clone(&in_flight);
            let peak = Arc::clone(&peak);
            let scans = Arc::clone(&scans);
            GamePresence::spawn_with(Duration::from_millis(1), move || {
                let concurrent = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(concurrent, Ordering::SeqCst);
                scans.fetch_add(1, Ordering::SeqCst);
                // Much slower than the interval: every scan overruns its slot,
                // the case that could pile duplicates onto the queue.
                std::thread::sleep(Duration::from_millis(30));
                in_flight.fetch_sub(1, Ordering::SeqCst);
                None
            })
        };
        presence.set_enabled(true);
        assert!(
            wait_for(|| scans.load(Ordering::SeqCst) >= 3),
            "the watcher stopped scanning after {} scans",
            scans.load(Ordering::SeqCst)
        );
        presence.shutdown();

        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "two scans overlapped; a slow scan must not queue another"
        );
    }

    /// A watcher that was never enabled (Manual / Custom, or a pinned pick) does
    /// not touch the process table at all.
    #[test]
    fn a_disabled_watcher_does_not_scan() {
        let scans = Arc::new(AtomicUsize::new(0));
        let presence = {
            let scans = Arc::clone(&scans);
            GamePresence::spawn_with(Duration::from_millis(10), move || {
                scans.fetch_add(1, Ordering::SeqCst);
                None
            })
        };
        // Several intervals' worth of loop iterations with no `set_enabled`.
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            scans.load(Ordering::SeqCst),
            0,
            "a disabled watcher must not scan: Manual/Custom must not start the engine"
        );
        drop(presence);
    }

    /// Turning Auto off stops further scans, so leaving Auto can never be
    /// followed by a scan - and the in-flight result is dropped by the UI.
    #[test]
    fn disabling_stops_further_scans() {
        let scans = Arc::new(AtomicUsize::new(0));
        let mut presence = {
            let scans = Arc::clone(&scans);
            GamePresence::spawn_with(Duration::from_millis(200), move || {
                scans.fetch_add(1, Ordering::SeqCst);
                None
            })
        };
        presence.set_enabled(true);
        assert!(
            wait_for(|| presence.latest().is_some()),
            "the enabled watcher never produced a scan"
        );
        presence.set_enabled(false);
        presence.discard_pending();
        let settled = scans.load(Ordering::SeqCst);

        // More than two intervals: an enabled watcher would have scanned again.
        std::thread::sleep(Duration::from_millis(450));
        assert_eq!(
            scans.load(Ordering::SeqCst),
            settled,
            "a disabled watcher kept scanning after the mode changed"
        );
        presence.shutdown();
    }

    /// Clean shutdown: the worker is joined (nothing survives the app) and
    /// calling it twice is harmless.
    #[test]
    fn shutdown_joins_the_worker_and_is_idempotent() {
        let scans = Arc::new(AtomicUsize::new(0));
        let mut presence = {
            let scans = Arc::clone(&scans);
            GamePresence::spawn_with(Duration::from_millis(10), move || {
                scans.fetch_add(1, Ordering::SeqCst);
                None
            })
        };
        presence.set_enabled(true);
        assert!(
            wait_for(|| scans.load(Ordering::SeqCst) > 0),
            "the enabled watcher never scanned"
        );
        presence.shutdown();
        let after_join = scans.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            scans.load(Ordering::SeqCst),
            after_join,
            "the joined worker was still scanning after shutdown"
        );
        presence.shutdown();
    }
}
