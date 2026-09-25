//! # WARDOGS Game Configuration
//!
//! Game-specific settings for BULKHEAD's WARDOGS (published by Team17), the
//! 100-player, three-team tactical FPS built on Unreal Engine 5 (Steam App ID
//! 1867240; Early Access since 2026-09-10).
//!
//! ## Network Profile
//!
//! WARDOGS uses **dedicated authoritative servers**, not peer-to-peer: players
//! join through an in-game server browser (Official + Community, split by
//! region/population/ping) or a backend-issued join code, and community
//! servers are rented from Bulkhead-approved hosts. The game runs its own
//! backend and is not a Steam P2P title, so `uses_sdr()` is `false`. Server
//! addresses/ports are dynamic per session, so the interceptor must re-detect
//! rather than lock onto a pre-seeded host (`dynamic_server()` is `true`).
//!
//! ## Ports (UNVERIFIED)
//!
//! Bulkhead has **not published** a client port list. The dedicated server is
//! launched with an operator-chosen `-port`/`-hostport`, so there is no fixed
//! gameplay port to copy. The only documented default is the RCON web console
//! on port 7776 (`WDRCONSettings.Port`), which is *not* the gameplay port. The
//! range below (7777-7788) is an UNVERIFIED third-party observation around
//! Unreal's 7777 default; confirm it with a packet capture before relying on
//! it.
//!
//! ## Anti-Cheat
//!
//! WARDOGS ships a **kernel-level** anti-cheat. The Steam store page still
//! advertises Easy Anti-Cheat, but launch-period (Sept 2026) reports are
//! consistent that the enforced system is **Elytra** (VAIIYA Corp). LightSpeed
//! forwards UDP transparently without injecting code or touching memory. The
//! client is Windows-only; Proton/Linux is currently blocked by the kernel
//! anti-cheat, so capture must happen on Windows.

use super::GameConfig;

/// WARDOGS (BULKHEAD / Team17) game configuration.
pub struct WardogsConfig;

impl GameConfig for WardogsConfig {
    fn name(&self) -> &str {
        "WARDOGS"
    }

    fn process_names(&self) -> &[&str] {
        // `WardogsClient-Win64-Shipping.exe` is the gameplay client; Steam
        // launches `WardogsLauncher-Shipping.exe` first.
        &[
            "WardogsClient-Win64-Shipping.exe",
            "WardogsLauncher-Shipping.exe",
        ]
    }

    fn ports(&self) -> (u16, u16) {
        // UNVERIFIED: no published game port. The server binds an
        // operator-chosen `-port`; this range is a third-party observation
        // around Unreal's 7777 default. Confirm by capture.
        (7777, 7788)
    }

    fn anti_cheat(&self) -> &str {
        // Enforced system per launch-period reports; the Steam store page
        // still lists Easy Anti-Cheat.
        "Elytra (kernel-mode)"
    }

    fn uses_sdr(&self) -> bool {
        // Dedicated servers reached directly via the server browser / join
        // code, on the game's own backend. No Steam Datagram Relay evidence.
        false
    }

    fn dynamic_server(&self) -> bool {
        // Server browser / join codes: addresses and ports vary per session.
        true
    }

    fn typical_pps(&self) -> u32 {
        // UNVERIFIED estimate; a hosting partner reports a 30 Hz server tick.
        30
    }

    fn packet_size_range(&self) -> (usize, usize) {
        // UNVERIFIED: typical UE5 shooter input/state snapshots up to MTU.
        (64, 1200)
    }

    fn redirect_instructions(&self) -> String {
        "WARDOGS redirect mode:\n\
         1. WARDOGS uses dedicated servers, so capture/intercept mode is preferred\n\
         2. Find the server IP:port from the in-game server browser or join code\n\
         3. Start LightSpeed: --game wardogs --game-server <SERVER_IP>:<PORT>\n\
         4. Anti-cheat: kernel-level Elytra; transparent UDP tunneling only"
            .to_string()
    }
}
