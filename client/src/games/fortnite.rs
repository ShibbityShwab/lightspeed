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
        // AWS-hosted game servers. Server ports are dynamic.
        (1024, 65535)
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
