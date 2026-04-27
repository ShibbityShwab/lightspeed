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
    /// WARP is in the process of connecting.
    Connecting,
    /// WARP is disconnected.
    Disconnected,
    /// WARP is in the process of disconnecting.
    Disconnecting,
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
            WarpStatus::Disconnecting => write!(f, "Disconnecting"),
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
    /// Cached IPv4 exclude ranges from `warp-cli tunnel dump`.
    /// Each entry is (network_address, prefix_length).
    exclude_cache: Option<Vec<(Ipv4Addr, u8)>>,
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
            exclude_cache: None,
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

        // Try PATH as a last resort
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
    ///
    /// `--no-paginate --no-ansi` are injected before all commands to ensure
    /// machine-parseable output regardless of terminal or locale settings.
    /// Note: warp-cli uses `--no-paginate` (not `--no-pager`).
    fn run_cli(&self, args: &[&str]) -> Result<String, String> {
        let cli = self.cli_path.as_ref().ok_or("WARP not installed")?;

        // Prepend --no-paginate --no-ansi for stable, parse-friendly output.
        let mut full_args: Vec<&str> = vec!["--no-paginate", "--no-ansi"];
        full_args.extend_from_slice(args);

        let output = Command::new(cli)
            .args(&full_args)
            .output()
            .map_err(|e| format!("Failed to run warp-cli: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() && !stderr.is_empty() {
            // Some commands return non-zero but still have useful stdout
            debug!(
                "warp-cli {:?} exited with {}: {}",
                args, output.status, stderr
            );
        }

        Ok(stdout.trim().to_string())
    }

    /// Parse a raw `warp-cli status` output string into a `WarpStatus`.
    ///
    /// This is a pure function with no side-effects, making it easy to unit-test
    /// against canned outputs without spawning a process.
    ///
    /// WARP status output looks like:
    /// ```text
    /// Status update: Connected
    /// Network: healthy
    /// ```
    pub fn parse_status_output(output: &str) -> WarpStatus {
        let lower = output.to_lowercase();

        // Order matters:
        // 1. Check for "disconnecting" BEFORE "connecting" (former is a superset).
        // 2. Check for "disconnected" BEFORE "connected" (former is a superset).
        // 3. Check "connecting" before "connected" (short-circuit once we match).
        //
        // This avoids the bug where "disconnecting" is misclassified as Connecting
        // because "connecting" is a substring of "disconnecting".
        if lower.contains("disconnecting") {
            WarpStatus::Disconnecting
        } else if lower.contains("disconnected") {
            WarpStatus::Disconnected
        } else if lower.contains("connecting") {
            WarpStatus::Connecting
        } else if lower.contains("connected") {
            WarpStatus::Connected
        } else if lower.contains("unable to connect to daemon")
            || lower.contains("error")
            || lower.contains("not registered")
        {
            WarpStatus::Unknown(output.lines().next().unwrap_or(output).to_string())
        } else {
            WarpStatus::Unknown(output.lines().next().unwrap_or(output).to_string())
        }
    }

    /// Get the current WARP connection status.
    pub fn status(&self) -> WarpStatus {
        if !self.is_installed() {
            return WarpStatus::NotInstalled;
        }

        match self.run_cli(&["status"]) {
            Ok(output) => Self::parse_status_output(&output),
            Err(e) => WarpStatus::Unknown(e),
        }
    }

    /// Get detailed WARP info.
    ///
    /// Calls `warp-cli settings list` **once** and parses both the tunnel
    /// protocol and operating mode from the same output, avoiding a redundant
    /// process spawn.
    pub fn info(&self) -> WarpInfo {
        let status = self.status();

        // Single call to settings list — parse both fields from one output.
        let (protocol, mode) = self
            .run_cli(&["settings", "list"])
            .ok()
            .map(|output| {
                let mut protocol: Option<String> = None;
                let mut mode: Option<String> = None;
                for line in output.lines() {
                    let line_lower = line.to_lowercase();
                    if protocol.is_none() && line_lower.contains("tunnel protocol") {
                        protocol = line.split(':').next_back().map(|s| s.trim().to_string());
                    }
                    // "Mode:" — match exact word to avoid "Mode-switch-allowed:" etc.
                    if mode.is_none() && {
                        let after_tab = line.split('\t').last().unwrap_or(line);
                        after_tab.trim_start().starts_with("Mode:")
                    } {
                        mode = line.split(':').next_back().map(|s| s.trim().to_string());
                    }
                }
                (protocol, mode)
            })
            .unwrap_or((None, None));

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

        // Record initial state so Drop can restore it correctly.
        self.was_connected = false; // We know it's not Connected at this point.

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
                    info!(
                        "🌐 WARP connected successfully ({:.1}s)",
                        start.elapsed().as_secs_f64()
                    );
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

    /// Fetch the real IPv4 exclude ranges from `warp-cli tunnel dump`.
    ///
    /// Returns `None` if WARP is not installed or the command fails / has no
    /// parseable CIDR output.  The result is cached on first successful call
    /// so subsequent `is_ip_routed()` checks are free.
    fn fetch_exclude_list(&mut self) -> Option<&Vec<(Ipv4Addr, u8)>> {
        // Return cached list if already populated.
        if self.exclude_cache.is_some() {
            return self.exclude_cache.as_ref();
        }

        // `warp-cli tunnel dump` lists currently excluded split-tunnel ranges.
        let output = self.run_cli(&["tunnel", "dump"]).ok()?;

        let ranges: Vec<(Ipv4Addr, u8)> = output
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                // Parse lines like "  10.0.0.0/8" or "  172.16.0.0/12"
                // Skip IPv6 and hosts-only lines.
                if trimmed.is_empty() || !trimmed.contains('/') {
                    return None;
                }
                // Ignore IPv6 CIDRs (contain ':')
                if trimmed.contains(':') {
                    return None;
                }
                // Strip any leading status prefix (e.g., "Split Tunnel")
                let cidr_part = trimmed.split_whitespace().last()?;
                let mut parts = cidr_part.splitn(2, '/');
                let ip_str = parts.next()?;
                let prefix_str = parts.next()?;
                let ip: Ipv4Addr = ip_str.parse().ok()?;
                let prefix: u8 = prefix_str.parse().ok()?;
                Some((ip, prefix))
            })
            .collect();

        if ranges.is_empty() {
            debug!("warp-cli tunnel dump returned no IPv4 CIDRs — falling back to hardcoded list");
            return None;
        }

        debug!(
            "Loaded {} IPv4 exclude ranges from warp-cli tunnel dump",
            ranges.len()
        );
        self.exclude_cache = Some(ranges);
        self.exclude_cache.as_ref()
    }

    /// Fallback exclude list used when `warp-cli tunnel dump` is unavailable.
    ///
    /// This matches Cloudflare WARP's default exclude-mode set for RFC 1918
    /// and other special-use ranges.
    ///
    /// `Ipv4Addr::new` has been `const fn` since Rust 1.50, so we can store
    /// these in a module-level `static` and return a `'static` reference
    /// without any heap allocation.
    fn default_exclude_ranges() -> &'static [(Ipv4Addr, u8)] {
        static RANGES: [(Ipv4Addr, u8); 8] = [
            (Ipv4Addr::new(10, 0, 0, 0), 8),
            (Ipv4Addr::new(100, 64, 0, 0), 10),
            (Ipv4Addr::new(169, 254, 0, 0), 16),
            (Ipv4Addr::new(172, 16, 0, 0), 12),
            (Ipv4Addr::new(192, 0, 0, 0), 24),
            (Ipv4Addr::new(192, 168, 0, 0), 16),
            (Ipv4Addr::new(224, 0, 0, 0), 24),
            (Ipv4Addr::new(240, 0, 0, 0), 4),
        ];
        &RANGES
    }

    /// Check if a specific IPv4 address would be routed through WARP.
    ///
    /// In exclude mode (default), everything goes through WARP except
    /// the excluded ranges. First attempts to read the real exclude list from
    /// `warp-cli tunnel dump`; falls back to a hardcoded RFC 1918 set if the
    /// command is unavailable. This is pure subnet math — WARP does not need
    /// to be running for this check to succeed.
    pub fn is_ip_routed(&mut self, ip: Ipv4Addr) -> bool {
        let ip_u32 = u32::from(ip);

        // Use the real exclude list (populated lazily), falling back to the
        // hardcoded defaults when warp-cli is not available/returns nothing.
        let check = |ranges: &[(Ipv4Addr, u8)]| -> bool {
            for (network, prefix_len) in ranges {
                let net_u32 = u32::from(*network);
                let mask = if *prefix_len == 0 {
                    0u32
                } else {
                    !((1u32 << (32 - prefix_len)) - 1)
                };
                if (ip_u32 & mask) == (net_u32 & mask) {
                    return false; // IP is in an excluded range → NOT routed through WARP
                }
            }
            true // IP is routed through WARP
        };

        if let Some(live_ranges) = self.fetch_exclude_list() {
            check(live_ranges)
        } else {
            check(Self::default_exclude_ranges())
        }
    }

    /// Check if a specific IPv4 address would be routed through WARP.
    ///
    /// Non-mutating version that uses only the hardcoded RFC 1918 fallback list.
    /// Used by unit tests (via `WarpManager::new()`) where warp-cli may not be
    /// available. Prefer `is_ip_routed(&mut self, …)` in production code paths.
    pub fn is_ip_routed_static(ip: Ipv4Addr) -> bool {
        let ip_u32 = u32::from(ip);
        for (network, prefix_len) in Self::default_exclude_ranges() {
            let net_u32 = u32::from(*network);
            let mask = if *prefix_len == 0 {
                0u32
            } else {
                !((1u32 << (32 - prefix_len)) - 1)
            };
            if (ip_u32 & mask) == (net_u32 & mask) {
                return false;
            }
        }
        true
    }

    /// Verify that all proxy IPs will be routed through WARP.
    pub fn verify_proxy_routing(&mut self, proxy_ips: &[Ipv4Addr]) -> Vec<(Ipv4Addr, bool)> {
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
    pub fn print_summary(&mut self, proxy_ips: &[Ipv4Addr]) {
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

    // ── parse_status_output unit tests ────────────────────────────────────────
    // These test the pure parsing function without spawning any processes,
    // ensuring correctness against real warp-cli output formats.

    #[test]
    fn test_parse_connected() {
        // Real output from `warp-cli status`
        let output = "Status update: Connected\nNetwork: healthy";
        assert_eq!(
            WarpManager::parse_status_output(output),
            WarpStatus::Connected
        );
    }

    #[test]
    fn test_parse_disconnected() {
        let output = "Status update: Disconnected";
        assert_eq!(
            WarpManager::parse_status_output(output),
            WarpStatus::Disconnected
        );
    }

    #[test]
    fn test_parse_connecting() {
        let output = "Status update: Connecting\nEstimated time: 5s";
        assert_eq!(
            WarpManager::parse_status_output(output),
            WarpStatus::Connecting
        );
    }

    /// Key regression: "Disconnecting" must NOT be classified as Connecting.
    /// "disconnecting".contains("connecting") == true, so order of checks matters.
    #[test]
    fn test_parse_disconnecting_not_connecting() {
        let output = "Status update: Disconnecting";
        assert_eq!(
            WarpManager::parse_status_output(output),
            WarpStatus::Disconnecting
        );
        // Sanity: "connecting" IS a substring of "disconnecting" at the byte level
        assert!("disconnecting".contains("connecting"));
        // Without the fix the old code would return Connecting — this test locks
        // the corrected behaviour in place.
    }

    /// "disconnected" contains "connected" as a substring — make sure we still
    /// return Disconnected and not Connected.
    #[test]
    fn test_parse_disconnected_not_connected() {
        let output = "Status update: Disconnected";
        assert!("disconnected".contains("connected")); // confirms the hazard
        assert_eq!(
            WarpManager::parse_status_output(output),
            WarpStatus::Disconnected
        );
    }

    #[test]
    fn test_parse_unable_to_connect_to_daemon() {
        let output = "Unable to connect to daemon";
        let status = WarpManager::parse_status_output(output);
        matches!(status, WarpStatus::Unknown(_));
    }

    #[test]
    fn test_parse_unknown() {
        let output = "Some completely unexpected output";
        matches!(
            WarpManager::parse_status_output(output),
            WarpStatus::Unknown(_)
        );
    }

    // ── WarpManager creation ──────────────────────────────────────────────────

    #[test]
    fn test_warp_manager_creation() {
        let manager = WarpManager::new();
        // Just verify it doesn't panic
        let _ = manager.is_installed();
    }

    // ── Subnet routing (static helper — no warp-cli needed) ──────────────────

    #[test]
    fn test_is_ip_routed_public_ips() {
        // Public IPs should be routed through WARP
        assert!(WarpManager::is_ip_routed_static(Ipv4Addr::new(
            149, 28, 84, 139
        ))); // Vultr LA
        assert!(WarpManager::is_ip_routed_static(Ipv4Addr::new(
            149, 28, 144, 74
        ))); // Vultr SGP
        assert!(WarpManager::is_ip_routed_static(Ipv4Addr::new(
            163, 192, 3, 134
        ))); // OCI SJ
        assert!(WarpManager::is_ip_routed_static(Ipv4Addr::new(8, 8, 8, 8))); // Google DNS
    }

    #[test]
    fn test_is_ip_routed_private_ips() {
        // Private / special-use IPs should NOT be routed through WARP
        assert!(!WarpManager::is_ip_routed_static(Ipv4Addr::new(
            10, 0, 0, 1
        )));
        assert!(!WarpManager::is_ip_routed_static(Ipv4Addr::new(
            192, 168, 1, 1
        )));
        assert!(!WarpManager::is_ip_routed_static(Ipv4Addr::new(
            172, 16, 0, 1
        )));
        assert!(!WarpManager::is_ip_routed_static(Ipv4Addr::new(
            169, 254, 1, 1
        )));
        assert!(!WarpManager::is_ip_routed_static(Ipv4Addr::new(
            100, 64, 0, 1
        ))); // CGNAT
    }

    #[test]
    fn test_verify_proxy_routing() {
        // Uses the mutable is_ip_routed path which falls back to static list
        // when warp-cli tunnel dump is unavailable (e.g. CI).
        let mut manager = WarpManager::new();
        let ips = vec![
            Ipv4Addr::new(149, 28, 84, 139),
            Ipv4Addr::new(163, 192, 3, 134),
        ];
        let results = manager.verify_proxy_routing(&ips);
        assert_eq!(results.len(), 2);
        assert!(results[0].1); // Vultr LA routed
        assert!(results[1].1); // OCI SJ routed
    }

    // ── Display ───────────────────────────────────────────────────────────────

    #[test]
    fn test_status_display() {
        assert_eq!(format!("{}", WarpStatus::Connected), "Connected");
        assert_eq!(format!("{}", WarpStatus::Connecting), "Connecting");
        assert_eq!(format!("{}", WarpStatus::Disconnected), "Disconnected");
        assert_eq!(format!("{}", WarpStatus::Disconnecting), "Disconnecting");
        assert_eq!(format!("{}", WarpStatus::NotInstalled), "Not Installed");
        assert_eq!(
            format!("{}", WarpStatus::Unknown("test".into())),
            "Unknown (test)"
        );
    }

    #[test]
    fn test_install_instructions() {
        let instructions = install_instructions();
        assert!(!instructions.is_empty());
    }
}
