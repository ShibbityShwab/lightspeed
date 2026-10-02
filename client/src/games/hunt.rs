//! # Hunt: Showdown Game Configuration
//!
//! Game-specific settings for Crytek's Hunt: Showdown 1896, the PvPvE
//! extraction shooter (Steam App ID 594650). Requested in issue #141.
//!
//! ## Network Profile
//!
//! Hunt runs **dedicated authoritative servers** reached through a matchmaker
//! rather than a server browser: the client is assigned a session and connects
//! out to it, so the same client talks to a different host every match. That
//! makes the interceptor re-detect instead of locking onto a pre-seeded server
//! (`dynamic_server()` is `true`). The game ships on Steam but does not route
//! gameplay through Steam Datagram Relay, so `uses_sdr()` is `false`.
//!
//! ## Ports
//!
//! The UDP range **20000-20099** is the figure the request supplies and is
//! consistent with Crytek's published server configuration for Hunt
//! (`gamePort`/`steamPort` pairs inside that block). The TCP port **61088** is
//! the backend/menu connection and is deliberately NOT part of `ports()`:
//! LightSpeed's interceptor and tunnel are UDP-only, so advertising a TCP port
//! as the capture range would make `build_capture_filter()` emit a filter that
//! matches nothing.
//!
//! ## Anti-Cheat
//!
//! Hunt ships **Easy Anti-Cheat (EAC)**. LightSpeed forwards UDP transparently
//! without injecting code, reading game memory, or modifying payloads, so the
//! tunnel stays compatible with it. Capture support is Windows-focused because
//! Hunt's EAC configuration makes Linux/Proton play a separate, unsupported
//! path for this purpose.

use super::GameConfig;

/// Hunt: Showdown (Crytek) game configuration.
pub struct HuntConfig;

impl GameConfig for HuntConfig {
    fn name(&self) -> &str {
        "Hunt: Showdown"
    }

    fn process_names(&self) -> &[&str] {
        // `HuntGame.exe` is the gameplay client shipped by Steam; the launcher
        // is a separate process that exits once the game is running.
        &["HuntGame.exe", "huntgame"]
    }

    fn ports(&self) -> (u16, u16) {
        // UDP gameplay block. The game's TCP 61088 backend connection is not
        // included: this interceptor captures UDP only, and a TCP port here
        // would produce a capture filter that never matches.
        (20000, 20099)
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC)"
    }

    fn uses_sdr(&self) -> bool {
        // Matches are assigned by Crytek's matchmaker onto dedicated servers,
        // not reached over Steam Datagram Relay.
        false
    }

    fn dynamic_server(&self) -> bool {
        // Every match is a different host chosen by the matchmaker, so the
        // interceptor must re-detect rather than keep a pre-seeded address.
        true
    }

    fn typical_pps(&self) -> u32 {
        // Estimate for a 12-player PvPvE match at a 30 Hz server tick with the
        // client sending at the same rate.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (64, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "Hunt: Showdown redirect mode:\n\
         1. Hunt uses matchmaker-assigned dedicated servers, so capture/intercept mode is preferred\n\
         2. The UDP gameplay range is 20000-20099 (TCP 61088 is the menu/backend link and is not tunneled)\n\
         3. Start LightSpeed: --game hunt --game-server <SERVER_IP>:<PORT>\n\
         4. Anti-cheat: Easy Anti-Cheat; transparent UDP tunneling only, no packet modification"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunt_ports_are_the_udp_gameplay_block() {
        let cfg = HuntConfig;
        assert_eq!(cfg.ports(), (20000, 20099));
    }

    #[test]
    fn hunt_excludes_the_tcp_backend_port_from_the_udp_range() {
        // 61088 is the TCP backend/menu connection. If it ever leaks into
        // ports() the capture filter becomes UDP-only and silently misses it,
        // or worse, suggests a range the tunnel cannot carry.
        let (lo, hi) = HuntConfig.ports();
        assert!(
            !(lo..=hi).contains(&61088),
            "TCP backend port 61088 must not appear inside the UDP capture range"
        );
    }

    #[test]
    fn hunt_matches_the_reported_process_name() {
        assert!(HuntConfig.process_names().contains(&"HuntGame.exe"));
    }

    #[test]
    fn hunt_servers_are_dynamic_and_not_sdr() {
        let cfg = HuntConfig;
        assert!(cfg.dynamic_server());
        assert!(!cfg.uses_sdr());
    }
}
