//! # PUBG: Battlegrounds Game Configuration
//!
//! Game-specific settings for Krafton's PUBG: Battlegrounds — the defining
//! battle-royale running on dedicated Unreal Engine servers worldwide.
//!
//! ## Network Profile
//!
//! PUBG uses direct UDP connections to Krafton-operated dedicated game servers,
//! with per-match server instances assigned dynamically. Primary UDP game-traffic
//! ports are **7000–7999** (intra-region) and **17000–17999** (cross-region play).
//! There is no relay layer, making PUBG an excellent LightSpeed target — players
//! in SEA with poor routing to US servers (e.g., PUBG global servers) see
//! 40–80ms+ extra latency from suboptimal BGP paths.
//!
//! ## Anti-Cheat
//!
//! PUBG ships with **BattlEye** — a kernel-mode anti-cheat that monitors
//! driver/memory modifications and injects into the game process.  LightSpeed
//! operates purely via transparent UDP socket forwarding with no driver
//! installation or memory access — fully compatible with BattlEye's threat model.
//!
//! ## Server Regions
//!
//! | Region | Server location | Typical port |
//! |--------|----------------|-------------|
//! | AS     | Singapore (AWS) | 7000–7999  |
//! | AS     | Tokyo (AWS)     | 7000–7999  |
//! | NA     | Virginia (AWS)  | 7000–7999  |
//! | EU     | Frankfurt (AWS) | 7000–7999  |
//! | SA     | São Paulo       | 7000–7999  |

use super::GameConfig;

/// PUBG: Battlegrounds (Krafton) game configuration.
pub struct PubgConfig;

impl GameConfig for PubgConfig {
    fn name(&self) -> &str {
        "PUBG: Battlegrounds"
    }

    fn process_names(&self) -> &[&str] {
        // PUBG's Unreal Engine 4 executable — the game process name has
        // remained `TslGame.exe` since early access (TSL = The Squad-based
        // Last-player-standing game, Unreal's UE4 project codename).
        &["TslGame.exe", "PUBG.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Primary UDP range covering both intra-region (7000-7999) and
        // cross-region (17000-17999) traffic.
        // Using 7000-17999 captures all known PUBG UDP game-server paths.
        (7000, 17999)
    }

    fn redirect_port(&self) -> u16 {
        // PUBG primary game-server UDP base port
        7777
    }

    fn redirect_instructions(&self) -> String {
        "PUBG: Battlegrounds redirect mode:\n\
         1. Start LightSpeed before launching PUBG:\n\
            lightspeed --game pubg --game-server <KRAFTON_SERVER_IP>:7777\n\
         2. Your match server IP appears in:\n\
            %APPDATA%\\..\\Local\\TslGame\\Saved\\Logs\\TslGame.log\n\
            (search for 'BeaconNetDriver' or 'LogNet' entries)\n\
         3. BattlEye is fully compatible — LightSpeed uses no drivers or\n\
            memory access, only transparent UDP socket forwarding"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "BattlEye (kernel-mode)"
    }

    fn uses_sdr(&self) -> bool {
        // No Valve SDR — Krafton operates its own AWS-based game servers.
        false
    }

    fn typical_pps(&self) -> u32 {
        // PUBG runs at ~30 Hz server tick (up to 60 Hz in some modes).
        // Client sends ~30–60 position/action packets/sec.
        // Early circles have lower data rates; final circles are higher.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Player position/gear updates: 60–400 bytes.
        // Zone-state and vehicle packets: up to 1400 bytes.
        (60, 1400)
    }
}
