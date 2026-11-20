use super::*;

#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::dto::{MeasuredServerLeg, ServerLegState, ServerPing};
#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::status::RelayLegs;

/// How long a run must have been relaying before the relay verdict starts
/// sampling. A match takes several seconds to open its flow, and judging it
/// before then would call a healthy relay broken.
pub(super) const RELAY_HEALTH_SETTLE: Duration = Duration::from_secs(10);

/// The window each relay verdict is measured over. One second is long enough
/// for a game's packet rate to make a lost share visible and short enough that
/// a fault is reported while the player is still in the match.
pub(super) const RELAY_HEALTH_WINDOW: Duration = Duration::from_secs(1);

/// Share of a window's relayed datagrams that may fail to leave before the
/// relay is called broken. UDP games tolerate loss; a fifth of the game's
/// traffic not leaving the machine for the whole window is a fault, not jitter.
pub(super) const RELAY_HEALTH_MAX_UNSENT_SHARE: f64 = 0.2;

/// One destination-aware ranking, kept as the single record of what the chooser
/// knew when it ranked relays for `server`.
///
/// The route surface has to say whether the relay-to-server leg was measured or
/// only estimated. That fact exists in exactly one place - the scored candidates
/// the choice was made from - so it is kept here instead of being re-derived
/// from the estimator and the geo table a second time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RouteRanking {
    /// The game server the ranking is for.
    pub(super) server: Ipv4Addr,
    /// Every candidate, best first in the engine's whole-path order.
    pub(super) scored: Vec<lightspeed_client::ScoredRelay>,
}

/// The tunnel counters as of the last relay-verdict sample.
#[derive(Clone, Copy, Debug)]
pub(super) struct RelayHealthBase {
    pub(super) at: std::time::Instant,
    pub(super) intercepted: u64,
    pub(super) sent: u64,
}

/// What one window of tunnel counters says about the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RelayHealthVerdict {
    /// Nothing was decided to relay during the window: menus, or a hold.
    Idle,
    /// Every datagram decided to relay left the machine.
    Healthy,
    /// A share of them did not leave the machine.
    Unsent { unsent: u64, decided: u64 },
}

impl RelayHealthVerdict {
    pub(super) fn sentence(&self) -> String {
        match self {
            Self::Idle => "no traffic to judge".to_string(),
            // The client sees these counters: intercepted datagrams against
            // datagrams the tunnel sent. It cannot see what the relay did with
            // them afterwards, so the sentence must not claim the relay's
            // forwarding is healthy - only this machine's outbound leg.
            Self::Healthy => {
                "every relayed datagram left for the relay; the outbound leg is clean".to_string()
            }
            Self::Unsent { unsent, decided } => format!(
                "{unsent} of {decided} relayed datagrams never left for the relay in the last \
                 second; if the match feels like packet loss, switch relay"
            ),
        }
    }
}

/// Whether a relay change may be applied to the live tunnel now.
///
/// The tunnel reads the current relay on every packet, so a change applied while
/// datagrams are flowing goes to a relay that may not have this session yet. A
/// hold withholds tunnelling, and that is the one state in which the switch is
/// safe; a backend with no gate never tunnels at all, so nothing is at risk.
///
/// A test pins it, which is why it is not dead code in a release build.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn switch_is_safe(tunnelling_enabled: Option<bool>) -> bool {
    tunnelling_enabled != Some(true)
}

/// Judge one window of tunnel counters.
///
/// The comparison is on *movement*: a run that relayed a million packets and
/// then started failing must not be excused by its healthy past, so the caller
/// passes the counters as of the previous sample, not the totals.
pub(super) fn relay_health_verdict(
    prev_intercepted: u64,
    prev_sent: u64,
    intercepted: u64,
    sent: u64,
) -> RelayHealthVerdict {
    let decided = intercepted.saturating_sub(prev_intercepted);
    let left = sent.saturating_sub(prev_sent).min(decided);
    if decided == 0 {
        return RelayHealthVerdict::Idle;
    }
    let unsent = decided - left;
    let share = unsent as f64 / decided as f64;
    if share > RELAY_HEALTH_MAX_UNSENT_SHARE {
        RelayHealthVerdict::Unsent { unsent, decided }
    } else {
        RelayHealthVerdict::Healthy
    }
}

/// Hysteresis for a connected re-race: a winner must beat the relay in use by
/// this many milliseconds before the GUI switches away from it.
pub(super) const RELAY_SWITCH_MARGIN_MS: u64 = 20;

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
