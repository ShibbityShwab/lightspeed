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
