//! # Cloudflare WARP Integration
//!
//! Detects and manages Cloudflare WARP to improve routing for LightSpeed
//! tunnel traffic. WARP routes through Cloudflare's NTT backbone, bypassing
//! ISP routing inefficiencies (e.g., HGC detour on BKK→US West routes).
//!
//! ## How it helps
//!
//! - **5-10ms latency improvement** by using NTT peering instead of ISP default
//! - **MASQUE protocol** tunnels all traffic including UDP game packets
//! - **Free tier** — no cost for the peering improvement
//!
//! ## Architecture
//!
//! ```text
//! Game → LightSpeed Client → [WARP/MASQUE] → CF Edge (NTT) → Proxy → Game Server
//! ```
//!
//! The WARP tunnel is transparent — LightSpeed packets are sent normally
//! to the proxy IP, but the OS routes them through WARP's virtual interface,
//! which uses Cloudflare's optimized backbone.

use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

/// WARP connection status.
#[derive(Debug, Clone, PartialEq)]
pub enum WarpStatus {
    /// WARP is connected and routing traffic.
    Connected,
    /// WARP is connecting.
    Connecting,
    /// WARP is disconnected.
    Disconnected,
    /// WARP is not installed on this system.
    NotInstalled,
    /// WARP status could not be determined.
    Unknown(String),
}

impl std::fmt::Display for WarpStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WarpStatus::Connected => write!(f, "Connected"),
            WarpStatus::Connecting => write!(f, "Connecting"),
            WarpStatus::Disconnected => write!(f, "Disconnected"),
            WarpStatus::NotInstalled => write!(f, "Not Installed"),
            WarpStatus::Unknown(s) => write!(f, "Unknown ({})", s),
        }
    }
}

/// WARP tunnel protocol info.
#[derive(Debug, Clone)]
pub struct WarpInfo {
    pub status: WarpStatus,
    pub protocol: Option<String>,
    pub mode: Option<String>,
}

/// Manager for Cloudflare WARP integration.
///
/// Handles detection, connection management, and health monitoring.
pub struct WarpManager {
    /// Path to the warp-cli binary.
    cli_path: Option<PathBuf>,
    /// Whether WARP was connected by us (so we can restore on shutdown).
    connected_by_us: bool,
    /// Whether WARP was already connected when we started.
    was_connected: bool,
}

impl WarpManager {
    /// Create a new WARP manager.
    ///
    /// Automatically detects the warp-cli binary location.
    pub fn new() -> Self {
        let cli_path = Self::find_warp_cli();
        if let Some(ref path) = cli_path {
            debug!("Found warp-cli at: {}", path.display());
        }
        Self {
            cli_path,
            connected_by_us: false,
            was_connected: false,
        }
    }

    /// Find the warp-cli binary on the system.
    fn find_warp_cli() -> Option<PathBuf> {
        // Windows default locations
        #[cfg(target_os = "windows")]
        {
            let paths = [
                r"C:\Program Files\Cloudflare\Cloudflare WARP\warp-cli.exe",
                r"C:\Program Files (x86)\Cloudflare\Cloudflare WARP\warp-cli.exe",
            ];
            for path in &paths {
                let p = PathBuf::from(path);
                if p.exists() {
                    return Some(p);
                }
            }
        }

        // macOS default location
        #[cfg(target_os = "macos")]
        {
            let paths = [
                "/usr/local/bin/warp-cli",
                "/Applications/Cloudflare WARP.app/Contents/Resources/warp-cli",
            ];
            for path in &paths {
                let p = PathBuf::from(path);
                if p.exists() {
                    return Some(p);
                }
            }
        }

        // Linux default location
        #[cfg(target_os = "linux")]
        {
            let p = PathBuf::from("/usr/bin/warp-cli");
            if p.exists() {
                return Some(p);
            }
        }

        // Try PATH
        if let Ok(output) = Command::new("warp-cli").arg("--version").output() {
            if output.status.success() {
                return Some(PathBuf::from("warp-cli"));
            }
        }

        None
    }

    /// Check if WARP is installed.
    pub fn is_installed(&self) -> bool {
        self.cli_path.is_some()
    }

    /// Run a warp-cli command and return stdout.
    fn run_cli(&self, args: &[&str]) -> Result<String, String> {
        let cli = self.cli_path.as_ref().ok_or("WARP not installed")?;

        let output = Command::new(cli)
            .args(args)
            .output()
            .map_err(|e| format!("Failed to run warp-cli: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() && !stdout.is_empty() {
            // Some commands return non-zero but still have useful output
            debug!("warp-cli {:?} exited with {}: {}", args, output.status, stderr);
        }

        Ok(stdout.trim().to_string())
    }

    /// Get the current WARP connection status.
    pub fn status(&self) -> WarpStatus {
        if !self.is_installed() {
            return WarpStatus::NotInstalled;
        }

        match self.run_cli(&["status"]) {
            Ok(output) => {
                let lower = output.to_lowercase();
                if lower.contains("connected") && !lower.contains("disconnected") {
                    WarpStatus::Connected
                } else if lower.contains("connecting") {
                    WarpStatus::Connecting
                } else if lower.contains("disconnected") {
                    WarpStatus::Disconnected
                } else {
                    WarpStatus::Unknown(output)
                }
            }
            Err(e) => WarpStatus::Unknown(e),
        }
    }

    /// Get detailed WARP info.
    pub fn info(&self) -> WarpInfo {
        let status = self.status();

        let protocol = self.run_cli(&["settings", "list"]).ok().and_then(|output| {
            for line in output.lines() {
                if line.contains("tunnel protocol") {
                    return Some(line.split(':').last()?.trim().to_string());
                }
            }
            None
        });

        let mode = self.run_cli(&["settings", "list"]).ok().and_then(|output| {
            for line in output.lines() {
                if line.contains("Mode:") {
                    return Some(line.split(':').last()?.trim().to_string());
                }
            }
            None
        });

        WarpInfo {
            status,
            protocol,
            mode,
        }
    }

    /// Connect WARP (enable the tunnel).
    ///
    /// Records whether we initiated the connection so we can restore
    /// the previous state on shutdown.
    pub fn connect(&mut self) -> Result<(), String> {
        if !self.is_installed() {
            return Err("WARP is not installed".to_string());
        }

        // Check current status
        let current = self.status();
        if current == WarpStatus::Connected {
            info!("🌐 WARP already connected");
            self.was_connected = true;
            return Ok(());
        }

        self.was_connected = current == WarpStatus::Connected;

        info!("🌐 Connecting WARP...");
        let output = self.run_cli(&["connect"])?;
        debug!("warp-cli connect: {}", output);

        // Wait for connection (up to 10 seconds)
        let start = Instant::now();
        let timeout = Duration::from_secs(10);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let status = self.status();
            match status {
                WarpStatus::Connected => {
                    self.connected_by_us = true;
                    info!("🌐 WARP connected successfully ({:.1}s)", start.elapsed().as_secs_f64());
                    return Ok(());
                }
                WarpStatus::Connecting => {
                    debug!("WARP still connecting...");
                }
                _ => {
                    if start.elapsed() > timeout {
                        return Err(format!(
                            "WARP failed to connect within {}s (status: {})",
                            timeout.as_secs(),
                            status
                        ));
                    }
                }
            }
            if start.elapsed() > timeout {
                return Err("WARP connection timed out".to_string());
            }
        }
    }

    /// Disconnect WARP (only if we connected it).
    ///
    /// If WARP was already connected before LightSpeed started,
    /// we leave it connected.
    pub fn disconnect(&mut self) -> Result<(), String> {
        if !self.is_installed() {
            return Ok(());
        }

        if self.connected_by_us && !self.was_connected {
            info!("🌐 Disconnecting WARP (was not connected before LightSpeed)");
            let output = self.run_cli(&["disconnect"])?;
            debug!("warp-cli disconnect: {}", output);
            self.connected_by_us = false;
        } else if self.was_connected {
            info!("🌐 Leaving WARP connected (was connected before LightSpeed)");
        }

        Ok(())
    }

    /// Check if a specific IP is routed through WARP.
    ///
    /// In exclude mode (default), everything goes through WARP except
    /// the excluded ranges. We check that the proxy IP is NOT in the
    /// excluded ranges.
    pub fn is_ip_routed(&self, ip: Ipv4Addr) -> bool {
        if !self.is_installed() {
            return false;
        }

        // Check if IP is in the excluded ranges
        let excluded_ranges: Vec<(Ipv4Addr, u8)> = vec![
            (Ipv4Addr::new(10, 0, 0, 0), 8),
            (Ipv4Addr::new(100, 64, 0, 0), 10),
            (Ipv4Addr::new(169, 254, 0, 0), 16),
            (Ipv4Addr::new(172, 16, 0, 0), 12),
            (Ipv4Addr::new(192, 0, 0, 0), 24),
            (Ipv4Addr::new(192, 168, 0, 0), 16),
            (Ipv4Addr::new(224, 0, 0, 0), 24),
            (Ipv4Addr::new(240, 0, 0, 0), 4),
        ];

        let ip_u32 = u32::from(ip);
        for (network, prefix_len) in &excluded_ranges {
            let net_u32 = u32::from(*network);
            let mask = if *prefix_len == 0 {
                0
            } else {
                !((1u32 << (32 - prefix_len)) - 1)
            };
            if (ip_u32 & mask) == (net_u32 & mask) {
                return false; // IP is in excluded range
            }
        }

        true // IP is routed through WARP
    }

    /// Verify that all proxy IPs will be routed through WARP.
    pub fn verify_proxy_routing(&self, proxy_ips: &[Ipv4Addr]) -> Vec<(Ipv4Addr, bool)> {
        proxy_ips
            .iter()
            .map(|ip| (*ip, self.is_ip_routed(*ip)))
            .collect()
    }

    /// Get WARP tunnel statistics.
    pub fn tunnel_stats(&self) -> Option<String> {
        self.run_cli(&["tunnel", "stats"]).ok()
    }

    /// Print a summary of WARP status and routing for the user.
    pub fn print_summary(&self, proxy_ips: &[Ipv4Addr]) {
        let info = self.info();

        info!("🌐 WARP Status: {}", info.status);
        if let Some(ref proto) = info.protocol {
            info!("   Protocol: {}", proto);
        }
        if let Some(ref mode) = info.mode {
            info!("   Mode: {}", mode);
        }

        if info.status == WarpStatus::Connected {
            let routing = self.verify_proxy_routing(proxy_ips);
            for (ip, routed) in &routing {
                if *routed {
                    info!("   ✅ {} → routed through WARP (NTT backbone)", ip);
                } else {
                    warn!("   ❌ {} → NOT routed through WARP (excluded)", ip);
                }
            }
        }
    }
}

impl Drop for WarpManager {
    fn drop(&mut self) {
        if let Err(e) = self.disconnect() {
            warn!("Failed to disconnect WARP on shutdown: {}", e);
        }
    }
}

/// Quick helper: detect WARP and return status without managing connection.
pub fn detect_warp() -> WarpStatus {
    WarpManager::new().status()
}

/// Helper: check if WARP is installed and get install instructions if not.
pub fn install_instructions() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "Download from: https://1.1.1.1/\nOr: winget install Cloudflare.Warp"
    }
    #[cfg(target_os = "macos")]
    {
        "Download from: https://1.1.1.1/\nOr: brew install cloudflare-warp"
    }
    #[cfg(target_os = "linux")]
    {
        "Install: https://developers.cloudflare.com/warp-client/get-started/linux/"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "Download from: https://1.1.1.1/"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warp_manager_creation() {
        let manager = WarpManager::new();
        // Just verify it doesn't panic
        let _ = manager.is_installed();
    }

    #[test]
    fn test_is_ip_routed() {
        let manager = WarpManager::new();

        // Public IPs should be routed through WARP
        assert!(manager.is_ip_routed(Ipv4Addr::new(149, 28, 84, 139))); // Vultr LA
        assert!(manager.is_ip_routed(Ipv4Addr::new(149, 28, 144, 74))); // Vultr SGP
        assert!(manager.is_ip_routed(Ipv4Addr::new(163, 192, 3, 134))); // OCI SJ
        assert!(manager.is_ip_routed(Ipv4Addr::new(8, 8, 8, 8)));       // Google DNS

        // Private IPs should NOT be routed through WARP
        assert!(!manager.is_ip_routed(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!manager.is_ip_routed(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!manager.is_ip_routed(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(!manager.is_ip_routed(Ipv4Addr::new(169, 254, 1, 1)));
    }

    #[test]
    fn test_verify_proxy_routing() {
        let manager = WarpManager::new();
        let ips = vec![
            Ipv4Addr::new(149, 28, 84, 139),
            Ipv4Addr::new(163, 192, 3, 134),
        ];
        let results = manager.verify_proxy_routing(&ips);
        assert_eq!(results.len(), 2);
        assert!(results[0].1); // Vultr LA routed
        assert!(results[1].1); // OCI SJ routed
    }

    #[test]
    fn test_status_display() {
        assert_eq!(format!("{}", WarpStatus::Connected), "Connected");
        assert_eq!(format!("{}", WarpStatus::Disconnected), "Disconnected");
        assert_eq!(format!("{}", WarpStatus::NotInstalled), "Not Installed");
    }

    #[test]
    fn test_install_instructions() {
        let instructions = install_instructions();
        assert!(!instructions.is_empty());
    }
}
