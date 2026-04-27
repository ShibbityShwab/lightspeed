//! # Rust (Facepunch) Game Configuration
//!
//! Game-specific settings for Facepunch Studios' Rust — the open-world
//! survival multiplayer game.
//!
//! > **Note:** This profile is for the *game* Rust (by Facepunch), not the
//! > Rust programming language that LightSpeed is written in. Yes, we see the
//! > irony.
//!
//! ## Network Profile
//!
//! Rust uses direct UDP connections to community and official servers on the
//! default port **28015**. There is no Steam Datagram Relay (SDR) — every
//! session is a raw connection to the server's public IP, making Rust an
//! ideal candidate for LightSpeed's proxy optimisation.
//!
//! ## Anti-Cheat
//!
//! Rust uses two layers:
//! - **Easy Anti-Cheat (EAC)** — kernel-level driver, validates the game binary.
//! - **Facepunch Anti-Hack (Rust+)** — server-side behavioural detection.
//!
//! LightSpeed operates as a transparent UDP forwarder with no code injection
//! or memory modification, so it is compatible with both systems.

use super::GameConfig;

/// Rust (Facepunch) game configuration.
pub struct RustConfig;

impl GameConfig for RustConfig {
    fn name(&self) -> &str {
        "Rust"
    }

    fn process_names(&self) -> &[&str] {
        &["RustClient.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Default Rust server port is 28015 (UDP game traffic).
        // RCON is 28016 (TCP, not captured), query is 28017 (UDP).
        // We capture the game port range: 28015–28017.
        (28015, 28017)
    }

    fn redirect_port(&self) -> u16 {
        28015 // Default Rust game server port
    }

    fn redirect_instructions(&self) -> String {
        "Rust redirect mode:\n\
         1. Find your server IP from the F1 console or server browser\n\
         2. Start LightSpeed: --game rust --game-server <SERVER_IP>:28015\n\
         3. Connect in the F1 console: `client.connect 127.0.0.1:28015`\n\
         4. Anti-cheat: EAC + Facepunch Anti-Hack are compatible (transparent UDP tunnel)\n\
         5. Note: Rust uses direct UDP — no Steam Datagram Relay, ideal for proxying"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC) + Facepunch Anti-Hack"
    }

    fn uses_sdr(&self) -> bool {
        // Rust uses direct UDP to community/official servers — no SDR.
        false
    }

    fn typical_pps(&self) -> u32 {
        // Rust runs at 16-30 server tick depending on config.
        // Client sends ~20-30 packets/sec; server sends ~20-60 depending on
        // proximity to other players and active events.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Rust packets range from small position updates to larger state syncs.
        (64, 1200)
    }
}
