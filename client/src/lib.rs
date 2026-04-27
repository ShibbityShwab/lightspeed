//! LightSpeed client — library API for GUI integration.
//!
//! Exposes [`LightSpeedEngine`] which manages the background tunnel loop
//! and provides [`EngineStatus`] snapshots for GUI rendering.

pub mod engine;
pub use engine::{EngineStatus, LightSpeedEngine};
