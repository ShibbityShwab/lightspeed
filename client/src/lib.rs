//! LightSpeed client — library API for GUI integration.
//!
//! Exposes [`LightSpeedEngine`] which manages the background tunnel loop,
//! provides [`EngineStatus`] snapshots for GUI rendering, and re-exports
//! game detection helpers for the game-routing UI.
//!
//! The internal modules below are included to resolve cross-module imports
//! (e.g. `games` → `tunnel::capture`, `engine` → `redirect`).  They are
//! not part of the public library surface.

pub mod engine;
pub mod games;
pub mod interceptor;

pub use engine::{EngineStatus, LightSpeedEngine};

// ── Internal modules needed to satisfy intra-crate imports ───────────────
// These mirror the mod declarations in main.rs.  Pub-accessibility is
// intentionally restricted — external callers should only use the types
// re-exported above.
#[allow(dead_code)]
pub(crate) mod capture;
#[allow(dead_code)]
pub(crate) mod cli;
#[allow(dead_code)]
pub(crate) mod config;
#[allow(dead_code)]
pub(crate) mod error;
#[allow(dead_code)]
pub(crate) mod ml;
#[allow(dead_code)]
pub(crate) mod modes;
#[allow(dead_code)]
pub(crate) mod quic;
#[allow(dead_code)]
pub(crate) mod redirect;
#[allow(dead_code)]
pub(crate) mod route;
#[allow(dead_code)]
pub(crate) mod telemetry;
#[allow(dead_code)]
pub(crate) mod tunnel;
#[allow(dead_code)]
pub(crate) mod warp;
