//! # Linux Packet Capture (AF_PACKET)
//!
//! Linux-specific capture backend using AF_PACKET raw sockets.
//! Provides higher performance than pcap by avoiding the libpcap
//! overhead and using kernel-level packet filtering.
//!
//! For MVP, this delegates to the pcap backend (via libpcap).
//! AF_PACKET native implementation planned for Phase 2.

// TODO (Phase 2): Implement AF_PACKET-based capture
// - Use socket2 for raw socket creation
// - PACKET_MMAP ring buffer for zero-copy
// - BPF attached directly to socket
// - PACKET_FANOUT for multi-core scaling
