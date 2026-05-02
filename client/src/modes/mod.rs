//! Operational modes for the LightSpeed client.
//!
//! Each submodule encapsulates one top-level mode that `main` can dispatch into,
//! keeping `main.rs` a thin orchestrator.

pub mod capture_mode;
pub mod control_test;
pub mod keepalive;
pub mod live_test;
pub mod proxy_probe;
pub mod redirect_windivert;
pub mod tunnel_test;
