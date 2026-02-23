//! # Fortnite Configuration
//!
//! Game-specific settings for Epic Games' Fortnite.

use super::GameConfig;

/// Fortnite game configuration.
pub struct FortniteConfig;

impl GameConfig for FortniteConfig {
    fn name(&self) -> &str {
        "Fortnite"
    }

    fn process_names(&self) -> &[&str] {
        &["FortniteClient-Win64-Shipping.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Fortnite uses ephemeral UDP ports (game client) to connect to
        // AWS-hosted game servers. Server ports are typically 7000-9000.
        (7000, 9000)
    }

    fn redirect_port(&self) -> u16 {
        7777 // Common Fortnite/Unreal Engine game port
    }

    fn redirect_instructions(&self) -> String {
        "Fortnite redirect mode:\n\
         1. Start LightSpeed with: --game fortnite --game-server <FORTNITE_SERVER_IP>:7777\n\
         2. Fortnite connects to dynamic AWS servers — use the --game-server flag\n\
         3. Anti-cheat: EAC is compatible (unencrypted tunneling, no IP masking)\n\
         4. Note: Fortnite uses ephemeral server IPs; capture mode is preferred"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "EasyAntiCheat (EAC)"
    }

    fn typical_pps(&self) -> u32 {
        // ~20-60 packets/sec depending on game state
        40
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (100, 500)
    }
}
