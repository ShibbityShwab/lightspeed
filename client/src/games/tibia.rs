//! # Tibia Game Configuration
//!
//! Game-specific settings for CipSoft's Tibia, the long-running 2D MMORPG,
//! requested in issue #171.
//!
//! ## Network Profile
//!
//! The client connects to the login server on **7171** and then to a game
//! world on **7172**; the request supplies that pair as the UDP range. The
//! player picks a world rather than being assigned one by a matchmaker, so the
//! profile keeps the default `dynamic_server()` of `false` and the interceptor
//! can lock onto the world it detected.
//!
//! ## Process Names
//!
//! `Tibia.exe` is the client and `TibiaLauncher.exe` the launcher that exits
//! once the game is running. The request also mentions a `client.exe` in newer
//! installs, which is deliberately **not** in this profile: `client.exe` is a
//! name dozens of unrelated applications use, and matching on it would label
//! the wrong process as Tibia. See the note in issue #171.
//!
//! ## Anti-Cheat
//!
//! Tibia ships **BattlEye**. LightSpeed forwards UDP transparently without
//! injecting code or touching memory, so the tunnel does not interact with it.

use super::GameConfig;

/// Tibia (CipSoft) game configuration.
pub struct TibiaConfig;

impl GameConfig for TibiaConfig {
    fn name(&self) -> &str {
        "Tibia"
    }

    fn process_names(&self) -> Vec<&str> {
        ["Tibia.exe", "TibiaLauncher.exe"].to_vec()
    }

    fn ports(&self) -> (u16, u16) {
        // 7171 is the login server, 7172 the game world the character enters.
        (7171, 7172)
    }

    fn anti_cheat(&self) -> &str {
        "BattlEye"
    }

    fn typical_pps(&self) -> u32 {
        // Estimate for a tile-based MMO at its walk/combat tick. Not measured.
        20
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (48, 900)
    }

    fn redirect_instructions(&self) -> String {
        "Tibia redirect mode:\n\
         1. The client talks UDP to the login server on 7171 and the game world on 7172\n\
         2. Start LightSpeed: --game tibia --game-server <SERVER_IP>:7172\n\
         3. Anti-cheat: BattlEye; transparent UDP tunneling only, no packet modification"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tibia_ports_cover_login_and_world() {
        assert_eq!(TibiaConfig.ports(), (7171, 7172));
    }

    #[test]
    fn tibia_capture_filter_names_both_ports() {
        let filter = TibiaConfig.build_capture_filter();
        assert!(filter.bpf.starts_with("udp"), "got {:?}", filter.bpf);
        assert!(filter.bpf.contains("7171"), "got {:?}", filter.bpf);
        assert!(filter.bpf.contains("7172"), "got {:?}", filter.bpf);
    }

    #[test]
    fn tibia_does_not_match_the_generic_client_exe() {
        let names = TibiaConfig.process_names();
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("Tibia.exe")));
        assert!(
            !names.iter().any(|n| n.eq_ignore_ascii_case("client.exe")),
            "client.exe is shared by unrelated software; matching it would mislabel them"
        );
    }

    #[test]
    fn tibia_is_not_sdr() {
        assert!(!TibiaConfig.uses_sdr());
    }
}
