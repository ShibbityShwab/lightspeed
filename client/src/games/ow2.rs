//! # Overwatch 2 Game Configuration
//!
//! Game-specific settings for Blizzard Entertainment's Overwatch 2 — the
//! 5v5 hero-shooter running on Blizzard's Battle.net infrastructure.
//!
//! ## Network Profile
//!
//! Overwatch 2 uses direct UDP connections to Blizzard's dedicated game
//! servers with no third-party relay layer. In-game traffic lands on Blizzard's
//! registered port ranges (3478–6250).  There is no Steam Datagram Relay, making
//! OW2 an excellent LightSpeed target for players with poor ISP routing to
//! Blizzard's data centres.
//!
//! ## Port Ranges
//!
//! | Range | Purpose |
//! |-------|---------|
//! | 3478–3479 | STUN (NAT traversal) |
//! | 5060, 5062 | SIP / voice negotiation |
//! | 3724 | Battle.net game traffic |
//! | 6250 | In-game UDP data |
//!
//! BPF capture filter covers 3478–6250 to catch all relevant paths.
//!
//! ## Anti-Cheat
//!
//! Overwatch 2 uses Blizzard's server-side anti-cheat ("Warden-style"
//! heuristics in battle.net's backend). No kernel-mode driver is required
//! on the client — LightSpeed's transparent UDP forwarding is fully compatible.

use super::GameConfig;

/// Overwatch 2 (Blizzard Entertainment) game configuration.
pub struct Ow2Config;

impl GameConfig for Ow2Config {
    fn name(&self) -> &str {
        "Overwatch 2"
    }

    fn process_names(&self) -> &[&str] {
        // Battle.net launches `Overwatch.exe` for the game client.
        // The launcher itself is `Battle.net.exe` — we do NOT capture that.
        &["Overwatch.exe", "Overwatch_retail.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Capture range covering all Blizzard UDP game-server ports:
        // STUN (3478-3479), SIP (5060/5062), Battle.net (3724), game data (6250).
        // Upper bound 6250 avoids capturing unrelated high-port traffic.
        (3478, 6250)
    }

    fn redirect_port(&self) -> u16 {
        // Battle.net game UDP primary port
        3724
    }

    fn redirect_instructions(&self) -> String {
        "Overwatch 2 redirect mode:\n\
         1. Start LightSpeed before launching OW2:\n\
            lightspeed --game ow2 --game-server <BLIZZARD_SERVER_IP>:3724\n\
         2. Accept the custom server in Battle.net settings if prompted\n\
         3. Blizzard's server-side anti-cheat is transparent to UDP routing\n\
         4. For best results, enable during pre-game lobby before match starts"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Blizzard Warden (server-side)"
    }

    fn uses_sdr(&self) -> bool {
        // Overwatch 2 uses Battle.net's own matchmaking; no Valve SDR.
        false
    }

    fn typical_pps(&self) -> u32 {
        // OW2 competitive servers run at 60 Hz tick.
        // Client sends ~60 position/ability updates/sec plus ACKs.
        // Plus ~20 pps inbound game-state packets.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Hero ability payloads: 32–512 bytes.
        // Full team state packets: up to 1200 bytes.
        (32, 1200)
    }
}
