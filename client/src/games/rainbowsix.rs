//! # Rainbow Six Siege Game Configuration
//!
//! Game-specific settings for Ubisoft's Rainbow Six Siege, the 5v5 tactical
//! shooter running on Ubisoft's AnvilNext engine.
//!
//! ## Network Profile
//!
//! Siege connects directly to Ubisoft's dedicated match servers over UDP on
//! **10000-10099**. There is no peer-to-peer matchmaking relay: every match
//! talks straight to a regional Ubisoft data centre, which makes Siege a good
//! LightSpeed candidate for players whose ISP routes poorly to those centres.
//!
//! ## Anti-Cheat
//!
//! Three layers ship with the game: **BattlEye** (user-mode module plus a
//! kernel driver), **R6 Shieldguard**, Ubisoft's kernel-level protection, which
//! refuses to start unless Secure Boot, HVCI and a TPM 2.0 are all present, and
//! Ubisoft's account-level anti-cheat.
//!
//! LightSpeed is a transparent UDP forwarder: it does not inject code, open the
//! game process, or touch game memory, so it sits outside what those systems
//! look for. What is **UNVERIFIED** is whether Shieldguard's launch-time
//! environment checks tolerate a local forwarding agent at all: it inspects
//! the boot chain rather than the network path, so the risk is indirect rather
//! than a detection. Treat this profile as untested against Shieldguard until
//! someone reports a real match through it.

use super::GameConfig;

/// Rainbow Six Siege (Ubisoft) game configuration.
pub struct RainbowSixConfig;

impl GameConfig for RainbowSixConfig {
    fn name(&self) -> &str {
        "Rainbow Six Siege"
    }

    fn process_names(&self) -> &[&str] {
        // `RainbowSix.exe` is the shipping binary. BattlEye games commonly
        // launch a `_BE` variant that owns the protected context, and Siege
        // does the same, so match both.
        &["RainbowSix.exe", "RainbowSix_BE.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Client-side match traffic, per Ubisoft's published port list as
        // mirrored by portforward.com. Voice and Ubisoft Connect traffic use
        // separate ranges and are not captured.
        (10000, 10099)
    }

    fn redirect_port(&self) -> u16 {
        10000
    }

    fn redirect_instructions(&self) -> String {
        "Rainbow Six Siege redirect mode:\n\
         1. Find your match server IP from the in-game scoreboard or a tool\n\
            like Resource Monitor while in a match\n\
         2. Start LightSpeed: --game rainbowsix --game-server <SERVER_IP>:10000\n\
         3. Anti-cheat: BattlEye and R6 Shieldguard. LightSpeed does not\n\
            modify game memory or inject drivers, but Shieldguard's Secure\n\
            Boot/HVCI requirements make this profile UNVERIFIED; report back\n\
            if you run matches through it"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "BattlEye + R6 Shieldguard"
    }

    fn uses_sdr(&self) -> bool {
        // No Steam Datagram Relay: direct UDP to Ubisoft data centres.
        false
    }

    fn typical_pps(&self) -> u32 {
        // Siege servers tick at 60 Hz; the client sends roughly one
        // position/action update per tick, coalescing to ~60 pps.
        60
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Movement and action updates: 40-200 bytes.
        // Round-start snapshots and destruction events: up to ~1200 bytes.
        (40, 1200)
    }
}
