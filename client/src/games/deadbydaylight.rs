//! # Dead by Daylight Game Configuration
//!
//! Game-specific settings for Behaviour Interactive's Dead by Daylight — the
//! asymmetric 4v1 survival horror multiplayer game.
//!
//! ## Network Profile
//!
//! Dead by Daylight dedicated servers use direct UDP in the Steam
//! **27000–27050** range (no Steam Datagram Relay layer). The backend/party
//! services use UDP **4380**, and the Steam client's STUN/TURN uses
//! **3478** — neither of which is the game-server traffic LightSpeed routes.
//!
//! ## Anti-Cheat
//!
//! Dead by Daylight uses **Easy Anti-Cheat (EAC)**. LightSpeed's transparent
//! UDP forwarding does not inject code or modify memory, so it is compatible.

use super::GameConfig;

/// Dead by Daylight (Behaviour Interactive) game configuration.
pub struct DeadByDaylightConfig;

impl GameConfig for DeadByDaylightConfig {
    fn name(&self) -> &str {
        "Dead by Daylight"
    }

    fn process_names(&self) -> &[&str] {
        &["DeadByDaylight-Win64-Shipping.exe", "DeadByDaylight.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Dedicated-server traffic uses the Steam datagram UDP range.
        (27000, 27050)
    }

    fn redirect_instructions(&self) -> String {
        "Dead by Daylight redirect mode:\n\
         1. Start LightSpeed: --game deadbydaylight --game-server <SERVER_IP>:27000\n\
         2. Dedicated servers use direct UDP 27000-27050 — auto-detect works\n\
         3. Anti-cheat: EAC is compatible (transparent UDP)"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC)"
    }

    fn uses_sdr(&self) -> bool {
        // Direct UDP to dedicated servers (no Steam Datagram Relay layer).
        false
    }

    fn typical_pps(&self) -> u32 {
        // Fast-paced 60 Hz gameplay; ~30-60 packets/sec typical.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Small input/state snapshots up to MTU-sized syncs.
        (64, 1200)
    }
}
