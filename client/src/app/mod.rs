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

pub mod config;
pub mod discovery;
pub mod dto;
pub mod status;
