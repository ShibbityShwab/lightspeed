//! # macOS Packet Capture (BPF)
//!
//! macOS-specific capture backend using Berkeley Packet Filter.
//!
//! For MVP, this delegates to the pcap backend (via libpcap).
//! Native BPF implementation may be explored in Phase 2.

// TODO (Phase 2): Evaluate native BPF on macOS
// - /dev/bpf device access
// - BPF filter programs
// - Performance comparison with pcap
