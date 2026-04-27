//! # Valorant Game Configuration
//!
//! Game-specific settings for Riot Games' Valorant — the 5v5 tactical
//! first-person shooter built on Riot's Vanguard infrastructure.
//!
//! ## Network Profile
//!
//! Valorant uses direct UDP connections to Riot's dedicated game servers.
//! Game traffic runs on UDP ports **7000–7500**. Unlike Valve games there
//! is no Steam Datagram Relay; every match connects directly to a regional
//! server, making Valorant an excellent LightSpeed candidate for players
//! whose ISP routes poorly to Riot's data centres.
//!
//! ## Anti-Cheat
//!
//! Valorant ships with **Riot Vanguard** — a kernel-mode anti-cheat driver
//! that loads at boot. LightSpeed operates as a transparent UDP forwarder
//! with zero code injection or memory modification, which is compatible
//! with Vanguard's threat model (it targets driver/memory tampering, not
//! transparent network routing).
//!
//! ## Regions
//!
//! | Region | Server prefix |
//! |--------|---------------|
//! | NA     | `na.` (AWS US-East) |
//! | EU     | `eu.` (AWS EU-West) |
//! | AP     | `ap.` (AWS AP-Southeast) |
//! | BR     | `br.` (AWS SA-East) |
//! | KR     | `kr.` (AWS AP-Northeast) |
//! | LATAM  | `latam.` |
//!
//! Servers are AWS-hosted at dynamic IPs — LightSpeed captures by port
//! range rather than destination IP.

use super::GameConfig;

/// Valorant (Riot Games) game configuration.
pub struct ValorantConfig;

impl GameConfig for ValorantConfig {
    fn name(&self) -> &str {
        "Valorant"
    }

    fn process_names(&self) -> &[&str] {
        // Main game process — Riot client (VALORANT.exe) launches the
        // shipping binary; both names appear in tasklist.
        &["VALORANT-Win64-Shipping.exe", "VALORANT.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Valorant game-server traffic: UDP 7000–7500.
        // The Riot client itself uses 2000-2100 for launcher comms (TCP),
        // which we do not capture.
        (7000, 7500)
    }

    fn redirect_port(&self) -> u16 {
        7000
    }

    fn redirect_instructions(&self) -> String {
        "Valorant redirect mode:\n\
         1. Find your match server IP from the Windows Event Viewer or\n\
            a tool like Rivatuner Statistics Server while in a match\n\
         2. Start LightSpeed: --game valorant --game-server <SERVER_IP>:7000\n\
         3. Riot Vanguard is compatible — LightSpeed does not modify game\n\
            memory or inject drivers; it only reroutes UDP packets\n\
         4. Note: Valorant uses direct UDP (no SDR), ideal for proxying"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Riot Vanguard (kernel-mode)"
    }

    fn uses_sdr(&self) -> bool {
        // No Steam Datagram Relay — Valorant connects directly to Riot servers.
        false
    }

    fn typical_pps(&self) -> u32 {
        // Valorant runs at 128 tick on ranked/competitive servers.
        // Client sends ~128 update packets/sec; receives similar rate.
        // Coalesces to ~60-128 pps depending on movement activity.
        64
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Position/ability updates: 40–200 bytes.
        // Full-state snapshots: up to 1200 bytes.
        (40, 1200)
    }
}
