//! # Genshin Impact Game Configuration
//!
//! Game-specific settings for HoYoverse's Genshin Impact — the open-world
//! action RPG.
//!
//! ## Network Profile
//!
//! Genshin Impact uses UDP on two disjoint ranges: **22101–22102** and
//! port **42472**. The capture range spans both endpoints. Clients connect
//! directly to HoYoverse's regional game servers with no relay layer.
//!
//! ## Anti-Cheat
//!
//! The PC client ships without a client-side anti-cheat (mhyprot2 was
//! retired). LightSpeed's transparent UDP forwarding is fully compatible.

use super::GameConfig;

/// Genshin Impact (HoYoverse) game configuration.
pub struct GenshinConfig;

impl GameConfig for GenshinConfig {
    fn name(&self) -> &str {
        "Genshin Impact"
    }

    fn process_names(&self) -> &[&str] {
        &["GenshinImpact.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Two disjoint UDP ranges: 22101-22102 and 42472.
        // The capture range spans both endpoints.
        (22101, 42472)
    }

    fn redirect_instructions(&self) -> String {
        "Genshin Impact redirect mode:\n\
         1. Select your region in the launcher, then find the server IP\n\
            from your firewall/log while connected\n\
         2. Start LightSpeed: --game genshin --game-server <SERVER_IP>:22101\n\
         3. Anti-cheat: none — LightSpeed's transparent tunnel is safe"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "None"
    }

    fn typical_pps(&self) -> u32 {
        // Mostly position/action updates at a moderate tick rate;
        // ~15-25 packets/sec during combat and co-op play.
        25
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (64, 1200)
    }
}
