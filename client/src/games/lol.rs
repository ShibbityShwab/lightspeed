//! # League of Legends Game Configuration
//!
//! Game-specific settings for Riot Games' League of Legends — a 5v5 MOBA
//! running on Riot's dedicated global server infrastructure.
//!
//! ## Network Profile
//!
//! League of Legends uses direct UDP connections to Riot's game servers on
//! ports **5000–5500**. There is no relay layer (no Steam Datagram Relay, no
//! Blizzard Battle.net relay) — clients connect directly to regional game
//! servers, making LoL an excellent LightSpeed candidate for SEA and OCE
//! players whose ISP routes poorly to Riot's data centres.
//!
//! ## Anti-Cheat
//!
//! LoL is progressively rolling out **Riot Vanguard** (same kernel-mode
//! anti-cheat as Valorant) globally after the 2024 expansion.  Regions may
//! run the older Lua-based client-side check or Vanguard depending on the
//! patch version.  LightSpeed performs transparent UDP forwarding with zero
//! memory access or driver installation — compatible with both systems.
//!
//! ## Regions & Servers
//!
//! | Region | Abbreviation | Data centre |
//! |--------|-------------|-------------|
//! | NA     | NA1 | Chicago / Oregon (AWS) |
//! | EUW    | EUW1 | Amsterdam (AWS) |
//! | EUNE   | EUN1 | Frankfurt (AWS) |
//! | SEA    | SG2  | Singapore |
//! | OCE    | OC1  | Sydney |
//! | BR     | BR1  | São Paulo |
//! | KR     | KR   | Seoul |
//!
//! All regions use direct UDP on port 5000-5500 range.

use super::GameConfig;

/// League of Legends (Riot Games) game configuration.
pub struct LolConfig;

impl GameConfig for LolConfig {
    fn name(&self) -> &str {
        "League of Legends"
    }

    fn process_names(&self) -> &[&str] {
        // Main game client process — launched by the Riot client launcher.
        &[
            "League of Legends.exe",
            "LeagueOfLegends.exe", // Alternative capitalisation seen on some installs
        ]
    }

    fn ports(&self) -> (u16, u16) {
        // LoL game-server UDP traffic: 5000–5500.
        // The Riot launcher uses TCP 2099 + HTTP — not captured.
        (5000, 5500)
    }

    fn redirect_port(&self) -> u16 {
        // LoL game servers receive client packets on 5000.
        5000
    }

    fn redirect_instructions(&self) -> String {
        "League of Legends redirect mode:\n\
         1. Start LightSpeed before entering champion select:\n\
            lightspeed --game lol --game-server <RIOT_SERVER_IP>:5000\n\
         2. The Riot launcher may show a yellow indicator — this is safe;\n\
            LightSpeed redirects UDP game traffic only, not the launcher\n\
         3. Riot Vanguard is compatible — no memory/driver access by LightSpeed\n\
         4. Your server IP appears in the game log:\n\
            %USERPROFILE%\\AppData\\Local\\Riot Games\\League of Legends\\Logs"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Riot Vanguard (kernel-mode, rolled out 2024+)"
    }

    fn uses_sdr(&self) -> bool {
        // No Valve SDR — Riot operates its own dedicated game servers.
        false
    }

    fn typical_pps(&self) -> u32 {
        // LoL runs at ~30 Hz server tick rate.
        // Client sends ~30 action packets/sec + ACKs.
        // Inbound game state at similar rate.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Champion position/ability packets: 50–600 bytes.
        // Full-state delta snapshots: up to 1200 bytes on team fights.
        (50, 1200)
    }
}
