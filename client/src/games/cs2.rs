//! # Counter-Strike 2 Configuration
//!
//! Game-specific settings for Valve's Counter-Strike 2.

use super::GameConfig;

/// CS2 game configuration.
pub struct Cs2Config;

impl GameConfig for Cs2Config {
    fn name(&self) -> &str {
        "Counter-Strike 2"
    }

    fn process_names(&self) -> &[&str] {
        &["cs2.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // CS2 game servers typically use ports 27015-27050
        (27015, 27050)
    }

    fn anti_cheat(&self) -> &str {
        "VAC (Valve Anti-Cheat)"
    }

    fn uses_sdr(&self) -> bool {
        // CS2 may use Steam Datagram Relay for some connections
        true
    }

    fn typical_pps(&self) -> u32 {
        // 64-128 tick rate → 30-60 packets/sec
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (50, 1200)
    }
}
