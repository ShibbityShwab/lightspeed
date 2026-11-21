//! GUI adapter for the shared warn-only watchdog.
//!
//! The runtime moved to `lightspeed_client::app::watchdog` at WF-081 W2-3a (S4):
//! the judged-window classification, the fault streak, the pure plan, the
//! relay-history demotion and the background thread are core logic shared with
//! any other front end. This module re-exports the shared items under the names
//! the window already used, so the GUI keeps only the thread wiring and the
//! presentation of the published facts.
//!
//! The two aliases at the bottom keep the shipped GUI vocabulary (`RelayReplyFault`,
//! `RelayStarvationWarning`) while the shared types are the DTO's `ReturnPathFault`
//! and `StarvationWarning`: a re-export, never a second definition.

#[allow(unused_imports)] // the in-crate tests and the shell address these names
pub use lightspeed_client::app::watchdog::{
    apply_plan, apply_starvation_verdict, clear_publication, plan_return_path_window,
    relay_health_verdict, relay_reply_fault, relay_reply_fault_message, relay_reply_window_message,
    return_path_recovered, spawn_relay_reply_watchdog, starvation_verdict, switch_is_safe,
    warning_is_about, warning_message_key, RelayHealthBase, RelayHealthVerdict, ReplyFaultStreak,
    ReturnPathFault, ShadowWindowCounters, StarvationVerdict, StarvationWarning, WatchdogPlan,
    WatchdogSnapshot, WindowEvent, RELAY_HEALTH_MAX_UNSENT_SHARE, RELAY_HEALTH_SETTLE,
    RELAY_HEALTH_WINDOW, RELAY_REPLY_MIN_INTERCEPTED, RELAY_REPLY_MIN_RETURNED,
    RELAY_REPLY_SHARE_DENOMINATOR, RELAY_REPLY_STREAK, RELAY_REPLY_TIMEOUT, RELAY_SWITCH_MARGIN_MS,
};

/// The shipped GUI name for the shared return-path fault taxonomy.
#[allow(unused_imports)] // used by the in-crate tests
pub use lightspeed_client::app::watchdog::ReturnPathFault as RelayReplyFault;
/// The shipped GUI name for the shared starvation warning DTO.
#[allow(unused_imports)] // used by the in-crate tests
pub use lightspeed_client::app::watchdog::StarvationWarning as RelayStarvationWarning;
