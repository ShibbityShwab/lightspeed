use super::*;

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

/// Every discovered relay's measured client leg, as the destination-aware
/// chooser consumes it (a relay that did not answer carries `None`).
pub(super) type RelayLegs = Vec<(SocketAddrV4, Option<discovery::RelaySample>)>;

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

/// A published quiet-flow server-leg measurement: the value, the samples behind
/// it, and the age of the newest one when the estimator reports an age.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MeasuredServerLeg {
    /// Relay-to-server round trip, microseconds.
    pub(super) dest_leg_us: u64,
    /// Samples behind the published estimate.
    pub(super) samples: usize,
    /// Age of the newest contributing sample; `None` only if the estimator
    /// publishes an estimate without an age, which the surface must not invent.
    pub(super) age: Option<Duration>,
}

/// The measured direct (ICMP) ping, and the game server it was measured for.
///
/// A ping is evidence about exactly one server's path. Storing the value without
/// its server let a relay choice for a new game server consume the previous
/// server's fresh ping as this server's triangle floor: `follow_game_server`
/// runs before `poll_relay_health` refreshes this record, so a choice for the new
/// server could read the old server's sample (observed live, task-24: a choice
/// for `91.199.82.221` carried `direct 281` from `91.199.82.162`). Every
/// consumer - the ranking floor, the choice trace comparison, the `Server leg`
/// bound, the header metric - must ask for the ping **of the server it is
/// describing** and treat a foreign record as "not measured".
#[derive(Clone, Copy, Debug)]
pub(super) struct ServerPing {
    /// The game server this sample was measured against.
    pub(super) server: Ipv4Addr,
    /// Median ICMP round trip, milliseconds.
    pub(super) ms: f32,
    /// Age of the newest burst sample, seconds.
    pub(super) age_secs: u64,
}

/// What the route surface can honestly say about the relay-to-server leg for the
/// relay in use.
///
/// Exactly one value drives the `Server leg` row; the window never derives the
/// same fact from two sources. Precedence: a published estimator measurement
/// first (it is the only thing that can be called measured), then the ranking's
/// own row for that relay - the geometric prior, or the triangle-inequality floor
/// that proves the leg is at least a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerLegState {
    /// The v1 quiet-flow estimator published an estimate for this exact pair.
    Measured(MeasuredServerLeg),
    /// The prior was below the triangle bound, so the leg is known to be at
    /// least this: a proof, not a measurement.
    Bounded { floor_us: u64 },
    /// No measurement existed, so the ranking used the geometric region prior.
    Prior { prior_us: u64 },
    /// No measurement and no usable region prior.
    Unknown,
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
