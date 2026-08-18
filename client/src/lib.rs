//! LightSpeed client — library API for GUI integration.
//!
//! NOTE: `dead_code` is allowed because this library exposes a wide internal
//! API surface (`pub(crate)`) that is used by the binary (`main.rs`) but not
//! always through the public library path. Items may appear dead from the
//! library's perspective while being actively used by the binary crate.
#![allow(dead_code)]

pub mod engine;
pub mod games;
pub mod interceptor;

pub use engine::{EngineStatus, LightSpeedEngine};

pub(crate) mod capture;
pub(crate) mod cli;
pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod ml;
pub(crate) mod modes;
pub(crate) mod quic;
pub(crate) mod redirect;
pub(crate) mod route;
pub(crate) mod session;
pub(crate) mod telemetry;
pub(crate) mod tunnel;
pub(crate) mod warp;
