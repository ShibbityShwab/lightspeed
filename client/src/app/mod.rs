//! Shared application facade: the logic the CLI and the GUI both consume.
//!
//! WF-081 "one logic, GUI is a wrapper" (`docs/design/gui-core-boundary.md`).
//! Business logic, session orchestration, the config schema, relay discovery,
//! status math and derived state live here, in `lightspeed-client`, so a fix
//! lands once; `client-gui/` consumes this module through its thin adapters and
//! keeps only rendering, platform integration, i18n lookup and view state.
//!
//! The module is built by staged migration (`gui-core-boundary.md` §6, S0–S6);
//! each stage moves one unit and keeps the repo gate green. The drift guard
//! `client/tests/gui_boundary.rs` (G1–G7) fails when logic creeps back into the
//! GUI and when a new unit is registered without a budget.
//!
//! Stage S0 is the guard skeleton: this module exists so the boundary rules
//! have a shared crate to point at. S1 adds the config model, S2 discovery, S3a
//! the status DTO, S3b commands, S4 the watchdog/history, S5 location/leg/trace
//! and S6 the CLI adoption.
//!
//! ## Presence-policy canon (Q9)
//!
//! The GUI's presence policy is the product canon: a **5-second poll interval**
//! that follows the **auto-detected game only** (a pinned manual selection is
//! honoured without a presence loop). The CLI has no presence loop at all — its
//! `--watch` mode is an explicit user action that requires `--game`, and
//! `--scan-processes` is a one-shot report — so the difference is intentional
//! and named, not drift. A future CLI adoption must reuse this canon rather than
//! invent a second presence policy.
//!
//! ## Named front-end adapters (WF-081 W2-3b S6)
//!
//! The CLI is a separate front end with its own engine-less data-plane model
//! (bin-local `session`/`tunnel`/`quic` modules and its own registration). Four
//! CLI surfaces deliberately stay front-end adapters because adopting the shared
//! engine path would change shipped behaviour or needs an engine surface that
//! does not exist yet. They are named here so the exception set cannot drift
//! silently (`client/tests/gui_boundary.rs` G1 carries the import-surface map):
//!
//! * **keepalive (default no-`--game-server` path)** — stays the CLI's
//!   UDP-relay echo loop (`client/src/modes/keepalive.rs`, dispatched from
//!   `client/src/main.rs`). The engine's keepalive is the QUIC control-plane
//!   loop (`LightSpeedEngine::connect`); forcing adoption would change the
//!   CLI's measurement/protocol behaviour. Lead-decided deferral (S6b).
//! * **`--live-test`** — stays CLI-specific (`client/src/modes/live_test.rs`):
//!   it drives raw QUIC/tunnel/FEC diagnostic phases and needs an engine
//!   diagnostic-phase surface that does not exist. Lead-decided deferral (S6b).
//! * **live session modes `--game-server`, `--start-interceptor`, `--watch`** —
//!   the CLI front-end paths are still the shipped ones (`client/src/main.rs`
//!   redirect branch; `client/src/modes/intercept_mode.rs`;
//!   `client/src/modes/watch_mode.rs`). The engine gaps S6b2 named
//!   (`wat/evidence/2026-11-20/w2-3b/s6b2/report.md`) are now closed by the
//!   additive surface in [`crate::engine`]: `InterceptorRequest` +
//!   `plan_interceptor`/`start_interceptor_plan` expose the discovery/print facts
//!   and apply the caller's adaptive-FEC policy verbatim, `RedirectOptions` +
//!   `redirect_counters` add the redirect `adaptive`/`tcp` options and the final
//!   stats counters, and `switch_relay` is the awaitable in-session relay switch
//!   the continuous rerouting loop needs. Re-pointing the three modes at
//!   [`commands::AppCore`] on top of that surface (with parity fixtures) is the
//!   remaining stage of post-W2 task-57.
//! * **CLI module dedup is closed for this program**: no CLI module is
//!   unreferenced, and the library counterparts are `pub(crate)`, so deleting or
//!   reusing them needs a public core API decision; the item-level G1 adapter
//!   map (41 rows) is the accepted end state.

pub mod commands;
pub mod config;
pub mod discovery;
pub mod dto;
pub mod history;
/// Lifecycle instrumentation v1 (WinDivert stability reset, P0-1).
///
/// Behaviour-neutral: a typed, stable `lifecycle.jsonl` event stream plus the
/// identity stamp and the rate-limited support bundle. Nothing in the product
/// reads these events; they exist so a field incident is attributable (build,
/// actor, reason, handle ids, registration ledger, UI card) instead of inferred.
/// See `docs/design/windivert-stability-roadmap-2026-11-23.md` P0-1.
pub mod lifecycle;
/// The adopted live session modes (post-W2 task-57).
///
/// `--watch` (and, as it migrates, `--start-interceptor` and the `--game-server`
/// redirect) run here, on [`commands::AppCore`] / [`runtime::SessionCore`],
/// instead of the binary's engine-less session model. Library-side by design:
/// the binary compiles its own `modes/*`/`quic`/`session` copies with separate
/// process-global statics, so a mode on the library engine must run its whole
/// session against the library modules (see `client/src/app/live/mod.rs`).
pub mod live;
/// The honest server-location ladder and its offline indexes (S5a).
///
/// Feature-gated, one meaning per feature (`Cargo.toml`): `server-location`
/// compiles the ladder and the three server indexes with their tables/manifests;
/// `geo-ip` adds only the country blob/table as the ladder's last rung (with it
/// off the ladder ends at `Place::Unknown`). The GUI enables both; the shipped
/// CLI's default build enables neither.
#[cfg(feature = "server-location")]
pub mod location;
/// The engine-backed composition root shared by front ends (S6b).
///
/// Installs the core seams, creates the engine and wraps it in
/// [`commands::AppCore`] in that order, so a front end does not re-implement the
/// construction sequence (`client/src/main.rs --check` adopts it first).
pub mod runtime;
pub mod status;
/// The client-vantage route trace runtime and its stable log line (S5b).
///
/// The OS-tool scheduler, parser, refusal guard and bounded runner are shared
/// with the CLI; the localized caption stays presentation over its
/// `dto::ClientRoute` fact (`client-gui/src/client_trace.rs`).
pub mod trace;
/// Pure update rules shared by every front end (S6): version comparison, the
/// release-asset coordinates and the checksum-file parser. Network, download
/// verification and elevation stay in the GUI adapter.
pub mod update;
pub mod watchdog;

/// Whether the process-wide interception driver pin is held.
///
/// The shared facade a front end uses instead of reaching into
/// [`crate::interceptor`] directly (drift guard G1): a user Stop keeps the pin,
/// so a held pin here means the engine was not explicitly shut down. Only
/// `AppCore::shutdown` / `LightSpeedEngine::shutdown` release it.
pub fn driver_pin_active() -> bool {
    crate::interceptor::driver_pin_active()
}
