//! # Bodycam Game Configuration
//!
//! Game-specific settings for Reissad Studio's Bodycam, the ultra-realistic
//! body-camera tactical FPS (Unreal Engine 5, Steam-only).
//!
//! ## Network Profile
//!
//! Bodycam is **peer-to-peer** (officially: "Bodycam's multiplayer runs peer
//! to peer") with **no dedicated servers**. Hosts run UE5 listen servers
//! advertised through Steam lobbies, and match traffic is carried over
//! Steam's P2P networking (the game ships the SteamCorePro Online Subsystem
//! plugin and `steam_api64.dll`). There is no published game-specific UDP
//! port; the range below covers Steam's datagram/P2P transport (UDP
//! 27000-27050, relay 27031-27036) rather than a fixed game-server port.
//! VOIP was rebuilt on Epic Online Services (EOS) in the v0.8 update.
//!
//! ## Anti-Cheat
//!
//! Bodycam ships **without** a client-side anti-cheat (no EAC, BattlEye, or
//! VAC) as of the v0.8 "Locked and Loaded" update (Sept 2026); anti-cheat
//! remains on the roadmap. LightSpeed's transparent UDP forwarding does not
//! inject code or modify memory, so it is fully compatible.

use super::GameConfig;

/// Bodycam (Reissad Studio) game configuration.
pub struct BodycamConfig;

impl GameConfig for BodycamConfig {
    fn name(&self) -> &str {
        "Bodycam"
    }

    fn process_names(&self) -> &[&str] {
        // `Bodycam-Win64-Shipping.exe` is the gameplay process; `Bodycam.exe`
        // is the small UE5 launcher/bootstrapper at the install root.
        // (Linux auto-detection handles the kernel's 15-char `comm` truncation
        // of the Shipping exe name automatically.)
        &["Bodycam-Win64-Shipping.exe", "Bodycam.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Peer-to-peer over Steam: no dedicated-server port. This range covers
        // Steam's P2P/datagram transport rather than a fixed game port.
        (27000, 27050)
    }

    fn redirect_instructions(&self) -> String {
        "Bodycam redirect mode:\n\
         1. Bodycam is peer-to-peer over Steam, so capture/intercept mode is\n\
            preferred; redirect only applies to a direct peer connection\n\
         2. Start LightSpeed: --game bodycam --game-server <PEER_IP>:27000\n\
         3. Anti-cheat: none, transparent UDP tunneling is fully compatible"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "None"
    }

    fn uses_sdr(&self) -> bool {
        // Steam-only P2P matchmaking over Steam networking / Datagram Relay.
        true
    }

    fn typical_pps(&self) -> u32 {
        // Fast-paced tactical shooter with a ~60 Hz network tick.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Small input/state snapshots up to MTU-sized syncs.
        (64, 1200)
    }
}
