use super::*;

// The rules moved to the shared crate at W2-1 S3a (`lightspeed_client::app`).
// They are re-exported so the window's call sites and the existing test suite
// address the same names they always did; the surviving label builders below are
// presentation only (`i18n` lookups, never a decision). W2-4b retired the
// duplicate relay-measured copy helpers that `route_surface` had replaced.
pub(super) use lightspeed_client::app::dto::FallbackRanking;
#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::status::{
    compare_to_direct, decide_switch, direct_us_for, early_triangle_floor, fresh_direct_us,
    measured_server_leg, pair_relay_legs, ping_for_server, prefer_proven, proxy_index_for,
    rank_without_destination, refreshed_server_ping, relay_cannot_beat_direct,
    relay_measured_app_server_leg, relay_measured_server_leg, relay_measured_trace,
    same_relay_endpoint, scored_for_relay, server_leg_state, session_delta, session_pair,
    without_starved, MAX_RELAYS_TRIED, SERVER_PING_FRESH_SECS,
};

/// The trace half of [`compare_to_direct`], as a sentence fragment.
pub(super) fn direct_claim_label(direct_us: Option<u64>, total_leg_us: u64) -> String {
    match compare_to_direct(total_leg_us, direct_us) {
        None => ", direct ping not measured".to_string(),
        Some((true, delta)) => format!(", better than direct by {} ms", delta / 1000),
        Some((false, delta)) => format!(
            ", not better than direct (+{} ms, direct {} ms)",
            delta / 1000,
            direct_us.unwrap_or(0) / 1000
        ),
    }
}

/// How the chooser obtained the server leg, for the choice trace line.
///
/// `Measured` carries its sample count, because "measured" without an `n` is a
/// claim the reader cannot size; the prior carries its uncertainty for the same
/// reason.
pub(super) fn ranking_state_label(
    source: lightspeed_client::DestinationLegSource,
    relay: SocketAddrV4,
    server: Ipv4Addr,
) -> String {
    use lightspeed_client::DestinationLegSource;
    match source {
        DestinationLegSource::Measured => match measured_server_leg(relay, server) {
            Some(measured) => format!("Measured(n={})", measured.samples),
            None => "Measured".to_string(),
        },
        DestinationLegSource::GeometricFloor => "GeometricFloor".to_string(),
        DestinationLegSource::RegionPrior => {
            format!("RegionPrior(±{} ms)", PRIOR_UNCERTAINTY_US / 1000)
        }
        DestinationLegSource::Unknown => "Unknown".to_string(),
    }
}

/// The catalogue key of the running game, when the shared detector finds one.
pub(super) fn try_auto_detect_game() -> Option<String> {
    match lightspeed_client::games::auto_detect() {
        Ok(game) => {
            let name_lower = game.name().to_lowercase();
            games().iter().find_map(|entry| {
                if entry.display.to_lowercase().contains(&name_lower)
                    || name_lower.contains(entry.key)
                {
                    Some(entry.key.to_string())
                } else {
                    None
                }
            })
        }
        Err(_) => None,
    }
}
