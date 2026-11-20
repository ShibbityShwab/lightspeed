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

pub mod commands;
pub mod config;
pub mod discovery;
pub mod dto;
pub mod history;
/// The honest server-location ladder and its offline indexes (S5a).
///
/// Feature-gated, one meaning per feature (`Cargo.toml`): `server-location`
/// compiles the ladder and the three server indexes with their tables/manifests;
/// `geo-ip` adds only the country blob/table as the ladder's last rung (with it
/// off the ladder ends at `Place::Unknown`). The GUI enables both; the shipped
/// CLI's default build enables neither.
#[cfg(feature = "server-location")]
pub mod location;
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
