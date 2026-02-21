//! # Platform-Specific Packet Capture
//!
//! Provides platform-specific packet capture backends:
//! - **Windows**: WFP (Windows Filtering Platform) or pcap via Npcap
//! - **Linux**: AF_PACKET raw sockets or pcap via libpcap
//! - **macOS**: BPF (Berkeley Packet Filter) or pcap
//!
//! The default backend for all platforms is pcap, which provides a
//! consistent cross-platform API. Platform-native backends can be
//! enabled for better performance.
//!
//! **Note**: Requires the `pcap-capture` feature to be enabled.

#[cfg(all(target_os = "windows", feature = "pcap-capture"))]
pub mod windows;

#[cfg(all(target_os = "linux", feature = "pcap-capture"))]
pub mod linux;

#[cfg(all(target_os = "macos", feature = "pcap-capture"))]
pub mod macos;

#[cfg(feature = "pcap-capture")]
pub mod pcap_backend;

use crate::error::CaptureError;
use crate::tunnel::capture::PacketCapture;

/// Create the default packet capture backend for the current platform.
#[cfg(feature = "pcap-capture")]
pub fn create_default_capture() -> Result<Box<dyn PacketCapture>, CaptureError> {
    Ok(Box::new(pcap_backend::PcapCapture::new()))
}

/// Stub when pcap-capture feature is not enabled.
#[cfg(not(feature = "pcap-capture"))]
pub fn create_default_capture() -> Result<Box<dyn PacketCapture>, CaptureError> {
    Err(CaptureError::UnsupportedPlatform(
        "Packet capture requires the 'pcap-capture' feature. Rebuild with: cargo build --features pcap-capture".into()
    ))
}

/// List available network interfaces suitable for capture.
pub fn list_interfaces() -> Vec<InterfaceInfo> {
    // TODO: Use pcap::Device::list() to enumerate interfaces
    vec![]
}

/// Information about a network interface.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// Interface name (e.g., "eth0", "Ethernet").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this interface is up and active.
    pub is_up: bool,
    /// Whether this is a loopback interface.
    pub is_loopback: bool,
}
