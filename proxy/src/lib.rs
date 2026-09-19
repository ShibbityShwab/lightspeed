//! # LightSpeed Proxy Library
//!
//! Core proxy relay engine, authentication, abuse detection, rate limiting,
//! and metrics. Exposed as a library for integration testing.

pub mod abuse;
pub mod auth;
pub mod config;
pub mod control;
pub mod geo;
pub mod handoff;
pub mod health;
pub mod metrics;
pub mod notify;
pub mod rate_limit;
pub mod relay;
pub mod update_state;
