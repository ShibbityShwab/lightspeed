//! # World of Tanks Game Configuration
//!
//! Game-specific settings for Wargaming's World of Tanks — the team-based
//! tank-combat MMO.
//!
//! ## Network Profile
//!
//! World of Tanks uses UDP **12000–29999** for game traffic, plus a set of
//! secondary ports: **32800–32900**, **3432**, **3478–3479**, **5060–5062**,
//! and **30443**. Clients connect directly to Wargaming's regional cluster
//! servers with no relay layer.
//!
//! ## Anti-Cheat
//!
//! World of Tanks ships without a client-side anti-cheat; server-side
//! behavioural detection is used instead. LightSpeed's transparent UDP
//! forwarding is fully compatible.

use super::GameConfig;

/// World of Tanks (Wargaming) game configuration.
pub struct WotConfig;

impl GameConfig for WotConfig {
    fn name(&self) -> &str {
        "World of Tanks"
    }

    fn process_names(&self) -> &[&str] {
        &["WorldOfTanks.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Primary game range 12000-29999. Secondary ports 32800-32900,
        // 3432, 3478-3479, 5060-5062, and 30443 are used for voice,
        // NAT traversal, and backend services.
        (12000, 29999)
    }

    fn redirect_instructions(&self) -> String {
        "World of Tanks redirect mode:\n\
         1. Find your battle server IP from the in-game ping indicator\n\
         2. Start LightSpeed: --game wot --game-server <SERVER_IP>:12000\n\
         3. Anti-cheat: none (server-side detection) — transparent UDP\n\
            tunneling is fully compatible"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "None"
    }

    fn typical_pps(&self) -> u32 {
        // Server runs at ~30 Hz; the client sends roughly 10-20
        // packets/sec with bursts during artillery/spotting events.
        20
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (64, 1200)
    }
}
