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
        self.session.clear_error();
    }

    /// The engine's current status snapshot (read-only passthrough).
    pub fn snapshot(&self) -> crate::EngineStatus {
        lock_or_recover(&self.engine).snapshot()
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
    pub fn start_boost(
        &mut self,
        game_key: &str,
        relay: SocketAddrV4,
        fec: bool,
        fec_k: u8,
        port_range: Option<(u16, u16)>,
        target: Option<ProcessTarget>,
    ) -> Result<(), AppError> {
        let game_key = game_key.to_string();
        self.run(SessionCommand::StartBoost, move |engine| {
            engine.set_telemetry_game(&game_key);
            engine.start_interceptor_with_target(&game_key, relay, fec, fec_k, port_range, target)
        })
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

    /// Stop every tunnel backend (interceptor, WinDivert, capture, redirect).
    pub fn stop_boost(&mut self) -> Result<(), AppError> {
        self.run(SessionCommand::StopBoost, |engine| {
            engine.stop_interceptor();
            engine.stop_windivert();
            engine.stop_capture();
            engine.stop_redirect();
            Ok(())
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
pub(crate) fn record_command_result<T>(
    session: &mut SessionCommandState,
    command: SessionCommand,
    result: Result<T, String>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            session.finish(command);
            Ok(value)
        }
        Err(detail) => {
            let error = AppError::for_kind(command_error_kind(command), detail);
            session.fail(command, error.clone());
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

        // A later success does not erase the recorded error: the Error panel
        // keeps its Details until the user retries (the shell clears it).
        let _ = record_command_result::<()>(&mut session, SessionCommand::StartBoost, Ok(()));
        assert!(session.last_error.is_some());
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
}
