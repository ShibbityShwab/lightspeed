//! # Rocket League Game Configuration
//!
//! Game-specific settings for Psyonix/Epic Games' Rocket League — the
//! vehicular-soccer multiplayer game.
//!
//! ## Network Profile
//!
//! Rocket League game servers listen on UDP **7000–9000** (the original
//! Psyonix range); Steam builds additionally use **27000–27030** for Steam
//! Datagram Relay (SDR) connections.
//!
//! ## Anti-Cheat
//!
//! Rocket League uses **Easy Anti-Cheat (EAC)** with **Epic Online
//! Services**. LightSpeed's transparent UDP forwarding does not inject
//! code or modify memory, so it is compatible with both systems.

use super::GameConfig;

/// Rocket League (Psyonix / Epic Games) game configuration.
pub struct RocketLeagueConfig;

impl GameConfig for RocketLeagueConfig {
    fn name(&self) -> &str {
        "Rocket League"
    }

    fn process_names(&self) -> &[&str] {
        &["RocketLeague.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Game servers use 7000-9000 (UDP). Steam builds also use
        // 27000-27030 for SDR relay — the capture range covers the
        // primary server range.
        (7000, 9000)
    }

    fn redirect_instructions(&self) -> String {
        "Rocket League redirect mode:\n\
         1. Start LightSpeed: --game rocketleague --game-server <SERVER_IP>:7000\n\
         2. Rocket League uses Steam Datagram Relay (SDR) — capture mode\n\
            is preferred; redirect only works for direct server connections\n\
         3. Anti-cheat: EAC + Epic Online Services are compatible (transparent UDP)"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC) / Epic Online Services"
    }

    fn uses_sdr(&self) -> bool {
        // Rocket League routes matchmaking traffic through Steam
        // Datagram Relay on Steam builds.
        true
    }

    fn typical_pps(&self) -> u32 {
        // 120 Hz physics simulation; the client sends ~30-60 packets/sec.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Small physics/input snapshots (~60 bytes) up to MTU-sized
        // state syncs.
        (64, 1200)
    }
}
