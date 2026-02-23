//! # Dota 2 Configuration
//!
//! Game-specific settings for Valve's Dota 2.

use super::GameConfig;

/// Dota 2 game configuration.
pub struct Dota2Config;

impl GameConfig for Dota2Config {
    fn name(&self) -> &str {
        "Dota 2"
    }

    fn process_names(&self) -> &[&str] {
        &["dota2.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Dota 2 uses similar ports to CS2 (Valve infrastructure)
        (27015, 27050)
    }

    fn redirect_port(&self) -> u16 {
        27015 // Standard Source engine game port
    }

    fn redirect_instructions(&self) -> String {
        "Dota 2 redirect mode:\n\
         1. Find your server IP from the Dota 2 console: `status`\n\
         2. Start LightSpeed: --game dota2 --game-server <SERVER_IP>:27015\n\
         3. Connect in Dota 2 console: `connect 127.0.0.1:27015`\n\
         4. Anti-cheat: VAC is compatible (unencrypted tunneling)\n\
         5. Note: Dota 2 primarily uses Steam Datagram Relay (SDR)"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "VAC (Valve Anti-Cheat)"
    }

    fn uses_sdr(&self) -> bool {
        true
    }

    fn typical_pps(&self) -> u32 {
        // 30 tick rate → ~30 packets/sec
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (50, 800)
    }
}
