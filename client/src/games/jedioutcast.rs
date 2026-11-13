//! # Star Wars Jedi Knight II: Jedi Outcast (NWH) Game Configuration
//!
//! Game-specific settings for Raven Software's Jedi Knight II: Jedi Outcast
//! played through the community **NWH Multiplayer** client. Requested in
//! issue #170.
//!
//! ## Network Profile
//!
//! The NWH client (`nwhmp.exe`) is a Quake III engine multiplayer build: it
//! speaks UDP to the server the player picked in the server browser. The
//! request supplies **UDP 28070** as the port, and that is the figure this
//! profile captures. It is a single port rather than a block, so the capture
//! filter is `udp port 28070`.
//!
//! Servers are chosen by the player rather than assigned by a matchmaker, so
//! the profile keeps the default `dynamic_server()` of `false`: the
//! interceptor may lock onto the server the session is talking to. A player
//! who switches servers gets a fresh lock from the next detection.
//!
//! ## Anti-Cheat
//!
//! The request names no anti-cheat, and a 2002 id Tech 3 title driven by a
//! community multiplayer client is not known to ship one. LightSpeed forwards
//! UDP transparently without injecting code or touching memory, so the tunnel
//! does not depend on this either way.

use super::GameConfig;

/// Jedi Knight II: Jedi Outcast (NWH Multiplayer) game configuration.
pub struct JediOutcastConfig;

impl GameConfig for JediOutcastConfig {
    fn name(&self) -> &str {
        "Jedi Knight II: Jedi Outcast (NWH)"
    }

    fn process_names(&self) -> Vec<&str> {
        // The NWH multiplayer client. Process matching is case-insensitive, so
        // one lower-case entry covers the image as Windows reports it.
        ["nwhmp.exe"].to_vec()
    }

    fn ports(&self) -> (u16, u16) {
        // Single UDP gameplay port, as supplied by the request.
        (28070, 28070)
    }

    fn anti_cheat(&self) -> &str {
        "None reported"
    }

    fn typical_pps(&self) -> u32 {
        // Estimate for a Quake III engine client at the stock 30 Hz packet
        // rate. Not measured from a live session.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (48, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "Jedi Knight II: Jedi Outcast (NWH) redirect mode:\n\
         1. The NWH client talks UDP to the server picked in the server browser\n\
         2. The gameplay port is 28070\n\
         3. Start LightSpeed: --game jedioutcast --game-server <SERVER_IP>:28070\n\
         4. Anti-cheat: none reported; transparent UDP tunneling only"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jedioutcast_ports_are_the_reported_single_port() {
        assert_eq!(JediOutcastConfig.ports(), (28070, 28070));
    }

    #[test]
    fn jedioutcast_capture_filter_names_the_port() {
        let filter = JediOutcastConfig.build_capture_filter();
        assert!(
            filter.bpf.contains("28070"),
            "capture filter must name the gameplay port, got {:?}",
            filter.bpf
        );
        assert!(
            filter.bpf.starts_with("udp"),
            "Jedi Outcast gameplay is UDP, got {:?}",
            filter.bpf
        );
    }

    #[test]
    fn jedioutcast_matches_the_reported_process_name() {
        let names = JediOutcastConfig.process_names();
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("nwhmp.exe")));
    }

    #[test]
    fn jedioutcast_is_not_sdr_and_keeps_the_default_server_lock() {
        let cfg = JediOutcastConfig;
        assert!(!cfg.uses_sdr());
        assert!(
            !cfg.dynamic_server(),
            "server-browser game: the chosen server should be locked, not re-detected every tick"
        );
    }
}
