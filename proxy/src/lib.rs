//! # LightSpeed Proxy Library
//!
//! Core proxy relay engine, authentication, abuse detection, rate limiting,
//! and metrics. Exposed as a library for integration testing.

pub mod config;
pub mod relay;
pub mod auth;
pub mod metrics;
pub mod health;
pub mod rate_limit;
pub mod abuse;
pub mod control;
