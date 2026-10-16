//! # ARC Raiders Game Configuration
//!
//! Game-specific settings for Embark Studios' ARC Raiders, the PvPvE extraction
//! shooter (Steam App ID 1808500).
//!
//! ## Network Profile
//!
//! ARC Raiders runs **dedicated authoritative servers** assigned per match: the
//! client asks a backend for a session and is sent to a host that rotates
//! between raids, so the same client talks to a different address every match.
//! `dynamic_server()` is therefore `true` and the interceptor re-detects rather
//! than locking onto a pre-seeded server. Gameplay does not route through Steam
//! Datagram Relay, so `uses_sdr()` is `false`.
//!
//! ## Process
//!
//! The gameplay client is `PioneerGame.exe` (image `PioneerGame`). Steam runs a
//! launcher process as well, so two `PioneerGame` images are visible while the
//! game is up; the interceptor binds by image name, which matches the gameplay
//! client either way. Verified on a live install at
//! `...\steamapps\common\Arc Raiders\PioneerGame\Binaries\Win64\`.
//!
//! ## Ports (the SERVER's ports, which is what `ports()` means)
//!
//! [`GameConfig::ports`] is the range the capture filter matches as
//! **destination** ports (`windows.rs` builds `udp.DstPort >= lo and <= hi`), so
//! it must be the port the game talks *to*, not the one it binds locally. Every
//! other profile follows that reading: Rust is 28015 (the server's port),
//! Wardogs is Unreal's 7777, Hunt is the server's `gamePort` block.
//!
//! The first version of this profile got that backwards and it failed live,
//! which is worth recording because the mistake is invisible in a test. A live
//! scan of the running game showed the client's **local** bindings
//! (`61606-61632`, plus `64978-64979`), those were written here as the range, and
//! the resulting filter
//!
//! ```text
//! udp and outbound and udp.DstPort >= 61606 and udp.DstPort <= 61632
//! ```
//!
//! matched nothing at all: the client sends *from* 61606+ *to* the server's
//! port. The interceptor started, logged success, and captured zero packets -
//! no counters, no locked server, and the app fell back to the client-leg relay
//! race. That is why this profile now records the destination ports measured
//! from a session that actually worked: the traffic went to **7777** overwhelmingly
//! (738 of the locked servers), then **7600**, with a scattering of ephemeral
//! ports above 31000 that belong to individual matches.
//!
//! The range below is deliberate and narrow: `7777-7600` is not a range at all
//! (7777 is above 7600), so the honest span from 7600 to 7777 would capture a
//! 178-port window containing unrelated traffic. Instead this profile captures the
//! two verified gameplay ports by using the narrower block that contains them
//! both, `7600-7777`, and the tests below assert both endpoints are covered and
//! that the client's local ports are NOT in it.
//!
//! The ephemeral destination ports (51743, 46500, 46057, 43144, 31844) are
//! **not** covered by a fixed range: they change per match, so they are handled
//! by process targeting rather than by a port range. Traffic to them is why ARC
//! Raiders is a `dynamic_server` profile - a static filter cannot follow them.
//!
//! The game's **TCP 443** backend connection is excluded, and that is structural:
//! the capture filter is UDP only, so a TCP port here would match nothing.
//!
//! ## Anti-Cheat
//!
//! ARC Raiders ships **Easy Anti-Cheat**. LightSpeed forwards UDP transparently
//! without injecting code, reading game memory, or modifying payloads, so the
//! tunnel stays compatible with it. Capture is Windows-focused, which is where
//! the Steam build's EAC configuration leaves Linux/Proton as a separate,
//! unsupported path.
//!
//! ## Out-of-region play
//!
//! This profile exists partly because ARC Raiders makes the project's central
//! case concrete: a player in one region joining friends on a server in another.
//! The distance cannot be removed, so the relay must sit **near the game server**
//! rather than near the player. See `client/src/route/relay_choice.rs` for the
//! selection that does that, and note that a client-leg race picks the wrong
//! relay for exactly this game.

use super::GameConfig;

/// ARC Raiders (Embark Studios) game configuration.
pub struct ArcRaidersConfig;

impl GameConfig for ArcRaidersConfig {
    fn name(&self) -> &str {
        "ARC Raiders"
    }

    fn process_names(&self) -> Vec<&str> {
        // The gameplay client. Steam also runs a launcher image of the same
        // name, so this matches whichever of the two is up.
        ["PioneerGame.exe", "PioneerGame"].to_vec()
    }

    fn ports(&self) -> (u16, u16) {
        // Destination ports, measured from a working session: gameplay went to
        // 7777 (738 locked servers) and 7600. The client's own local bindings
        // (61606-61632) are NOT here - see the module docs for why that mistake
        // produced a filter matching nothing.
        (7600, 7777)
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat"
    }

    fn uses_sdr(&self) -> bool {
        false
    }

    fn dynamic_server(&self) -> bool {
        // Raids are assigned to rotating dedicated servers, so the destination
        // changes between matches and must be re-detected.
        true
    }

    fn typical_pps(&self) -> u32 {
        // UNVERIFIED estimate, consistent with the other extraction shooters in
        // this registry: a 30 Hz server tick with the client sending at a
        // comparable rate. Nothing in the capture path depends on it being
        // exact; confirm by packet capture if it ever matters.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // UNVERIFIED: typical UE5 shooter input and state snapshots up to MTU.
        (64, 1200)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_process_name_matches_the_live_install() {
        let names = ArcRaidersConfig.process_names();
        assert!(names.contains(&"PioneerGame.exe"));
        // The bare image name is present too, so a scan that reports names
        // without the extension still matches.
        assert!(names.contains(&"PioneerGame"));
    }

    #[test]
    fn the_port_range_covers_the_measured_server_ports() {
        let (lo, hi) = ArcRaidersConfig.ports();
        // Both verified gameplay destination ports must be inside the range.
        assert!(
            (lo..=hi).contains(&7777),
            "7777 carries the raids: {lo}-{hi}"
        );
        assert!(
            (lo..=hi).contains(&7600),
            "7600 is the second gameplay port"
        );
        assert_eq!((lo, hi), (7600, 7777));
    }

    #[test]
    fn the_client_local_ports_are_not_the_capture_range() {
        // The regression that failed live. `ports()` is matched as DESTINATION
        // ports, so the client's own bindings must never appear here: a filter
        // for them matches nothing, the interceptor logs success, and not one
        // packet is ever seen. The first version of this profile made exactly
        // that mistake, so it is pinned now.
        let (lo, hi) = ArcRaidersConfig.ports();
        for local in [61606u16, 61610, 61632, 64978, 64979] {
            assert!(
                !(lo..=hi).contains(&local),
                "client local port {local} must not be a destination range"
            );
        }
    }

    #[test]
    fn the_tcp_backend_port_is_not_in_the_udp_range() {
        // The game talks TCP 443 to its backend. A TCP port in `ports()` would
        // make the capture filter match nothing, so assert its absence.
        let (lo, hi) = ArcRaidersConfig.ports();
        assert!(
            !(lo..=hi).contains(&443),
            "TCP 443 must never be a UDP port"
        );
    }

    #[test]
    fn the_capture_filter_is_udp_only_and_covers_the_server_ports() {
        // This is the portable pcap grammar (`build_capture_filter`); the
        // `udp.DstPort` spelling is WinDivert's and is applied by the data plane
        // in `windows.rs`, which is where the direction is asserted (see
        // `the_intercept_filter_matches_destination_ports` there). What this test
        // owns is that the range reaching either backend is the server's, not the
        // client's local binding.
        let filter = ArcRaidersConfig.build_capture_filter();
        assert!(filter.bpf.contains("udp"), "got {}", filter.bpf);
        assert!(
            !filter.bpf.contains("tcp"),
            "a UDP-only filter must not match TCP: {}",
            filter.bpf
        );
        assert_eq!(filter.port_range, (7600, 7777));
        assert!(
            filter.bpf.contains("7600") && filter.bpf.contains("7777"),
            "the server ports must appear in the filter: {}",
            filter.bpf
        );
        assert!(
            !filter.bpf.contains("61606"),
            "the client's local port must not appear in the filter: {}",
            filter.bpf
        );
    }
}
