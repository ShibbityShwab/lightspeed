//! GUI adapter for the shared relay history and its carry verdicts.
//!
//! The per-(relay, server) memory moved to `lightspeed_client::app::history` at
//! WF-081 W2-3a (S4), together with its file format and its 24 tests. The GUI
//! keeps only the file path (`paths::relay_history_file`) and this re-export, so
//! the CLI can adopt the same memory at S6 and the demotion rule cannot drift.
//!
//! The persisted `relay-history.json` format is unchanged (backward-compatible
//! load): a machine's existing file keeps working across the move.

#[allow(unused_imports)] // the orchestration and the in-crate tests address these names
pub use lightspeed_client::app::history::{
    now_secs, CarryVerdict, Record, RelayHistory, MAX_ENTRIES, MIN_FORWARDS,
    RETURN_SHARE_DENOMINATOR, VERDICT_TTL,
};
