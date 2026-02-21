//! # LightSpeed Error Types
//!
//! Centralized error definitions for the client application.

use thiserror::Error;

/// Top-level client errors.
#[derive(Error, Debug)]
pub enum LightSpeedError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Tunnel error: {0}")]
    Tunnel(#[from] TunnelError),

    #[error("Capture error: {0}")]
    Capture(#[from] CaptureError),

    #[error("Route error: {0}")]
    Route(#[from] RouteError),

    #[error("QUIC control plane error: {0}")]
    Quic(#[from] QuicError),

    #[error("ML model error: {0}")]
    Ml(#[from] MlError),

    #[error("Game detection error: {0}")]
    Game(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Tunnel engine errors.
#[derive(Error, Debug)]
pub enum TunnelError {
    #[error("Protocol decode error: {0}")]
    Decode(#[from] lightspeed_protocol::DecodeError),

    #[error("Relay error: {0}")]
    Relay(String),

    #[error("Tunnel not connected")]
    NotConnected,

    #[error("Tunnel timeout after {0}ms")]
    Timeout(u64),

    #[error("Socket error: {0}")]
    Socket(#[from] std::io::Error),
}

/// Packet capture errors.
#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("No suitable network interface found")]
    NoInterface,

    #[error("Interface {0} not available")]
    InterfaceUnavailable(String),

    #[error("Capture requires elevated privileges")]
    PermissionDenied,

    #[error("Pcap error: {0}")]
    Pcap(String),

    #[error("Platform not supported: {0}")]
    UnsupportedPlatform(String),
}

/// Route selection errors.
#[derive(Error, Debug)]
pub enum RouteError {
    #[error("No proxies available")]
    NoProxies,

    #[error("All proxies unhealthy")]
    AllUnhealthy,

    #[error("Route selection timeout")]
    Timeout,

    #[error("Failover exhausted — no routes remaining")]
    FailoverExhausted,
}

/// QUIC control plane errors.
#[derive(Error, Debug)]
pub enum QuicError {
    #[error("Connection to proxy failed: {0}")]
    ConnectionFailed(String),

    #[error("Proxy discovery failed: {0}")]
    DiscoveryFailed(String),

    #[error("Health check failed for {0}")]
    HealthCheckFailed(String),

    #[error("TLS configuration error: {0}")]
    Tls(String),
}

/// ML model errors.
#[derive(Error, Debug)]
pub enum MlError {
    #[error("Model not loaded")]
    NotLoaded,

    #[error("Prediction failed: {0}")]
    PredictionFailed(String),

    #[error("Feature extraction error: {0}")]
    FeatureExtraction(String),

    #[error("Model file not found: {0}")]
    ModelNotFound(String),
}
