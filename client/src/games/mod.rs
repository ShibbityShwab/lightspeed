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
//!
//! ## Capture Filters
//!
//! Each game provides a `CaptureFilter` via `build_capture_filter()` that
//! generates an appropriate BPF filter for pcap capture mode.

pub mod cs2;
pub mod dota2;
pub mod fortnite;

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
        "dota2" | "dota" => Ok(Box::new(dota2::Dota2Config)),
        _ => anyhow::bail!("Unknown game: '{}'. Supported: fortnite, cs2, dota2", name),
    }
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
        tracing::debug!("Scanning {} running processes for known games", processes.len());
    }

    // Check each supported game
    let all_games: Vec<Box<dyn GameConfig>> = vec![
        Box::new(fortnite::FortniteConfig),
        Box::new(cs2::Cs2Config),
        Box::new(dota2::Dota2Config),
    ];

    for game in all_games {
        for process_name in game.process_names() {
            if processes.iter().any(|p| p.eq_ignore_ascii_case(process_name)) {
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
        "dota2.exe",
    ];
    tracing::debug!(
        "No matching processes found. Looking for: {}",
        known_procs.join(", ")
    );

    anyhow::bail!(
        "No supported game detected. Use --game to specify manually.\n\
         Supported games: fortnite, cs2, dota2"
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

    #[test]
    fn test_detect_known_games() {
        assert!(detect_game("fortnite").is_ok());
        assert!(detect_game("cs2").is_ok());
        assert!(detect_game("counter-strike").is_ok());
        assert!(detect_game("dota2").is_ok());
        assert!(detect_game("dota").is_ok());
    }

    #[test]
    fn test_detect_unknown_game() {
        assert!(detect_game("minecraft").is_err());
    }

    #[test]
    fn test_game_config_properties() {
        let cs2 = cs2::Cs2Config;
        assert_eq!(cs2.name(), "Counter-Strike 2");
        assert!(cs2.process_names().contains(&"cs2.exe"));
        assert_eq!(cs2.ports(), (27015, 27050));
        assert_eq!(cs2.redirect_port(), 27015);
        assert!(cs2.typical_pps() > 0);

        let fortnite = fortnite::FortniteConfig;
        assert_eq!(fortnite.name(), "Fortnite");
        assert_eq!(fortnite.redirect_port(), 7777);

        let dota = dota2::Dota2Config;
        assert_eq!(dota.name(), "Dota 2");
    }

    #[test]
    fn test_build_capture_filter() {
        let cs2 = cs2::Cs2Config;
        let filter = cs2.build_capture_filter();
        assert!(filter.bpf.contains("udp"));
        assert!(filter.bpf.contains("27015"));
        assert_eq!(filter.port_range, (27015, 27050));
    }

    #[test]
    fn test_list_processes_doesnt_panic() {
        // Just verify it doesn't crash — may return empty on CI
        let procs = list_running_processes();
        // On a real system, there should be at least some processes
        // But in CI/containers this could be empty
        let _ = procs;
    }
}
