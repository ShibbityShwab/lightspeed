//! # Pcap Capture Backend
//!
//! Cross-platform packet capture using the pcap crate (libpcap/Npcap).
//! This is the default backend for all platforms in the MVP.

use crate::error::CaptureError;
use crate::tunnel::capture::{CaptureFilter, CapturedPacket, PacketCapture};

/// Pcap-based packet capture.
pub struct PcapCapture {
    /// Whether capture is currently active.
    active: bool,
    /// The network interface to capture on.
    interface: Option<String>,
}

impl PcapCapture {
    /// Create a new pcap capture instance.
    pub fn new() -> Self {
        Self {
            active: false,
            interface: None,
        }
    }

    /// Set the network interface to capture on.
    pub fn with_interface(mut self, interface: &str) -> Self {
        self.interface = Some(interface.to_string());
        self
    }
}

impl PacketCapture for PcapCapture {
    fn start(&mut self, filter: &CaptureFilter) -> Result<(), CaptureError> {
        tracing::info!("Starting pcap capture with BPF filter: {}", filter.bpf);

        // TODO (WF-001 Step 2a): Initialize pcap capture
        // 1. Find or use specified network interface
        // 2. Open capture handle with pcap::Capture::from_device()
        // 3. Set promiscuous mode
        // 4. Apply BPF filter
        // 5. Start capture loop

        self.active = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        tracing::info!("Stopping pcap capture");
        self.active = false;
        Ok(())
    }

    fn next_packet(&mut self) -> Result<CapturedPacket, CaptureError> {
        if !self.active {
            return Err(CaptureError::Pcap("Capture not active".into()));
        }

        // TODO (WF-001 Step 2a): Read next packet from pcap handle
        // 1. Read raw packet from pcap
        // 2. Parse Ethernet → IP → UDP headers
        // 3. Extract source/dest addresses and payload
        // 4. Return CapturedPacket

        Err(CaptureError::Pcap("Not implemented yet".into()))
    }

    fn is_active(&self) -> bool {
        self.active
    }
}
