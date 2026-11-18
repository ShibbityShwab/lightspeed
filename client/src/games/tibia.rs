//! # Tibia Game Configuration
//!
//! Game-specific settings for CipSoft's Tibia, the long-running 2D MMORPG,
//! requested in issue #171.
//!
//! ## Network Profile
//!
//! Tibia is a **TCP** game (#176): the client connects to the login server on
//! **7171** and then to a game world on **7172**. Both connections are carried
//! by the tunnel's TCP path - LightSpeed terminates TCP at the client, relays
//! the stream through the v5 datagram tunnel, and the relay splices its own TCP
//! connection to the world server (see `docs/tcp-tunnel-design.md`). This needs
//! a relay that advertises the `TCP_TUNNEL` capability; every community relay
//! does. The profile reports the TCP pair through both [`GameConfig::ports`]
//! (for diagnostics and redirect text) and [`GameConfig::tcp_ports`] (the
//! range the capture lane must intercept), and declares
//! [`TransportProto::Tcp`].
//!
//! The player picks a world rather than being assigned one by a matchmaker, so
//! the profile keeps the default `dynamic_server()` of `false`: within a
//! session the world address is fixed, and the process scanner's **TCP**
//! routes let the interceptor lock onto the world the client connected to. A
//! different world next session is a new connection, which the TCP terminator
//! tracks per connection rather than through a single cached server address.
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
//! Tibia ships **BattlEye**. LightSpeed forwards payloads unmodified - it
//! terminates the local TCP connection and relays the stream through the
//! tunnel without injecting code or touching memory - so the tunnel does not
//! interact with BattlEye.

use super::{GameConfig, TransportProto};

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
        // Kept for diagnostics and the redirect text; the transport is TCP, so
        // this pair is also what `tcp_ports()` reports.
        (7171, 7172)
    }

    fn tcp_ports(&self) -> (u16, u16) {
        // Login 7171, game/world 7172 - both TCP.
        (7171, 7172)
    }

    fn transport(&self) -> TransportProto {
        TransportProto::Tcp
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
         1. Tibia uses TCP: the login server is on 7171 and the game world on 7172;\n\
            LightSpeed terminates TCP and carries the stream through the tunnel\n\
         2. Start LightSpeed: --game tibia --game-server <WORLD_SERVER_IP>:7172\n\
         3. The relay must support the TCP tunnel (all community relays do)\n\
         4. \"Target a process\" captures UDP only and cannot carry Tibia's TCP\n\
            connection; use the game profile above instead\n\
         5. Anti-cheat: BattlEye; payloads are forwarded unmodified, no code injection"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tibia_ports_cover_login_and_world() {
        assert_eq!(TibiaConfig.ports(), (7171, 7172));
        assert_eq!(TibiaConfig.redirect_port(), 7171);
    }

    #[test]
    fn tibia_is_tcp_on_the_game_range() {
        assert_eq!(TibiaConfig.transport(), TransportProto::Tcp);
        assert_eq!(TibiaConfig.tcp_ports(), (7171, 7172));
    }

    #[test]
    fn tibia_capture_filter_is_tcp_and_names_both_ports() {
        let filter = TibiaConfig.build_capture_filter();
        assert!(filter.bpf.starts_with("tcp"), "got {:?}", filter.bpf);
        assert!(filter.bpf.contains("7171"), "got {:?}", filter.bpf);
        assert!(filter.bpf.contains("7172"), "got {:?}", filter.bpf);
    }

    #[test]
    fn tibia_redirect_text_describes_the_tcp_path() {
        let text = TibiaConfig.redirect_instructions();
        assert!(text.contains("TCP"), "got {text}");
        assert!(text.contains("7171") && text.contains("7172"), "got {text}");
        assert!(
            text.contains("--game tibia --game-server") && text.contains(":7172"),
            "the world connection is the one to name: {text}"
        );
        assert!(
            !text.contains("transparent UDP") && !text.contains("talks UDP"),
            "Tibia is TCP; the redirect text must not advertise a UDP path: {text}"
        );
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
