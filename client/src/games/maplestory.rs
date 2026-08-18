//! # MapleStory Game Configuration
//!
//! Game-specific settings for Nexon's MapleStory — the long-running 2D
//! side-scrolling MMORPG.
//!
//! ## Network Profile
//!
//! MapleStory game traffic runs over UDP on the login/chat port **8484**
//! and the channel-server range **7575–7615**. Clients connect directly to
//! Nexon's regional channel servers — there is no relay layer, which makes
//! the game a good candidate for LightSpeed proxying.
//!
//! ## Anti-Cheat
//!
//! MapleStory uses **BlackCipher** (Nexon Game Security, NGS), a user-mode
//! anti-tamper module that validates the game client. LightSpeed is a
//! transparent UDP forwarder with no injection, so it is compatible with
//! NGS's detection model.
//!
//! ## Platforms
//!
//! Windows and macOS clients are supported by this profile. (The macOS
//! client shares the same network profile.)

use super::GameConfig;

/// MapleStory (Nexon) game configuration.
pub struct MapleStoryConfig;

impl GameConfig for MapleStoryConfig {
    fn name(&self) -> &str {
        "MapleStory"
    }

    fn process_names(&self) -> &[&str] {
        &["MapleStory.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Channel servers listen on 7575-7615; login/chat uses 8484.
        (7575, 8484)
    }

    fn redirect_port(&self) -> u16 {
        8484 // Default MapleStory login/game port
    }

    fn redirect_instructions(&self) -> String {
        "MapleStory redirect mode:\n\
         1. Start LightSpeed: --game maplestory --game-server <SERVER_IP>:8484\n\
         2. Nexon assigns channel servers per region — redirect mode works\n\
            best for the login/chat connection on port 8484\n\
         3. Anti-cheat: BlackCipher (NGS) is compatible — LightSpeed only\n\
            reroutes UDP, it does not inject or modify game memory"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "BlackCipher / Nexon Game Security (NGS)"
    }

    fn typical_pps(&self) -> u32 {
        // 2D game with a low tick rate — roughly 10-20 packets/sec during
        // normal play, spiking during mob-heavy maps and events.
        20
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (64, 1200)
    }
}
