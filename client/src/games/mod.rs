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
//! - **WARDOGS**: `WardogsClient-Win64-Shipping.exe`
//! - **Minecraft**: `Minecraft.Windows.exe` (Bedrock Edition)
//! - **Minecraft Java Edition**: `javaw.exe`/`java.exe` whose command line
//!   carries `net.minecraft.client.main.Main` (the image name alone is shared
//!   by every Java application, so it is never trusted on its own)
//! - **Hunt: Showdown**: `HuntGame.exe`
//!
//! ## Capture Filters
//!
//! Each game provides a `CaptureFilter` via `build_capture_filter()` that
//! generates an appropriate BPF filter for pcap capture mode: UDP games emit
//! the `udp port`/`udp portrange` grammar, while a game that declares
//! `tcp_ports()` emits the `tcp` equivalent.

pub mod apex;
pub mod bodycam;
pub mod cs2;
pub mod csgo;
pub mod deadbydaylight;
pub mod dota2;
pub mod fortnite;
pub mod genshin;
pub mod hunt;
pub mod lol;
pub mod maplestory;
pub mod minecraft;
pub mod ow2;
pub mod pubg;
pub mod roblox;
pub mod rocketleague;
pub mod rust;
pub mod valorant;
pub mod wardogs;
pub mod wot;
pub mod zomboid;

use std::net::Ipv4Addr;
use std::sync::OnceLock;

use crate::tunnel::capture::CaptureFilter;

/// Which IP transport a game's gameplay traffic uses, and which transport a
/// tunnel leg is carrying.
///
/// Every profile before Minecraft Java Edition is UDP; Java Edition is the
/// first [`TransportProto::Tcp`] profile, carried through the tunnel's v5
/// datagram path rather than as raw TCP segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TransportProto {
    /// UDP - the tunnel's default datagram transport.
    #[default]
    Udp,
    /// TCP - terminated at both tunnel ends and carried as sequenced datagrams.
    Tcp,
}

impl std::fmt::Display for TransportProto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TransportProto::Udp => "UDP",
            TransportProto::Tcp => "TCP",
        })
    }
}

impl TransportProto {
    /// Whether a game on this transport is forbidden to ride a tunnel leg on
    /// `leg`.
    ///
    /// TCP-in-TCP is the only forbidden pairing: the tunnel's TCP fallback
    /// already nests a congestion controller and a retransmission loop, and a
    /// terminated TCP game stream must not be re-nested inside it (see
    /// `docs/tcp-tunnel-design.md`). Everything else - including a UDP game on
    /// the TCP fallback - stays allowed.
    pub fn forbidden_over(self, leg: Option<TransportProto>) -> bool {
        matches!(
            (self, leg),
            (TransportProto::Tcp, Some(TransportProto::Tcp))
        )
    }
}

/// Trait for game-specific configuration.
pub trait GameConfig: Send + Sync {
    /// Game display name.
    fn name(&self) -> &str;

    /// Process name(s) to detect the game.
    fn process_names(&self) -> &[&str];

    /// The game's primary network port range.
    ///
    /// UDP for every UDP game. A TCP game (Minecraft Java Edition) reports its
    /// TCP port here too so diagnostics and [`GameConfig::redirect_port`] point
    /// at the right port; the authoritative transport is
    /// [`GameConfig::transport`].
    fn ports(&self) -> (u16, u16);

    /// TCP port range used by the game, or `(0, 0)` when the game carries no
    /// TCP gameplay traffic (the default for every UDP profile).
    fn tcp_ports(&self) -> (u16, u16) {
        (0, 0)
    }

    /// Which IP transport carries this game's gameplay traffic.
    ///
    /// Defaults to UDP. Minecraft Java Edition returns
    /// [`TransportProto::Tcp`].
    fn transport(&self) -> TransportProto {
        TransportProto::Udp
    }

    /// Command-line substring that must appear in a matched process's command
    /// line before this profile may be locked in, or `None` when the image
    /// name alone is unambiguous (the default).
    ///
    /// Minecraft Java Edition is the motivating case: `javaw.exe`/`java.exe`
    /// are shared by every Java application, so a name-only match would
    /// mislabel IDEs and build tools as the game. Its launcher always passes
    /// `net.minecraft.client.main.Main`.
    fn command_line_marker(&self) -> Option<&str> {
        None
    }

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
    ///
    /// UDP profiles keep the `udp port`/`udp portrange` grammar unchanged
    /// (their `tcp_ports()` default is `(0, 0)`); a profile that declares a
    /// TCP port range emits the `tcp` equivalent.
    fn build_capture_filter(&self) -> CaptureFilter {
        let (tcp_lo, tcp_hi) = self.tcp_ports();
        if tcp_lo == 0 && tcp_hi == 0 {
            return CaptureFilter::new(self.server_ips(), self.ports());
        }

        let server_ips = self.server_ips();
        let port_expr = if tcp_lo == tcp_hi {
            format!("tcp port {tcp_lo}")
        } else {
            format!("tcp portrange {tcp_lo}-{tcp_hi}")
        };
        let bpf = if server_ips.is_empty() {
            port_expr
        } else {
            let ip_filter: Vec<String> = server_ips.iter().map(|ip| format!("host {ip}")).collect();
            format!("{port_expr} and ({})", ip_filter.join(" or "))
        };

        CaptureFilter {
            server_ips,
            port_range: (tcp_lo, tcp_hi),
            bpf,
        }
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
        "minecraft" | "mc" | "minecraftbedrock" | "minecraft-bedrock" => {
            Ok(Box::new(minecraft::MinecraftConfig))
        }
        "minecraft-java" | "minecraftjava" | "java" => {
            Ok(Box::new(minecraft::MinecraftJavaConfig))
        }
        "genshin" | "genshinimpact" | "genshin-impact" => Ok(Box::new(genshin::GenshinConfig)),
        "rocketleague" | "rocket-league" | "rocket" => Ok(Box::new(rocketleague::RocketLeagueConfig)),
        "roblox" => Ok(Box::new(roblox::RobloxConfig)),
        "wot" | "worldoftanks" | "world-of-tanks" => Ok(Box::new(wot::WotConfig)),
        "zomboid" | "projectzomboid" | "project-zomboid" | "pz" => {
            Ok(Box::new(zomboid::ZomboidConfig))
        }
        "wardogs" => Ok(Box::new(wardogs::WardogsConfig)),
        "hunt" | "huntshowdown" | "hunt-showdown" | "hunt showdown" => {
            Ok(Box::new(hunt::HuntConfig))
        }
        _ => anyhow::bail!(
            "Unknown game: '{}'. Supported: fortnite, cs2, csgo, bodycam, deadbydaylight, dota2, rust, valorant, apex, ow2, lol, pubg, maplestory, minecraft, minecraft-java, genshin, rocketleague, roblox, wot, zomboid, wardogs, hunt",
            name
        ),
    }
}

/// Canonical CLI key + display name for every supported game, in menu order.
///
/// This is the single source of truth: [`detect_game`] resolves the keys and
/// [`all_games`] / [`all_game_keys`] derive from it. Register every new game
/// here (the `games` tests enforce the 20-entry count and key/name agreement).
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
    ("minecraft", "Minecraft"),
    ("minecraft-java", "Minecraft Java Edition"),
    ("genshin", "Genshin Impact"),
    ("rocketleague", "Rocket League"),
    ("roblox", "Roblox"),
    ("wot", "World of Tanks"),
    ("zomboid", "Project Zomboid"),
    ("wardogs", "WARDOGS"),
    ("hunt", "Hunt: Showdown"),
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

/// Resolve a game's process names from its exact display name.
///
/// Mirrors [`dynamic_server_for_name`]: the profile is found in [`all_games`]
/// by display name, so a caller holding only `InterceptorConfig::game_name`
/// can recover the same process-name list the profile declares. The names are
/// compile-time string literals; profiles are leaked once behind a cache so
/// those literals can be returned as `&'static str` without per-call copying.
/// Unknown names return an empty vec.
pub fn process_names_for_name(display_name: &str) -> Vec<&'static str> {
    process_name_table()
        .iter()
        .find(|(name, _)| *name == display_name)
        .map(|(_, names)| names.clone())
        .unwrap_or_default()
}

fn process_name_table() -> &'static Vec<(&'static str, Vec<&'static str>)> {
    static TABLE: OnceLock<Vec<(&'static str, Vec<&'static str>)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        all_games()
            .into_iter()
            .map(|game| {
                let game: &'static dyn GameConfig = Box::leak(game);
                (game.name(), game.process_names().to_vec())
            })
            .collect()
    })
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
    // An image name reaching us short is one that was truncated somewhere:
    // Linux `comm` is capped at TASK_COMM_LEN (15), and Proton/Wine inherits
    // that, which is why the original check tested exactly 15. Any other
    // truncation length - 25, 31 - fell through and never matched, and that hit
    // the longest names hardest: WARDOGS is 32 and 28 characters, so it could
    // stay undetected while cs2.exe (7) and dota2.exe (9) were unaffected. That
    // asymmetry is what users report. Accept any truncation of at least 15: this
    // strictly widens the old rule, since every input that matched still does.
    observed.len() >= 15
        && known
            .get(..observed.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(observed))
}

/// Auto-detect which supported game is currently running.
///
/// Scans running processes and matches against known game process names.
/// Returns the first detected game, or an error if none found.
pub fn auto_detect() -> anyhow::Result<Box<dyn GameConfig>> {
    let processes = list_running_processes_with_cmdline();

    if processes.is_empty() {
        tracing::debug!("Process list empty — may need elevated privileges");
    } else {
        tracing::debug!(
            "Scanning {} running processes for known games",
            processes.len()
        );
    }

    // Check each supported game. A profile that gates on a command-line marker
    // (Minecraft Java Edition, whose `javaw.exe`/`java.exe` image names are
    // shared by every Java application) is checked before marker-free profiles,
    // so a shared image name can never shadow a verified, more specific match.
    let mut games = all_games();
    games.sort_by_key(|g| std::cmp::Reverse(g.command_line_marker().is_some()));

    for game in games {
        let marker = game.command_line_marker();
        let hit = game.process_names().iter().find(|&&process_name| {
            processes
                .iter()
                .any(|p| process_entry_matches(p, process_name, marker))
        });

        if let Some(&process_name) = hit {
            tracing::info!(
                "🎮 Auto-detected game: {} (matched process: {})",
                game.name(),
                process_name
            );
            return Ok(game);
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
        "ProjectZomboid64.exe",
        "ProjectZomboid",
        "WardogsClient-Win64-Shipping.exe",
        "WardogsLauncher-Shipping.exe",
        "Minecraft.Windows.exe",
        "Minecraft.exe",
        "javaw.exe",
    ];
    tracing::debug!(
        "No matching processes found. Looking for: {}",
        known_procs.join(", ")
    );

    anyhow::bail!(
        "No supported game detected. Use --game to specify manually.\n\
         Supported: fortnite, cs2, csgo, bodycam, deadbydaylight, dota2, rust, valorant, apex, ow2, lol, pubg, maplestory, minecraft, minecraft-java, genshin, rocketleague, roblox, wot, zomboid, wardogs"
    )
}

/// A running process: its image name plus every command line observed for
/// that image name (empty when the platform does not expose command lines).
#[derive(Debug, Clone)]
struct RunningProcess {
    name: String,
    command_lines: Vec<String>,
}

/// Whether an observed process matches `known_name` and, when the profile
/// declares a command-line `marker`, whether any of its command lines carries
/// that marker.
///
/// A profile with a marker never matches on the image name alone; that is what
/// keeps a bare `javaw.exe` (any Java application) from being labeled
/// Minecraft Java Edition.
fn process_entry_matches(
    observed: &RunningProcess,
    known_name: &str,
    marker: Option<&str>,
) -> bool {
    if !process_name_matches(&observed.name, known_name) {
        return false;
    }
    match marker {
        Some(marker) => observed
            .command_lines
            .iter()
            .any(|line| line.contains(marker)),
        None => true,
    }
}

/// List names of currently running processes.
///
/// Uses platform-specific commands to enumerate processes without
/// adding external crate dependencies (e.g., sysinfo).
fn list_running_processes() -> Vec<String> {
    list_running_processes_with_cmdline()
        .into_iter()
        .map(|p| p.name)
        .collect()
}

/// Enumerate running processes, attaching command lines where the platform
/// exposes them.
fn list_running_processes_with_cmdline() -> Vec<RunningProcess> {
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

/// List running processes on Windows using `tasklist`, plus command lines from
/// `Win32_Process` when PowerShell is available.
#[cfg(target_os = "windows")]
fn list_processes_windows() -> Vec<RunningProcess> {
    let names: Vec<String> = match crate::process::silent_command("tasklist")
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
            return vec![];
        }
    };

    let mut command_lines = windows_command_lines();
    names
        .into_iter()
        .map(|name| {
            let command_lines = command_lines
                .remove(&name.to_lowercase())
                .unwrap_or_default();
            RunningProcess {
                name,
                command_lines,
            }
        })
        .collect()
}

/// Command lines per image name (lower-cased) from `Win32_Process`.
///
/// PowerShell is used because `wmic` is absent from current Windows builds.
/// Any failure is non-fatal: profiles gated on a command-line marker simply
/// stay undetected rather than matching on an ambiguous image name.
#[cfg(target_os = "windows")]
fn windows_command_lines() -> std::collections::HashMap<String, Vec<String>> {
    use std::collections::HashMap;

    let mut command_lines: HashMap<String, Vec<String>> = HashMap::new();
    let output = match crate::process::silent_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.Name)`t$($_.CommandLine)\" }",
        ])
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            tracing::debug!(
                "PowerShell process query failed with status: {}",
                output.status
            );
            return command_lines;
        }
        Err(e) => {
            tracing::debug!("Failed to run PowerShell for command lines: {}", e);
            return command_lines;
        }
    };

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((name, command_line)) = line.trim_end().split_once('\t') else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        command_lines
            .entry(name.to_lowercase())
            .or_default()
            .push(command_line.to_string());
    }
    command_lines
}

/// List running processes on Linux/macOS using `ps`.
///
/// These platforms do not supply command lines here; the only marker-gated
/// profile (Minecraft Java Edition) is Windows-only, so it fails closed rather
/// than matching on an ambiguous image name.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn list_processes_unix() -> Vec<RunningProcess> {
    match crate::process::silent_command("ps")
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
                .map(|name| RunningProcess {
                    name,
                    command_lines: vec![],
                })
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
        // Project Zomboid
        "zomboid",
        "projectzomboid",
        "project-zomboid",
        "pz",
        // WARDOGS
        "wardogs",
        // Hunt: Showdown
        "hunt",
        "huntshowdown",
        "hunt-showdown",
        "hunt showdown",
        // Minecraft Java Edition (TCP)
        "minecraft-java",
        "minecraftjava",
        "java",
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
    fn test_all_game_keys_count_matches_registry() {
        assert_eq!(
            all_game_keys().len(),
            GAME_REGISTRY.len(),
            "GAME_REGISTRY must stay in sync with the supported games"
        );
        assert_eq!(GAME_REGISTRY.len(), 22, "22 supported games");
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

    /// Cross-crate completeness guard: every canonical CLI key the client
    /// advertises in [`GAME_REGISTRY`] must resolve to a real wire id in the
    /// protocol's append-only registry. A missing entry means telemetry would
    /// silently report `UNKNOWN` for that game. Fix by adding the key in
    /// `protocol/src/control.rs` (do not weaken this test).
    #[test]
    fn test_every_registry_key_resolves_to_game_id() {
        for (key, display) in GAME_REGISTRY {
            let id = lightspeed_protocol::game_id::id_for_key(key);
            assert_ne!(
                id,
                lightspeed_protocol::game_id::UNKNOWN,
                "GAME_REGISTRY key {key:?} ({display:?}) is missing from the protocol GAME_IDS registry"
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
    fn test_process_names_for_name_matches_profiles() {
        assert_eq!(
            process_names_for_name("Fortnite"),
            vec!["FortniteClient-Win64-Shipping.exe"]
        );
        assert!(process_names_for_name("Rust").contains(&"RustClient.exe"));
        assert!(process_names_for_name("Definitely Not A Game").is_empty());

        let cs2 = all_games()
            .into_iter()
            .find(|g| g.name() == "Counter-Strike 2")
            .expect("Counter-Strike 2 must be registered");
        assert_eq!(
            process_names_for_name("Counter-Strike 2"),
            cs2.process_names().to_vec()
        );
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
        assert!(detect_game("minecraft-java-edition").is_err());
        assert!(detect_game("minecraft-education").is_err());
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
        // Truncation at lengths other than 15 must resolve too. These are the
        // longest names in the registry, so they are the ones a 15-only rule
        // silently drops while short names keep working.
        assert!(process_name_matches(
            "WardogsClient-Win64-Shippin",
            "WardogsClient-Win64-Shipping.exe"
        ));
        assert!(process_name_matches(
            "WardogsLauncher-Shipping.",
            "WardogsLauncher-Shipping.exe"
        ));
        assert!(process_name_matches(
            "Minecraft.Windows.exe",
            "Minecraft.Windows.exe"
        ));
        // A short prefix is not a truncation and must not match.
        assert!(!process_name_matches("Minecraft", "Minecraft.Windows.exe"));
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

        let minecraft = minecraft::MinecraftConfig;
        assert_eq!(minecraft.name(), "Minecraft");
        assert!(minecraft.process_names().contains(&"Minecraft.Windows.exe"));
        assert_eq!(minecraft.ports(), (19132, 19133));
        assert_eq!(minecraft.redirect_port(), 19132);
        assert!(!minecraft.uses_sdr());
        assert!(minecraft.typical_pps() > 0);
        assert_eq!(minecraft.anti_cheat(), "None (server-side validation)");
        // Java Edition runs on TCP 25565 and is now carried through the TCP
        // path rather than the UDP tunnel, so it has its own profile. Bedrock's
        // name list still omits the shared `javaw.exe` image name.
        assert!(!minecraft.process_names().contains(&"javaw.exe"));

        let java = minecraft::MinecraftJavaConfig;
        assert_eq!(java.name(), "Minecraft Java Edition");
        assert!(java.process_names().contains(&"javaw.exe"));
        assert!(java.process_names().contains(&"java.exe"));
        assert_eq!(java.ports(), (25565, 25565));
        assert_eq!(java.tcp_ports(), (25565, 25565));
        assert_eq!(java.transport(), TransportProto::Tcp);
        assert_eq!(java.redirect_port(), 25565);
        assert_eq!(
            java.command_line_marker(),
            Some("net.minecraft.client.main.Main")
        );
        assert_eq!(java.anti_cheat(), "None (server-side validation)");
        assert!(!java.uses_sdr());
        assert_eq!(java.typical_pps(), 20);
        let (jlo, jhi) = java.packet_size_range();
        assert!(jlo < jhi, "packet_size_range lo must be < hi");

        let wardogs = wardogs::WardogsConfig;
        assert_eq!(wardogs.name(), "WARDOGS");
        assert!(wardogs
            .process_names()
            .contains(&"WardogsClient-Win64-Shipping.exe"));
        assert_eq!(wardogs.ports(), (7777, 7788));
        assert_eq!(wardogs.redirect_port(), 7777);
        assert!(!wardogs.uses_sdr());
        assert!(wardogs.dynamic_server());
        assert!(wardogs.typical_pps() > 0);
        assert_eq!(wardogs.anti_cheat(), "Elytra (kernel-mode)");
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
    fn test_zomboid_game_profile() {
        // The Project Zomboid profile must be registered in all_games() with
        // its canonical process names and UDP port range.
        let zomboid = all_games()
            .into_iter()
            .find(|g| g.name() == "Project Zomboid")
            .expect("Project Zomboid must be registered in all_games()");
        assert_eq!(zomboid.name(), "Project Zomboid");
        assert!(zomboid.process_names().contains(&"ProjectZomboid64.exe"));
        assert_eq!(zomboid.ports(), (16261, 16262));
        assert_eq!(zomboid.redirect_port(), 16261);
        assert_eq!(zomboid.anti_cheat(), "None");
        assert!(!zomboid.uses_sdr());
        assert!(zomboid.typical_pps() > 0);
        let (lo, hi) = zomboid.packet_size_range();
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
    fn test_tcp_ports_and_transport_default_to_udp() {
        assert_eq!(cs2::Cs2Config.tcp_ports(), (0, 0));
        assert_eq!(cs2::Cs2Config.transport(), TransportProto::Udp);
        assert_eq!(TransportProto::default(), TransportProto::Udp);
        assert_eq!(
            cs2::Cs2Config.command_line_marker(),
            None,
            "name-only profiles must not require a command-line verification"
        );
    }

    #[test]
    fn test_tcp_game_capture_filter_uses_tcp_bpf() {
        // Bedrock stays on the UDP grammar, byte-for-byte.
        let bedrock = minecraft::MinecraftConfig.build_capture_filter();
        assert!(bedrock.bpf.contains("udp"), "got {}", bedrock.bpf);
        assert!(bedrock.bpf.contains("19132"), "got {}", bedrock.bpf);

        // Java Edition emits the TCP equivalent on its single port.
        let java = minecraft::MinecraftJavaConfig.build_capture_filter();
        assert_eq!(java.bpf, "tcp port 25565");
        assert_eq!(java.port_range, (25565, 25565));
    }

    #[test]
    fn test_tcp_in_tcp_is_the_only_forbidden_transport_pairing() {
        assert!(TransportProto::Tcp.forbidden_over(Some(TransportProto::Tcp)));
        assert!(!TransportProto::Tcp.forbidden_over(Some(TransportProto::Udp)));
        assert!(!TransportProto::Tcp.forbidden_over(None));
        assert!(!TransportProto::Udp.forbidden_over(Some(TransportProto::Tcp)));
        assert!(!TransportProto::Udp.forbidden_over(Some(TransportProto::Udp)));
        assert!(!TransportProto::Udp.forbidden_over(None));
    }

    #[test]
    fn test_java_profile_requires_command_line_marker() {
        let marker = minecraft::MinecraftJavaConfig.command_line_marker();

        let java_app = RunningProcess {
            name: "javaw.exe".to_string(),
            command_lines: vec!["\"C:\\jdk\\bin\\javaw.exe\" -jar build-tool.jar".to_string()],
        };
        let java_game = RunningProcess {
            name: "javaw.exe".to_string(),
            command_lines: vec![
                "\"C:\\java\\bin\\javaw.exe\" --mainClass net.minecraft.client.main.Main --version 1.21"
                    .to_string(),
            ],
        };
        let no_command_line = RunningProcess {
            name: "javaw.exe".to_string(),
            command_lines: vec![],
        };

        // A bare javaw.exe (any Java application) must not be labeled Minecraft.
        assert!(!process_entry_matches(&java_app, "javaw.exe", marker));
        assert!(!process_entry_matches(
            &no_command_line,
            "javaw.exe",
            marker
        ));
        // The launcher's main class matches.
        assert!(process_entry_matches(&java_game, "javaw.exe", marker));

        // Marker-free profiles (Bedrock) still match on the image name alone.
        let bedrock = RunningProcess {
            name: "Minecraft.Windows.exe".to_string(),
            command_lines: vec![],
        };
        assert!(process_entry_matches(
            &bedrock,
            "Minecraft.Windows.exe",
            None
        ));

        // The profile is registered in all_games() under its CLI display name.
        let registered = all_games()
            .into_iter()
            .find(|g| g.name() == "Minecraft Java Edition")
            .expect("Minecraft Java Edition must be registered in all_games()");
        assert_eq!(
            registered.process_names().to_vec(),
            vec!["javaw.exe", "java.exe"]
        );
        assert_eq!(registered.transport(), TransportProto::Tcp);
    }

    #[test]
    fn test_java_cli_aliases_resolve() {
        for key in ["minecraft-java", "minecraftjava", "java"] {
            let game = detect_game(key).unwrap_or_else(|e| panic!("detect_game({key:?}): {e}"));
            assert_eq!(game.name(), "Minecraft Java Edition");
        }
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
