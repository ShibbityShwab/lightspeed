//! # Project Zomboid Configuration
//!
//! Game-specific settings for The Indie Stone's Project Zomboid.

use super::GameConfig;

/// Project Zomboid game configuration.
pub struct ZomboidConfig;

impl GameConfig for ZomboidConfig {
    fn name(&self) -> &str {
        "Project Zomboid"
    }

    fn process_names(&self) -> &[&str] {
        // Windows: ProjectZomboid64.exe (32-bit build: ProjectZomboid32.exe).
        // macOS/Linux (Steam Java/native wrapper): the exact `ps comm` name is
        // TBD — may surface as `ProjectZomboid`, `ProjectZomboid64`, or `java`.
        // "java" is intentionally omitted to avoid false positives; verify the
        // real process name at runtime on a Mac.
        &[
            "ProjectZomboid64.exe",
            "ProjectZomboid32.exe",
            "ProjectZomboid64",
            "ProjectZomboid",
        ]
    }

    fn ports(&self) -> (u16, u16) {
        // 16261 = default server / Steam query port, 16262 = direct connection.
        (16261, 16262)
    }

    fn anti_cheat(&self) -> &str {
        "None"
    }

    fn typical_pps(&self) -> u32 {
        // Cooperative multiplayer with a modest tick rate.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (40, 1400)
    }

    fn redirect_port(&self) -> u16 {
        16261
    }

    fn redirect_instructions(&self) -> String {
        "Project Zomboid redirect mode:\n\
         1. Find the server IP:port (server browser, or Join > direct IP)\n\
         2. Start LightSpeed: --game zomboid --server <SERVER_IP>:16261\n\
         3. Connect in-game to 127.0.0.1:16261\n\
         4. Anti-cheat: none (unencrypted tunneling is compatible)"
            .to_string()
    }
}
