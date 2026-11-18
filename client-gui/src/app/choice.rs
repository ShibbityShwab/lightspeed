use super::*;

/// Pair measured client-leg samples back onto the proxy entries they belong to.
///
/// # Why this exists
///
/// A client-leg measurement is taken against a relay's `/health` endpoint, which
/// serves on `discovery::HEALTH_PORT`, while the relay's tunnel address - the
/// thing the rest of the app, the config file, and the chooser all call "the
/// relay" - is the advertised data port. Samples therefore come back keyed by
/// `ip:8080` while every consumer compares them against `ip:4434`.
///
/// A full-address comparison across that seam never matches, and it fails
/// **silently**: the lookup yields `None`, every candidate is dropped, the
/// destination-aware chooser returns `None`, and the app keeps whatever the
/// client-leg race picked. That is how a live match got routed through Los
/// Angeles to a server in Sao Paulo, 17,000 km the wrong way.
///
/// Pairing happens on the IP, which the health endpoint preserves, and the result
/// is keyed by the entry's data address so nothing downstream sees a health port.
/// One pair per entry that has a sample, in entry order.
pub(super) fn pair_relay_legs(
    entries: &[crate::config::ProxyEntry],
    measured: &[(SocketAddrV4, Option<discovery::RelaySample>)],
) -> Vec<(SocketAddrV4, Option<discovery::RelaySample>)> {
    entries
        .iter()
        .filter_map(|entry| {
            measured
                .iter()
                .find(|(addr, _)| addr.ip() == entry.addr.ip())
                .map(|(_, latency)| (entry.addr, *latency))
        })
        .collect()
}

/// The proxy entry a measured relay endpoint belongs to.
///
/// Client legs are measured against each relay's `/health` endpoint, which lives
/// on [`discovery::HEALTH_PORT`], while [`crate::config::ProxyEntry::addr`] is the
/// advertised data port. A full-address comparison therefore matches nothing and
/// silently discards every race winner; the public IP identifies the relay.
pub(super) fn proxy_index_for(
    proxies: &[crate::config::ProxyEntry],
    addr: SocketAddrV4,
) -> Option<usize> {
    proxies
        .iter()
        .position(|entry| same_relay_endpoint(entry.addr, addr))
}

/// Whether two endpoints name the same relay. The `/health` probe uses a
/// different port than the data plane, so only the IP can be compared.
pub(super) fn same_relay_endpoint(a: SocketAddrV4, b: SocketAddrV4) -> bool {
    a.ip() == b.ip()
}

/// The direct-ping measurement in microseconds, only while it is fresh enough to
/// be treated as evidence about the path.
///
/// A missing, zero or stale sample yields `None`, so nothing floors the ranking
/// against a number the client is no longer measuring. This freshness step is
/// about *time* only; [`direct_us_for`] adds the server key.
pub(super) fn fresh_direct_us(ping: Option<(f32, u64)>) -> Option<u64> {
    let (ms, age) = ping?;
    // A non-finite value (NaN/inf) is not evidence either; spelling it this way
    // rejects it explicitly instead of through a negated float comparison.
    if age > SERVER_PING_FRESH_SECS || !ms.is_finite() || ms <= 0.0 {
        return None;
    }
    Some((f64::from(ms) * 1000.0).round() as u64)
}

/// The direct-ping record, only when it belongs to `server`.
///
/// The keying half of the cross-server guard: a sample measured against server A
/// describes A's path and nothing about B's. Consumers that rank, bound or
/// display a path for a server ask through here.
pub(super) fn ping_for_server(ping: Option<ServerPing>, server: Ipv4Addr) -> Option<ServerPing> {
    ping.filter(|record| record.server == server)
}

/// The direct ping in microseconds for `server`, only when the record belongs
/// to that server and is fresh.
///
/// This is the single input every consumer passes to the ranking floor, the
/// choice-trace comparison and the triangle verdict. `None` means "this server's
/// direct path is not measured" - never "use the previous server's value".
pub(super) fn direct_us_for(ping: Option<ServerPing>, server: Ipv4Addr) -> Option<u64> {
    let record = ping_for_server(ping, server)?;
    fresh_direct_us(Some((record.ms, record.age_secs)))
}

/// The direct-ping record after one `refresh_server_ping` poll for `server`.
///
/// The record is replaced wholesale (or cleared when the server has no sample),
/// so it can never silently keep describing a server the client has moved away
/// from. Pure so the "new server has no ping yet" ordering is testable.
pub(super) fn refreshed_server_ping(
    server: Ipv4Addr,
    sample: Option<(f32, u64)>,
) -> Option<ServerPing> {
    sample.map(|(ms, age_secs)| ServerPing {
        server,
        ms,
        age_secs,
    })
}

/// Compare a whole-path estimate with the measured direct ping.
///
/// `Some((faster, delta_us))` where `faster` is true only when the estimated
/// relayed total is strictly below the direct measurement; `None` without a
/// direct measurement, because then nothing can be claimed. This is the only
/// place the window or the trace may call a relayed path "better than direct".
pub(super) fn compare_to_direct(total_leg_us: u64, direct_us: Option<u64>) -> Option<(bool, u64)> {
    let direct = direct_us?;
    if total_leg_us < direct {
        Some((true, direct - total_leg_us))
    } else {
        Some((false, total_leg_us - direct))
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

/// The v1 estimator's published server-leg measurement for `(relay, server)`,
/// if it has one.
///
/// `estimate()` is the estimator's single "publishable or None" gate (sample
/// count, physical bounds, 120 s freshness), so the window implements no rule of
/// its own here. The age comes from the estimator's own timestamp
/// (`measurement_age`), which is the only thing that may say how old the newest
/// sample is; `None` there is rendered as no age, never as zero.
pub(super) fn measured_server_leg(
    relay: SocketAddrV4,
    server: Ipv4Addr,
) -> Option<MeasuredServerLeg> {
    let estimator = lightspeed_client::destination_estimator()?;
    let estimate = estimator.estimate(relay, server)?;
    Some(MeasuredServerLeg {
        dest_leg_us: estimate.dest_leg_us,
        samples: estimate.samples,
        age: estimator.measurement_age(relay, server),
    })
}

/// The route surface's state for the relay-to-server leg of the relay in use.
///
/// Precedence is deliberate and single:
/// 1. a published estimator measurement (the only thing that may be called
///    measured), else
/// 2. the ranking's own row for that relay: the triangle floor (a proof, so
///    "at least"), the geometric prior, or nothing, unless
/// 3. a fresh direct ping already proves a stronger triangle floor than the
///    ranking row carries (`early_floor_us`). The ranking only re-runs every
///    `RELAY_RECHECK_INTERVAL` (600 s, longer than most matches), so without this
///    the row would sit on the prior for up to ten minutes after the ping that
///    makes the bound provable landed. This is the same verdict the next ranking
///    reaches, from measurements already in hand: it re-ranks nothing, changes no
///    relay, and never claims a measurement.
pub(super) fn server_leg_state(
    scored: Option<&lightspeed_client::ScoredRelay>,
    measured: Option<MeasuredServerLeg>,
    early_floor_us: Option<u64>,
) -> ServerLegState {
    use lightspeed_client::DestinationLegSource;
    if let Some(measured) = measured {
        return ServerLegState::Measured(measured);
    }
    let Some(scored) = scored else {
        return match early_floor_us {
            Some(floor_us) => ServerLegState::Bounded { floor_us },
            None => ServerLegState::Unknown,
        };
    };
    match scored.dest_source {
        DestinationLegSource::Measured => ServerLegState::Unknown,
        DestinationLegSource::GeometricFloor => ServerLegState::Bounded {
            floor_us: scored.dest_leg_us,
        },
        DestinationLegSource::RegionPrior => match early_floor_us {
            // A proof stronger than the estimate is shown as the proof; the
            // estimate is never dressed up as one.
            Some(floor_us) if floor_us > scored.dest_leg_us => ServerLegState::Bounded { floor_us },
            _ => ServerLegState::Prior {
                prior_us: scored.dest_leg_us,
            },
        },
        DestinationLegSource::Unknown => match early_floor_us {
            Some(floor_us) => ServerLegState::Bounded { floor_us },
            None => ServerLegState::Unknown,
        },
    }
}

/// The triangle-inequality floor a fresh direct ping proves for the relay in
/// use: `direct - client leg`, microseconds.
///
/// This is the same arithmetic `relay_choice::apply_floor` performs at the next
/// ranking. The route surface may state it earlier because it is a proof from
/// two measurements the client already owns - a fresh ICMP ping and the relay's
/// measured one-RTT client leg - not a new choice: nothing here re-ranks, moves
/// the session, or claims to have measured the server leg. `None` until both
/// inputs are present, the ping is fresh ([`SERVER_PING_FRESH_SECS`]) **and the
/// ping was measured against `server`**; a stale or foreign ping must never
/// manufacture a bound.
pub(super) fn early_triangle_floor(
    ping: Option<ServerPing>,
    server: Ipv4Addr,
    client_leg_us: Option<u64>,
) -> Option<u64> {
    let direct = direct_us_for(ping, server)?;
    let client = client_leg_us?;
    Some(direct.saturating_sub(client))
}

/// The scored row for `relay` in a ranking, if the relay was a candidate at all.
///
/// The surface describes the relay actually carrying traffic, which is not
/// necessarily the winner: a better relay for this server applies next session
/// when the live session cannot be moved, so looking up the winner would name
/// the wrong relay's leg.
pub(super) fn scored_for_relay(
    scored: &[lightspeed_client::ScoredRelay],
    relay: SocketAddrV4,
) -> Option<&lightspeed_client::ScoredRelay> {
    scored.iter().find(|candidate| candidate.relay == relay)
}

/// The triangle-inequality verdict for the relay in use, when it holds.
///
/// A relayed round trip is `client_leg + server_leg`; when the client leg alone
/// already reaches the measured direct ping, no server leg can make the relayed
/// path shorter, whatever the server leg is. Both inputs are client
/// measurements: the one-RTT client leg and the ICMP ping, and the ping must be
/// fresh **and measured against `server`** for the claim to stand. `None` when
/// either is missing.
pub(super) fn relay_cannot_beat_direct(
    client_leg_us: Option<u64>,
    ping: Option<ServerPing>,
    server: Ipv4Addr,
) -> Option<(u64, u64)> {
    let client = client_leg_us?;
    let direct = direct_us_for(ping, server)?;
    (client >= direct).then_some((client, direct))
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
