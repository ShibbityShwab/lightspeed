//! # EA SPORTS FC 27 Game Configuration
//!
//! Game-specific settings for EA SPORTS FC 27, the football/soccer title from
//! EA Sports (the successor of the FIFA series). Requested in issue #166.
//!
//! ## Network Profile
//!
//! FC 27 plays its matches on **dedicated AWS instances assigned per match**:
//! the reporter recorded mostly `eu-south-1` (Milan) hosts in `15.160.0.0/16`
//! plus one `3.251.144.248`, and both the address and the UDP port change every
//! match. `dynamic_server()` is therefore `true`, so the interceptor re-detects
//! the host for each match instead of locking onto a pre-seeded address.
//!
//! Gameplay does not route through Steam Datagram Relay (`uses_sdr() == false`),
//! even for the Steam build: the match is hosted by EA on AWS rather than
//! relayed through Valve's network. The match servers do **not** answer ICMP, so
//! the relay ranking has no direct-ping anchor for this title and falls back to
//! its geometric floor / region prior (see `client/src/route/relay_choice.rs`).
//!
//! ## Ports (the SERVER's ports, which is what `ports()` means)
//!
//! [`GameConfig::ports`] is the range the capture filter matches as **destination**
//! ports (`windows.rs` builds `udp.DstPort >= lo and <= hi`), so it must name the
//! port the game talks *to*, not the one it binds locally. The reporter measured
//! the **server-side** gameplay ports over ~10 matches: `21006-21039`.
//!
//! This profile ships the enclosing 50-port block, `21000-21050`, rather than
//! the exact measured span. The reporter's own note is that the real range is
//! "probably a bit wider (e.g. 21000-21050)", the match server port is assigned
//! per match inside that block, and a filter a few ports too narrow fails
//! silently - the interceptor starts, logs success and captures nothing (the
//! failure recorded in the ARC Raiders profile). The two measured endpoints are
//! pinned by a test inside the range; if a future capture shows traffic outside
//! it, the range is what changes.
//!
//! The EA app's own control traffic (login, friends, store) is not gameplay and
//! is deliberately not profiled: EA FC gameplay is a single UDP flow per match,
//! roughly 30-60 packets/s each way, starting at kickoff.
//!
//! ## Process and launcher
//!
//! The game is started from the **EA app** (`EADesktop.exe`). The EA app keeps
//! running after the game is up and owns no gameplay sockets, so it must not be
//! treated as the game: matching the launcher would capture nothing. Only the
//! gameplay image `FC27.exe` is listed in `process_names()`; the anti-cheat's
//! service process (`EAAntiCheat.GameService.exe`) is not the game either.
//!
//! ## Anti-Cheat
//!
//! FC 27 ships **EA Javelin**, a kernel-level anti-cheat with the service
//! process `EAAntiCheat.GameService.exe`. LightSpeed forwards UDP transparently
//! without injecting code, reading game memory, or modifying payloads, so the
//! tunnel does not interact with it; the profile only names the right process
//! and the UDP gameplay range. This is the same class of anti-cheat as EAC,
//! Vanguard and Elytra, which the project documents as permitted. It has not
//! been field-confirmed with this title yet; `docs/supported-games.md` records
//! that caveat rather than promising compatibility.
//!
//! ## Redirect mode does not fit this title
//!
//! The matchmaker assigns the host, and its address and port change every
//! match, so FC 27 cannot be pointed at a fixed `127.0.0.1` address and a
//! `--game-server IP:PORT` value is stale before kickoff. Capture/intercept
//! mode (`--game fc27`, or auto-detection) is the supported path, and
//! [`GameConfig::redirect_instructions`] says so.

use super::GameConfig;

/// EA SPORTS FC 27 (EA Sports) game configuration.
pub struct Fc27Config;

impl GameConfig for Fc27Config {
    fn name(&self) -> &str {
        "EA SPORTS FC 27"
    }

    fn process_names(&self) -> Vec<&str> {
        // The gameplay client only. `EADesktop.exe` is the launcher and
        // `EAAntiCheat.GameService.exe` the anti-cheat service; neither owns
        // gameplay sockets, so neither may be matched as the game.
        ["FC27.exe"].to_vec()
    }

    fn ports(&self) -> (u16, u16) {
        // Server-side (destination) UDP gameplay range. The reporter measured
        // 21006-21039 over ~10 matches; this is the enclosing block - see the
        // module docs for why the measured span is not used verbatim.
        (21000, 21050)
    }

    fn anti_cheat(&self) -> &str {
        "EA Javelin (kernel-level)"
    }

    fn uses_sdr(&self) -> bool {
        // Matches are hosted by EA on AWS; the Steam build does not route
        // gameplay through Steam Datagram Relay.
        false
    }

    fn dynamic_server(&self) -> bool {
        // The matchmaker assigns a fresh AWS host (address and port) every
        // match, so the interceptor must re-detect rather than lock one host.
        true
    }

    fn typical_pps(&self) -> u32 {
        // Reporter measurement: one UDP flow each way at roughly 30-60
        // packets/s once kickoff starts. 45 is the midpoint; nothing in the
        // capture path depends on it being exact.
        45
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // UNVERIFIED: no packet-size capture was supplied with the request.
        // Typical bounds for a real-time sports title, capped near the tunnel's
        // datagram size. Confirm by capture if it ever matters.
        (64, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "EA SPORTS FC 27 redirect mode is not supported:\n\
         1. Match hosts are assigned by the matchmaker and change address and port every match\n\
         2. The game cannot be pointed at a fixed 127.0.0.1 address\n\
         3. Use capture/intercept mode: --game fc27 (auto-detection matches FC27.exe)\n\
         4. UDP gameplay range: 21000-21050 (server-side ports; the EA app is the launcher, not the game)\n\
         5. Anti-cheat: EA Javelin; transparent UDP forwarding only, no packet modification"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::TransportProto;

    #[test]
    fn the_profile_identity_matches_the_request() {
        let cfg = Fc27Config;
        assert_eq!(cfg.name(), "EA SPORTS FC 27");
        assert!(cfg.process_names().contains(&"FC27.exe"));
        assert_eq!(cfg.ports(), (21000, 21050));
        assert_eq!(cfg.transport(), TransportProto::Udp);
        assert_eq!(cfg.tcp_ports(), (0, 0));
        assert_eq!(cfg.capture_tcp_ports(), (0, 0));
        assert!(cfg.dynamic_server(), "match servers change every match");
        assert!(!cfg.uses_sdr(), "gameplay is hosted by EA, not over SDR");
        assert!(
            cfg.anti_cheat().contains("Javelin"),
            "got {:?}",
            cfg.anti_cheat()
        );
        assert!(cfg.typical_pps() > 0);
        let (lo, hi) = cfg.packet_size_range();
        assert!(lo < hi, "packet_size_range lo must be < hi");
    }

    /// The measured endpoints must stay inside the shipped range: a range that
    /// excludes them captures nothing for a match on those ports.
    #[test]
    fn the_measured_server_ports_are_inside_the_range() {
        let (lo, hi) = Fc27Config.ports();
        for measured in [21006u16, 21039] {
            assert!(
                (lo..=hi).contains(&measured),
                "measured server port {measured} is outside {lo}-{hi}"
            );
        }
    }

    #[test]
    fn the_capture_filter_is_udp_and_names_the_range() {
        let filter = Fc27Config.build_capture_filter();
        assert!(filter.bpf.contains("udp"), "got {}", filter.bpf);
        assert!(
            !filter.bpf.contains("tcp"),
            "a UDP-only filter must not match TCP: {}",
            filter.bpf
        );
        assert_eq!(filter.port_range, (21000, 21050));
        assert!(
            filter.bpf.contains("21000") && filter.bpf.contains("21050"),
            "the gameplay range must appear in the filter: {}",
            filter.bpf
        );
    }

    /// The EA app is the launcher, not the game. Matching it would capture
    /// nothing (it owns no gameplay sockets), and the anti-cheat service is not
    /// the game either.
    #[test]
    fn the_ea_launcher_is_not_treated_as_the_game() {
        let names = Fc27Config.process_names();
        for not_the_game in ["EADesktop.exe", "Origin.exe", "EAAntiCheat.GameService.exe"] {
            assert!(
                !names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(not_the_game)),
                "{not_the_game} must not be a game process: {names:?}"
            );
        }
        assert_eq!(names, vec!["FC27.exe"]);
    }

    #[test]
    fn redirect_instructions_name_capture_mode_and_the_dynamic_hosts() {
        let instructions = Fc27Config.redirect_instructions();
        assert!(instructions.contains("--game fc27"), "{instructions}");
        assert!(
            instructions.contains("capture/intercept mode"),
            "the mismatch with a fixed --game-server must be stated: {instructions}"
        );
        assert!(
            instructions.contains("21000-21050"),
            "the gameplay range must be stated: {instructions}"
        );
    }

    /// The registered profile must be loadable through the public registry, not
    /// only constructible in this module.
    #[test]
    fn the_profile_is_reachable_from_the_registry() {
        use crate::games::{all_game_keys, detect_game};
        assert!(
            all_game_keys()
                .iter()
                .any(|(key, display)| *key == "fc27" && *display == "EA SPORTS FC 27"),
            "fc27 must be in GAME_REGISTRY"
        );
        assert_eq!(
            detect_game("fc27").expect("fc27 must resolve").name(),
            "EA SPORTS FC 27"
        );
        // Aliases resolve to the same profile.
        for alias in ["eafc27", "ea-sports-fc-27"] {
            assert_eq!(
                detect_game(alias)
                    .unwrap_or_else(|e| panic!("{alias}: {e}"))
                    .name(),
                "EA SPORTS FC 27"
            );
        }
    }
}
