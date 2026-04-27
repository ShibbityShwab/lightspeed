//! # Apex Legends Game Configuration
//!
//! Game-specific settings for Respawn Entertainment's Apex Legends — the
//! free-to-play battle-royale built on a modified Source-engine networking
//! stack hosted on EA's infrastructure.
//!
//! ## Network Profile
//!
//! Apex uses a custom UDP protocol (derived from Valve's Source engine
//! `netchan`) to dedicated EA data-centre servers. Game traffic runs on
//! UDP port **37015** (the server's fixed listen port). The client uses
//! an ephemeral source port for each session.
//!
//! There is no Steam Datagram Relay; every match is a direct UDP connection
//! to the assigned regional server, which makes Apex an ideal candidate
//! for LightSpeed proxying.
//!
//! ## Anti-Cheat
//!
//! Apex Legends uses **Easy Anti-Cheat (EAC)** — the same kernel-level
//! driver used by Rust (Facepunch) and Fortnite. LightSpeed operates as a
//! transparent UDP forwarder with no code injection or memory modification,
//! which is fully compatible with EAC's detection model.
//!
//! ## Regions
//!
//! EA hosts Apex servers in US-East, US-West, Europe (Frankfurt), Asia
//! (Tokyo, Singapore), South America, and Oceania. The matchmaker assigns
//! a server IP at runtime; LightSpeed captures all outbound traffic to
//! port 37015 regardless of destination IP.

use super::GameConfig;

/// Apex Legends (Respawn / EA) game configuration.
pub struct ApexConfig;

impl GameConfig for ApexConfig {
    fn name(&self) -> &str {
        "Apex Legends"
    }

    fn process_names(&self) -> &[&str] {
        // Windows shipping binary.  The EA Desktop launcher also spawns
        // `EADesktop.exe` and `EABackgroundService.exe` but those are
        // launcher-only and do not handle game traffic.
        &["r5apex.exe", "r5apex_dx12.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Apex servers listen on UDP 37015.  Capture a small range around
        // it to handle any alternative server ports used in some regions.
        (37000, 37050)
    }

    fn redirect_port(&self) -> u16 {
        37015 // Default Apex Legends server port
    }

    fn redirect_instructions(&self) -> String {
        "Apex Legends redirect mode:\n\
         1. Find your match server IP from the in-game network stats\n\
            (Settings → Gameplay → display network info)\n\
         2. Start LightSpeed: --game apex --game-server <SERVER_IP>:37015\n\
         3. Easy Anti-Cheat is compatible — LightSpeed only reroutes UDP,\n\
            it does not touch game memory or inject code\n\
         4. Note: Apex uses direct UDP to EA servers — no relay layer,\n\
            ideal for LightSpeed proxying"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC)"
    }

    fn uses_sdr(&self) -> bool {
        // No Steam Datagram Relay — Apex is on EA's own infrastructure.
        false
    }

    fn typical_pps(&self) -> u32 {
        // Apex servers run at 20 tick (60 Hz from Season 19+
        // on high-performance servers).  Client sends ~20-60 pps.
        // Round up for burst periods (taking damage, reviving).
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Small movement/input updates: ~50 bytes.
        // Large world-state deltas: up to 1400 bytes (near MTU).
        (50, 1400)
    }
}
