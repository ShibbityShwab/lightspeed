//! # Pcap Capture Backend
//!
//! Cross-platform packet capture using the pcap crate (libpcap/Npcap).
//! This is the default backend for all platforms.
//!
//! ## How it works
//!
//! 1. Opens a live capture on the specified (or default) network interface
//! 2. Applies a BPF filter to capture only game-relevant UDP traffic
//! 3. Parses raw Ethernet → IPv4 → UDP frames to extract game payloads
//! 4. Returns `CapturedPacket` structs ready for tunnel encapsulation
//!
//! ## Requirements
//!
//! - **Windows**: Npcap (https://npcap.com) or WinPcap
//! - **Linux**: libpcap-dev (`apt install libpcap-dev`)
//! - **macOS**: libpcap (included with Xcode Command Line Tools)

#[cfg(feature = "pcap-capture")]
use pcap::{Capture, Active, Device};

#[cfg(feature = "pcap-capture")]
use std::net::{Ipv4Addr, SocketAddrV4};

#[cfg(feature = "pcap-capture")]
use bytes::Bytes;

use crate::error::CaptureError;
use crate::tunnel::capture::{CaptureFilter, CapturedPacket, PacketCapture};

// Ethernet + IPv4 + UDP minimum header sizes
const ETH_HEADER_LEN: usize = 14;
const IPV4_MIN_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;
const MIN_PACKET_LEN: usize = ETH_HEADER_LEN + IPV4_MIN_HEADER_LEN + UDP_HEADER_LEN;

// EtherType for IPv4
const ETHERTYPE_IPV4: u16 = 0x0800;
// IP protocol number for UDP
const IP_PROTO_UDP: u8 = 17;

/// Pcap-based packet capture.
///
/// Captures raw network frames, parses Ethernet/IP/UDP headers,
/// and extracts game UDP payloads for tunnel encapsulation.
pub struct PcapCapture {
    /// Whether capture is currently active.
    active: bool,
    /// The network interface to capture on.
    interface: Option<String>,
    /// Live pcap capture handle.
    #[cfg(feature = "pcap-capture")]
    capture: Option<Capture<Active>>,
}

impl PcapCapture {
    /// Create a new pcap capture instance.
    pub fn new() -> Self {
        Self {
            active: false,
            interface: None,
            #[cfg(feature = "pcap-capture")]
            capture: None,
        }
    }

    /// Set the network interface to capture on.
    pub fn with_interface(mut self, interface: &str) -> Self {
        self.interface = Some(interface.to_string());
        self
    }
}

#[cfg(feature = "pcap-capture")]
impl PacketCapture for PcapCapture {
    fn start(&mut self, filter: &CaptureFilter) -> Result<(), CaptureError> {
        tracing::info!("Starting pcap capture with BPF filter: {}", filter.bpf);

        // Find the target device
        let device = if let Some(ref iface_name) = self.interface {
            // User specified an interface
            let devices = Device::list()
                .map_err(|e| CaptureError::Pcap(format!("Failed to list devices: {}", e)))?;
            devices
                .into_iter()
                .find(|d| d.name == *iface_name)
                .ok_or_else(|| CaptureError::InterfaceUnavailable(iface_name.clone()))?
        } else {
            // Use the default device (first non-loopback with an address)
            let devices = Device::list()
                .map_err(|e| CaptureError::Pcap(format!("Failed to list devices: {}", e)))?;
            devices
                .into_iter()
                .find(|d| {
                    !d.flags.is_loopback()
                        && d.flags.is_up()
                        && d.flags.is_running()
                        && !d.addresses.is_empty()
                })
                .ok_or(CaptureError::NoInterface)?
        };

        tracing::info!("Capturing on interface: {} ({})",
            device.name,
            device.desc.as_deref().unwrap_or("no description")
        );

        // Open the capture handle
        let mut cap = Capture::from_device(device)
            .map_err(|e| CaptureError::Pcap(format!("Failed to open device: {}", e)))?
            .promisc(false)      // Don't need promiscuous mode for our own traffic
            .snaplen(65535)      // Capture full packets
            .timeout(100)        // 100ms read timeout for non-blocking behavior
            .immediate_mode(true) // Low-latency capture (important for games!)
            .open()
            .map_err(|e| {
                if e.to_string().contains("permission") || e.to_string().contains("Permission") {
                    CaptureError::PermissionDenied
                } else {
                    CaptureError::Pcap(format!("Failed to start capture: {}", e))
                }
            })?;

        // Apply the BPF filter
        cap.filter(&filter.bpf, true)
            .map_err(|e| CaptureError::Pcap(format!("Failed to set BPF filter '{}': {}", filter.bpf, e)))?;

        tracing::info!("Pcap capture started successfully");
        self.capture = Some(cap);
        self.active = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        tracing::info!("Stopping pcap capture");
        self.capture = None;
        self.active = false;
        Ok(())
    }

    fn next_packet(&mut self) -> Result<CapturedPacket, CaptureError> {
        let cap = self.capture.as_mut()
            .ok_or_else(|| CaptureError::Pcap("Capture not active".into()))?;

        loop {
            let raw_packet = cap.next_packet()
                .map_err(|e| CaptureError::Pcap(format!("Capture read error: {}", e)))?;

            let data = raw_packet.data;

            // Need at least Ethernet + IPv4 + UDP headers
            if data.len() < MIN_PACKET_LEN {
                continue;
            }

            // ── Parse Ethernet header (14 bytes) ────────────────
            let eth_type = u16::from_be_bytes([data[12], data[13]]);
            if eth_type != ETHERTYPE_IPV4 {
                continue; // Not IPv4 — skip (could be ARP, IPv6, etc.)
            }

            // ── Parse IPv4 header ───────────────────────────────
            let ip_start = ETH_HEADER_LEN;
            let ip_version = (data[ip_start] >> 4) & 0x0F;
            if ip_version != 4 {
                continue; // Not IPv4
            }

            let ip_header_len = ((data[ip_start] & 0x0F) as usize) * 4;
            if ip_header_len < IPV4_MIN_HEADER_LEN {
                continue; // Invalid IP header length
            }

            let ip_protocol = data[ip_start + 9];
            if ip_protocol != IP_PROTO_UDP {
                continue; // Not UDP
            }

            // Extract source and destination IPs
            let src_ip = Ipv4Addr::new(
                data[ip_start + 12],
                data[ip_start + 13],
                data[ip_start + 14],
                data[ip_start + 15],
            );
            let dst_ip = Ipv4Addr::new(
                data[ip_start + 16],
                data[ip_start + 17],
                data[ip_start + 18],
                data[ip_start + 19],
            );

            // ── Parse UDP header (8 bytes) ──────────────────────
            let udp_start = ip_start + ip_header_len;
            if data.len() < udp_start + UDP_HEADER_LEN {
                continue; // Truncated packet
            }

            let src_port = u16::from_be_bytes([data[udp_start], data[udp_start + 1]]);
            let dst_port = u16::from_be_bytes([data[udp_start + 2], data[udp_start + 3]]);
            let udp_length = u16::from_be_bytes([data[udp_start + 4], data[udp_start + 5]]) as usize;

            // ── Extract UDP payload ─────────────────────────────
            let payload_start = udp_start + UDP_HEADER_LEN;
            let payload_end = (udp_start + udp_length).min(data.len());

            if payload_start >= payload_end {
                continue; // No payload
            }

            let payload = &data[payload_start..payload_end];

            // ── Build timestamp from pcap header ────────────────
            let timestamp_us = raw_packet.header.ts.tv_sec as u64 * 1_000_000
                + raw_packet.header.ts.tv_usec as u64;

            return Ok(CapturedPacket {
                payload: Bytes::copy_from_slice(payload),
                src: SocketAddrV4::new(src_ip, src_port),
                dst: SocketAddrV4::new(dst_ip, dst_port),
                timestamp_us,
            });
        }
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

/// Stub implementation when pcap-capture feature is not enabled.
#[cfg(not(feature = "pcap-capture"))]
impl PacketCapture for PcapCapture {
    fn start(&mut self, _filter: &CaptureFilter) -> Result<(), CaptureError> {
        Err(CaptureError::UnsupportedPlatform(
            "Packet capture requires the 'pcap-capture' feature. Rebuild with: cargo build --features pcap-capture".into()
        ))
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.active = false;
        Ok(())
    }

    fn next_packet(&mut self) -> Result<CapturedPacket, CaptureError> {
        Err(CaptureError::Pcap("Not implemented — pcap-capture feature not enabled".into()))
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

/// List available network interfaces using pcap.
///
/// Returns interface metadata suitable for display in --list-interfaces.
#[cfg(feature = "pcap-capture")]
pub fn list_pcap_interfaces() -> Vec<super::InterfaceInfo> {
    match Device::list() {
        Ok(devices) => {
            devices.into_iter().map(|d| {
                super::InterfaceInfo {
                    name: d.name,
                    description: d.desc.unwrap_or_default(),
                    is_up: d.flags.is_up(),
                    is_loopback: d.flags.is_loopback(),
                }
            }).collect()
        }
        Err(e) => {
            tracing::warn!("Failed to list pcap devices: {}", e);
            vec![]
        }
    }
}

#[cfg(not(feature = "pcap-capture"))]
pub fn list_pcap_interfaces() -> Vec<super::InterfaceInfo> {
    vec![]
}
