//! # Windows Packet Capture (WFP)
//!
//! Windows-specific capture backend using Windows Filtering Platform.
//! This provides better integration with Windows networking stack
//! and avoids the Npcap dependency for advanced deployments.
//!
//! For MVP, this delegates to the pcap backend (via Npcap).
//! WFP native implementation planned for Phase 2.

// TODO (Phase 2): Implement WFP-based capture
// - Use windows-rs crate for WFP API
// - Register callout driver for UDP intercept
// - Zero-copy packet path
// - Better anti-cheat compatibility than raw pcap
