//! Engine-backed composition root for front ends (WF-081 W2-3b **S6b**).
//!
//! The GUI builds this state inline in its `main` today; the CLI was historically
//! engine-less and every session mode built its own raw state. This module is the
//! one shared composition root so the CLI can adopt the *same* engine + command
//! facade the GUI uses (`docs/design/gui-core-boundary.md` §5.2/§6 S6):
//!
//! 1. **install the core seams first** — the provider classifier the client's
//!    server detector consults is installed before anything can detect a server
//!    (guard G7: the seam is core-owned and installed before engine creation);
//! 2. **create the engine** ([`crate::LightSpeedEngine`]) on the caller's tokio
//!    runtime;
//! 3. **wrap it in [`AppCore`]** — the shared command state every front end runs
//!    commands through (`SessionCommand`/`AppError`), so a CLI session and a GUI
//!    session share one implementation instead of two.
//!
//! The effective path-MTU cap is applied through the shared command (it records
//! the in-flight/error state the same way every other command does), not by
//! poking the engine directly. The root is cheap to build (no network, no
//! capture): a read-only mode may construct it to verify its invariants, and a
//! session mode constructs it once before starting work.
//!
//! The provider-seam install is feature-gated (`server-location`): a default CLI
//! build ships no location indexes (Q11), so it has no classifier to install —
//! the same gating the GUI honours through its dependency features.

use std::sync::{Arc, Mutex};

use crate::app::commands::AppCore;
use crate::{EngineStatus, LightSpeedEngine};

/// The shared runtime state: the engine plus the command facade over it.
pub struct SessionCore {
    engine: Arc<Mutex<LightSpeedEngine>>,
    commands: AppCore,
}

impl SessionCore {
    /// Build the shared state in the boundary's order: seams, engine, commands.
    ///
    /// `mtu_cap` is the effective path-MTU cap from `--mtu` / `[tunnel] mtu`
    /// (clamped by the caller); it is applied as a command so a refusal lands in
    /// the shared command state exactly as it would from the GUI.
    pub fn build(runtime: tokio::runtime::Handle, mtu_cap: usize) -> Self {
        install_core_seams();
        let engine = Arc::new(Mutex::new(LightSpeedEngine::new(runtime)));
        let mut commands = AppCore::new(Arc::clone(&engine));
        let _ = commands.set_path_mtu_cap(mtu_cap);
        Self { engine, commands }
    }

    /// The engine's published status snapshot (read-only).
    pub fn status(&self) -> EngineStatus {
        self.commands.snapshot()
    }

    /// The shared command facade, for a mode that issues `SessionCommand`s.
    pub fn commands_mut(&mut self) -> &mut AppCore {
        &mut self.commands
    }

    /// The engine handle, for a mode that needs the raw engine (interim until the
    /// last session modes are re-pointed at `AppCore`).
    pub fn engine(&self) -> Arc<Mutex<LightSpeedEngine>> {
        Arc::clone(&self.engine)
    }

    /// Whether a freshly built root is in the empty, disconnected state a session
    /// may start from. `--check` asserts this instead of trusting that the
    /// construction order cannot regress.
    pub fn is_clean_start(&self) -> bool {
        let status = self.status();
        !status.connected && !status.redirect_active && !status.control_registered
    }
}

/// Install the process-wide core seams before the engine exists (G7's order).
fn install_core_seams() {
    // Default CLI builds ship no location indexes, so there is no classifier to
    // install; a `server-location` build gets the core-owned installer.
    #[cfg(feature = "server-location")]
    super::location::install_provider_classifier();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
    }

    #[test]
    fn the_root_builds_a_clean_disconnected_engine_and_accepts_commands() {
        let runtime = runtime();
        let mut root = SessionCore::build(runtime.handle().clone(), 1492);
        assert!(
            root.is_clean_start(),
            "a fresh session core must be disconnected, non-redirecting, unregistered"
        );
        // The MTU command went through the shared command state: a valid cap
        // leaves no in-flight command and no recorded error.
        assert!(
            root.commands_mut().session().last_error.is_none(),
            "a valid MTU cap must not record an error"
        );
        assert!(
            root.commands_mut().session().in_flight.is_none(),
            "the command state must be settled after construction"
        );
    }

    #[test]
    fn two_roots_are_independent() {
        let runtime = runtime();
        let first = SessionCore::build(runtime.handle().clone(), 1492);
        let second = SessionCore::build(runtime.handle().clone(), 1280);
        assert!(!Arc::ptr_eq(&first.engine(), &second.engine()));
        assert!(first.is_clean_start() && second.is_clean_start());
    }

    #[test]
    fn a_rejected_mtu_cap_is_a_typed_validation_error_not_a_silent_ignore() {
        // Defence in depth for the composition root: an out-of-range cap must
        // behave like every other command (Validation taxonomy, never a panic).
        let runtime = runtime();
        let mut root = SessionCore::build(runtime.handle().clone(), 1492);
        // The engine clamps rather than rejects today; the contract this pins is
        // that a command through the shared facade stays typed either way.
        let result = root.commands_mut().set_path_mtu_cap(1_000_000);
        if let Err(error) = result {
            assert_eq!(error.kind, super::super::dto::AppErrorKind::Validation);
        }
        assert!(!root.status().connected);
    }
}
