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
//! ## Feature Flags
//!
//! - `pcap-capture`: Enables the pcap backend (requires Npcap/libpcap)
//!
//! ## Usage
//!
//! ```no_run
//! use lightspeed_client::capture;
//!
//! // List available interfaces
//! let interfaces = capture::list_interfaces();
//! for iface in &interfaces {
//!     println!("{} [{}] — {}", iface.name,
//!         if iface.is_up { "UP" } else { "DOWN" },
//!         iface.description);
//! }
//!
//! // Create a capture backend
//! let mut cap = capture::create_default_capture().unwrap();
//! ```

#[cfg(all(target_os = "windows", feature = "pcap-capture"))]
pub mod windows;

#[cfg(all(target_os = "linux", feature = "pcap-capture"))]
pub mod linux;

#[cfg(all(target_os = "macos", feature = "pcap-capture"))]
pub mod macos;

pub mod pcap_backend;
pub mod injector;

use crate::error::CaptureError;
use crate::tunnel::capture::PacketCapture;

/// Create the default packet capture backend for the current platform.
#[cfg(feature = "pcap-capture")]
pub fn create_default_capture() -> Result<Box<dyn PacketCapture>, CaptureError> {
    Ok(Box::new(pcap_backend::PcapCapture::new()))
}

/// Create a packet capture backend for a specific network interface.
#[cfg(feature = "pcap-capture")]
pub fn create_capture_on(interface: &str) -> Result<Box<dyn PacketCapture>, CaptureError> {
    Ok(Box::new(
        pcap_backend::PcapCapture::new().with_interface(interface),
    ))
}

/// Stub when pcap-capture feature is not enabled.
#[cfg(not(feature = "pcap-capture"))]
pub fn create_default_capture() -> Result<Box<dyn PacketCapture>, CaptureError> {
    Err(CaptureError::UnsupportedPlatform(
        "Packet capture requires the 'pcap-capture' feature. Rebuild with: cargo build --features pcap-capture".into()
    ))
}

/// Stub when pcap-capture feature is not enabled.
#[cfg(not(feature = "pcap-capture"))]
pub fn create_capture_on(_interface: &str) -> Result<Box<dyn PacketCapture>, CaptureError> {
    Err(CaptureError::UnsupportedPlatform(
        "Packet capture requires the 'pcap-capture' feature. Rebuild with: cargo build --features pcap-capture".into()
    ))
}

/// List available network interfaces suitable for capture.
///
/// Uses pcap to enumerate interfaces when the `pcap-capture` feature is enabled.
/// Returns an empty list otherwise.
pub fn list_interfaces() -> Vec<InterfaceInfo> {
    pcap_backend::list_pcap_interfaces()
}

/// Information about a network interface.
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// Interface name (e.g., "eth0", "Ethernet", "\\Device\\NPF_{GUID}").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Whether this interface is up and active.
    pub is_up: bool,
    /// Whether this is a loopback interface.
    pub is_loopback: bool,
}
