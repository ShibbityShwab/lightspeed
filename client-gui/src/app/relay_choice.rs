use super::*;

#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::dto::{MeasuredServerLeg, ServerLegState, ServerPing};
#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::status::RelayLegs;

// The relay-health judgement and the switch margin moved to the shared crate's
// watchdog module at WF-081 W2-3a (S4). Re-exported so the GUI call sites and
// the remaining log-line tests keep their names.
#[allow(unused_imports)]
pub(super) use lightspeed_client::app::watchdog::{
    relay_health_verdict, switch_is_safe, RelayHealthBase, RelayHealthVerdict,
    RELAY_HEALTH_MAX_UNSENT_SHARE, RELAY_HEALTH_SETTLE, RELAY_HEALTH_WINDOW,
    RELAY_SWITCH_MARGIN_MS,
};

/// One destination-aware ranking, kept as the single record of what the chooser
/// knew when it ranked relays for `server`.
///
/// The route surface has to say whether the relay-to-server leg was measured or
/// only estimated. That fact exists in exactly one place - the scored candidates
/// the choice was made from - so it is kept here instead of being re-derived
/// from the estimator and the geo table a second time. (Presentation-side
/// bookkeeping; the ranking rules themselves live in the shared crate.)
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RouteRanking {
    /// The game server the ranking is for.
    pub(super) server: Ipv4Addr,
    /// Every candidate, best first in the engine's whole-path order.
    pub(super) scored: Vec<lightspeed_client::ScoredRelay>,
}

// ── Proxy nodes ────────────────────────────────────────────────────────────

/// Custom proxies from the environment, as a fallback for users who have not
/// let discovery populate `config.toml` yet.
///
///   LIGHTSPEED_PROXIES="1.2.3.4:4434,5.6.7.8:4434"
///   LIGHTSPEED_PROXY="1.2.3.4:4434"  (legacy single-proxy variable)
pub fn env_proxies() -> Vec<ProxyEntry> {
    let raw = std::env::var("LIGHTSPEED_PROXIES")
        .or_else(|_| std::env::var("LIGHTSPEED_PROXY"))
        .unwrap_or_default();
    raw.split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            match s.parse::<SocketAddrV4>() {
                Ok(addr) => Some(ProxyEntry::custom(addr, "Custom")),
                Err(_) => {
                    tracing::warn!("Skipping invalid proxy address: {s}");
                    None
                }
            }
        })
        .collect()
}
