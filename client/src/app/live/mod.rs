//! Adopted live session modes (post-W2 task-57).
//!
//! The CLI's live modes used to be front-end adapters because the engine could
//! not express what they print, the adaptive-FEC policy they resolve, the
//! redirect options/counters they expose, or an in-session relay switch. The
//! additive engine surface (WF-081b checkpoint 1) closed those gaps; this module
//! is the shared home of the migrated loops, so the binary stops carrying a
//! second engine-less session model (`client/src/modes/watch_mode.rs` is retired).
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
pub mod reroute;
pub mod watch;
