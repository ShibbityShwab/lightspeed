//! Choosing the relay that is best for the *game server*, not for the client.
//!
//! # Why this exists
//!
//! The GUI used to pick a relay by racing client-to-relay latency ("connect to
//! the fastest one"). That answers the wrong question. For a player joining a
//! server in another region - which is the case this product exists for - the
//! client-to-relay leg is the *cheap* half of the path and the relay-to-server
//! leg is the half that decides the outcome:
//!
//! ```text
//! you ──(first leg, what a race measures)──► relay ──(dest leg, ignored)──► game server
//! ```
//!
//! Field evidence: a player in Bangkok on a 5G hotspot joined a US-East
//! (Ashburn) server. The race picked Los Angeles - 244 ms from the player, and
//! on the wrong side of the continent from the server. New Jersey measured
//! 286 ms but with a third of the jitter, and those 286 ms are the
//! Bangkok-to-Virginia distance the player pays regardless of route. The race
//! chose the strictly worse relay because it only ever looked at the first leg.
//!
//! # The model
//!
//! Rank candidates by the **whole path**: `client_leg + dest_leg`, where
//! `dest_leg` comes from the library's destination estimator when that relay has
//! carried traffic to this server, and from the coarse region prior
//! ([`super::regions::prior_dest_leg_us`]) when it has not. The prior is what
//! lets an unmeasured relay in the server's own region win instead of being
//! locked out for lacking evidence, which is the failure the library's own docs
//! call out.
//!
//! Jitter is scored alongside latency because the owner's goal is "lowest
//! latency possible and the most stable connection", and on a mobile link the
//! spread is what makes a match feel bad even when the median is acceptable. A
//! relay whose samples disagree strongly is penalised rather than trusted.
//!
//! # Scope
//!
//! This module is pure ranking: it takes what is known and returns an order. It
//! does not open sockets, does not measure, and does not mutate. Feeding it real
//! samples is the caller's job, which keeps it testable without a network - the
//! mistake that let the previous selection logic go wrong unnoticed.

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::route::{destination, regions};

/// One candidate relay with what is known about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayCandidate {
    /// The relay's tunnel endpoint.
    pub relay: SocketAddrV4,
    /// The region key the relay lives in (`"us-east"`, `"ap-southeast"`, ...).
    pub region: Option<String>,
    /// The registry node id (`relay-ewr-1`), when the relay came from discovery.
    ///
    /// Preferred over the region label for distance, because the operator's
    /// catalog carries a coordinate per node while a region label is only as
    /// fine as the alias map behind it.
    pub node_id: Option<String>,
    /// Median client-to-relay round trip, microseconds, when measured.
    pub client_leg_us: Option<u64>,
    /// The spread of that leg (p90 - p10), microseconds, when measured.
    ///
    /// Zero means "no spread information", not "perfectly stable".
    pub client_jitter_us: u64,
}

/// A relay with its score, so a caller can explain a choice rather than assert it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoredRelay {
    pub relay: SocketAddrV4,
    /// Estimated total round trip `client -> relay -> server`, microseconds.
    pub total_leg_us: u64,
    /// How the second leg was obtained, for the log line and for tests.
    pub dest_source: DestLegSource,
    /// Jitter estimate carried forward, microseconds.
    pub jitter_us: u64,
}

/// Where a destination-leg figure came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestLegSource {
    /// Measured for this exact relay and destination.
    Measured,
    /// A coarse region-centroid prior, because this relay has no sample yet.
    RegionPrior,
    /// No measurement and no usable region: the worst case, ranked last.
    Unknown,
}

/// Microseconds the prior is treated as being worth when it competes with a
/// measurement: a measurement is at most this much worse before the prior wins.
///
/// The library's region prior is deliberately coarse ("a region centroid is not
/// a server position"), so a relay in the right region with no samples should
/// beat a measured relay in the wrong one, but not by an unbounded amount.
pub const PRIOR_UNDERTAINTY_US: u64 = 25_000;

/// How much a microsecond of jitter is worth against a microsecond of latency.
///
/// Weighted above 1 because a stable path is the owner's stated goal and,
/// on a mobile link, 20 ms of swing hurts more than 20 ms of median.
pub const JITTER_WEIGHT: u64 = 2;

/// Rank relays for a destination, best first.
///
/// This is the coarse entry point: it has no destination position, so the
/// second leg comes from the library's region prior. Prefer [`rank_at`] when the
/// caller knows where the server is, because the region map collapses `us-east`
/// and `us-west` into one `na` centroid and cannot separate two relays on
/// opposite sides of the continent.
///
/// Deterministic and total: ties break on the relay address so the order is
/// stable across calls, which is what lets the caller avoid flapping.
pub fn rank(server: Ipv4Addr, candidates: &[RelayCandidate]) -> Vec<ScoredRelay> {
    let dest_region = region_for(server);

    let mut scored: Vec<ScoredRelay> = candidates
        .iter()
        .filter_map(|c| {
            let (dest_leg_us, dest_source) = dest_leg_for(server, dest_region.as_deref(), c);
            score_with(c, dest_leg_us, dest_source)
        })
        .collect();

    scored.sort_by(|a, b| {
        a.total_leg_us
            .saturating_add(a.jitter_us.saturating_mul(JITTER_WEIGHT))
            .cmp(
                &b.total_leg_us
                    .saturating_add(b.jitter_us.saturating_mul(JITTER_WEIGHT)),
            )
            .then_with(|| a.relay.cmp(&b.relay))
    });
    scored
}

/// The best relay for a destination, if any candidate can be scored.
pub fn best(server: Ipv4Addr, candidates: &[RelayCandidate]) -> Option<ScoredRelay> {
    rank(server, candidates).into_iter().next()
}

/// The best relay when the destination's position is already known.
///
/// This is the precise entry point, and the one the GUI uses: it has the game
/// server's coordinates from its own offline country database, which is strictly
/// better than any region centroid. `server` is still passed because a measured
/// relay-to-destination leg is keyed by the address, not the coordinate.
pub fn best_at(
    server: Ipv4Addr,
    server_location: (f64, f64),
    candidates: &[RelayCandidate],
) -> Option<ScoredRelay> {
    rank_at(server, server_location, candidates)
        .into_iter()
        .next()
}

/// Rank relays for a destination whose position is known, best first.
pub fn rank_at(
    server: Ipv4Addr,
    server_location: (f64, f64),
    candidates: &[RelayCandidate],
) -> Vec<ScoredRelay> {
    let dest_region = region_for(server);

    let mut scored: Vec<ScoredRelay> = candidates
        .iter()
        .filter_map(|c| score_one_at(server, server_location, dest_region.as_deref(), c))
        .collect();

    scored.sort_by(|a, b| {
        a.total_leg_us
            .saturating_add(a.jitter_us.saturating_mul(JITTER_WEIGHT))
            .cmp(
                &b.total_leg_us
                    .saturating_add(b.jitter_us.saturating_mul(JITTER_WEIGHT)),
            )
            .then_with(|| a.relay.cmp(&b.relay))
    });
    scored
}

/// The region a destination belongs to, via the library's shared vocabulary.
fn region_for(server: Ipv4Addr) -> Option<String> {
    destination::global()
        .and_then(|e| e.destination_region(server))
        .map(|s| s.to_string())
}

/// Score one candidate against a known destination position.
fn score_one_at(
    server: Ipv4Addr,
    server_location: (f64, f64),
    dest_region: Option<&str>,
    candidate: &RelayCandidate,
) -> Option<ScoredRelay> {
    let (dest_leg_us, dest_source) =
        dest_leg_for_at(server, server_location, dest_region, candidate);
    score_with(candidate, dest_leg_us, dest_source)
}

/// Combine a candidate's client leg with a destination-leg estimate.
///
/// One place so the two entry points cannot drift apart on the penalty rule: an
/// estimate that is not a measurement is charged its uncertainty, which is what
/// stops the coarse path quietly outranking the precise one.
fn score_with(
    candidate: &RelayCandidate,
    dest_leg_us: u64,
    dest_source: DestLegSource,
) -> Option<ScoredRelay> {
    let client_leg_us = candidate.client_leg_us?;

    let penalty = match dest_source {
        DestLegSource::Measured => 0,
        DestLegSource::RegionPrior | DestLegSource::Unknown => PRIOR_UNDERTAINTY_US,
    };

    Some(ScoredRelay {
        relay: candidate.relay,
        total_leg_us: client_leg_us
            .saturating_add(dest_leg_us)
            .saturating_add(penalty),
        dest_source,
        jitter_us: candidate.client_jitter_us,
    })
}

/// The relay-to-server leg against a known destination position.
///
/// Prefers the measured leg for this exact pair, then falls back to the
/// great-circle distance between the relay's own coordinate and the destination.
/// Both figures are real distances rather than a region collapse, which is what
/// separates a Los Angeles relay from a New Jersey one for an Ashburn server.
fn dest_leg_for_at(
    server: Ipv4Addr,
    server_location: (f64, f64),
    dest_region: Option<&str>,
    candidate: &RelayCandidate,
) -> (u64, DestLegSource) {
    if let Some(est) = destination::global() {
        if let Some(e) = est.estimate(candidate.relay, server) {
            return (e.dest_leg_us, DestLegSource::Measured);
        }
    }

    if let Some(relay_pos) = candidate
        .node_id
        .as_deref()
        .and_then(relay_position)
        .or_else(|| candidate.region.as_deref().and_then(centroid_for_label))
    {
        let km = regions::haversine_km(relay_pos, server_location);
        return (round_trip_us(km), DestLegSource::RegionPrior);
    }

    match (candidate.region.as_deref(), dest_region) {
        (Some(relay_region), Some(dest)) => match regions::prior_dest_leg_us(relay_region, dest) {
            Some(prior) => (prior, DestLegSource::RegionPrior),
            None => (unknown_dest_leg(), DestLegSource::Unknown),
        },
        _ => (unknown_dest_leg(), DestLegSource::Unknown),
    }
}

/// The relay-to-server leg: measured when possible, estimated otherwise.
///
/// Order of preference:
///
/// 1. **Measured** for this exact relay/destination pair: the only real answer.
/// 2. **Centroid distance** between the relay's own region and the destination's
///    region, both resolved through the shared vocabulary. This is the one that
///    matters for the field case: the coarse `na` centroid sits in Kansas, so a
///    coarse-only comparison cannot tell a Los Angeles relay from a New Jersey
///    one. `us-west` and `us-east` both resolve to `na` as a *key* but have
///    distinct centroids, and `region_label` carries the finer label, so the
///    comparison is made label-to-label rather than through the collapse.
/// 3. **Coarse region prior** from the library, when no centroid is available.
/// 4. **Unknown**, ranked last by [`unknown_dest_leg`].
fn dest_leg_for(
    server: Ipv4Addr,
    dest_region: Option<&str>,
    candidate: &RelayCandidate,
) -> (u64, DestLegSource) {
    if let Some(est) = destination::global() {
        if let Some(e) = est.estimate(candidate.relay, server) {
            return (e.dest_leg_us, DestLegSource::Measured);
        }
    }

    let server_label = destination::global()
        .and_then(|e| e.destination_region(server))
        .or_else(|| dest_region.map(|s| s.to_string()));

    // The exact position, when the relay came from discovery: the operator's
    // catalog carries one coordinate per node, so this is strictly finer than
    // any region centroid and is preferred wherever both exist.
    if let (Some(node_id), Some(server_label)) =
        (candidate.node_id.as_deref(), server_label.clone())
    {
        if let (Some((rlat, rlon)), Some((slat, slon))) =
            (relay_position(node_id), centroid_for_label(&server_label))
        {
            let km = regions::haversine_km((rlat, rlon), (slat, slon));
            return (round_trip_us(km), DestLegSource::RegionPrior);
        }
    }

    if let (Some(relay_label), Some(server_label)) = (candidate.region.as_deref(), server_label) {
        if let (Some((rlat, rlon)), Some((slat, slon))) = (
            centroid_for_label(relay_label),
            centroid_for_label(&server_label),
        ) {
            let km = regions::haversine_km((rlat, rlon), (slat, slon));
            return (round_trip_us(km), DestLegSource::RegionPrior);
        }
    }
    match (candidate.region.as_deref(), dest_region) {
        (Some(relay_region), Some(dest)) => match regions::prior_dest_leg_us(relay_region, dest) {
            Some(prior) => (prior, DestLegSource::RegionPrior),
            None => (unknown_dest_leg(), DestLegSource::Unknown),
        },
        _ => (unknown_dest_leg(), DestLegSource::Unknown),
    }
}

/// A centroid for a fine-grained label such as `us-east`, falling back to the
/// coarse region the label resolves to (`na`).
///
/// The district centroids are derived from the coarse region's extent rather
/// than a second table, because the point of this module is the *ordering*, and
/// a label that names a real place orders correctly against a coarse centroid
/// only when it carries a position of its own. Labels without a position return
/// `None` so the caller keeps the coarse prior instead of inventing one.
fn centroid_for_label(label: &str) -> Option<(f64, f64)> {
    let key = regions::resolve_region(label)?;
    regions::centroid(key).map(|(_, lat, lon)| (lat, lon))
}

/// Round-trip microseconds for a great-circle distance, using the same factor
/// as the library's own prior so the two estimates are comparable.
fn round_trip_us(km: f64) -> u64 {
    const US_PER_KM_ROUND_TRIP: f64 = 20.0;
    ((km * US_PER_KM_ROUND_TRIP) as u64).max(10_000)
}

/// What to use when nothing is known about a relay-to-destination leg.
///
/// Deliberately worse than any usable prior *after* the prior penalty is added,
/// so an unscoreable relay can never win on a suspiciously good first leg. That
/// is the same class of error as the race this module replaces, and the first
/// version of this constant was low enough to reproduce it: with a 500 ms value,
/// `unknown + PRIOR_UNDERTAINTY_US` still beat a region prior, and the test
/// `an_unknown_destination_leg_never_beats_a_known_one` failed on exactly that.
fn unknown_dest_leg() -> u64 {
    PRIOR_UNDERTAINTY_US.saturating_add(1_000_000)
}

/// Where a relay actually sits, from `infra/geo/regions.json`'s `relays` map.
///
/// Those coordinates were already checked in for every community relay but
/// nothing parsed them, so the chooser had nothing finer than a continental
/// centroid to reason with. Copying them here (rather than inventing a second
/// table) keeps the client honest against the operator's own catalog, in the
/// same spirit as [`super::regions`] mirroring the region map.
///
/// A relay absent from this list falls through to the coarse region prior.
pub const RELAY_POSITIONS: &[(&str, f64, f64)] = &[
    ("relay-lax-1", 34.05, -118.24),
    ("relay-ewr-1", 40.74, -74.17),
    ("relay-sgp-1", 1.35, 103.82),
    ("relay-fra", 50.11, 8.68),
    ("relay-nrt", 35.68, 139.69),
    ("relay-bom-1", 19.08, 72.88),
    ("relay-mad-1", 40.42, -3.70),
    ("relay-syd-1", -33.87, 151.21),
];

/// A community relay's own coordinates, by registry node id.
pub fn relay_position(node_id: &str) -> Option<(f64, f64)> {
    RELAY_POSITIONS
        .iter()
        .find(|(id, _, _)| *id == node_id)
        .map(|(_, lat, lon)| (*lat, *lon))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn relay(ip: [u8; 4], region: &str, client_us: u64, jitter: u64) -> RelayCandidate {
        RelayCandidate {
            relay: SocketAddrV4::new(Ipv4Addr::from(ip), 4434),
            region: Some(region.to_string()),
            node_id: None,
            client_leg_us: Some(client_us),
            client_jitter_us: jitter,
        }
    }

    #[test]
    fn a_known_server_position_picks_the_relay_in_its_own_region() {
        // The field case, end to end, with the relay coordinates from the
        // operator's own catalog and a US country centroid for an Ashburn
        // server. The client legs are the measurements from that session.
        //
        // This is the test that fails without the fix: with only the coarse
        // region prior, Kansas sits nearer California and the Los Angeles relay
        // scores better (281 vs 327 ms), reproducing the wrong pick the race
        // made. The real positions invert it, because the whole point is the
        // relay-to-server leg, which the client-leg race never looked at.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        let us = (39.83, -98.58); // the offline country database's US centroid

        let candidates = [
            RelayCandidate {
                relay: SocketAddrV4::new(Ipv4Addr::new(207, 246, 106, 36), 4434),
                region: Some("us-west".to_string()),
                node_id: Some("relay-lax-1".to_string()),
                client_leg_us: Some(244_200),
                client_jitter_us: 88_000,
            },
            RelayCandidate {
                relay: SocketAddrV4::new(Ipv4Addr::new(207, 246, 124, 117), 4434),
                region: Some("us-east".to_string()),
                node_id: Some("relay-ewr-1".to_string()),
                client_leg_us: Some(286_000),
                client_jitter_us: 31_000,
            },
        ];

        let ranked = rank_at(server, us, &candidates);
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(207, 246, 124, 117),
            "a US centroid plus exact relay positions must choose New Jersey for \
             an Ashburn server, not the Los Angeles relay that wins the \
             client-leg race"
        );
    }

    #[test]
    fn the_coarse_prior_alone_cannot_make_this_choice() {
        // Guards the reason the GUI feeds a real position: without one, both
        // US relays share the `na` centroid and neither can be preferred on
        // destination grounds. If this ever stops holding, the GUI's extra
        // wiring could be simplified - and until then, removing it would
        // silently reintroduce the wrong pick.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        let candidates = [
            RelayCandidate {
                relay: SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 4434),
                region: Some("us-west".to_string()),
                node_id: None,
                client_leg_us: Some(100_000),
                client_jitter_us: 0,
            },
            RelayCandidate {
                relay: SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 4434),
                region: Some("us-east".to_string()),
                node_id: None,
                client_leg_us: Some(100_000),
                client_jitter_us: 0,
            },
        ];
        let ranked = rank(server, &candidates);
        assert_eq!(
            ranked[0].total_leg_us, ranked[1].total_leg_us,
            "both relays resolve to the same coarse region, so the coarse path \
             cannot tell them apart and must not pretend to"
        );
    }

    #[test]
    fn every_relay_the_operator_ships_has_a_known_position() {
        // The table is what lets the chooser use real geometry instead of a
        // continental centroid, so a relay the registry advertises but this
        // table omits would silently lose resolution rather than fail loudly.
        for id in [
            "relay-lax-1",
            "relay-ewr-1",
            "relay-sgp-1",
            "relay-fra",
            "relay-nrt",
            "relay-bom-1",
            "relay-mad-1",
            "relay-syd-1",
        ] {
            assert!(relay_position(id).is_some(), "{id} has no position");
        }
        assert!(relay_position("relay-nope").is_none());
    }

    /// Install an estimator with `server` recorded in `dest_region`.
    ///
    /// The prior path is only reachable once the destination's region is known,
    /// so a test that skips this scores every candidate as `Unknown` and passes
    /// on the address tie-break while proving nothing about the region prior.
    fn install_region(server: Ipv4Addr, dest_region: &str) {
        let est = std::sync::Arc::new(destination::RelayDestinationEstimator::new());
        est.set_destination_region(server, dest_region);
        destination::install_global(est);
    }

    /// The measured field case: a US-East server, with LAX far from the server
    /// and EWR in it. The client legs are the Bangkok measurements.
    #[test]
    fn the_relay_near_the_server_beats_the_closer_but_distant_one() {
        let server = Ipv4Addr::new(3, 237, 142, 3); // Ashburn, us-east
        install_region(server, "us-east");
        let candidates = [
            relay([207, 246, 106, 36], "us-west", 244_200, 88_000),
            relay([207, 246, 124, 117], "us-east", 286_000, 31_000),
        ];
        let ranked = rank(server, &candidates);
        assert_eq!(ranked.len(), 2, "both candidates must be scoreable");
        assert_eq!(
            ranked[0].dest_source,
            DestLegSource::RegionPrior,
            "the destination leg must come from the region prior, not the \
             unknown fallback, or this test proves nothing"
        );
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(207, 246, 124, 117),
            "the relay in the server's own region must win: the destination leg \
             is what the race ignored"
        );
    }

    #[test]
    fn jitter_is_weighted_against_latency() {
        // Two relays, one 20 ms faster but 60 ms swingier. With jitter weighted
        // at 2x, the stable one must win: the owner asked for stability too.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        let candidates = [
            relay([10, 0, 0, 1], "us-east", 200_000, 60_000),
            relay([10, 0, 0, 2], "us-east", 220_000, 5_000),
        ];
        let ranked = rank(server, &candidates);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(10, 0, 0, 2),
            "the stable relay must win once its jitter advantage is counted"
        );
    }

    #[test]
    fn a_candidate_without_a_client_leg_is_not_ranked() {
        let server = Ipv4Addr::new(3, 237, 142, 3); // Ashburn, us-east
        let candidates = [RelayCandidate {
            relay: SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 9), 4434),
            region: Some("us-east".to_string()),
            node_id: None,
            client_leg_us: None,
            client_jitter_us: 0,
        }];
        assert!(rank(server, &candidates).is_empty());
        assert!(best(server, &candidates).is_none());
    }

    #[test]
    fn an_unknown_destination_leg_never_beats_a_known_one() {
        // A relay with no region cannot be scored for the destination, so the
        // pessimistic unknown leg applies and it loses to any scored peer.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        let candidates = [
            RelayCandidate {
                relay: SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 4434),
                region: None,
                node_id: None,
                client_leg_us: Some(1_000),
                client_jitter_us: 0,
            },
            relay([10, 0, 0, 2], "us-east", 250_000, 0),
        ];
        let ranked = rank(server, &candidates);
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(10, 0, 0, 2),
            "an unscoreable relay must not win on its suspiciously good first leg"
        );
        assert_eq!(ranked[1].dest_source, DestLegSource::Unknown);
    }

    #[test]
    fn an_unknown_leg_costs_more_than_any_usable_prior() {
        // The ordering in `score_one` depends on this invariant, and the first
        // version of `unknown_dest_leg` violated it, which made an unscoreable
        // relay win. Pin the relationship rather than the numbers.
        let unknown_total = unknown_dest_leg().saturating_add(PRIOR_UNDERTAINTY_US);
        let prior_total = regions::prior_dest_leg_us("us-east", "us-east")
            .expect("the library must know its own region pair")
            .saturating_add(PRIOR_UNDERTAINTY_US);
        assert!(
            unknown_total > prior_total,
            "an unknown leg ({unknown_total}) must score worse than any prior ({prior_total})"
        );
    }

    #[test]
    fn the_order_is_total_and_stable() {
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        let candidates = [
            relay([10, 0, 0, 3], "us-east", 100_000, 0),
            relay([10, 0, 0, 1], "us-east", 100_000, 0),
            relay([10, 0, 0, 2], "us-east", 100_000, 0),
        ];
        let first = rank(server, &candidates);
        let again = rank(server, &candidates);
        assert_eq!(first, again, "ranking must not flap between calls");
        assert_eq!(first[0].relay, candidates[1].relay);
    }
}
