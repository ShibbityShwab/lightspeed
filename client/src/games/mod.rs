//! # Game Detection & Configuration
//!
//! Detects running games and provides game-specific tunnel configuration:
//! - Port ranges for packet capture
//! - Known server IP ranges
//! - Anti-cheat considerations
//! - Game-specific packet handling

pub mod cs2;
pub mod dota2;
pub mod fortnite;

use std::net::Ipv4Addr;

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
pub fn auto_detect() -> anyhow::Result<Box<dyn GameConfig>> {
    // TODO (WF-004 Step 1): Implement process scanning
    // 1. List running processes
    // 2. Match against known game process names
    // 3. Return the first match

    tracing::warn!("Game auto-detection not yet implemented");
    anyhow::bail!("No supported game detected. Use --game to specify manually.")
}
