//! # Game Detection & Configuration
//!
//! Detects running games and provides game-specific tunnel configuration:
//! - Port ranges for packet capture
//! - Known server IP ranges
//! - Anti-cheat considerations
//! - Game-specific packet handling
//!
//! ## Auto-Detection
//!
//! `auto_detect()` scans running processes and matches them against known
//! game process names. Supports:
//! - **Fortnite**: `FortniteClient-Win64-Shipping.exe`
//! - **CS2**: `cs2.exe`
//! - **Dota 2**: `dota2.exe`
//! - **Rust** (Facepunch): `RustClient.exe`
//! - **Valorant**: `VALORANT-Win64-Shipping.exe`
//! - **Apex Legends**: `r5apex.exe`
//! - **Overwatch 2**: `Overwatch.exe`
//! - **League of Legends**: `League of Legends.exe`
//! - **PUBG: Battlegrounds**: `TslGame.exe`
//! - **MapleStory**: `MapleStory.exe`
//! - **Genshin Impact**: `GenshinImpact.exe`
//! - **Rocket League**: `RocketLeague.exe`
//! - **World of Tanks**: `WorldOfTanks.exe`
//! - **Roblox**: `RobloxPlayerBeta.exe`
//!
//! ## Capture Filters
//!
//! Each game provides a `CaptureFilter` via `build_capture_filter()` that
//! generates an appropriate BPF filter for pcap capture mode.

pub mod apex;
pub mod bodycam;
pub mod cs2;
pub mod csgo;
pub mod deadbydaylight;
pub mod dota2;
pub mod fortnite;
pub mod genshin;
pub mod lol;
pub mod maplestory;
pub mod ow2;
pub mod pubg;
pub mod roblox;
pub mod rocketleague;
pub mod rust;
pub mod valorant;
pub mod wot;

use std::net::Ipv4Addr;

use crate::tunnel::capture::CaptureFilter;

/// Trait for game-specific configuration.
pub trait GameConfig: Send + Sync {
    /// Game display name.
    fn name(&self) -> &str;

    /// Process name(s) to detect the game.
    fn process_names(&self) -> &[&str];

    /// UDP port range used by the game.
    fn ports(&self) -> (u16, u16);

    /// Known game server IP ranges (if any).
    fn server_ips(&self) -> Vec<Ipv4Addr> {
        vec![] // Default: discover dynamically
    }

    /// Anti-cheat system used by the game.
    fn anti_cheat(&self) -> &str;

    /// Whether this game uses Steam Datagram Relay.
    fn uses_sdr(&self) -> bool {
        false
    }

    /// Whether this game rotates through ephemeral server addresses/ports.
    ///
    /// When `true` the interceptor must not lock onto a pre-seeded server and
    /// must re-detect on rotation. Defaults to `false` (existing behaviour).
    fn dynamic_server(&self) -> bool {
        false
    }

    /// Typical packets per second for this game.
    fn typical_pps(&self) -> u32;

    /// Typical packet size range in bytes.
    fn packet_size_range(&self) -> (usize, usize);

    /// Suggested local port for redirect mode.
    /// This is the port the game client should connect to (127.0.0.1:port).
    /// Returns the default game server port if applicable.
    fn redirect_port(&self) -> u16 {
        self.ports().0
    }

    /// Setup instructions for configuring the game in redirect mode.
    fn redirect_instructions(&self) -> String {
        let (port_lo, port_hi) = self.ports();
        format!(
            "Configure {} to connect to 127.0.0.1:{}\n\
             Game server ports: {}-{}",
            self.name(),
            self.redirect_port(),
            port_lo,
            port_hi,
        )
    }

    /// Build a BPF capture filter for this game.
    ///
    /// Used with the pcap-capture feature to sniff game traffic
    /// directly from the network interface.
    fn build_capture_filter(&self) -> CaptureFilter {
        CaptureFilter::new(self.server_ips(), self.ports())
    }
}

/// Detect a game by name string.
pub fn detect_game(name: &str) -> anyhow::Result<Box<dyn GameConfig>> {
    match name.to_lowercase().as_str() {
        "fortnite" => Ok(Box::new(fortnite::FortniteConfig)),
        "cs2" | "counter-strike" | "counterstrike" => Ok(Box::new(cs2::Cs2Config)),
        "csgo" | "cs-go" | "csgolegacy" | "csgo-legacy" => Ok(Box::new(csgo::CsgoConfig)),
        "bodycam" | "body-cam" => Ok(Box::new(bodycam::BodycamConfig)),
        "deadbydaylight" | "dbd" | "dead-by-daylight" => {
            Ok(Box::new(deadbydaylight::DeadByDaylightConfig))
        }
        "dota2" | "dota" => Ok(Box::new(dota2::Dota2Config)),
        "rust" | "rustgame" => Ok(Box::new(rust::RustConfig)),
        "valorant" => Ok(Box::new(valorant::ValorantConfig)),
        "apex" | "apexlegends" | "apex-legends" => Ok(Box::new(apex::ApexConfig)),
        "ow2" | "overwatch2" | "overwatch-2" | "overwatch" => Ok(Box::new(ow2::Ow2Config)),
        "lol" | "leagueoflegends" | "league-of-legends" | "league" => Ok(Box::new(lol::LolConfig)),
        "pubg" | "battlegrounds" => Ok(Box::new(pubg::PubgConfig)),
        "maplestory" | "maple" => Ok(Box::new(maplestory::MapleStoryConfig)),
        "genshin" | "genshinimpact" | "genshin-impact" => Ok(Box::new(genshin::GenshinConfig)),
        "rocketleague" | "rocket-league" | "rocket" => Ok(Box::new(rocketleague::RocketLeagueConfig)),
        "roblox" => Ok(Box::new(roblox::RobloxConfig)),
        "wot" | "worldoftanks" | "world-of-tanks" => Ok(Box::new(wot::WotConfig)),
        _ => anyhow::bail!(
            "Unknown game: '{}'. Supported: fortnite, cs2, csgo, bodycam, deadbydaylight, dota2, rust, valorant, apex, ow2, lol, pubg, maplestory, genshin, rocketleague, roblox, wot",
            name
        ),
    }
}

/// Canonical CLI key + display name for every supported game, in menu order.
///
/// This is the single source of truth: [`detect_game`] resolves the keys and
/// [`all_games`] / [`all_game_keys`] derive from it. Register every new game
/// here (the `games` tests enforce the 17-entry count and key/name agreement).
pub const GAME_REGISTRY: &[(&str, &str)] = &[
    ("fortnite", "Fortnite"),
    ("cs2", "Counter-Strike 2"),
    ("csgo", "Counter-Strike: Global Offensive (Legacy)"),
    ("bodycam", "Bodycam"),
    ("deadbydaylight", "Dead by Daylight"),
    ("dota2", "Dota 2"),
    ("rust", "Rust"),
    ("valorant", "Valorant"),
    ("apex", "Apex Legends"),
    ("ow2", "Overwatch 2"),
    ("lol", "League of Legends"),
    ("pubg", "PUBG: Battlegrounds"),
    ("maplestory", "MapleStory"),
    ("genshin", "Genshin Impact"),
    ("rocketleague", "Rocket League"),
    ("roblox", "Roblox"),
    ("wot", "World of Tanks"),
];

/// Return every CLI key paired with its display name.
///
/// Consumers (the GUI game list, `--list-games`) use this instead of
/// hand-maintaining a parallel table. Derived from [`GAME_REGISTRY`], the same
/// table [`detect_game`] resolves against.
pub fn all_game_keys() -> Vec<(&'static str, &'static str)> {
    GAME_REGISTRY.to_vec()
}

/// Return the full list of supported game configs.
///
/// This is the canonical registry shared by `auto_detect` and `--list-games`.
/// Register every new game profile in [`GAME_REGISTRY`].
pub fn all_games() -> Vec<Box<dyn GameConfig>> {
    GAME_REGISTRY
        .iter()
        .filter_map(|(key, _)| detect_game(key).ok())
        .collect()
}

/// Whether the game with the given display name rotates through ephemeral
/// server addresses.
///
/// Resolves the profile from [`all_games`] by exact display name so a caller
/// holding only `InterceptorConfig::game_name` can consult the same
/// `dynamic_server()` flag without re-deriving CLI keys. Unknown names return
/// `false`, preserving the legacy lock-onto-first-route behaviour.
pub fn dynamic_server_for_name(name: &str) -> bool {
    all_games()
        .into_iter()
        .find(|g| g.name() == name)
        .is_some_and(|g| g.dynamic_server())
}

/// Match an observed process name against a known game process name.
///
/// Linux truncates `/proc/<pid>/comm` to 15 chars (`TASK_COMM_LEN`), so a
/// Windows-style name like `Bodycam-Win64-Shipping.exe` surfaces under
/// Proton/Wine as its 15-char prefix. Exact match is checked first (short
/// names and full-name platforms are unaffected); the prefix fallback handles
/// the truncation.
pub(crate) fn process_name_matches(observed: &str, known: &str) -> bool {
    if observed.eq_ignore_ascii_case(known) {
        return true;
    }
    observed.len() == 15
        && known
            .get(..15)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(observed))
}

/// Auto-detect which supported game is currently running.
///
/// Scans running processes and matches against known game process names.
/// Returns the first detected game, or an error if none found.
pub fn auto_detect() -> anyhow::Result<Box<dyn GameConfig>> {
    let processes = list_running_processes();

    if processes.is_empty() {
        tracing::debug!("Process list empty — may need elevated privileges");
    } else {
        tracing::debug!(
            "Scanning {} running processes for known games",
            processes.len()
        );
    }

    // Check each supported game
    for game in all_games() {
        for process_name in game.process_names() {
            if processes
                .iter()
                .any(|p| process_name_matches(p, process_name))
            {
                tracing::info!(
                    "🎮 Auto-detected game: {} (matched process: {})",
                    game.name(),
                    process_name
                );
                return Ok(game);
            }
        }
    }

    // No game found — provide helpful diagnostic
    let known_procs: Vec<&str> = vec![
        "FortniteClient-Win64-Shipping.exe",
        "cs2.exe",
        "Bodycam-Win64-Shipping.exe",
        "DeadByDaylight-Win64-Shipping.exe",
        "dota2.exe",
        "RustClient.exe",
        "VALORANT-Win64-Shipping.exe",
        "r5apex.exe",
        "Overwatch.exe",
        "League of Legends.exe",
        "TslGame.exe",
        "MapleStory.exe",
        "GenshinImpact.exe",
        "RocketLeague.exe",
        "WorldOfTanks.exe",
        "RobloxPlayerBeta.exe",
    ];
    tracing::debug!(
        "No matching processes found. Looking for: {}",
        known_procs.join(", ")
    );

    anyhow::bail!(
        "No supported game detected. Use --game to specify manually.\n\
         Supported: fortnite, cs2, csgo, bodycam, deadbydaylight, dota2, rust, valorant, apex, ow2, lol, pubg, maplestory, genshin, rocketleague, roblox, wot"
    )
}

/// List names of currently running processes.
///
/// Uses platform-specific commands to enumerate processes without
/// adding external crate dependencies (e.g., sysinfo).
fn list_running_processes() -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        list_processes_windows()
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        list_processes_unix()
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        tracing::warn!("Process listing not supported on this platform");
        vec![]
    }
}

/// List running processes on Windows using `tasklist`.
#[cfg(target_os = "windows")]
fn list_processes_windows() -> Vec<String> {
    match std::process::Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                tracing::debug!("tasklist failed with status: {}", output.status);
                return vec![];
            }
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| {
                    // CSV format: "ImageName.exe","PID","Session Name","Session#","Mem Usage"
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    // Extract the first CSV field (process name)
                    trimmed
                        .split(',')
                        .next()
                        .map(|s| s.trim_matches('"').to_string())
                })
                .filter(|s| !s.is_empty())
                .collect()
        }
        Err(e) => {
            tracing::debug!("Failed to run tasklist: {}", e);
            vec![]
        }
    }
}

/// List running processes on Linux/macOS using `ps`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn list_processes_unix() -> Vec<String> {
    match std::process::Command::new("ps")
        .args(["-e", "-o", "comm="])
        .output()
    {
        Ok(output) => {
            if !output.status.success() {
                tracing::debug!("ps failed with status: {}", output.status);
                return vec![];
            }
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| {
                    // ps may show full path on some systems — extract basename
                    let trimmed = s.trim();
                    if let Some(pos) = trimmed.rfind('/') {
                        trimmed[pos + 1..].to_string()
                    } else {
                        trimmed.to_string()
                    }
                })
                .filter(|s| !s.is_empty())
                .collect()
        }
        Err(e) => {
            tracing::debug!("Failed to run ps: {}", e);
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical names and aliases for every supported game.
    /// Adding a new game without updating this list will cause
    /// `test_all_registered_games_are_detectable` to fail — this is
    /// intentional drift-prevention.
    const ALL_GAME_KEYS: &[&str] = &[
        // Original 6 games
        "fortnite",
        "cs2",
        "counter-strike",
        "counterstrike",
        "bodycam",
        "body-cam",
        "deadbydaylight",
        "dbd",
        "dead-by-daylight",
        "dota2",
        "dota",
        "rust",
        "rustgame",
        "valorant",
        "apex",
        "apexlegends",
        "apex-legends",
        // New games (v0.4.0-dev)
        "ow2",
        "overwatch2",
        "overwatch-2",
        "overwatch",
        "lol",
        "leagueoflegends",
        "league-of-legends",
        "league",
        "pubg",
        "battlegrounds",
        // New games (v1.0.0)
        "maplestory",
        "maple",
        "genshin",
        "genshinimpact",
        "genshin-impact",
        "rocketleague",
        "rocket-league",
        "rocket",
        "roblox",
        "wot",
        "worldoftanks",
        "world-of-tanks",
    ];

    #[test]
    fn test_all_registered_games_are_detectable() {
        // Regression guard: every entry in ALL_GAME_KEYS must resolve via
        // detect_game without error.  If you add a game profile, add its
        // CLI key(s) to ALL_GAME_KEYS above.
        for key in ALL_GAME_KEYS {
            assert!(
                detect_game(key).is_ok(),
                "detect_game(\"{key}\") returned Err — did you forget to add it to detect_game()?"
            );
        }
    }

    #[test]
    fn test_all_game_keys_has_seventeen_entries() {
        assert_eq!(
            all_game_keys().len(),
            17,
            "GAME_REGISTRY must stay in sync with the 17 supported games"
        );
    }

    #[test]
    fn test_all_game_keys_resolve_and_match_display_names() {
        for (key, display) in all_game_keys() {
            let game = detect_game(key)
                .unwrap_or_else(|e| panic!("all_game_keys() key {key:?} is not detectable: {e}"));
            assert_eq!(
                game.name(),
                display,
                "display name drift for CLI key {key:?}"
            );
        }
    }

    #[test]
    fn test_all_games_derives_from_registry() {
        let games = all_games();
        assert_eq!(games.len(), GAME_REGISTRY.len());
        for (_, display) in GAME_REGISTRY {
            assert!(
                games.iter().any(|g| g.name() == *display),
                "all_games() is missing {display:?}"
            );
        }
    }

    #[test]
    fn test_dynamic_server_flag_defaults_false() {
        assert!(fortnite::FortniteConfig.dynamic_server());
        assert!(!rust::RustConfig.dynamic_server());
        assert!(!cs2::Cs2Config.dynamic_server());
        assert!(!valorant::ValorantConfig.dynamic_server());
    }

    #[test]
    fn test_dynamic_server_for_name_matches_profiles() {
        assert!(dynamic_server_for_name("Fortnite"));
        assert!(!dynamic_server_for_name("Rust"));
        assert!(!dynamic_server_for_name("Counter-Strike 2"));
        assert!(!dynamic_server_for_name("Definitely Not A Game"));
    }

    #[test]
    fn test_detect_unknown_game() {
        assert!(detect_game("minecraft").is_err());
        // Note: "overwatch" is now a valid alias for Overwatch 2
        assert!(detect_game("warzone").is_err());
        assert!(detect_game("").is_err());
    }

    #[test]
    fn test_process_name_matches_truncation() {
        // Exact match (case-insensitive).
        assert!(process_name_matches("cs2.exe", "cs2.exe"));
        assert!(process_name_matches("CS2.EXE", "cs2.exe"));
        // Linux 15-char comm truncation under Proton/Wine.
        assert!(process_name_matches(
            "Bodycam-Win64-S",
            "Bodycam-Win64-Shipping.exe"
        ));
        assert!(process_name_matches(
            "DeadByDaylight-",
            "DeadByDaylight-Win64-Shipping.exe"
        ));
        // Short names never truncate, and no false positives.
        assert!(!process_name_matches(
            "RustClient.exe",
            "RustClient.exe.old"
        ));
        assert!(!process_name_matches(
            "Bodycam-Win64-X",
            "Bodycam-Win64-Shipping.exe"
        ));
        assert!(!process_name_matches(
            "not-a-game",
            "Bodycam-Win64-Shipping.exe"
        ));
    }

    #[test]
    fn test_game_config_properties() {
        let cs2 = cs2::Cs2Config;
        assert_eq!(cs2.name(), "Counter-Strike 2");
        assert!(cs2.process_names().contains(&"cs2.exe"));
        assert_eq!(cs2.ports(), (27015, 27050));
        assert_eq!(cs2.redirect_port(), 27015);
        assert!(cs2.typical_pps() > 0);
        let (lo, hi) = cs2.packet_size_range();
        assert!(lo < hi, "packet_size_range lo must be < hi");

        let fortnite = fortnite::FortniteConfig;
        assert_eq!(fortnite.name(), "Fortnite");
        assert_eq!(fortnite.redirect_port(), 7777);

        let dota = dota2::Dota2Config;
        assert_eq!(dota.name(), "Dota 2");

        let rust_game = rust::RustConfig;
        assert_eq!(rust_game.name(), "Rust");
        assert!(rust_game.process_names().contains(&"RustClient.exe"));
        assert_eq!(rust_game.redirect_port(), 28015);
        assert!(!rust_game.uses_sdr());
        assert!(rust_game.typical_pps() > 0);

        let valorant = valorant::ValorantConfig;
        assert_eq!(valorant.name(), "Valorant");
        assert!(valorant
            .process_names()
            .contains(&"VALORANT-Win64-Shipping.exe"));
        assert_eq!(valorant.ports(), (7000, 7500));
        assert_eq!(valorant.redirect_port(), 7000);
        assert!(!valorant.uses_sdr());
        assert!(valorant.typical_pps() > 0);
        assert_eq!(valorant.anti_cheat(), "Riot Vanguard (kernel-mode)");

        let apex = apex::ApexConfig;
        assert_eq!(apex.name(), "Apex Legends");
        assert!(apex.process_names().contains(&"r5apex.exe"));
        assert_eq!(apex.ports(), (37000, 37050));
        assert_eq!(apex.redirect_port(), 37015);
        assert!(!apex.uses_sdr());
        assert!(apex.typical_pps() > 0);
        assert_eq!(apex.anti_cheat(), "Easy Anti-Cheat (EAC)");

        let ow2 = ow2::Ow2Config;
        assert_eq!(ow2.name(), "Overwatch 2");
        assert!(ow2.process_names().contains(&"Overwatch.exe"));
        let (ow2_lo, ow2_hi) = ow2.ports();
        assert!(ow2_lo < ow2_hi);
        assert!(!ow2.uses_sdr());
        assert!(ow2.typical_pps() > 0);

        let lol = lol::LolConfig;
        assert_eq!(lol.name(), "League of Legends");
        assert!(lol.process_names().contains(&"League of Legends.exe"));
        assert_eq!(lol.ports(), (5000, 5500));
        assert_eq!(lol.redirect_port(), 5000);
        assert!(!lol.uses_sdr());
        assert!(lol.typical_pps() > 0);

        let pubg = pubg::PubgConfig;
        assert_eq!(pubg.name(), "PUBG: Battlegrounds");
        assert!(pubg.process_names().contains(&"TslGame.exe"));
        let (pu_lo, pu_hi) = pubg.ports();
        assert!(pu_lo < pu_hi);
        assert!(!pubg.uses_sdr());
        assert!(pubg.typical_pps() > 0);
        assert_eq!(pubg.anti_cheat(), "BattlEye (kernel-mode)");

        let maple = maplestory::MapleStoryConfig;
        assert_eq!(maple.name(), "MapleStory");
        assert!(maple.process_names().contains(&"MapleStory.exe"));
        assert_eq!(maple.ports(), (7575, 8484));
        assert_eq!(maple.redirect_port(), 8484);
        assert!(!maple.uses_sdr());
        assert!(maple.typical_pps() > 0);
        assert_eq!(
            maple.anti_cheat(),
            "BlackCipher / Nexon Game Security (NGS)"
        );

        let genshin = genshin::GenshinConfig;
        assert_eq!(genshin.name(), "Genshin Impact");
        assert!(genshin.process_names().contains(&"GenshinImpact.exe"));
        assert_eq!(genshin.ports(), (22101, 42472));
        assert_eq!(genshin.redirect_port(), 22101);
        assert!(!genshin.uses_sdr());
        assert!(genshin.typical_pps() > 0);
        assert_eq!(genshin.anti_cheat(), "None");

        let rocket = rocketleague::RocketLeagueConfig;
        assert_eq!(rocket.name(), "Rocket League");
        assert!(rocket.process_names().contains(&"RocketLeague.exe"));
        assert_eq!(rocket.ports(), (7000, 9000));
        assert_eq!(rocket.redirect_port(), 7000);
        assert!(rocket.uses_sdr());
        assert!(rocket.typical_pps() > 0);
        assert_eq!(
            rocket.anti_cheat(),
            "Easy Anti-Cheat (EAC) / Epic Online Services"
        );

        let wot = wot::WotConfig;
        assert_eq!(wot.name(), "World of Tanks");
        assert!(wot.process_names().contains(&"WorldOfTanks.exe"));
        assert_eq!(wot.ports(), (12000, 29999));
        assert_eq!(wot.redirect_port(), 12000);
        assert!(!wot.uses_sdr());
        assert!(wot.typical_pps() > 0);
        assert_eq!(wot.anti_cheat(), "None");

        let dbd = deadbydaylight::DeadByDaylightConfig;
        assert_eq!(dbd.name(), "Dead by Daylight");
        assert!(dbd
            .process_names()
            .contains(&"DeadByDaylight-Win64-Shipping.exe"));
        assert_eq!(dbd.ports(), (27000, 27050));
        assert_eq!(dbd.redirect_port(), 27000);
        assert!(!dbd.uses_sdr());
        assert!(dbd.typical_pps() > 0);
        assert_eq!(dbd.anti_cheat(), "Easy Anti-Cheat (EAC)");

        let bodycam = bodycam::BodycamConfig;
        assert_eq!(bodycam.name(), "Bodycam");
        assert!(bodycam
            .process_names()
            .contains(&"Bodycam-Win64-Shipping.exe"));
        assert_eq!(bodycam.ports(), (27000, 27050));
        assert_eq!(bodycam.redirect_port(), 27000);
        assert!(bodycam.uses_sdr());
        assert!(bodycam.typical_pps() > 0);
        assert_eq!(bodycam.anti_cheat(), "None");
    }

    #[test]
    fn test_roblox_game_profile() {
        // The Roblox profile must be registered in all_games() with the
        // canonical process name and full high-ephemeral UDP range.
        let roblox = all_games()
            .into_iter()
            .find(|g| g.name() == "Roblox")
            .expect("Roblox must be registered in all_games()");
        assert_eq!(roblox.name(), "Roblox");
        assert!(roblox.process_names().contains(&"RobloxPlayerBeta.exe"));
        assert_eq!(roblox.ports(), (49152, 65535));
        assert_eq!(roblox.anti_cheat(), "Byfron (Hyperion)");
        assert!(roblox.typical_pps() > 0);
        let (lo, hi) = roblox.packet_size_range();
        assert!(lo < hi, "packet_size_range lo must be < hi");
    }

    #[test]
    fn test_build_capture_filter() {
        let cs2 = cs2::Cs2Config;
        let filter = cs2.build_capture_filter();
        assert!(filter.bpf.contains("udp"));
        assert!(filter.bpf.contains("27015"));
        assert_eq!(filter.port_range, (27015, 27050));

        let valorant = valorant::ValorantConfig;
        let vf = valorant.build_capture_filter();
        assert!(vf.bpf.contains("udp"));
        assert!(vf.bpf.contains("7000"));
        assert_eq!(vf.port_range, (7000, 7500));

        let apex = apex::ApexConfig;
        let af = apex.build_capture_filter();
        assert!(af.bpf.contains("udp"));
        assert!(af.bpf.contains("37000"));
        assert_eq!(af.port_range, (37000, 37050));
    }

    #[test]
    fn test_list_processes_doesnt_panic() {
        // Just verify it doesn't crash — may return empty on CI
        let procs = list_running_processes();
        // On a real system there should be some processes, but CI containers
        // may return empty — that's fine.
        let _ = procs;
    }
}
