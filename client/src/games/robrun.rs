//! # ROB RUN Game Configuration
//!
//! Game-specific settings for Kefir's ROB RUN, measured on a live session
//! rather than taken from a report.
//!
//! ## Network Profile
//!
//! Kefir runs every match server, so there is no player-hosted port to target.
//! A 45-second capture of a live session on this machine showed the client
//! talking to `91.199.82.233:36703`, `91.199.82.163:36502`, `34.38.12.1:36006`
//! and `78.14.208.199:54748` - each server listening on a different
//! ephemeral-range port, and a new one per session. The port range below is
//! therefore the standard ephemeral band rather than a fixed block; pinning a
//! narrow range would miss the next server.
//!
//! The client also holds a pool of local sockets (49186, 57865, a 58708-58735
//! block, 60820, 62941), so the interceptor's process-attributed capture is the
//! reliable path here: the network-layer filter cannot match a process id, and
//! a server-port filter alone is a guess.
//!
//! ## Anti-Cheat
//!
//! ROB RUN ships **Easy Anti-Cheat**. LightSpeed forwards UDP transparently
//! without injecting code or touching memory, so the tunnel does not interact
//! with it.

use super::GameConfig;

/// ROB RUN (Kefir) game configuration.
pub struct RobRunConfig;

impl GameConfig for RobRunConfig {
    fn name(&self) -> &str {
        "ROB RUN"
    }

    fn process_names(&self) -> Vec<&str> {
        // The image carries a space in its name; Windows reports it that way.
        ["ROB RUN.exe"].to_vec()
    }

    fn ports(&self) -> (u16, u16) {
        // Measured server ports (36006, 36502, 36703, 54748) all fall in the
        // ephemeral band, and a session picks a new one.
        (32768, 60999)
    }

    fn anti_cheat(&self) -> &str {
        "Easy Anti-Cheat"
    }

    fn uses_sdr(&self) -> bool {
        false
    }

    fn dynamic_server(&self) -> bool {
        // Kefir assigns the match server; the address and port change per
        // session, so a pre-seeded lock would go stale.
        true
    }

    fn typical_pps(&self) -> u32 {
        // ~410 packets/s across the session measured on this host, counting
        // both directions of the gameplay flow.
        205
    }

    fn packet_size_range(&self) -> (usize, usize) {
        (40, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "ROB RUN redirect mode:\n\
         1. Kefir runs the match servers and assigns one per session on an ephemeral port\n\
         2. Start LightSpeed: --game robrun --proxy <RELAY>:4434 (auto-detect finds the process)\n\
         3. Pin the server only for the session you are in: --game-server <IP>:<PORT>\n\
         4. Anti-cheat: Easy Anti-Cheat; transparent UDP tunneling only, no packet modification"
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn robrun_ports_cover_the_measured_servers() {
        let (lo, hi) = RobRunConfig.ports();
        for port in [36006u16, 36502, 36703, 54748] {
            assert!(
                (lo..=hi).contains(&port),
                "measured server port {port} must be inside the capture range"
            );
        }
    }

    #[test]
    fn robrun_matches_the_measured_process_name() {
        let names = RobRunConfig.process_names();
        assert!(names.iter().any(|n| n.eq_ignore_ascii_case("ROB RUN.exe")));
    }

    #[test]
    fn robrun_servers_are_dynamic() {
        let cfg = RobRunConfig;
        assert!(cfg.dynamic_server());
        assert!(!cfg.uses_sdr());
    }
}
