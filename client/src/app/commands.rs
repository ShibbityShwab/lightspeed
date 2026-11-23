//! Session commands and the app core — S3b of the GUI/core migration.
//!
//! WF-081 W2-3a (`docs/design/gui-core-boundary.md` §5.2, spec §5 W2-3a). Every
//! mutation of the engine that used to live in `client-gui/src/app.rs` is a
//! named command here. The GUI keeps the composition root, the view state and
//! the thread/channel plumbing; it calls these methods and renders the result.
//!
//! Properties the boundary fixes here:
//! - **one command state**: [`SessionCommandState`] is owned by the core, so the
//!   action loading state (`STARTING…`) and the Error row's source come from the
//!   same place the command ran;
//! - **errors are typed**: every fallible command returns [`AppError`] (a
//!   taxonomy kind + the verbatim detail), never a bare `String`, and records it
//!   as `last_error`;
//! - **in-flight is real**: a command is marked in flight for the whole call,
//!   even today's synchronous ones, so the S3b contract survives W2-2's shell
//!   and any future async adoption unchanged.
//!
//! `AppCore::new` will also install the server-provider classifier once the
//! location ladder moves (S5a); today the install stays in the GUI composition
//! root, as documented by guard G7.

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::interceptor::ProcessTarget;
use crate::LightSpeedEngine;

use super::dto::{AppError, AppErrorKind, SessionCommand, SessionCommandState};
use super::lifecycle::{
    self, ActionSource, Actor, CaptureState, ErrorSource, GuiAction, GuiActionKind, Intent,
    InterceptReason, UiError, UiErrorOp,
};

/// The capture-plane state a `gui.action` record names (P0-4 / T15).
///
/// Read from the engine, not from presentation: the route word can be ERROR for
/// a non-fatal registration warning while the capture run is healthy, and that
/// must never read as a capture transition.
fn capture_state_of(engine: &LightSpeedEngine) -> CaptureState {
    let fault = engine.snapshot().capture_fault.is_some();
    CaptureState::of(engine.is_intercepting(), fault)
}

/// The capture-plane intent a start expresses (P0-1: records only).
///
/// The source decides the reason/actor so the trace can tell a user Optimize
/// from a presence auto-start without reading prose. Nothing branches on it.
fn start_intent(source: ActionSource, command_id: &str) -> Intent {
    let (reason, actor) = match source {
        ActionSource::Auto => (InterceptReason::PresenceStart, Actor::Auto),
        ActionSource::System => (InterceptReason::AutoRecover, Actor::System),
        _ => (InterceptReason::UserStart, Actor::User),
    };
    Intent {
        reason,
        actor,
        command_id: command_id.to_string(),
    }
}

/// The capture-plane intent a stop expresses (P0-1: records only).
///
/// The only `System` stop in the GUI is the elevation handoff's pre-stop, so it
/// is labelled as that handoff instead of masquerading as a user Stop.
fn stop_intent(source: ActionSource) -> Intent {
    let (reason, actor) = match source {
        ActionSource::Auto => (InterceptReason::PresenceStop, Actor::Auto),
        ActionSource::System => (InterceptReason::ElevationHandoff, Actor::System),
        _ => (InterceptReason::UserStop, Actor::User),
    };
    Intent {
        reason,
        actor,
        command_id: format!("{:?}", SessionCommand::StopBoost),
    }
}

/// The capture-plane intent an explicit restart expresses (P0-1: records only).
///
/// A restart always names a recovery reason (`error_retry` for a user action,
/// `auto_recover` for a presence/system one), never a plain start.
fn restart_intent(source: ActionSource, command_id: &str) -> Intent {
    let (reason, actor) = match source {
        ActionSource::Auto | ActionSource::System => (InterceptReason::AutoRecover, Actor::System),
        _ => (InterceptReason::ErrorRetry, Actor::User),
    };
    Intent {
        reason,
        actor,
        command_id: command_id.to_string(),
    }
}

/// The lifecycle intent a command expresses (P0-1: records only).
///
/// Set immediately before the command runs so the engine's `interceptor_state`
/// and the interceptor's `windivert_open` records carry the actor and reason the
/// field trace could not answer on 2026-11-23. Nothing branches on this value.
fn intent_for_command(command: SessionCommand) -> Intent {
    let (reason, actor) = match command {
        SessionCommand::StartBoost => (InterceptReason::UserStart, Actor::User),
        SessionCommand::StopBoost => (InterceptReason::UserStop, Actor::User),
        SessionCommand::SelectRelay => (InterceptReason::RelaySwitch, Actor::Auto),
        SessionCommand::Disconnect => (InterceptReason::UserStop, Actor::User),
        SessionCommand::SetPathMtu => (InterceptReason::Unknown, Actor::Internal),
        SessionCommand::Hold => (InterceptReason::Unknown, Actor::Internal),
    };
    Intent {
        reason,
        actor,
        command_id: format!("{command:?}"),
    }
}

/// The error-record source for a command failure (P0-3 owns the epoch rules;
/// v1 records the source so a card can be attributed).
///
/// The mapping itself lives in [`crate::app::dto::source_for_kind`] so the
/// command-side record and the render-side record cannot disagree about which
/// subsystem one visible card belongs to.
pub(crate) fn error_source_for(kind: AppErrorKind) -> ErrorSource {
    crate::app::dto::source_for_kind(kind)
}

/// The `ui_error op=show` record for a failed command (pure; tested).
pub(crate) fn ui_error_for_failure(command: SessionCommand, error: &AppError) -> UiError {
    UiError {
        op: UiErrorOp::Show,
        source: Some(error_source_for(error.kind)),
        kind: format!("{:?}", error.kind),
        detail_key: None,
        source_command: Some(format!("{command:?}")),
        visible_primary_state: None,
        reason_code: error.detail.clone(),
        cleared_by: None,
        age_ms: None,
    }
}

/// The `ui_error op=clear` record for a cleared/replaced card (pure; tested).
pub(crate) fn ui_error_for_clear(cleared_by: &str) -> UiError {
    UiError {
        op: UiErrorOp::Clear,
        source: None,
        kind: String::new(),
        detail_key: None,
        source_command: None,
        visible_primary_state: None,
        reason_code: None,
        cleared_by: Some(cleared_by.to_string()),
        age_ms: None,
    }
}

/// The shared application core every front end runs commands through.
///
/// It holds the engine handle the GUI composition root already created (the
/// engine lifecycle stays where it is until the S5/S6 ownership moves) and the
/// one command state. Reads stay available to a front end that needs the raw
/// snapshot; commands go through this type.
pub struct AppCore {
    engine: Arc<Mutex<LightSpeedEngine>>,
    session: SessionCommandState,
}

impl AppCore {
    /// Wrap the engine the composition root created.
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>) -> Self {
        Self {
            engine,
            session: SessionCommandState::default(),
        }
    }

    /// The command state (in-flight command and last error) for the DTO.
    pub fn session(&self) -> &SessionCommandState {
        &self.session
    }

    /// Clear the recorded command error (a successful retry).
    pub fn clear_error(&mut self) {
        let had_error = self.session.last_error.is_some();
        self.session.clear_error();
        if had_error {
            lifecycle::emit("ui_error", &ui_error_for_clear("clear_error"));
        }
    }

    /// Clear a stale registration error once the engine reports the control
    /// plane registered. The retry may have succeeded in the background while
    /// the error card kept the previous command failure on screen.
    pub fn clear_stale_registration_error(&mut self) {
        let cleared = self
            .session
            .last_error
            .as_ref()
            .and_then(|error| error.detail.as_deref())
            .is_some_and(|detail| detail.contains("control-plane registration failed"));
        self.session
            .clear_error_matching("control-plane registration failed");
        if cleared {
            lifecycle::emit("ui_error", &ui_error_for_clear("registration_succeeded"));
        }
    }

    /// The engine's current status snapshot (read-only passthrough).
    pub fn snapshot(&self) -> crate::EngineStatus {
        lock_or_recover(&self.engine).snapshot()
    }

    /// The capture-plane state the engine is in right now (T15 `gui.action`).
    pub fn capture_state(&self) -> CaptureState {
        capture_state_of(&lock_or_recover(&self.engine))
    }

    /// Record the one `gui.action` for a capture-crossing invocation.
    ///
    /// Called exactly once per action, after the command attempt. `from` is read
    /// before the command so a refused start records `from == to`, and the trace
    /// then shows both the refusal and that nothing was torn down.
    fn record_action(
        &self,
        action: GuiActionKind,
        source: ActionSource,
        from: CaptureState,
        to: CaptureState,
        command_id: &str,
    ) {
        lifecycle::record_gui_action(&GuiAction {
            action,
            source,
            from,
            to,
            command_id,
        });
    }

    /// [`Self::record_action`] with `to` read from the engine now.
    ///
    /// The engine lock must not be held by the caller: the command has already
    /// returned, so this reads the state the action actually left.
    fn record_action_now(
        &self,
        action: GuiActionKind,
        source: ActionSource,
        from: CaptureState,
        command_id: &str,
    ) {
        let to = self.capture_state();
        self.record_action(action, source, from, to, command_id);
    }

    /// Run one command, folding its result into the shared command state.
    ///
    /// The in-flight marker is set before the engine is touched and cleared on
    /// both outcomes; a failure becomes a typed [`AppError`] that is also the
    /// recorded `last_error` (the Error row source). This is the one mapping
    /// every fallible command below uses, so the state machine cannot drift
    /// between commands.
    fn run<T>(
        &mut self,
        command: SessionCommand,
        call: impl FnOnce(&mut LightSpeedEngine) -> Result<T, String>,
    ) -> Result<T, AppError> {
        self.run_with_intent(command, intent_for_command(command), call)
    }

    /// Run a command with an explicit lifecycle intent.
    ///
    /// Used by the shutdown path, which must not overwrite the reason the front
    /// end stamped (`quit` / `update` / `elevation_handoff`) with the generic
    /// `user_stop` that the `StopBoost` command would derive.
    fn run_with_intent<T>(
        &mut self,
        command: SessionCommand,
        intent: Intent,
        call: impl FnOnce(&mut LightSpeedEngine) -> Result<T, String>,
    ) -> Result<T, AppError> {
        // Behaviour-neutral intent stamp: the engine and the interceptor read it
        // when they record the transition this command causes.
        lifecycle::set_intent(intent);
        self.session.begin(command);
        let result = {
            let mut engine = lock_or_recover(&self.engine);
            call(&mut engine)
        };
        record_command_result(&mut self.session, command, result)
    }

    /// Connect the control plane to `relay` and remember its registry node id.
    ///
    /// Returns whether an interception was already running, so the caller can
    /// move the data plane to the same relay (the two planes are set at
    /// different moments; see the GUI's restart path).
    pub fn select_relay(
        &mut self,
        relay: SocketAddrV4,
        node_id: Option<String>,
    ) -> Result<bool, AppError> {
        self.run(SessionCommand::SelectRelay, |engine| {
            let was_intercepting = engine.is_intercepting();
            engine.connect(relay);
            engine.set_node_id(node_id);
            Ok(was_intercepting)
        })
    }

    /// Drop the control-plane link. The data plane is stopped separately by a
    /// caller that owns one.
    pub fn disconnect(&mut self) -> Result<(), AppError> {
        self.run(SessionCommand::Disconnect, |engine| {
            engine.disconnect();
            Ok(())
        })
    }

    /// Apply the effective path-MTU cap (clamped by the engine).
    pub fn set_path_mtu_cap(&mut self, cap: usize) -> Result<(), AppError> {
        self.run(SessionCommand::SetPathMtu, |engine| {
            engine.set_path_mtu_cap(cap);
            Ok(())
        })
    }

    /// Start the interceptor boost for `game` through `relay`.
    ///
    /// A start failure is the Tunnel taxonomy with the engine's verbatim detail:
    /// the Error row leads with the sentence and keeps this detail behind
    /// `Details` (spec §2.8). The game key is attributed before the start so the
    /// telemetry samples describe this session.
    ///
    /// P0-4: a start is **not** a Stop. The engine refuses a start while a
    /// healthy run is live (`"interception is already running; stop it first, or
    /// use restart"`), and this command leaves that run alone; the refusal is
    /// recorded as exactly one `gui.action` whose `from == to`. `source` names
    /// who asked (button/tray/shortcut/auto/system); `hold_on_start` asks the
    /// backend to install the tunnelling hold before the first packet (T12).
    #[allow(clippy::too_many_arguments)]
    pub fn start_boost(
        &mut self,
        game_key: &str,
        relay: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        port_range: Option<(u16, u16)>,
        target: Option<ProcessTarget>,
        source: ActionSource,
        hold_on_start: bool,
    ) -> Result<(), AppError> {
        let game_key = game_key.to_string();
        let from = self.capture_state();
        let request = crate::engine::InterceptorRequest::new(&game_key, relay, fec, fec_k)
            .with_port_range(port_range)
            .with_process_target(target)
            .with_hold_on_start(hold_on_start);
        let result = self.run_with_intent(
            SessionCommand::StartBoost,
            start_intent(source, "StartBoost"),
            move |engine| {
                engine.set_telemetry_game(&game_key);
                engine.start_interceptor_request(request)
            },
        );
        // Exactly one record per invocation, refused or not (T15).
        self.record_action_now(GuiActionKind::Optimize, source, from, "StartBoost");
        result
    }

    /// Explicitly restart interception: stop -> start in one named action.
    ///
    /// This is the only start that may replace a healthy run, so a front end
    /// that intends a restart asks for one instead of relying on a start to
    /// pre-stop (P0-4 / invariant I2). The lifecycle intent is a recovery reason
    /// (`error_retry`/`auto_recover`), never a plain user start.
    ///
    /// No GUI control calls this today — the frozen route surface offers only
    /// Optimize/Stop, and the error-card Retry path is P1 — but it is the API the
    /// explicit-restart action is required to exist for, and its `gui.action`
    /// record is tested.
    #[allow(clippy::too_many_arguments)]
    pub fn restart_boost(
        &mut self,
        game_key: &str,
        relay: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        port_range: Option<(u16, u16)>,
        target: Option<ProcessTarget>,
        source: ActionSource,
        hold_on_start: bool,
    ) -> Result<(), AppError> {
        let game_key = game_key.to_string();
        let from = self.capture_state();
        let request = crate::engine::InterceptorRequest::new(&game_key, relay, fec, fec_k)
            .with_port_range(port_range)
            .with_process_target(target)
            .with_hold_on_start(hold_on_start);
        let result = self.run_with_intent(
            SessionCommand::StartBoost,
            restart_intent(source, "StartBoost"),
            move |engine| {
                engine.set_telemetry_game(&game_key);
                engine.restart_interceptor_request(request)
            },
        );
        self.record_action_now(GuiActionKind::Restart, source, from, "StartBoost");
        result
    }

    /// Build the interceptor plan for a front end that owns its policy (the
    /// read half of [`Self::start_boost`]).
    ///
    /// Planning changes no state, so a live mode can print the discovered routes,
    /// backend and availability before starting; a failure is the same Tunnel
    /// taxonomy [`Self::start_boost`] reports, recorded in the command state.
    pub fn plan_boost(
        &mut self,
        request: crate::engine::InterceptorRequest,
    ) -> Result<crate::engine::InterceptorPlan, AppError> {
        self.run(SessionCommand::StartBoost, move |engine| {
            engine.plan_interceptor(request)
        })
    }

    /// Start a plan returned by [`Self::plan_boost`].
    ///
    /// The write half of the adopted boost path: the engine applies the plan's
    /// policy verbatim and records the same status fields as the shipped
    /// one-shot path. A backend that turns out to be unavailable fails here with
    /// the backend's own reason (Tunnel taxonomy).
    pub fn start_planned_boost(
        &mut self,
        plan: crate::engine::InterceptorPlan,
    ) -> Result<(), AppError> {
        self.run(SessionCommand::StartBoost, move |engine| {
            engine.start_interceptor_plan(plan)
        })
    }

    /// Redirect an explicit server for a front end that owns its options
    /// (adaptive policy and TCP leg), keeping the shipped start semantics.
    pub fn start_redirect_with_options(
        &mut self,
        server: SocketAddrV4,
        local_port: u16,
        game_name: &str,
        relay: SocketAddrV4,
        options: crate::engine::RedirectOptions,
    ) -> Result<(), AppError> {
        let game_name = game_name.to_string();
        self.run(SessionCommand::StartBoost, move |engine| {
            engine.start_redirect_with_options(server, local_port, game_name, relay, options);
            Ok(())
        })
    }

    /// Move the data plane onto `relay` while the session keeps running
    /// (the relay-switch command the CLI's continuous rerouting loop uses).
    ///
    /// The returned [`RelaySwitch`](crate::engine::RelaySwitch) is the awaited
    /// registration result; the current relay is published only when it succeeds,
    /// so a caller can keep its session state when a candidate fails.
    pub fn switch_relay(
        &mut self,
        relay: SocketAddrV4,
        destination: Option<std::net::Ipv4Addr>,
    ) -> Result<crate::engine::RelaySwitch, AppError> {
        self.run(SessionCommand::SelectRelay, move |engine| {
            Ok(engine.switch_relay(relay, destination))
        })
    }

    /// Stop every tunnel backend: a true Stop.
    ///
    /// This is the shared entry point the adopted CLI live modes call. It is the
    /// `System`-sourced form of [`Self::stop_boost_from`] (a CLI process has no
    /// lifecycle sink, so no `gui.action` is emitted there); the GUI calls the
    /// sourced form so its Stop carries the button/tray source.
    pub fn stop_boost(&mut self) -> Result<(), AppError> {
        self.stop_boost_from(ActionSource::System)
    }

    /// Stop every tunnel backend, recording the T15 `gui.action` for `source`.
    ///
    /// The diverting intercept/inject/shadow handles close through the normal
    /// interceptor teardown; the process-wide driver pin stays held for the next
    /// Optimize. A teardown that cannot be acknowledged is the Tunnel taxonomy
    /// error and latches the engine fail-closed until restart. Exactly one
    /// `gui.action` record is emitted per invocation (P0-4 / T15).
    pub fn stop_boost_from(&mut self, source: ActionSource) -> Result<(), AppError> {
        let from = self.capture_state();
        let result =
            self.run_with_intent(SessionCommand::StopBoost, stop_intent(source), |engine| {
                engine.stop_boost()
            });
        self.record_action_now(GuiActionKind::Stop, source, from, "StopBoost");
        result
    }

    /// Explicit session shutdown: the true Stop, then release the driver pin.
    ///
    /// Only a Quit/exit path calls this. A user Stop must use [`Self::stop_boost`]
    /// so the pin survives to make the next Optimize cheap. An unclean teardown
    /// is returned as the Tunnel taxonomy error so a caller about to hand the
    /// process away (self-update) can refuse instead of killing a live filter.
    ///
    /// `source` names who initiated the exit (system for quit/update/elevation);
    /// the invocation records exactly one `gui.action` whose kind follows the
    /// exit reason the front end stamped (P0-4 / T15).
    pub fn shutdown(&mut self, source: ActionSource) -> Result<(), AppError> {
        // Keep the front end's shutdown reason when it supplied one; a caller
        // that did not (tests, the CLI) gets the truthful `quit` default. The
        // generic `StopBoost` mapping would report `user_stop`, hiding a Quit in
        // the very trace P0-1 exists to make attributable.
        let current = lifecycle::current_intent();
        let intent = match current.reason {
            InterceptReason::Quit | InterceptReason::Update | InterceptReason::ElevationHandoff => {
                current
            }
            _ => Intent {
                reason: InterceptReason::Quit,
                actor: Actor::User,
                command_id: "quit".to_string(),
            },
        };
        let action = match intent.reason {
            InterceptReason::Update => GuiActionKind::UpdateInstall,
            InterceptReason::ElevationHandoff => GuiActionKind::ElevationHandoff,
            _ => GuiActionKind::Quit,
        };
        let command_id = intent.command_id.clone();
        let from = self.capture_state();
        let result = self.run_with_intent(SessionCommand::StopBoost, intent, |engine| {
            engine.shutdown()
        });
        // The exit hands the process away: the capture plane is not merely idle,
        // it is being left for good.
        self.record_action(action, source, from, CaptureState::Shutdown, &command_id);
        result
    }

    /// Record the one `gui.action` for the self-update install.
    ///
    /// The install shuts the engine down from its own thread (it must own the
    /// `Arc<Mutex<LightSpeedEngine>>` for the exchange), so the action is
    /// recorded here at the click that started it — exactly once, before the
    /// thread runs (P0-4 / T15).
    pub fn record_update_install(&self) {
        let from = self.capture_state();
        self.record_action(
            GuiActionKind::UpdateInstall,
            ActionSource::System,
            from,
            CaptureState::Shutdown,
            "update",
        );
    }

    /// Stop the interceptor but keep the session's control-plane registration.
    ///
    /// The adopted live modes own their teardown UX (they print their own warning)
    /// and keep watching for the game to return, so the data plane stops while the
    /// relay session lives. Returns whether the platform teardown completed within
    /// `timeout`; `false` means the caller prints the shipped warning. No backend
    /// other than the interceptor is touched.
    pub fn stop_boost_keep_registration(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<bool, AppError> {
        self.run(SessionCommand::StopBoost, move |engine| {
            Ok(engine.stop_interceptor_keep_registration(timeout))
        })
    }

    /// Start the manual/custom redirect for an explicit server.
    pub fn start_redirect(
        &mut self,
        server: SocketAddrV4,
        local_port: u16,
        fec: bool,
        fec_k: u8,
        game_name: &str,
        relay: SocketAddrV4,
    ) -> Result<(), AppError> {
        let game_name = game_name.to_string();
        self.run(SessionCommand::StartBoost, move |engine| {
            engine.start_redirect(server, local_port, fec, fec_k, game_name, relay);
            Ok(())
        })
    }

    /// Withhold tunnelling until a relay is chosen for the locked server.
    pub fn hold_tunnelling(&mut self) -> Result<bool, AppError> {
        self.run(SessionCommand::Hold, |engine| Ok(engine.hold_tunnelling()))
    }

    /// Arm the engine-side hold release for when `connected` turns true.
    pub fn release_hold_when_connected(
        &mut self,
        connected: impl Fn() -> bool + Send + 'static,
    ) -> Result<bool, AppError> {
        self.run(SessionCommand::Hold, move |engine| {
            Ok(engine.release_hold_when_connected(connected))
        })
    }
}

/// Fold one command outcome into the shared state and map errors to the DTO.
///
/// Pure and separately tested: every command above routes through it, so the
/// in-flight/`last_error` transitions and the `String`→[`AppError`] mapping are
/// one implementation, not twelve.
///
/// P0-3 (roadmap D4 item 3): a successful command clears the error **it** left
/// behind. The field incident's second sticky path was a failed Optimize pinning
/// the window to ERROR until the process restarted; the command slot no longer
/// latches (`docs/audit/game-transition-state-machine-2026-11-23.md` §3.5/F5).
pub(crate) fn record_command_result<T>(
    session: &mut SessionCommandState,
    command: SessionCommand,
    result: Result<T, String>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            if session.finish(command) {
                lifecycle::emit("ui_error", &ui_error_for_clear(&format!("{command:?}")));
            }
            Ok(value)
        }
        Err(detail) => {
            let replacing = session.last_error.is_some();
            let error = AppError::for_kind(command_error_kind(command), detail);
            session.fail(command, error.clone());
            // The stored record is stamped by `fail`; emit that same record so
            // the event and the state cannot disagree about its epoch.
            let recorded = session.last_error.clone().unwrap_or(error.clone());
            let mut event = ui_error_for_failure(command, &recorded);
            if replacing {
                event.op = UiErrorOp::Replace;
            }
            lifecycle::emit("ui_error", &event);
            Err(error)
        }
    }
}

/// The Error-row taxonomy for one command's failure (QA W2-3a N1).
///
/// The taxonomy must name what failed, not default every command to "tunnel":
/// `select_relay` failure is the relay/registration path ([`AppErrorKind::Route`]),
/// `set_path_mtu_cap` is a refused setting value ([`AppErrorKind::Validation`]),
/// and the traffic commands are tunnel/interception failures
/// ([`AppErrorKind::Tunnel`]). The Error row's leading sentence comes from this
/// kind; the verbatim engine detail stays behind `Details`.
pub fn command_error_kind(command: SessionCommand) -> AppErrorKind {
    match command {
        SessionCommand::SelectRelay => AppErrorKind::Route,
        SessionCommand::SetPathMtu => AppErrorKind::Validation,
        SessionCommand::Disconnect
        | SessionCommand::StartBoost
        | SessionCommand::StopBoost
        | SessionCommand::Hold => AppErrorKind::Tunnel,
    }
}

/// Lock a mutex, recovering a poisoned lock instead of panicking.
///
/// A poisoned engine lock must not crash the command slot; the UI thread and
/// the poller both recover rather than propagate a panic.
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_command_clears_the_in_flight_marker() {
        let mut session = SessionCommandState::default();
        session.begin(SessionCommand::StartBoost);
        let result: Result<(), AppError> =
            record_command_result(&mut session, SessionCommand::StartBoost, Ok(()));
        assert!(result.is_ok());
        assert_eq!(session.in_flight, None);
        assert_eq!(session.last_error, None);
    }

    #[test]
    fn a_failed_command_returns_the_typed_error_and_records_it() {
        let mut session = SessionCommandState::default();
        session.begin(SessionCommand::StartBoost);
        let error = record_command_result::<()>(
            &mut session,
            SessionCommand::StartBoost,
            Err("interceptor: access denied".to_string()),
        )
        .expect_err("a failed command returns an error");
        assert_eq!(error.kind, crate::app::dto::AppErrorKind::Tunnel);
        assert_eq!(error.detail.as_deref(), Some("interceptor: access denied"));
        assert_eq!(session.in_flight, None);
        assert_eq!(
            session.last_error.as_ref().map(|e| e.kind),
            Some(crate::app::dto::AppErrorKind::Tunnel),
            "the Error row reads the same failure the caller received"
        );

        // P0-3: the same command succeeding is the retry, and it clears the
        // record it made. No error may latch for the process lifetime.
        let _ = record_command_result::<()>(&mut session, SessionCommand::StartBoost, Ok(()));
        assert_eq!(
            session.last_error, None,
            "the successful retry clears the stale card"
        );
    }

    #[test]
    fn a_different_commands_success_does_not_clear_the_failure() {
        let mut session = SessionCommandState::default();
        let _ = record_command_result::<()>(
            &mut session,
            SessionCommand::SelectRelay,
            Err("control-plane registration failed".to_string()),
        );
        let _ = record_command_result::<()>(&mut session, SessionCommand::StartBoost, Ok(()));
        assert_eq!(
            session.last_error.as_ref().map(|error| error.kind),
            Some(crate::app::dto::AppErrorKind::Route),
            "only the command that failed may clear its error"
        );
        let _ = record_command_result::<()>(&mut session, SessionCommand::SelectRelay, Ok(()));
        assert_eq!(session.last_error, None);
    }

    #[test]
    fn every_command_reports_the_taxonomy_kind_that_names_what_failed() {
        // N1 (QA W2-3a): the Error row must lead with the right sentence, not
        // default every failure to "tunnel". One table pins the mapping.
        for (command, expected) in [
            (SessionCommand::SelectRelay, AppErrorKind::Route),
            (SessionCommand::SetPathMtu, AppErrorKind::Validation),
            (SessionCommand::Disconnect, AppErrorKind::Tunnel),
            (SessionCommand::StartBoost, AppErrorKind::Tunnel),
            (SessionCommand::StopBoost, AppErrorKind::Tunnel),
            (SessionCommand::Hold, AppErrorKind::Tunnel),
        ] {
            assert_eq!(
                command_error_kind(command),
                expected,
                "command_error_kind({command:?})"
            );
            let mut session = SessionCommandState::default();
            let error = record_command_result::<()>(&mut session, command, Err("boom".to_string()))
                .expect_err("failure is typed");
            assert_eq!(error.kind, expected);
            assert_eq!(error.detail.as_deref(), Some("boom"));
            assert_eq!(
                session.last_error.as_ref().map(|e| e.kind),
                Some(expected),
                "the Error row and the caller read the same kind"
            );
        }
    }

    // ── Adopted live-mode commands (post-W2 task-57) ─────────────────────

    fn session_core() -> (tokio::runtime::Runtime, crate::app::runtime::SessionCore) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let core = crate::app::runtime::SessionCore::build(runtime.handle().clone(), 1492);
        (runtime, core)
    }

    #[test]
    fn a_plan_boost_failure_is_a_typed_command_error() {
        let (_runtime, mut core) = session_core();
        let relay = SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4434);
        let request = crate::engine::InterceptorRequest::new("not-a-game", relay, false, 4);
        let error = core
            .commands_mut()
            .plan_boost(request)
            .expect_err("an unknown game must fail the plan");
        assert_eq!(
            error.kind,
            crate::app::dto::AppErrorKind::Tunnel,
            "a refused boost is the Tunnel taxonomy, like the shipped start command"
        );
        assert!(
            core.commands_mut().session().last_error.is_some(),
            "the Error row must record the refusal"
        );
    }

    #[test]
    fn adopted_redirect_and_relay_switch_commands_reach_the_shared_engine() {
        let (_runtime, mut core) = session_core();
        let relay = SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4434);
        let server = SocketAddrV4::new(std::net::Ipv4Addr::new(198, 51, 100, 9), 28015);
        core.commands_mut()
            .start_redirect_with_options(
                server,
                0,
                "sample",
                relay,
                crate::engine::RedirectOptions {
                    fec: false,
                    fec_k: 4,
                    adaptive: crate::engine::AdaptivePolicy {
                        enabled: true,
                        k_size: 8,
                        max_overhead_pct: 10,
                    },
                    tcp: false,
                    shutdown_notice: None,
                },
            )
            .expect("the redirect command must be accepted");
        assert!(
            core.status().redirect_active,
            "the redirect command must reach the shared engine"
        );
        assert_eq!(
            core.status().redirect_server,
            server.to_string(),
            "the command must carry the explicit server"
        );
        // The switch command returns the awaitable handle and leaves the shared
        // command state settled; the registration itself runs on the engine's
        // runtime (this test's runtime is not driven, so nothing is published).
        let switch = core
            .commands_mut()
            .switch_relay(relay, None)
            .expect("the switch command must be accepted");
        drop(switch);
        assert!(
            core.commands_mut().session().in_flight.is_none(),
            "returning the switch handle must settle the in-flight command marker"
        );
    }

    #[test]
    fn the_shutdown_command_settles_the_slot() {
        let (_runtime, mut core) = session_core();
        // The Quit path: the true Stop runs and the (never-taken) pin release is
        // a no-op. The command slot must settle, or the next frame shows STARTING
        // on a process that is about to exit.
        core.commands_mut()
            .shutdown(ActionSource::System)
            .expect("a fresh engine shuts down cleanly");
        assert!(core.commands_mut().session().in_flight.is_none());
        assert!(core.commands_mut().session().last_error.is_none());
    }

    // ── P0-1 lifecycle instrumentation (behaviour-neutral) ────────────────

    #[test]
    fn a_quit_shutdown_keeps_the_quit_reason_instead_of_user_stop() {
        let (_runtime, mut core) = session_core();
        lifecycle::set_intent(Intent {
            reason: InterceptReason::Quit,
            actor: Actor::User,
            command_id: "tray-quit".to_string(),
        });
        core.commands_mut()
            .shutdown(ActionSource::System)
            .expect("a fresh engine shuts down cleanly");
        let intent = lifecycle::current_intent();
        assert_eq!(
            intent.reason,
            InterceptReason::Quit,
            "the StopBoost command must not relabel a Quit as user_stop"
        );
        assert_eq!(intent.command_id, "tray-quit");

        // No front-end reason: the shutdown defaults to a truthful quit.
        lifecycle::set_intent(Intent::default());
        core.commands_mut()
            .shutdown(ActionSource::System)
            .expect("a fresh engine shuts down cleanly");
        assert_eq!(lifecycle::current_intent().reason, InterceptReason::Quit);
    }

    #[test]
    fn command_failures_and_clears_build_the_ui_error_events() {
        let error = AppError::for_kind(AppErrorKind::Tunnel, "WinDivert open failed");
        let show = ui_error_for_failure(SessionCommand::StartBoost, &error);
        assert_eq!(show.op, UiErrorOp::Show);
        assert_eq!(show.source, Some(ErrorSource::Capture));
        assert_eq!(show.kind, "Tunnel");
        assert_eq!(show.source_command.as_deref(), Some("StartBoost"));
        assert_eq!(show.reason_code.as_deref(), Some("WinDivert open failed"));

        let clear = ui_error_for_clear("StopBoost");
        assert_eq!(clear.op, UiErrorOp::Clear);
        assert_eq!(clear.cleared_by.as_deref(), Some("StopBoost"));

        // The source mapping names what failed, not one blanket subsystem.
        assert_eq!(
            error_source_for(AppErrorKind::Route),
            ErrorSource::Registration
        );
        assert_eq!(error_source_for(AppErrorKind::Config), ErrorSource::Config);
        assert_eq!(error_source_for(AppErrorKind::Game), ErrorSource::Game);
    }

    #[test]
    fn commands_stamp_the_interception_intent_without_changing_behaviour() {
        let start = intent_for_command(SessionCommand::StartBoost);
        assert_eq!(start.reason, InterceptReason::UserStart);
        assert_eq!(start.actor, Actor::User);
        assert_eq!(start.command_id, "StartBoost");

        let stop = intent_for_command(SessionCommand::StopBoost);
        assert_eq!(stop.reason, InterceptReason::UserStop);

        let relay = intent_for_command(SessionCommand::SelectRelay);
        assert_eq!(relay.reason, InterceptReason::RelaySwitch);
        assert_eq!(relay.actor, Actor::Auto);
    }

    // ── P0-4 / T15: the capture-crossing action audit ─────────────────────

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightspeed-p0-4-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The `gui.action` records the isolated test sink captured, oldest first.
    fn gui_actions(dir: &std::path::Path) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(dir.join(lifecycle::LIFECYCLE_FILE)).unwrap_or_default();
        text.lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|event| event["event"] == "gui.action")
            .collect()
    }

    /// A live in-memory run, so "a start while a healthy run is live" can be
    /// tested without a driver handle.
    fn healthy_mock_handle(held: bool) -> crate::interceptor::InterceptorHandle {
        use crate::interceptor::TrafficInterceptor as _;
        let mut config = crate::interceptor::mock::test_config();
        config.start_held = held;
        crate::interceptor::mock::MockInterceptor::new()
            .start(config)
            .expect("the mock backend starts")
    }

    #[test]
    fn every_capture_crossing_command_records_exactly_one_gui_action() {
        let (_runtime, mut core) = session_core();
        let relay = SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4434);
        let dir = temp_dir("actions");
        let _sink = lifecycle::install_test_sink(dir.clone());

        // The game is unknown, so each start/restart fails after the invocation
        // began; the audit is about the invocation, not about the backend. A
        // plan failure is the honest "no capture happened" outcome here.
        let _ = core.commands_mut().start_boost(
            "not-a-game",
            relay,
            false,
            4,
            None,
            None,
            ActionSource::Button,
            true,
        );
        let _ = core.commands_mut().start_boost(
            "not-a-game",
            relay,
            false,
            4,
            None,
            None,
            ActionSource::Auto,
            true,
        );
        core.commands_mut()
            .stop_boost_from(ActionSource::Tray)
            .expect("a stop with no capture run is clean");
        let _ = core.commands_mut().restart_boost(
            "not-a-game",
            relay,
            false,
            4,
            None,
            None,
            ActionSource::Button,
            true,
        );
        core.commands_mut()
            .shutdown(ActionSource::System)
            .expect("a fresh engine shuts down cleanly");
        core.commands_mut().record_update_install();

        let actions: Vec<(String, String, String, String, String)> = gui_actions(&dir)
            .into_iter()
            .map(|event| {
                (
                    event["action"].as_str().unwrap_or_default().to_string(),
                    event["source"].as_str().unwrap_or_default().to_string(),
                    event["from"].as_str().unwrap_or_default().to_string(),
                    event["to"].as_str().unwrap_or_default().to_string(),
                    event["command_id"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        let expected = vec![
            (
                "optimize".to_string(),
                "button".to_string(),
                "idle".to_string(),
                "idle".to_string(),
                "StartBoost".to_string(),
            ),
            (
                "optimize".to_string(),
                "auto".to_string(),
                "idle".to_string(),
                "idle".to_string(),
                "StartBoost".to_string(),
            ),
            (
                "stop".to_string(),
                "tray".to_string(),
                "idle".to_string(),
                "idle".to_string(),
                "StopBoost".to_string(),
            ),
            (
                "restart".to_string(),
                "button".to_string(),
                "idle".to_string(),
                "idle".to_string(),
                "StartBoost".to_string(),
            ),
            (
                "quit".to_string(),
                "system".to_string(),
                "idle".to_string(),
                "shutdown".to_string(),
                "quit".to_string(),
            ),
            (
                "update_install".to_string(),
                "system".to_string(),
                "idle".to_string(),
                "shutdown".to_string(),
                "update".to_string(),
            ),
        ];
        assert_eq!(
            actions, expected,
            "one gui.action per capture-crossing invocation, with the source and transition"
        );
    }

    #[test]
    fn a_refused_start_records_one_action_and_leaves_the_healthy_run_alone() {
        let (_runtime, mut core) = session_core();
        let relay = SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4434);
        let dir = temp_dir("refused");
        let _sink = lifecycle::install_test_sink(dir.clone());

        // A healthy run already exists, exactly as it does when a stale UI tries
        // to Optimize over a live session.
        core.engine()
            .lock()
            .expect("engine")
            .install_mock_interceptor_for_test(healthy_mock_handle(false));
        assert!(core.status().interceptor_active);

        let error = core
            .commands_mut()
            .start_boost(
                "not-a-game",
                relay,
                false,
                4,
                None,
                None,
                ActionSource::Button,
                true,
            )
            .expect_err("a start over a healthy run must be refused");
        assert_eq!(
            error.detail.as_deref(),
            Some(crate::engine::ALREADY_INTERCEPTING),
            "the command carries the engine's typed refusal verbatim"
        );
        assert!(
            core.status().interceptor_active,
            "the refused start must not tear the healthy run down"
        );

        let actions = gui_actions(&dir);
        assert_eq!(
            actions.len(),
            1,
            "the refusal is recorded exactly once, not once per attempted step"
        );
        assert_eq!(actions[0]["action"], "optimize");
        assert_eq!(actions[0]["source"], "button");
        assert_eq!(
            actions[0]["from"], actions[0]["to"],
            "a refused start leaves the capture plane exactly where it was"
        );
        assert_eq!(actions[0]["from"], "capturing");
        assert_eq!(actions[0]["command_id"], "StartBoost");
    }
}
