//! Adopted live session modes (post-W2 task-57).
//!
//! The CLI's live modes used to be front-end adapters because the engine could
//! not express what they print, the adaptive-FEC policy they resolve, the
//! redirect options/counters they expose, or an in-session relay switch. The
//! additive engine surface (WF-081b checkpoint 1) closed those gaps; this module
//! is the shared home of the migrated loops, so the binary stops carrying a
//! second engine-less session model (`client/src/modes/watch_mode.rs` retired;
//! `intercept_mode.rs` and the redirect branch follow).
//!
//! ## Why library-side
//!
//! The binary (`client/src/main.rs`) declares its own `mod` tree of the shared
//! sources, so `modes/*`, `quic`, `session` and `telemetry` are compiled into two
//! crates with **separate process-global statics**. [`crate::LightSpeedEngine`]
//! is library-only and starts the library data plane; a mode migrated onto it
//! must therefore run its whole session (registration, reroute, telemetry flush)
//! against the library copies too, or the data plane and the session would
//! silently diverge. Unmigrated modes keep the binary copies until they migrate.
//!
//! ## Parity
//!
//! Every user-visible line of a migrated mode is produced by a pure builder in
//! this module's submodules and pinned byte-for-byte in their tests, so the
//! migration cannot drift the CLI text.
pub mod interceptor;
pub mod redirect;
pub mod reroute;
pub mod watch;

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::app::runtime::SessionCore;
use crate::engine::AdaptivePolicy;

/// One adopted live session, resolved by the CLI dispatch.
pub struct LiveSession {
    /// Game profile key (`rust`, `cs2`, …).
    pub game_key: String,
    /// Data-plane relay chosen for the session.
    pub relay: SocketAddrV4,
    /// Relay candidates for the (parity-preserved) continuous rerouting loop.
    pub candidates: Vec<(String, String, String)>,
    /// Game server reported to the relay during registration, when known
    /// (`--game-server` / `--server-addr`).
    pub destination: Option<Ipv4Addr>,
    /// Known server seeded as an interceptor route (`--server-addr`).
    pub server_override: Option<SocketAddrV4>,
    /// Fixed-FEC opt-in.
    pub fec: bool,
    /// Fixed FEC block size.
    pub fec_k: u8,
    /// Adaptive-FEC policy as the CLI resolved it.
    pub adaptive: AdaptivePolicy,
    /// Whether the session reports anonymous telemetry.
    pub telemetry_enabled: bool,
    /// Relay data port (rerouting probe).
    pub data_port: u16,
    /// Relay control port (registration/rerouting).
    pub control_port: u16,
    /// Route strategy name (`nearest`, `destination-aware`, …).
    pub route_strategy: String,
    /// Multipath path count (1 = off).
    pub multipath_max_paths: u8,
    /// Effective client-to-relay path-MTU cap.
    pub path_mtu_cap: usize,
    /// Game server the rerouting loop ranks for (unspecified = none).
    pub game_server: SocketAddrV4,
}

/// Boot the shared session for an adopted mode: telemetry, registration, relay
/// publication and the (parity-preserved) continuous rerouting loop.
///
/// The shipped CLI registered with the known game server and then published the
/// relay **unconditionally**; the engine switch publishes only on success, so the
/// failure path is mirrored explicitly here. The rerouting loop keeps the shipped
/// construction, including its first-tick stop (see `self::reroute`).
pub async fn start_session(core: &mut SessionCore, session: &LiveSession) -> anyhow::Result<()> {
    // Telemetry: the adopted session reports through the library collector, so
    // the samples the library data plane records are the ones actually sent.
    {
        let engine = core.engine();
        let mut engine = engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        engine.set_telemetry_enabled(session.telemetry_enabled);
        engine.set_telemetry_game(&session.game_key);
    }

    let switch = core
        .commands_mut()
        .switch_relay(session.relay, session.destination)
        .map_err(registration_error)?;
    if switch.registered().await.is_none() {
        crate::session::set_current_proxy(session.relay);
    }

    // Continuous rerouting: same construction and first-tick behaviour as the
    // shipped caller (see `self::reroute` module notes); the handle is kept only
    // for the task lifetime of the session.
    let _reroute = reroute::spawn(
        std::sync::Arc::clone(&core.engine()),
        session.candidates.clone(),
        session.data_port,
        session.control_port,
        session.game_server,
        session.route_strategy.clone(),
        session.multipath_max_paths,
        session.path_mtu_cap,
    );
    Ok(())
}

/// The registration command error, flattened to the shipped CLI message.
fn registration_error(error: crate::app::dto::AppError) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        error
            .detail
            .unwrap_or_else(|| "relay registration failed".to_string())
    )
}
