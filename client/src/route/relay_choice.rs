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
//! That total is the only ranking key ([`sort_scored`]). The destination leg is a
//! validity/uncertainty term inside it - a prior is charged
//! [`PRIOR_UNDERTAINTY_US`], a measurement or the triangle bound is not - and not
//! a key of its own. Ordering by the destination leg first (the previous
//! `dest_leg / 50 ms` bucket) can put a relay whose own estimated whole path is
//! 368.9 ms above one at 283.5 ms merely because its leg landed in a lower
//! bucket; `docs/server-leg-signal.md` §4.3 has the counterfactual arithmetic.
//!
//! Jitter is the tie-break, not a weighted term. It used to be worth twice its
//! value in the score, which on the field session added 176 ms to the Los Angeles
//! relay (88 ms of mobile-link spread) and is what actually produced the New
//! Jersey pick - the spread, not the destination leg. A steadier relay should win
//! only when the whole-path estimates are exactly equal, so that is what the key
//! does.
//!
//! One exception comes before jitter: the triangle floor (`GeometricFloor`) makes
//! every floored candidate's total exactly the measured direct ping, so the
//! totals tie by construction. Those ties are ordered by the pre-floor
//! destination prior - the magnitude was proven optimistic, but its ordering is
//! still the only destination evidence - because jitter and the numerical relay
//! address carry no destination signal at all. The live ROB RUN session of
//! 2026-11-19 is the regression: without this key, relay-syd-1 (`45.76.114.93`)
//! won a floor tie for a Virginia server purely because its address sorted first.
//!
//! What the ranking can and cannot do is part of being honest. With a measured
//! leg, or the triangle bound derived from a measured direct ping, the
//! destination side moves the order; with only the country-level prior every US
//! address resolves to one point, so two coasts are 4.2 ms apart in the prior
//! while their real relay-to-server legs differ by 63 ms, and the client leg
//! decides. That is the measurement gap
//! `docs/server-leg-signal.md` §4.4 quantifies - the fix is a real L2 signal or a
//! precise server position, not another sort key.
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
    ///
    /// The only ranking key: client leg plus destination leg plus that estimate's
    /// own uncertainty (see [`sort_scored`]).
    pub total_leg_us: u64,
    /// Estimated relay-to-server leg, microseconds.
    ///
    /// A validity/uncertainty term inside [`ScoredRelay::total_leg_us`], not a
    /// ranking key of its own.
    pub dest_leg_us: u64,
    /// The destination leg *before* the triangle floor raised it, microseconds.
    ///
    /// A floored leg is a lower bound, not an estimate, so every floored candidate
    /// ends up with exactly the direct-ping total and the totals cannot order
    /// them. The prior's magnitude was proven too optimistic, but its *order*
    /// still says which relay is nearer the server; keeping the pre-floor figure
    /// lets [`sort_scored`] use that order instead of falling through to jitter or
    /// the numerical relay address, neither of which knows anything about the
    /// destination. Equals `dest_leg_us` when no floor was applied.
    pub dest_leg_prior_us: u64,
    /// The candidate's measured client-to-relay leg, microseconds.
    ///
    /// The one measured quantity every candidate has: the destination leg is a
    /// measurement only for a relay that has already carried traffic to this
    /// server, and a prior otherwise. When the whole-path totals and the pre-floor
    /// priors both tie - every US relay resolving to the same country centroid is
    /// the common case - a shorter measured client leg is real evidence about the
    /// path while the jitter and the numerical relay address are not. Ranking it
    /// here is what keeps a floor tie from being decided by which relay's IP sorts
    /// first (the live 2026-11-19 defect).
    pub client_leg_us: u64,
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
    /// Raised to the geometric lower bound implied by the measured direct ping.
    /// Network round trips obey the triangle inequality, so a relay-to-server leg
    /// can never be shorter than `direct - client_leg`; a prior that says
    /// otherwise is provably too optimistic.
    GeometricFloor,
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

/// Sort by the whole-path total, then by jitter, then by address.
///
/// [`ScoredRelay::total_leg_us`] is the quantity the chooser minimises: the
/// client leg plus the destination-leg estimate plus that estimate's own
/// uncertainty. Comparing it first is what makes the destination leg a
/// validity/uncertainty term rather than an overriding key - the previous
/// `dest_leg_us / 50_000` bucket could rank a relay whose estimated whole path is
/// 368.9 ms above one at 283.5 ms
/// (`tests::the_whole_path_total_outranks_the_destination_bucket`).
///
/// Jitter only separates exact ties, and the relay address separates the
/// remaining ones, so the order is total and deterministic: two equal candidates
/// cannot swap between calls, which is what keeps a live session from flapping.
/// The previous key added `2 x jitter` to the score; that was a proxy for the
/// destination leg, not the destination leg, and on the field session it added
/// 176 ms to the Los Angeles relay from mobile-link spread alone.
///
/// The old doc justified the bucket with a possible server-side region check ("a
/// relay on the wrong side of the world can be rejected"). The ranking has no
/// evidence about any such check; if one is ever observed it belongs in candidate
/// selection as a validity filter, not as an ordering key that overrides the
/// estimate.
/// Sort by the whole-path total, then by the pre-floor destination prior, then
/// by the measured client leg, then by jitter, then by address.
///
/// [`ScoredRelay::total_leg_us`] is the quantity the chooser minimises: the
/// client leg plus the destination-leg estimate plus that estimate's own
/// uncertainty. Comparing it first is what makes the destination leg a
/// validity/uncertainty term rather than an overriding key - the previous
/// `dest_leg_us / 50_000` bucket could rank a relay whose estimated whole path is
/// 368.9 ms above one at 283.5 ms
/// (`tests::the_whole_path_total_outranks_the_destination_bucket`).
///
/// The pre-floor prior is the second key because the triangle floor deliberately
/// makes several candidates' totals *equal*: `GeometricFloor` carries no prior
/// penalty, so a floored candidate's total is exactly the direct ping and the
/// totals cannot separate them. Before this key existed, those ties fell through
/// to jitter and then the numerical relay address - on the live ROB RUN session
/// of 2026-11-19 that selected relay-syd-1 (`45.76.114.93`) for a Virginia
/// server purely because its address sorted first. The prior's magnitude was
/// invalidated by the bound, but its ordering (which relay is nearer the server)
/// is still destination evidence, and it is the only such evidence left at that
/// point.
///
/// The measured client leg comes next, before jitter and the address, because
/// the prior cannot always separate the candidates either: every US address
/// resolves to one country centroid, so LAX and EWR carry the *same* prior, and
/// a floor tie between them used to fall through to jitter then to whichever IP
/// sorted first. A real client-leg measurement is evidence; a mobile link's
/// jitter and the numerical relay address are not.
fn sort_scored(scored: &mut [ScoredRelay]) {
    scored.sort_by(|a, b| {
        a.total_leg_us
            .cmp(&b.total_leg_us)
            .then_with(|| a.dest_leg_prior_us.cmp(&b.dest_leg_prior_us))
            .then_with(|| a.client_leg_us.cmp(&b.client_leg_us))
            .then_with(|| a.jitter_us.cmp(&b.jitter_us))
            .then_with(|| a.relay.cmp(&b.relay))
    });
}

/// Rank relays for a destination, best first.
///
/// This is the coarse entry point: it has no destination position, so the
/// second leg comes from the library's region prior. Prefer [`rank_at`] when the
/// caller knows where the server is, because the region map collapses `us-east`
/// and `us-west` into one `na` centroid and cannot separate two relays on
/// opposite sides of the continent.
///
/// Deterministic and total: ties break on jitter and then on the relay address,
/// so the order is stable across calls, which is what lets the caller avoid
/// flapping.
pub fn rank(server: Ipv4Addr, candidates: &[RelayCandidate]) -> Vec<ScoredRelay> {
    rank_with_direct(server, candidates, None)
}

/// Rank on the region prior alone, with the measured direct ping folded in.
///
/// The coordinate-free sibling of [`rank_at_with_direct`], for when the server's
/// position is not known: the same floor applies to whatever the region prior
/// claims.
pub fn rank_with_direct(
    server: Ipv4Addr,
    candidates: &[RelayCandidate],
    direct_rtt_us: Option<u64>,
) -> Vec<ScoredRelay> {
    let dest_region = region_for(server);

    let mut scored: Vec<ScoredRelay> = candidates
        .iter()
        .filter_map(|c| {
            let (dest_leg_us, dest_source) = dest_leg_for(server, dest_region.as_deref(), c);
            let dest_leg_prior_us = dest_leg_us;
            let (dest_leg_us, dest_source) =
                apply_floor(dest_leg_us, dest_source, direct_rtt_us, c.client_leg_us);
            score_with(c, dest_leg_us, dest_source, dest_leg_prior_us)
        })
        .collect();

    sort_scored(&mut scored);
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
    rank_at_with_direct(server, server_location, candidates, None)
}

/// Rank with the measured direct ping to the destination folded in.
///
/// `direct_rtt_us` is a correlated measurement (ICMP to the game server, no relay
/// in the path). Round trips obey the triangle inequality, so for every candidate
/// `relay_to_server >= direct - client_leg`, and a prior claiming less is provably
/// too optimistic. Raising it to that bound has a property worth stating: a relay
/// whose client leg is already at least the direct ping scores a total at least
/// equal to the direct path, which is the truth - a relay adds a hop, it cannot
/// remove latency the direct route does not have.
pub fn rank_at_with_direct(
    server: Ipv4Addr,
    server_location: (f64, f64),
    candidates: &[RelayCandidate],
    direct_rtt_us: Option<u64>,
) -> Vec<ScoredRelay> {
    let dest_region = region_for(server);

    let mut scored: Vec<ScoredRelay> = candidates
        .iter()
        .filter_map(|c| {
            score_one_at(
                server,
                server_location,
                dest_region.as_deref(),
                c,
                direct_rtt_us,
            )
        })
        .collect();

    sort_scored(&mut scored);
    scored
}

/// The region a destination belongs to, via the library's shared vocabulary.
fn region_for(server: Ipv4Addr) -> Option<String> {
    destination::global()
        .and_then(|e| e.destination_region(server))
        .map(|s| s.to_string())
}

/// Score one candidate against a known destination position.
/// Raise a destination-leg figure to the triangle-inequality floor when the
/// measured direct ping proves it too optimistic.
fn apply_floor(
    dest_leg_us: u64,
    source: DestLegSource,
    direct_rtt_us: Option<u64>,
    client_leg_us: Option<u64>,
) -> (u64, DestLegSource) {
    match (direct_rtt_us, client_leg_us) {
        (Some(direct), Some(client)) => {
            let floor = direct.saturating_sub(client);
            if dest_leg_us < floor {
                (floor, DestLegSource::GeometricFloor)
            } else {
                (dest_leg_us, source)
            }
        }
        _ => (dest_leg_us, source),
    }
}

fn score_one_at(
    server: Ipv4Addr,
    server_location: (f64, f64),
    dest_region: Option<&str>,
    candidate: &RelayCandidate,
    direct_rtt_us: Option<u64>,
) -> Option<ScoredRelay> {
    let (mut dest_leg_us, mut dest_source) =
        dest_leg_for_at(server, server_location, dest_region, candidate);
    // Keep the prior's own figure: the floor below may replace it with a lower
    // bound, but its ordering is the only destination signal left for a tie.
    let dest_leg_prior_us = dest_leg_us;
    let (raised, raised_source) = apply_floor(
        dest_leg_us,
        dest_source,
        direct_rtt_us,
        candidate.client_leg_us,
    );
    dest_leg_us = raised;
    dest_source = raised_source;
    score_with(candidate, dest_leg_us, dest_source, dest_leg_prior_us)
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
    dest_leg_prior_us: u64,
) -> Option<ScoredRelay> {
    let client_leg_us = candidate.client_leg_us?;

    let penalty = match dest_source {
        DestLegSource::Measured => 0,
        // A bound is still not a measurement of this pair, but it is a proof
        // rather than a guess, so it does not carry the prior's uncertainty.
        DestLegSource::GeometricFloor => 0,
        DestLegSource::RegionPrior | DestLegSource::Unknown => PRIOR_UNDERTAINTY_US,
    };

    Some(ScoredRelay {
        relay: candidate.relay,
        total_leg_us: client_leg_us
            .saturating_add(dest_leg_us)
            .saturating_add(penalty),
        dest_leg_us,
        dest_leg_prior_us,
        client_leg_us,
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
        // The field case with the server's actual position: Ashburn, Virginia.
        // The client legs are the one-RTT measurements from that session
        // (`docs/server-leg-signal.md` §4.3). The destination leg is what decides
        // it - Los Angeles is 73.0 ms from Ashburn while New Jersey is 10.0 ms,
        // which is more than the 41.8 ms by which the client leg favours LAX - so
        // the relay near the game server wins on the whole path even though it is
        // the further relay for the player.
        //
        // This is a destination-leg win: it needs either a precise server
        // position (what `route::server_location` resolves) or a measured L2.
        // With only a country centroid the prior cannot tell the coasts apart;
        // see `the_country_centroid_prior_cannot_separate_the_coasts`.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        let ashburn = (39.0, -77.5);

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

        let ranked = rank_at(server, ashburn, &candidates);
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(207, 246, 124, 117),
            "with a precise server position the 63 ms destination-leg advantage \
             outweighs the 41.8 ms client-leg advantage of the Los Angeles relay"
        );
        assert_eq!(ranked[1].dest_leg_us, 73_021, "LAX to Ashburn");
        assert_eq!(
            ranked[0].dest_leg_us, 10_000,
            "EWR to Ashburn, intra-region"
        );
        assert_eq!(ranked[0].total_leg_us, 321_000);
        assert_eq!(ranked[1].total_leg_us, 342_221);
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
    fn a_prior_below_the_geometric_floor_is_raised_to_it() {
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        // 120 ms client leg, 300 ms direct: the relay-to-server leg cannot be
        // below 180 ms however optimistic the prior is.
        let candidates = [relay([207, 246, 106, 36], "us-west", 120_000, 10_000)];
        let ranked = rank_at_with_direct(server, (39.0, -77.5), &candidates, Some(300_000));
        let best = &ranked[0];
        assert_eq!(best.dest_source, DestLegSource::GeometricFloor);
        assert_eq!(best.dest_leg_us, 180_000, "direct minus client leg");
        assert!(
            best.total_leg_us >= 300_000,
            "a relayed path is never faster than the direct path it replaces: {} ",
            best.total_leg_us
        );
    }

    #[test]
    fn a_leg_above_the_floor_is_left_alone() {
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        // 286 ms client leg against a 300 ms direct ping leaves a 14 ms floor,
        // which any real relay-to-server leg already exceeds.
        let candidates = [relay([207, 246, 124, 117], "us-east", 286_000, 31_000)];
        let ranked = rank_at_with_direct(server, (39.0, -77.5), &candidates, Some(300_000));
        assert_eq!(
            ranked[0].dest_source,
            DestLegSource::RegionPrior,
            "a prior above the bound is not a floor case"
        );
        assert!(ranked[0].dest_leg_us >= 14_000);
    }

    #[test]
    fn without_a_direct_measurement_the_ranking_is_unchanged() {
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        let candidates = [relay([207, 246, 106, 36], "us-west", 120_000, 10_000)];
        let without = rank_at_with_direct(server, (39.0, -77.5), &candidates, None);
        assert_eq!(
            without[0].dest_source,
            DestLegSource::RegionPrior,
            "no direct ping means no bound to enforce"
        );
        let raised = rank_at_with_direct(server, (39.0, -77.5), &candidates, Some(400_000));
        assert_eq!(
            raised[0].dest_source,
            DestLegSource::GeometricFloor,
            "the same prior is provably too low once the direct ping is known"
        );
        assert_eq!(
            raised[0].dest_leg_us, 280_000,
            "400 ms direct - 120 ms client"
        );
    }

    /// The rank-harness counterfactual (`docs/server-leg-signal.md` §4.3): with
    /// one-RTT client legs and the live direct ping, the destination-leg bucket
    /// used to put EWR first even though its own estimated whole path is 85 ms
    /// longer than LAX's.
    #[test]
    fn the_whole_path_total_outranks_the_destination_bucket() {
        // The live ROB RUN server, and the DB-IP point every US address
        // resolves to - the prior's own input.
        let server = Ipv4Addr::new(91, 199, 82, 166);
        let us_point = (37.09, -95.71);
        // The same session's measured direct ICMP ping (5 bursts, median
        // 283.47 ms) and the one-RTT client legs from §4.3.
        let direct_us = 283_474;
        let candidates = [
            relay_node([207, 246, 106, 36], "relay-lax-1", 220_170),
            relay_node([207, 246, 124, 117], "relay-ewr-1", 305_840),
        ];

        let ranked = rank_at_with_direct(server, us_point, &candidates, Some(direct_us));

        assert_eq!(ranked.len(), 2);
        let lax = &ranked[0];
        let ewr = &ranked[1];
        // LAX's prior (41.2 ms) is below `direct - client_leg` (63.3 ms), so the
        // triangle bound raises it and the whole path equals the direct ping
        // exactly: a relay cannot be faster than the path it replaces.
        assert_eq!(lax.relay.ip(), &Ipv4Addr::new(207, 246, 106, 36));
        assert_eq!(lax.dest_source, DestLegSource::GeometricFloor);
        assert_eq!(lax.dest_leg_us, 63_304, "direct minus the LAX client leg");
        assert_eq!(lax.total_leg_us, direct_us);
        // EWR's prior is above its floor (its client leg alone already exceeds
        // the direct ping), so it keeps the prior and its uncertainty charge.
        assert_eq!(ewr.relay.ip(), &Ipv4Addr::new(207, 246, 124, 117));
        assert_eq!(ewr.dest_source, DestLegSource::RegionPrior);
        assert_eq!(ewr.dest_leg_us, 38_042);
        assert_eq!(ewr.total_leg_us, 368_882);
        // The regression is the key order, not a tie: EWR's destination leg
        // falls in bucket 0 and LAX's floored leg in bucket 1.
        assert!(
            ewr.dest_leg_us / 50_000 < lax.dest_leg_us / 50_000,
            "the old key ranked on this bucket before the whole path"
        );
        // No candidate may be presented as better than the direct path it
        // replaces: the floor makes every total at least the measured direct one.
        assert!(
            ranked.iter().all(|scored| scored.total_leg_us >= direct_us),
            "a relayed estimate below the direct ping is not a possible path: {:?}",
            ranked
                .iter()
                .map(|scored| scored.total_leg_us)
                .collect::<Vec<_>>()
        );
    }

    /// A candidate row with its total composed exactly as the ranking composes
    /// it, so a comparator test cannot drift from `score_with`'s penalty rule.
    fn scored_from(
        candidate: &RelayCandidate,
        dest_leg_us: u64,
        source: DestLegSource,
    ) -> ScoredRelay {
        score_with(candidate, dest_leg_us, source, dest_leg_us)
            .expect("a candidate with a client leg scores")
    }

    #[test]
    fn a_measured_destination_leg_is_compared_on_the_whole_path() {
        // The destination leg is a validity/uncertainty term, not the key: a
        // measured 50 ms leg (bucket 1) beats a prior 40 ms leg (bucket 0) once
        // the prior's 25 ms uncertainty is charged, because 150 < 165 ms.
        let measured = relay([10, 0, 0, 1], "us-east", 100_000, 0);
        let prior = relay([10, 0, 0, 2], "us-east", 100_000, 0);
        let measured_row = scored_from(&measured, 50_000, DestLegSource::Measured);
        let prior_row = scored_from(&prior, 40_000, DestLegSource::RegionPrior);
        assert_eq!(measured_row.total_leg_us, 150_000);
        assert_eq!(prior_row.total_leg_us, 165_000);
        assert!(
            prior_row.dest_leg_us < measured_row.dest_leg_us,
            "the prior's destination leg is the smaller one, and that used to be the key"
        );
        let mut ranked = vec![prior_row, measured_row.clone()];
        sort_scored(&mut ranked);
        assert_eq!(ranked[0], measured_row, "the smaller whole path wins");

        // The same comparison the other way: the prior only wins when its whole
        // path really is shorter, not because its leg is smaller.
        let worse_measurement = scored_from(&measured, 200_000, DestLegSource::Measured);
        let prior_row = scored_from(&prior, 40_000, DestLegSource::RegionPrior);
        let mut ranked = vec![worse_measurement, prior_row.clone()];
        sort_scored(&mut ranked);
        assert_eq!(ranked[0], prior_row);
    }

    #[test]
    fn the_country_centroid_prior_cannot_separate_the_coasts() {
        // The honest limitation the whole-path key makes visible. The GUI's
        // offline country database resolves every US address to one point, so the
        // prior it feeds is the same for a US-East and a US-West server. For the
        // field case (an Ashburn server, client legs from §4.3) that puts LAX's
        // destination leg at 37.1 ms and EWR's at 41.3 ms - 4.2 ms apart, while
        // the relay-to-server legs really differ by 63.0 ms
        // (`a_known_server_position_picks_the_relay_in_its_own_region`). The
        // prior's 4.2 ms cannot outweigh a 41.8 ms client-leg difference, so the
        // client leg decides: LAX, the relay the field evidence says is on the
        // wrong coast.
        //
        // That is not a ranking defect to hide behind a weighted jitter term or a
        // destination bucket - the estimate really is that compressed. It is the
        // measurement gap `docs/server-leg-signal.md` §4.4 quantifies: the fix is
        // a real L2 signal (v1/v2) or a precise server position, not a different
        // sort key.
        let server = Ipv4Addr::new(3, 237, 142, 3);
        install_region(server, "us-east");
        let us_centroid = (39.83, -98.58); // the `na` region centroid

        let candidates = [
            relay_node([207, 246, 106, 36], "relay-lax-1", 244_200),
            relay_node([207, 246, 124, 117], "relay-ewr-1", 286_000),
        ];

        let ranked = rank_at(server, us_centroid, &candidates);
        let lax = &ranked[0];
        let ewr = &ranked[1];
        assert_eq!(lax.relay.ip(), &Ipv4Addr::new(207, 246, 106, 36));
        assert_eq!(lax.dest_leg_us, 37_138);
        assert_eq!(ewr.dest_leg_us, 41_326);
        assert!(
            ewr.dest_leg_us - lax.dest_leg_us < 5_000,
            "the country point separates the coasts by {} us, far below the \
             41.8 ms client-leg difference",
            ewr.dest_leg_us - lax.dest_leg_us
        );
        assert_eq!(lax.total_leg_us, 306_338);
        assert_eq!(ewr.total_leg_us, 352_326);
    }

    #[test]
    fn ties_are_deterministic_and_jitter_breaks_an_exact_total_tie() {
        // Equal whole paths: the steadier relay wins, and the same inputs always
        // produce the same order so a live session cannot flap between them.
        let row = |ip: [u8; 4], jitter_us: u64| {
            scored_from(
                &relay(ip, "us-east", 100_000, jitter_us),
                50_000,
                DestLegSource::Measured,
            )
        };
        let mut ranked = vec![row([10, 0, 0, 2], 9_000), row([10, 0, 0, 1], 5_000)];
        sort_scored(&mut ranked);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(10, 0, 0, 1),
            "equal totals: the steadier relay wins"
        );
        let first = ranked.clone();
        sort_scored(&mut ranked);
        assert_eq!(ranked, first, "the same inputs must produce the same order");
        // Equal totals and equal jitter: the address decides, so the order is
        // total and a sort can never swap two equal candidates between calls.
        let mut tied = vec![row([10, 0, 0, 2], 5_000), row([10, 0, 0, 1], 5_000)];
        sort_scored(&mut tied);
        assert_eq!(tied[0].relay.ip(), &Ipv4Addr::new(10, 0, 0, 1));
    }

    #[test]
    fn a_floored_candidate_is_ordered_by_the_prior_not_by_jitter_or_address() {
        // The live defect this key fixes. When the direct ping floors several
        // candidates, every total is exactly the direct ping (`GeometricFloor`
        // carries no penalty), the totals tie, and the old key fell through to
        // jitter and then the numerical relay address. On 2026-11-19 that picked
        // relay-syd-1 (45.76.114.93) for a Virginia server because its address
        // sorted first. The prior's magnitude was proven too optimistic, but its
        // order still says which relay is nearer the server; that must decide.
        let nearest = relay([10, 0, 0, 2], "us-east", 100_000, 50_000);
        let farther = relay([10, 0, 0, 1], "us-east", 100_000, 0);
        // Both floored to the same bound: 100 ms client + 200 ms floor = 300 ms.
        let nearest_row = score_with(&nearest, 200_000, DestLegSource::GeometricFloor, 20_000)
            .expect("a candidate with a client leg scores");
        let farther_row = score_with(&farther, 200_000, DestLegSource::GeometricFloor, 150_000)
            .expect("a candidate with a client leg scores");
        assert_eq!(nearest_row.total_leg_us, 300_000);
        assert_eq!(farther_row.total_leg_us, 300_000);
        assert!(farther_row.jitter_us < nearest_row.jitter_us);
        assert!(farther_row.relay < nearest_row.relay);
        let mut ranked = vec![farther_row, nearest_row.clone()];
        sort_scored(&mut ranked);
        assert_eq!(
            ranked[0], nearest_row,
            "with every total at the floor, the destination prior decides - not the \
             steadier jitter and not the smaller relay address"
        );
    }

    #[test]
    fn a_floor_tie_with_equal_priors_is_broken_by_the_client_leg_not_the_address() {
        // The destination-less shape of the live 2026-11-19 defect. The client has
        // no exact server position, so candidates are raised to the measured
        // direct ping and every total is exactly that ping; when the priors tie
        // too - every US address resolves to the same country centroid, and the
        // two US relays are the canonical pair - the old key fell through to
        // jitter and then to the numerically smaller relay address. The measured
        // client leg is the only real evidence left, so it must decide.
        let faster_client = relay([10, 0, 0, 2], "us-west", 45_500, 0);
        let slower_client = relay([10, 0, 0, 1], "us-east", 216_500, 0);
        // Both floored to 315 ms direct: client + (direct - client) = direct.
        let faster_row = score_with(
            &faster_client,
            269_500,
            DestLegSource::GeometricFloor,
            10_000,
        )
        .expect("a candidate with a client leg scores");
        let slower_row = score_with(
            &slower_client,
            98_500,
            DestLegSource::GeometricFloor,
            10_000,
        )
        .expect("a candidate with a client leg scores");
        assert_eq!(faster_row.total_leg_us, 315_000);
        assert_eq!(slower_row.total_leg_us, 315_000);
        assert_eq!(
            faster_row.dest_leg_prior_us, slower_row.dest_leg_prior_us,
            "the country centroid gives both US relays the same prior"
        );
        assert_eq!(faster_row.jitter_us, slower_row.jitter_us);
        assert!(
            faster_row.relay > slower_row.relay,
            "the address key alone would pick the slower client leg"
        );
        let mut ranked = vec![faster_row.clone(), slower_row.clone()];
        sort_scored(&mut ranked);
        assert_eq!(
            ranked[0], faster_row,
            "a floored tie with equal priors is broken by the measured client leg"
        );
        assert_eq!(ranked[0].client_leg_us, 45_500);
        assert_eq!(ranked[1].client_leg_us, 216_500);
        // And the ordering key is the client leg, not the relay address: the
        // winner is the numerically larger address, which the old key ranked last.
        assert!(
            ranked[0].relay > ranked[1].relay,
            "the numerical relay address must not decide the tie"
        );
    }

    #[test]
    fn the_client_leg_never_outranks_a_smaller_whole_path() {
        // The key is a tie-break, not a fourth term: a relay that is closer to the
        // client but on a longer whole path still loses.
        let short_leg = scored_from(
            &relay([10, 0, 0, 1], "us-east", 45_500, 0),
            400_000,
            DestLegSource::Measured,
        );
        let shorter_path = scored_from(
            &relay([10, 0, 0, 9], "us-east", 216_500, 0),
            50_000,
            DestLegSource::Measured,
        );
        assert_eq!(short_leg.total_leg_us, 445_500);
        assert_eq!(shorter_path.total_leg_us, 266_500);
        let mut ranked = vec![short_leg, shorter_path.clone()];
        sort_scored(&mut ranked);
        assert_eq!(ranked[0], shorter_path);
    }

    /// The live ROB RUN case of 2026-11-19: the measured client legs, the direct
    /// ICMP ping of that session, and the wrong JP country point the shipped
    /// DB-IP table still resolves for the server's hosting range.
    #[test]
    fn the_live_rob_run_floor_tie_is_not_won_by_the_relay_address() {
        let server = Ipv4Addr::new(178, 107, 14, 31);
        let direct_us = 325_000;
        let candidates = [
            relay_node([207, 246, 106, 36], "relay-lax-1", 216_500),
            relay_node([207, 246, 124, 117], "relay-ewr-1", 304_700),
            relay_node([45, 77, 32, 236], "relay-sgp-1", 45_500),
            relay_node([95, 179, 164, 138], "relay-fra", 200_200),
            relay_node([167, 179, 77, 127], "relay-nrt", 120_900),
            relay_node([65, 20, 92, 201], "relay-bom-1", 95_500),
            relay_node([65, 20, 99, 61], "relay-mad-1", 196_100),
            relay_node([45, 76, 114, 93], "relay-syd-1", 138_400),
        ];
        let ranked = rank_at_with_direct(server, (36.2, 138.3), &candidates, Some(direct_us));
        let floored: Vec<&ScoredRelay> = ranked
            .iter()
            .filter(|scored| scored.dest_source == DestLegSource::GeometricFloor)
            .collect();
        assert!(
            floored.len() >= 3,
            "the live ping floors several candidates, got {}",
            floored.len()
        );
        assert_eq!(ranked[0].dest_source, DestLegSource::GeometricFloor);
        assert_eq!(
            ranked[0].dest_leg_prior_us,
            floored
                .iter()
                .map(|scored| scored.dest_leg_prior_us)
                .min()
                .expect("at least one floored candidate"),
            "the winner must be the floored candidate nearest the destination point"
        );
        // The old key picked the smallest relay address among the floored set
        // (relay-syd-1, 45.76.114.93); that must no longer be what wins.
        let smallest_address = floored
            .iter()
            .min_by(|a, b| a.relay.cmp(&b.relay))
            .expect("at least one floored candidate");
        assert_ne!(
            ranked[0].relay, smallest_address.relay,
            "the numerical relay address must not decide a floor tie"
        );
    }

    #[test]
    fn jitter_only_breaks_an_exact_tie_never_a_larger_total() {
        // The key used to add `2 x jitter` to the score. On the field session
        // that added 176 ms to the Los Angeles relay (88 ms of spread) and is
        // what actually produced the New Jersey pick: a mobile link's width, not
        // the destination leg. The whole-path total is the estimate, so a
        // swingier relay with the shorter estimate must still win.
        let swingy = scored_from(
            &relay([10, 0, 0, 1], "us-east", 100_000, 60_000),
            50_000,
            DestLegSource::Measured,
        );
        let steady = scored_from(
            &relay([10, 0, 0, 2], "us-east", 130_000, 5_000),
            50_000,
            DestLegSource::Measured,
        );
        assert_eq!(swingy.total_leg_us, 150_000);
        assert_eq!(steady.total_leg_us, 180_000);
        // What the removed weighting would have done: steady 180 + 10 beats
        // swingy 150 + 120, inverting the order the estimate supports.
        assert!(
            steady.total_leg_us + 2 * steady.jitter_us < swingy.total_leg_us + 2 * swingy.jitter_us,
            "the weighted score did favour the steadier relay here"
        );
        let mut ranked = vec![steady, swingy.clone()];
        sort_scored(&mut ranked);
        assert_eq!(
            ranked[0], swingy,
            "the smaller whole path wins even when the relay is the swingier one"
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

    fn relay_node(ip: [u8; 4], node_id: &str, client_us: u64) -> RelayCandidate {
        RelayCandidate {
            relay: SocketAddrV4::new(Ipv4Addr::from(ip), 4434),
            region: None,
            node_id: Some(node_id.to_string()),
            client_leg_us: Some(client_us),
            client_jitter_us: 0,
        }
    }

    #[test]
    fn a_relay_near_the_server_beats_a_short_client_leg_on_the_wrong_continent() {
        // The owner's requirement: a European game server must use the European
        // relay even when the client's shortest leg is to Singapore. The server
        // may reject a source from another region, and the relay-to-server leg
        // is the latency the client cannot recover.
        let server = Ipv4Addr::new(35, 195, 201, 202); // Brussels, europe-west
        let brussels = (50.85, 4.35);
        let candidates = [
            relay_node([45, 77, 32, 236], "relay-sgp-1", 60_000),
            relay_node([95, 179, 164, 138], "relay-fra", 220_000),
        ];

        let ranked = rank_at(server, brussels, &candidates);
        assert_eq!(
            ranked[0].relay.ip(),
            &Ipv4Addr::new(95, 179, 164, 138),
            "the relay nearest the server must win"
        );
    }
}
