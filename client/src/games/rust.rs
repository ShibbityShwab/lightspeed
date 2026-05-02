//! # Rust (Facepunch) Game Configuration
//!
//! Game-specific settings for Facepunch Studios' Rust — the open-world
//! survival multiplayer game.
//!
//! > **Note:** This profile is for the *game* Rust (by Facepunch), not the
//! > Rust programming language that LightSpeed is written in. Yes, we see the
//! > irony.
//!
//! ## Network Profile
//!
//! Rust uses direct UDP connections to community and official servers on the
//! default port **28015**. There is no Steam Datagram Relay (SDR) — every
//! session is a raw connection to the server's public IP, making Rust an
//! ideal candidate for LightSpeed's proxy optimisation.
//!
//! ## Anti-Cheat
//!
//! Rust uses two layers:
//! - **Easy Anti-Cheat (EAC)** — kernel-level driver, validates the game binary.
//! - **Facepunch Anti-Hack (Rust+)** — server-side behavioural detection.
//!
//! LightSpeed operates as a transparent UDP forwarder with no code injection
//! or memory modification, so it is compatible with both systems.

use super::GameConfig;
use crate::tunnel::capture::CaptureFilter;

/// Rust (Facepunch) game configuration.
pub struct RustConfig;

impl GameConfig for RustConfig {
    fn name(&self) -> &str {
        "Rust"
    }

    fn process_names(&self) -> &[&str] {
        &["RustClient.exe"]
    }

    fn ports(&self) -> (u16, u16) {
        // Default Rust server port is 28015 (UDP game traffic).
        // RCON is 28016 (TCP, not captured), query is 28017 (UDP).
        // We capture the game port range: 28015–28017.
        (28015, 28017)
    }

    fn redirect_port(&self) -> u16 {
        28015 // Default Rust game server port
    }

    fn redirect_instructions(&self) -> String {
        "Rust redirect mode:\n\
         1. Find your server IP from the F1 console or server browser\n\
         2. Start LightSpeed: --game rust --game-server <SERVER_IP>:28015\n\
         3. Connect in the F1 console: `client.connect 127.0.0.1:28015`\n\
         4. Anti-cheat: EAC + Facepunch Anti-Hack are compatible (transparent UDP tunnel)\n\
         5. Note: Rust uses direct UDP — no Steam Datagram Relay, ideal for proxying"
            .to_string()
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat (EAC) + Facepunch Anti-Hack"
    }

    fn uses_sdr(&self) -> bool {
        // Rust uses direct UDP to community/official servers — no SDR.
        false
    }

    fn typical_pps(&self) -> u32 {
        // Rust runs at 16-30 server tick depending on config.
        // Client sends ~20-30 packets/sec; server sends ~20-60 depending on
        // proximity to other players and active events.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // Rust packets range from small position updates to larger state syncs.
        (64, 1200)
    }

    /// Override the default BPF filter for Rust:
    ///
    /// 1. Try to enumerate ALL UDP sockets owned by `RustClient.exe` via
    ///    `netstat -ano`.  We OR every candidate port into a single BPF so that
    ///    whichever socket is actually carrying game data is captured.
    /// 2. Fall back to `udp portrange 28000-28100` (covers most community
    ///    servers) if the game isn't running or netstat fails.
    fn build_capture_filter(&self) -> CaptureFilter {
        let ports = detect_rust_all_ports();
        if !ports.is_empty() {
            tracing::info!(
                "🎮 RustClient.exe UDP sockets detected: {:?} — using multi-port BPF",
                ports
            );
            return CaptureFilter::new_multi_port(vec![], ports);
        }
        tracing::debug!(
            "RustClient.exe not found or has no UDP sockets — \
             using community range 28000-28100"
        );
        // Wide fallback covering the overwhelming majority of community servers.
        CaptureFilter::new(vec![], (28000, 28100))
    }
}

// ── netstat-based port detection ─────────────────────────────────────────────

/// Well-known non-game UDP ports that RustClient.exe opens purely for
/// Steam services (voice, matchmaking, streaming, etc.).  We skip these
/// when building the capture filter so we don't flood the tunnel with
/// Steam traffic.
#[cfg(target_os = "windows")]
const STEAM_SERVICE_PORTS: &[u16] = &[
    3478, 4379, 4380,  // Steam NAT punch / relay
    27005, // Steam client source / game server UDP
    27015, // Steam SRCDS / query
    27020, // Steam TV
    27036, // Steam remote play
    27037, // Steam remote play
];

/// Return ALL local UDP ports that `RustClient.exe` currently has open,
/// excluding well-known Steam-service ports and the LightSpeed tunnel port.
///
/// Returns an empty vec if the game isn't running or the commands fail.
fn detect_rust_all_ports() -> Vec<u16> {
    let pid = match rust_client_pid() {
        Some(p) => p,
        None => return vec![],
    };
    rust_udp_all_ports(pid)
}

/// Fetch the PID of the first running `RustClient.exe` process via `tasklist`.
#[cfg(target_os = "windows")]
fn rust_client_pid() -> Option<u32> {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq RustClient.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    // If the game is not running tasklist prints "INFO: No tasks running …"
    if text.trim().to_ascii_lowercase().starts_with("info:") || text.trim().is_empty() {
        return None;
    }

    text.lines()
        .filter_map(|line| {
            // CSV line: "RustClient.exe","12345","Console","1","123,456 K"
            let mut fields = line.split(',');
            let _name = fields.next()?;
            let pid_field = fields.next()?.trim().trim_matches('"');
            pid_field.parse::<u32>().ok()
        })
        .next()
}

#[cfg(not(target_os = "windows"))]
fn rust_client_pid() -> Option<u32> {
    None // netstat-based detection is Windows-only for now
}

/// Collect every local UDP port owned by `pid`, filtering out known
/// Steam-service ports and the LightSpeed proxy port.
#[cfg(target_os = "windows")]
fn rust_udp_all_ports(pid: u32) -> Vec<u16> {
    let output = match std::process::Command::new("netstat")
        .args(["-ano", "-p", "UDP"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    if !output.status.success() {
        return vec![];
    }

    let pid_str = pid.to_string();
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        // Windows netstat UDP line (4 whitespace-separated columns):
        //   UDP    <local_addr>:<port>    *:*    <pid>
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        if !parts[0].eq_ignore_ascii_case("UDP") {
            continue;
        }
        if parts[3] != pid_str {
            continue;
        }

        // Extract port from "addr:port" — use rsplit to handle IPv6 addresses.
        if let Some(port_str) = parts[1].rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                // Skip port 0, well-known ports, LightSpeed proxy port,
                // and known Steam service ports.
                if port >= 1024
                    && port != 4434
                    && !STEAM_SERVICE_PORTS.contains(&port)
                    && !ports.contains(&port)
                {
                    ports.push(port);
                }
            }
        }
    }

    tracing::debug!("RustClient.exe (PID {}) UDP ports: {:?}", pid, ports);
    ports
}

#[cfg(not(target_os = "windows"))]
fn rust_udp_all_ports(_pid: u32) -> Vec<u16> {
    vec![]
}
