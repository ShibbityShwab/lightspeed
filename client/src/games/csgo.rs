//! # Counter-Strike: Global Offensive (Legacy) Configuration
//!
//! Game-specific settings for the standalone CS:GO Legacy client
//! (Steam App ID 4465480), the pre-CS2 2012 build that Valve re-released
//! for legacy/community play. Distinct from CS2 (`cs2.exe`), this build
//! runs `csgo.exe` but shares the same Source-engine networking stack.

use super::GameConfig;

/// CS:GO Legacy game configuration.
pub struct CsgoConfig;

impl GameConfig for CsgoConfig {
    fn name(&self) -> &str {
        "Counter-Strike: Global Offensive (Legacy)"
    }

    fn process_names(&self) -> &[&str] {
        &["csgo.exe", "csgo"]
    }

    fn ports(&self) -> (u16, u16) {
        // Source-engine servers listen on 27015+; the client uses an
        // ephemeral source port. 26900 is also used by some builds.
        (27000, 27050)
    }

    fn redirect_port(&self) -> u16 {
        27015 // Standard Source engine game port
    }

    fn redirect_instructions(&self) -> String {
        "CS:GO Legacy redirect mode:\n\
         1. Find your server IP from the console: `status`\n\
         2. Start LightSpeed: --game csgo --game-server <SERVER_IP>:27015\n\
         3. Connect in console: `connect 127.0.0.1:27015`\n\
         4. Anti-cheat: VAC is compatible (unencrypted tunneling)\n\
         5. Note: CS:GO Legacy may use Steam Datagram Relay (SDR)"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "VAC (Valve Anti-Cheat)"
    }

    fn uses_sdr(&self) -> bool {
        true
    }

    fn typical_pps(&self) -> u32 {
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (50, 1200)
    }
}
