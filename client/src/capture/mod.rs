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
//! ```ignore
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

pub mod injector;
pub mod pcap_backend;
pub mod windivert_redirect;

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

/// Auto-select the best capture interface for game traffic.
///
/// Prefers wired Ethernet over WiFi; skips loopback, virtual adapters,
/// WARP, Tailscale, and Hyper-V virtual NICs.
pub fn pick_best_interface() -> Option<String> {
    let interfaces = list_interfaces();
    // Keywords in description that indicate virtual / tunnel adapters to skip
    let skip_kw = [
        "loopback",
        "warp",
        "tailscale",
        "hyper-v",
        "virtual",
        "miniport",
        "wi-fi direct",
        "bluetooth",
    ];

    // Score each interface: 2 = wired ethernet, 1 = wifi, 0 = other
    interfaces
        .iter()
        .filter(|i| i.is_up && !i.is_loopback)
        .filter(|i| {
            let desc = i.description.to_lowercase();
            !skip_kw.iter().any(|kw| desc.contains(kw))
        })
        .max_by_key(|i| {
            let desc = i.description.to_lowercase();
            if desc.contains("gbe")
                || desc.contains("2.5g")
                || desc.contains("ethernet")
                || desc.contains("pcie")
                || desc.contains("lan")
            {
                2u8 // Wired Ethernet — preferred
            } else if desc.contains("wifi")
                || desc.contains("wi-fi")
                || desc.contains("wireless")
                || desc.contains("802.11")
            {
                1u8 // WiFi — second choice
            } else {
                0u8 // Other — avoid
            }
        })
        .map(|i| i.name.clone())
}

/// Returns the human-readable description of a capture interface by name.
pub fn interface_description(name: &str) -> String {
    list_interfaces()
        .into_iter()
        .find(|i| i.name == name)
        .map(|i| i.description)
        .unwrap_or_else(|| name.to_string())
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
