use super::*;

// The rules moved to the shared crate at W2-1 S3a (`lightspeed_client::app`).
// They are re-exported so the window's call sites and the existing test suite
// address the same names they always did; the copy builders below are the only
// presentation left in this module (`i18n` lookups, never a decision).
pub(super) use lightspeed_client::app::dto::FallbackRanking;
#[allow(unused_imports)] // re-exported for the in-crate tests and the W2-2 shell
pub(super) use lightspeed_client::app::status::{
    compare_to_direct, direct_us_for, early_triangle_floor, fresh_direct_us, measured_server_leg,
    pair_relay_legs, ping_for_server, prefer_proven, proxy_index_for, rank_without_destination,
    refreshed_server_ping, relay_cannot_beat_direct, relay_measured_app_server_leg,
    relay_measured_server_leg, relay_measured_trace, same_relay_endpoint, scored_for_relay,
    server_leg_state, session_delta, session_pair, without_starved, MAX_RELAYS_TRIED,
    SERVER_PING_FRESH_SECS,
};

/// The carry-memory seam for the shared starvation filter.
///
/// `RelayHistory` (and its disk format) moves at S4; until then the GUI is its
/// owner, but the *filter* is shared. This impl is the one adapter between the
/// two, so the rule cannot drift: the core filter reads `CarryLookup`, and the
/// only implementation over the GUI's memory converts its verdict once, here.
impl lightspeed_client::app::dto::CarryLookup for crate::relay_history::RelayHistory {
    fn carry_verdict(
        &self,
        relay: SocketAddrV4,
        server: Ipv4Addr,
    ) -> lightspeed_client::app::dto::CarryVerdict {
        use lightspeed_client::app::dto::CarryVerdict;
        match crate::relay_history::RelayHistory::carry_verdict(self, relay, server) {
            crate::relay_history::CarryVerdict::Proven => CarryVerdict::Proven,
            crate::relay_history::CarryVerdict::Starved => CarryVerdict::Starved,
            crate::relay_history::CarryVerdict::Unproven => CarryVerdict::Unproven,
        }
    }
}

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

/// The `Server leg` row for a relay-measured figure.
///
/// One label, one number: the value is the relay's ICMP median and the sample
/// count behind it, never a derived or rounded-up claim. A report the client
/// accepted fail-open - the triangle floor could not be checked - is still
/// shown, but its label says `measured, unvalidated` instead of plainly
/// `measured`: the figure is the relay's measurement, the validation is absent.
#[allow(dead_code)]
pub(super) fn relay_measured_leg_text(report: &lightspeed_client::ServerLegReport) -> String {
    let ms = format!("{:.0}", f64::from(report.median_us) / 1000.0);
    let samples = report.samples.to_string();
    let secs = report.age.as_secs().to_string();
    let key = if report.validated {
        "route.server_leg_relay_measured"
    } else {
        "route.server_leg_relay_measured_unvalidated"
    };
    i18n::t_with(key, &[("ms", &ms), ("n", &samples), ("secs", &secs)])
}

/// The `Relay→server (app)` value: the passive median in ms and its sample
/// count, never the age or a blended figure.
#[allow(dead_code)]
pub(super) fn relay_measured_app_leg_text(
    report: &lightspeed_client::ServerLegAppReport,
) -> String {
    let ms = format!("{:.0}", f64::from(report.passive_median_us) / 1000.0);
    let samples = report.samples.to_string();
    i18n::t_with(
        "route.server_leg_app_measured",
        &[("ms", &ms), ("n", &samples)],
    )
}

/// The `Route` row for a relay-measured traceroute: hops and last router RTT.
#[allow(dead_code)]
pub(super) fn relay_measured_trace_text(report: &lightspeed_client::ServerTraceReport) -> String {
    let hops = report.hops.to_string();
    match report.last_hop_rtt_us {
        Some(rtt_us) => {
            let ms = format!("{:.0}", f64::from(rtt_us) / 1000.0);
            i18n::t_with(
                "route.route_trace_hops",
                &[("hops", hops.as_str()), ("ms", ms.as_str())],
            )
        }
        None => i18n::t_with("route.route_trace_hops_only", &[("hops", hops.as_str())]),
    }
}

/// The `Server leg` row: the one measurement the two measured legs support,
/// `L + S`. The arithmetic moved to the shared crate; only the copy stays here.
#[allow(dead_code)]
pub(super) fn expected_path_text(client_leg_ms: f64, server_leg_us: u32) -> Option<String> {
    let total_ms = lightspeed_client::app::dto::expected_path_ms(client_leg_ms, server_leg_us)?;
    let ms = format!("{total_ms:.0}");
    Some(i18n::t_with("route.expected_path", &[("ms", &ms)]))
}

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
