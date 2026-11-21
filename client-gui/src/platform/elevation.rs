//! The elevation handshake state machine (spec §2.2 row 7 / C8; W2-4 (e)).
//!
//! The defect this models is C8: the old Windows relaunch called
//! `Start-Process -Verb RunAs` and then **exited immediately**, so a user who
//! clicked "No" at the UAC prompt was left with no window at all. The repair is
//! a handshake: the current process asks for elevation, keeps running, and only
//! exits once the elevated instance is confirmed to have started; a declined
//! prompt returns the app to its ready state instead of killing it.
//!
//! The machine is pure and unit-tested on every host:
//!
//! ```text
//! Ready --request--> Restarting --confirm--> Confirmed (exit allowed)
//!                        |  \--decline--> Declined (stay alive; notice copy)
//!                        \----fail-------> Failed   (stay alive; log only)
//! ```
//!
//! Rules that the tests pin:
//!
//! * only `Ready` may start a request (a second click is ignored while one is
//!   in flight);
//! * only `Restarting` may resolve, so no callback can confirm a prompt that
//!   was never started;
//! * `may_exit` is true **only** on `Confirmed` — the current process never
//!   exits before the elevated instance confirms;
//! * `Declined` and `Failed` return to `Ready` after the app handles the
//!   notice, and the window stays alive throughout.
//!
//! The interactive UAC-decline path is **U** on this host: this agent shell is
//! elevated and the prompt lives on the secure desktop. The fixture is an
//! interactive verifier on a non-elevated standard-user session who clicks No
//! and observes the window stay on the Needs-elevation overlay. Everything
//! except that click is covered by the tests below.

/// The Windows `ERROR_CANCELLED` code the relaunch helper uses to report a
/// declined UAC prompt. The helper itself exits with it from its `catch` block;
/// the parent maps it to [`ElevationPhase::Declined`].
pub const USER_CANCELLED_EXIT: i32 = 1223;

/// Where the handshake is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElevationPhase {
    /// Nothing in flight; the action may ask again.
    Ready,
    /// The prompt is open or the elevated instance is starting. Loading label.
    Restarting,
    /// The elevated instance confirmed startup: the current process may exit.
    Confirmed,
    /// The user declined the prompt: stay alive and show the notice.
    Declined,
    /// The relaunch could not start: stay alive, log it.
    Failed,
}

impl ElevationPhase {
    /// Whether the current (unelevated) process may exit now.
    ///
    /// Only a confirmed elevated instance justifies the exit: anything else
    /// would recreate C8 (a closed window with nothing taking over).
    pub fn may_exit(self) -> bool {
        self == Self::Confirmed
    }

    /// Whether a request is in flight (the action slot shows the loading label).
    pub fn is_pending(self) -> bool {
        self == Self::Restarting
    }

    /// The catalog key for the notice this phase implies, when it has one.
    ///
    /// `Declined` is the copy the spec names (`notice.elevation_declined`); the
    /// in-flight loading label is `notice.restarting`. Rendering them is shell
    /// work; the machine names the keys so the copy cannot drift.
    pub fn notice_key(self) -> Option<&'static str> {
        match self {
            Self::Declined => Some("notice.elevation_declined"),
            Self::Restarting => Some("notice.restarting"),
            _ => None,
        }
    }
}

/// The handshake state machine. One per app process.
#[derive(Clone, Debug)]
pub struct ElevationHandshake {
    phase: ElevationPhase,
}

impl ElevationHandshake {
    /// A fresh machine in [`ElevationPhase::Ready`].
    pub fn new() -> Self {
        Self {
            phase: ElevationPhase::Ready,
        }
    }

    /// The current phase.
    pub fn phase(&self) -> ElevationPhase {
        self.phase
    }

    /// Ask for elevation. Returns `false` when a request is already in flight
    /// or the machine is not ready, so a double click cannot launch two prompts.
    pub fn request(&mut self) -> bool {
        self.transition(ElevationPhase::Restarting)
    }

    /// The elevated instance confirmed startup.
    pub fn confirm(&mut self) -> bool {
        self.transition(ElevationPhase::Confirmed)
    }

    /// The user declined the prompt.
    pub fn decline(&mut self) -> bool {
        self.transition(ElevationPhase::Declined)
    }

    /// The relaunch could not start.
    pub fn fail(&mut self) -> bool {
        self.transition(ElevationPhase::Failed)
    }

    /// Return to `Ready` after the app handled the outcome (or reset a stale
    /// `Ready`). A no-op while `Restarting`: the prompt is still open.
    pub fn reset(&mut self) {
        if self.phase != ElevationPhase::Restarting {
            self.phase = ElevationPhase::Ready;
        }
    }

    fn transition(&mut self, next: ElevationPhase) -> bool {
        let allowed = matches!(
            (self.phase, next),
            (ElevationPhase::Ready, ElevationPhase::Restarting)
                | (ElevationPhase::Restarting, ElevationPhase::Confirmed)
                | (ElevationPhase::Restarting, ElevationPhase::Declined)
                | (ElevationPhase::Restarting, ElevationPhase::Failed)
        );
        if allowed {
            self.phase = next;
        }
        allowed
    }
}

impl Default for ElevationHandshake {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify the relaunch helper's exit status into a resolved phase.
///
/// `None` means the helper is still running (the prompt may still be open).
/// `Some(0)` means the elevated instance started, `Some(USER_CANCELLED_EXIT)`
/// that the user clicked No, and anything else that the launch failed.
pub fn classify_relaunch_exit(code: Option<i32>) -> ElevationPhase {
    match code {
        None => ElevationPhase::Restarting,
        Some(0) => ElevationPhase::Confirmed,
        Some(USER_CANCELLED_EXIT) => ElevationPhase::Declined,
        Some(_) => ElevationPhase::Failed,
    }
}

/// A running elevation attempt the frame loop polls.
///
/// Windows spawns the helper process and stores it here; the other platforms
/// hand back an already-resolved attempt (their relaunch path exits the process
/// itself, so nothing is polled after it). Keeping the child handle here is
/// what makes "the current process stays alive until the elevated instance
/// confirms" implementable: the process only exits when [`Self::poll`] returns
/// `Confirmed`.
pub struct ElevationAttempt {
    handshake: ElevationHandshake,
    /// The helper process whose exit code is the confirmation. Only Windows
    /// has a handshake helper today; the field exists on every target so the
    /// app is platform-generic.
    child: Option<std::process::Child>,
}

impl ElevationAttempt {
    /// An attempt with nothing to poll: the phase is already resolved.
    pub fn resolved(phase: ElevationPhase) -> Self {
        let mut handshake = ElevationHandshake::new();
        if phase == ElevationPhase::Restarting {
            handshake.request();
        } else if phase != ElevationPhase::Ready {
            // Start from Ready, then land on the requested terminal phase.
            handshake.request();
            match phase {
                ElevationPhase::Confirmed => {
                    handshake.confirm();
                }
                ElevationPhase::Declined => {
                    handshake.decline();
                }
                ElevationPhase::Failed => {
                    handshake.fail();
                }
                _ => {}
            }
        }
        Self {
            handshake,
            child: None,
        }
    }

    /// The current phase.
    pub fn phase(&self) -> ElevationPhase {
        self.handshake.phase()
    }

    /// Poll the helper without blocking. Returns the phase this frame observes.
    ///
    /// `Restarting` while the prompt is open; a resolved phase once the helper
    /// exits. The caller must exit only on [`ElevationPhase::may_exit`].
    pub fn poll(&mut self) -> ElevationPhase {
        if self.handshake.phase() != ElevationPhase::Restarting {
            return self.handshake.phase();
        }
        let Some(child) = self.child.as_mut() else {
            return self.handshake.phase();
        };
        let code = match child.try_wait() {
            Ok(Some(status)) => status.code(),
            Ok(None) => None,
            Err(_) => Some(-1),
        };
        let phase = classify_relaunch_exit(code);
        match phase {
            ElevationPhase::Confirmed => {
                self.handshake.confirm();
            }
            ElevationPhase::Declined => {
                self.handshake.decline();
            }
            ElevationPhase::Failed => {
                self.handshake.fail();
            }
            ElevationPhase::Restarting | ElevationPhase::Ready => {}
        }
        self.handshake.phase()
    }

    /// Attach a spawned helper process and return the pending attempt. Windows
    /// only: the other backends resolve synchronously inside their relaunch.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn watch(mut child: std::process::Child) -> Self {
        let mut handshake = ElevationHandshake::new();
        handshake.request();
        // Already exited between spawn and watch? Let the first poll see it.
        let phase = match child.try_wait() {
            Ok(Some(status)) => classify_relaunch_exit(status.code()),
            _ => ElevationPhase::Restarting,
        };
        match phase {
            ElevationPhase::Confirmed => {
                handshake.confirm();
            }
            ElevationPhase::Declined => {
                handshake.decline();
            }
            ElevationPhase::Failed => {
                handshake.fail();
            }
            _ => {}
        }
        Self {
            handshake,
            child: Some(child),
        }
    }

    /// Return to a fresh `Ready` state after the app handled the outcome, so a
    /// retry can ask again. A no-op while the prompt is still open.
    pub fn reset(&mut self) {
        self.handshake.reset();
        if !self.handshake.phase().is_pending() {
            self.child = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_handshake_is_ready_and_may_not_exit() {
        let handshake = ElevationHandshake::new();
        assert_eq!(handshake.phase(), ElevationPhase::Ready);
        assert!(!handshake.phase().may_exit());
        assert!(!handshake.phase().is_pending());
        assert_eq!(handshake.phase().notice_key(), None);
    }

    #[test]
    fn a_request_moves_to_restarting_with_the_loading_label() {
        let mut handshake = ElevationHandshake::new();
        assert!(handshake.request());
        assert_eq!(handshake.phase(), ElevationPhase::Restarting);
        assert!(handshake.phase().is_pending());
        assert_eq!(handshake.phase().notice_key(), Some("notice.restarting"));
        // The process must NOT exit while the prompt is open (C8).
        assert!(!handshake.phase().may_exit());
    }

    #[test]
    fn a_second_request_is_ignored_while_one_is_in_flight() {
        let mut handshake = ElevationHandshake::new();
        assert!(handshake.request());
        assert!(
            !handshake.request(),
            "double click must not launch a second prompt"
        );
        assert_eq!(handshake.phase(), ElevationPhase::Restarting);
    }

    #[test]
    fn confirmation_is_the_only_phase_that_may_exit() {
        let mut handshake = ElevationHandshake::new();
        handshake.request();
        assert!(handshake.confirm());
        assert_eq!(handshake.phase(), ElevationPhase::Confirmed);
        assert!(handshake.phase().may_exit());
        assert_eq!(handshake.phase().notice_key(), None);
    }

    #[test]
    fn a_decline_returns_to_ready_with_the_decline_notice() {
        let mut handshake = ElevationHandshake::new();
        handshake.request();
        assert!(handshake.decline());
        assert_eq!(handshake.phase(), ElevationPhase::Declined);
        assert_eq!(
            handshake.phase().notice_key(),
            Some("notice.elevation_declined")
        );
        // The whole C8 rule: a declined prompt never exits the process.
        assert!(!handshake.phase().may_exit());
        handshake.reset();
        assert_eq!(handshake.phase(), ElevationPhase::Ready);
        // After the reset the user can try again.
        assert!(handshake.request());
    }

    #[test]
    fn a_failed_launch_stays_alive_and_logs_only() {
        let mut handshake = ElevationHandshake::new();
        handshake.request();
        assert!(handshake.fail());
        assert_eq!(handshake.phase(), ElevationPhase::Failed);
        assert!(!handshake.phase().may_exit());
        assert_eq!(handshake.phase().notice_key(), None);
        handshake.reset();
        assert_eq!(handshake.phase(), ElevationPhase::Ready);
    }

    #[test]
    fn only_a_pending_request_can_resolve() {
        let mut handshake = ElevationHandshake::new();
        // Nothing in flight: no callback may confirm or decline.
        assert!(!handshake.confirm());
        assert!(!handshake.decline());
        assert!(!handshake.fail());
        assert_eq!(handshake.phase(), ElevationPhase::Ready);
        handshake.request();
        assert!(handshake.confirm());
        // A resolved prompt cannot be resolved again.
        assert!(!handshake.decline());
        assert!(!handshake.fail());
        assert_eq!(handshake.phase(), ElevationPhase::Confirmed);
    }

    #[test]
    fn reset_does_not_cancel_an_open_prompt() {
        let mut handshake = ElevationHandshake::new();
        handshake.request();
        handshake.reset();
        assert_eq!(
            handshake.phase(),
            ElevationPhase::Restarting,
            "resetting must not close the prompt or forget it"
        );
        assert!(handshake.confirm());
    }

    #[test]
    fn the_relaunch_exit_codes_map_to_the_handshake() {
        assert_eq!(
            classify_relaunch_exit(None),
            ElevationPhase::Restarting,
            "a running helper is still a pending prompt"
        );
        assert_eq!(classify_relaunch_exit(Some(0)), ElevationPhase::Confirmed);
        assert_eq!(
            classify_relaunch_exit(Some(USER_CANCELLED_EXIT)),
            ElevationPhase::Declined
        );
        assert_eq!(classify_relaunch_exit(Some(1)), ElevationPhase::Failed);
        assert_eq!(classify_relaunch_exit(Some(-1)), ElevationPhase::Failed);
    }

    #[test]
    fn a_resolved_attempt_reports_its_phase_without_a_child() {
        for (phase, may_exit) in [
            (ElevationPhase::Ready, false),
            (ElevationPhase::Restarting, false),
            (ElevationPhase::Confirmed, true),
            (ElevationPhase::Declined, false),
            (ElevationPhase::Failed, false),
        ] {
            let mut attempt = ElevationAttempt::resolved(phase);
            assert_eq!(attempt.phase(), phase);
            assert_eq!(attempt.poll(), phase);
            assert_eq!(attempt.phase().may_exit(), may_exit, "{phase:?}");
        }
    }
}
