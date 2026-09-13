//! # Roblox Game Configuration
//!
//! Game-specific settings for Roblox Corporation's Roblox platform client.
//!
//! ## Network Profile
//!
//! The Roblox client (`RobloxPlayerBeta.exe`) selects its outbound UDP source
//! port from the high ephemeral range **49152–65535** when connecting to a
//! game server instance. There is no single fixed server port; the capture
//! range below spans the full high ephemeral band the client uses.
//!
//! ## Anti-Cheat
//!
//! Roblox ships **Byfron (Hyperion)**, a user-mode anti-cheat. LightSpeed's
//! transparent UDP forwarding does not inject code or modify memory, so it is
//! fully compatible.

use super::GameConfig;

/// Roblox (Roblox Corporation) game configuration.
pub struct RobloxConfig;

impl GameConfig for RobloxConfig {
    fn name(&self) -> &str {
        "Roblox"
    }

    fn process_names(&self) -> &[&str] {
        &["RobloxPlayerBeta.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // The client picks its outbound UDP source port from the high
        // ephemeral range; no single fixed server port.
        (49152, 65535)
    }

    fn redirect_instructions(&self) -> String {
        "Roblox redirect mode:\n\
         1. Roblox selects a high ephemeral source port per server instance,\n\
            so capture/intercept mode is preferred over redirect\n\
         2. Start LightSpeed: --game roblox --game-server <SERVER_IP>:PORT\n\
         3. Anti-cheat: Byfron (Hyperion) — transparent UDP tunneling is safe"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Byfron (Hyperion)"
    }

    fn typical_pps(&self) -> u32 {
        // ~60 Hz physics/state tick with position and input updates.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Small input/state snapshots up to MTU-sized syncs.
        (64, 1200)
    }
}
