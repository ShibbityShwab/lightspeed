//! Where is the game server? The honest ladder for the route card.
//!
//! The country blob in [`crate::geo`] is a free database (DB-IP Lite, CC BY
//! 4.0) and it is wrong for hosting ranges: a G-Core US-Virginia server
//! resolves to JP/Tokyo, which is how the window once showed a Virginia server
//! in Japan. Three offline indexes, generated at build time, fix the cases the
//! country table cannot:
//!
//! 1. **Cloud-provider ranges** (AWS and GCP public IP-range feeds) give the
//!    exact provider *region* for a server hosted on those clouds, which is a
//!    fact rather than a geolocation guess.
//! 2. **Operator geofeeds** (public RFC 8805 feeds, for example G-Core's) state
//!    the city and region the network operator publishes for a prefix. They are
//!    shown as-is with the operator named and the feed's as-of date. This table
//!    is display-only: it never steers [`chooser_point`] or the relay ranking.
//!    The owner decision of 2026-11-20 chose public operator geofeeds over any
//!    licensed database (no MaxMind/GeoLite2), with attribution and a takedown
//!    list - see `infra/geo/geofeeds/`.
//! 3. **Hosting-network ranges** (curated game-hosting/CDN ASNs, announced
//!    prefixes from the free RIPE Stat API at build time) identify servers on
//!    networks where free geoIP is unreliable. Such a server is reported as
//!    [`Place::Hosting`] with no position - an honest "hosting network, location
//!    unknown" instead of a country the data cannot support.
//!
//! Only when none of those indexes matches does the country table answer, and
//! the card labels it as a country-level estimate. The shipped client never
//! calls a network service for any of this; every blob is committed.
//!
//! Record format (`server-cloud.bin`, `server-hosting.bin`,
//! `server-geofeed.bin`): `start u32 | end u32 | index u16`, little-endian,
//! sorted by start and non-overlapping (the build scripts normalise the ranges).

use std::net::Ipv4Addr;

use crate::server_geofeed_tables::{
    GeofeedPlace, GeofeedSource, SERVER_GEOFEED_PLACES, SERVER_GEOFEED_SOURCES,
};
use crate::server_tables::{CLOUD_REGIONS, HOSTING_PROVIDERS};

const CLOUD: &[u8] = include_bytes!("../data/server-cloud.bin");
const HOSTING: &[u8] = include_bytes!("../data/server-hosting.bin");
/// Operator geofeed ranges and the places/sources they index into.
const GEOFEED: &[u8] = include_bytes!("../data/server-geofeed.bin");
/// Bytes per range record: start (4) + end (4) + table index (2).
const RECORD: usize = 10;

/// What the client can honestly say about a server's location.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Place {
    /// A cloud-provider region: exact, from the provider's own published ranges.
    /// `at` is absent for anycast scopes (`global`) and uncurated regions.
    Cloud {
        /// Provider name as it appears in the feed (`aws`, `gcp`).
        provider: &'static str,
        /// Provider region or scope (`us-east-1`, `europe-west1`, `global`).
        region: &'static str,
        /// Representative point for the region, when one is known.
        at: Option<(f32, f32)>,
    },
    /// An operator's published RFC 8805 geofeed entry: the network operator
    /// states the city/region for a prefix. Display-only - the feed carries no
    /// coordinate, so the card states the place instead of drawing a dot, and
    /// this variant is never used for ranking.
    Geofeed {
        /// Short operator name shown in the label (`G-Core`).
        provider: &'static str,
        /// ISO 3166-1 alpha-2 country code, as published (uppercased).
        country: &'static str,
        /// Region/subdivision as published (`US-VA`), when the feed gives one.
        region: Option<&'static str>,
        /// City as published (`Manassas`), when the feed gives one.
        city: Option<&'static str>,
        /// Public feed URL the entry came from, for the source line.
        source: &'static str,
        /// Feed generation date (UTC, `YYYY-MM-DD`).
        as_of: &'static str,
    },
    /// A hosting network whose geoIP country is not trustworthy.
    Hosting {
        /// Autonomous system number of the network.
        asn: u32,
        /// Network name (for the ledger label).
        name: &'static str,
    },
    /// The offline country database's answer: country level, an estimate.
    Country {
        /// ISO 3166-1 alpha-2 country code.
        code: &'static str,
        /// Country centroid, the point the map has always used.
        at: (f32, f32),
    },
    /// Nothing known: no cloud, no hosting match, no country.
    Unknown,
}

/// Last range whose start is at or below `target`, then check its end.
fn record_at(blob: &[u8], target: u32) -> Option<u16> {
    let n = blob.len() / RECORD;
    if n == 0 {
        return None;
    }
    let start_at = |i: usize| -> u32 {
        let o = i * RECORD;
        u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]])
    };
    let (mut lo, mut hi) = (0usize, n - 1);
    let mut found = None;
    while lo <= hi {
        let mid = lo + (hi - lo) / 2;
        if start_at(mid) <= target {
            found = Some(mid);
            lo = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            hi = mid - 1;
        }
    }
    let idx = found?;
    let o = idx * RECORD;
    let start = u32::from_le_bytes([blob[o], blob[o + 1], blob[o + 2], blob[o + 3]]);
    let end = u32::from_le_bytes([blob[o + 4], blob[o + 5], blob[o + 6], blob[o + 7]]);
    if target < start || target > end {
        return None;
    }
    Some(u16::from_le_bytes([blob[o + 8], blob[o + 9]]))
}

/// Resolve one address through a geofeed blob and its place/source tables.
///
/// Separate from [`place`] so the empty-table case and the index bounds are
/// testable with fixture slices: an empty blob resolves to `None` without
/// touching the tables, and a record whose index is past either table is
/// ignored instead of panicking.
fn geofeed_at(
    ranges: &[u8],
    places: &[GeofeedPlace],
    sources: &[GeofeedSource],
    target: u32,
) -> Option<Place> {
    let index = record_at(ranges, target)? as usize;
    let place = places.get(index)?;
    let source = sources.get(place.source as usize)?;
    Some(Place::Geofeed {
        provider: source.provider,
        country: place.country,
        region: place.region,
        city: place.city,
        source: source.url,
        as_of: source.as_of,
    })
}

/// The human place label for an operator geofeed entry: `Manassas, US-VA`.
///
/// Falls back to `City, COUNTRY` when the feed omits the region and to the
/// region/country when it omits the city, so a sparse feed still yields a
/// readable label instead of an empty or dangling-comma one.
pub fn geofeed_place_text(country: &str, region: Option<&str>, city: Option<&str>) -> String {
    fn non_empty(value: Option<&str>) -> Option<&str> {
        value.filter(|text| !text.trim().is_empty())
    }
    match (non_empty(city), non_empty(region)) {
        (Some(city), Some(region)) => format!("{city}, {region}"),
        (Some(city), None) => format!("{city}, {country}"),
        (None, Some(region)) => format!("{region}, {country}"),
        (None, None) => country.to_string(),
    }
}

/// Resolve an address through the ladder: cloud region, operator geofeed,
/// hosting network, offline country, unknown.
pub fn place(ip: Ipv4Addr) -> Place {
    let target = u32::from(ip);
    if let Some(index) = record_at(CLOUD, target) {
        if let Some(region) = CLOUD_REGIONS.get(index as usize) {
            return Place::Cloud {
                provider: region.provider,
                region: region.region,
                at: region.at,
            };
        }
    }
    if let Some(named) = geofeed_at(
        GEOFEED,
        SERVER_GEOFEED_PLACES,
        SERVER_GEOFEED_SOURCES,
        target,
    ) {
        return named;
    }
    if let Some(index) = record_at(HOSTING, target) {
        if let Some(provider) = HOSTING_PROVIDERS.get(index as usize) {
            return Place::Hosting {
                asn: provider.asn,
                name: provider.name,
            };
        }
    }
    match crate::geo::locate(ip) {
        Some((code, at)) => Place::Country { code, at },
        None => Place::Unknown,
    }
}

/// The point the route card may draw for this address.
///
/// Cloud regions use the provider region's point; a country-level estimate uses
/// the country centroid it has always used. An operator geofeed entry and a
/// hosting match have **no** point on purpose: the geofeed has no coordinate,
/// and for a hosting range the country beside it is exactly what the free
/// database gets wrong, so the card states the network instead of drawing a dot
/// somewhere false.
pub fn display_point(ip: Ipv4Addr) -> Option<(f32, f32)> {
    match place(ip) {
        Place::Cloud { at, .. } => at,
        Place::Country { at, .. } => Some(at),
        Place::Geofeed { .. } | Place::Hosting { .. } | Place::Unknown => None,
    }
}

/// The point the destination-aware chooser may use as a server prior.
///
/// Strictly narrower than [`display_point`]: only a provider region is exact
/// enough to rank relays on. A country centroid is not used here any more,
/// because for the ranges this feature exists for it is provably wrong (the
/// G-Core case), and an operator geofeed entry is **display-only**: the feed
/// publishes a city, not coordinates or a measured server leg, so it never
/// steers the chooser until a measurement validates it. Without a point the
/// engine ranks on measurements and the triangle floor, which is the honest
/// fallback.
pub fn chooser_point(ip: Ipv4Addr) -> Option<(f64, f64)> {
    match place(ip) {
        Place::Cloud {
            at: Some((lat, lon)),
            ..
        } => Some((lat as f64, lon as f64)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn addr(s: &str) -> Ipv4Addr {
        Ipv4Addr::from_str(s).unwrap()
    }

    #[test]
    fn an_aws_server_gets_its_exact_region() {
        // 3.237.221.191 is in AWS's us-east-1 range and its PTR is
        // ec2-3-237-221-191.compute-1.amazonaws.com (verified 2026-11-19).
        match place(addr("3.237.221.191")) {
            Place::Cloud {
                provider,
                region,
                at,
            } => {
                assert_eq!(provider, "aws");
                assert_eq!(region, "us-east-1");
                assert_eq!(at, Some((39.0, -77.5)));
            }
            other => panic!("expected an AWS region, got {other:?}"),
        }
    }

    #[test]
    fn a_gcp_server_gets_its_exact_region() {
        // 34.38.12.1 is GCP europe-west1 (DB-IP also said BE for this range).
        match place(addr("34.38.12.1")) {
            Place::Cloud {
                provider,
                region,
                at,
            } => {
                assert_eq!(provider, "gcp");
                assert_eq!(region, "europe-west1");
                assert_eq!(at, Some((50.4, 4.4)));
            }
            other => panic!("expected a GCP region, got {other:?}"),
        }
    }

    #[test]
    fn the_gcore_virginia_server_gets_the_operator_geofeed_city() {
        // The live ROB RUN server. DB-IP Lite/City both say JP, but G-Core's
        // own RFC 8805 geofeed publishes US-VA Manassas. The card must state the
        // operator's answer with attribution - never Japan, and no longer the
        // "hosting unknown" placeholder now that the operator says where it is.
        match place(addr("178.107.14.201")) {
            Place::Geofeed {
                provider,
                country,
                region,
                city,
                source,
                as_of,
            } => {
                assert_eq!(provider, "G-Core");
                assert_eq!(country, "US");
                assert_eq!(region, Some("US-VA"));
                assert_eq!(city, Some("Manassas"));
                assert_eq!(source, "https://geofeed.gcore.lu/IP-Range.csv");
                assert_eq!(as_of, "2026-11-19");
            }
            other => panic!("expected the G-Core operator geofeed, got {other:?}"),
        }
        assert_eq!(
            display_point(addr("178.107.14.201")),
            None,
            "the geofeed has no coordinate, so the card draws no dot"
        );
        assert_eq!(
            chooser_point(addr("178.107.14.201")),
            None,
            "a geofeed entry is display-only until a measurement validates it"
        );
    }

    #[test]
    fn a_hosting_network_without_a_geofeed_stays_hosting() {
        // 2.26.132.10 is an OVH range in the RIPE-derived hosting index. OVH
        // publishes no feed we ingest yet, so the honest answer is still
        // "hosting network - location unknown", not a guessed country.
        match place(addr("2.26.132.10")) {
            Place::Hosting { asn, name } => {
                assert_eq!(asn, 16276);
                assert_eq!(name, "OVH");
            }
            other => panic!("expected OVH hosting, got {other:?}"),
        }
        assert_eq!(display_point(addr("2.26.132.10")), None);
        assert_eq!(chooser_point(addr("2.26.132.10")), None);
    }

    #[test]
    fn an_ordinary_address_falls_back_to_the_country_estimate() {
        // 8.8.8.8 is neither in the curated hosting list nor in the cloud
        // feeds; the country table is the last resort and is labelled as such.
        match place(addr("8.8.8.8")) {
            Place::Country { code, .. } => assert_eq!(code, "US"),
            other => panic!("expected the country fallback, got {other:?}"),
        }
        assert!(display_point(addr("8.8.8.8")).is_some());
        assert_eq!(
            chooser_point(addr("8.8.8.8")),
            None,
            "a country centroid must not steer the chooser"
        );
    }

    #[test]
    fn nothing_known_stays_unknown() {
        assert_eq!(place(addr("0.0.0.0")), Place::Unknown);
        assert_eq!(display_point(addr("0.0.0.0")), None);
        assert_eq!(chooser_point(addr("0.0.0.0")), None);
    }

    #[test]
    fn cloud_ranges_are_sorted_and_index_their_table() {
        assert_eq!(CLOUD.len() % RECORD, 0, "cloud blob is not whole records");
        assert!(!CLOUD.is_empty(), "cloud blob is empty");
        let mut previous_end = None;
        for i in 0..CLOUD.len() / RECORD {
            let o = i * RECORD;
            let start = u32::from_le_bytes(CLOUD[o..o + 4].try_into().unwrap());
            let end = u32::from_le_bytes(CLOUD[o + 4..o + 8].try_into().unwrap());
            let index = u16::from_le_bytes(CLOUD[o + 8..o + 10].try_into().unwrap());
            assert!(start <= end, "range {i} is inverted");
            if let Some(previous) = previous_end {
                assert!(previous < start, "ranges overlap or are unsorted at {i}");
            }
            previous_end = Some(end);
            assert!(
                (index as usize) < CLOUD_REGIONS.len(),
                "range {i} indexes past the region table"
            );
        }
    }

    #[test]
    fn hosting_ranges_are_sorted_and_index_their_table() {
        assert_eq!(HOSTING.len() % RECORD, 0);
        assert!(
            HOSTING.len() / RECORD > 1_000,
            "hosting blob looks truncated"
        );
        let mut previous_end = None;
        for i in 0..HOSTING.len() / RECORD {
            let o = i * RECORD;
            let start = u32::from_le_bytes(HOSTING[o..o + 4].try_into().unwrap());
            let end = u32::from_le_bytes(HOSTING[o + 4..o + 8].try_into().unwrap());
            let index = u16::from_le_bytes(HOSTING[o + 8..o + 10].try_into().unwrap());
            assert!(start <= end);
            if let Some(previous) = previous_end {
                assert!(previous < start, "hosting ranges overlap at {i}");
            }
            previous_end = Some(end);
            assert!((index as usize) < HOSTING_PROVIDERS.len());
        }
    }

    #[test]
    fn every_region_point_is_on_the_planet() {
        for region in CLOUD_REGIONS {
            if let Some((lat, lon)) = region.at {
                assert!(
                    (-90.0..=90.0).contains(&lat),
                    "{} latitude {lat}",
                    region.region
                );
                assert!(
                    (-180.0..=180.0).contains(&lon),
                    "{} longitude {lon}",
                    region.region
                );
            }
        }
    }

    #[test]
    fn a_second_gcore_range_gets_the_same_operator_city() {
        // 91.199.82.233 was the earlier "hosting survives" fixture; the same
        // feed now places it, which is the point of the table - the operator's
        // statement beats "unknown" wherever it covers.
        match place(addr("91.199.82.233")) {
            Place::Geofeed { city, region, .. } => {
                assert_eq!(city, Some("Manassas"));
                assert_eq!(region, Some("US-VA"));
            }
            other => panic!("expected the G-Core operator geofeed, got {other:?}"),
        }
    }

    #[test]
    fn a_non_us_geofeed_row_is_shown_as_the_operator_published_it() {
        // 103.151.134.0/24 is G-Core Frankfurt, DE-HE, 60326 in the feed.
        match place(addr("103.151.134.1")) {
            Place::Geofeed {
                country,
                region,
                city,
                ..
            } => {
                assert_eq!(country, "DE");
                assert_eq!(region, Some("DE-HE"));
                assert_eq!(city, Some("Frankfurt"));
            }
            other => panic!("expected the Frankfurt geofeed row, got {other:?}"),
        }
    }

    #[test]
    fn a_geofeed_place_never_steers_the_ranking() {
        // The variant exists for the label only: no display dot and no chooser
        // prior, even though the operator says exactly which city it is. This is
        // the gate the design doc requires before a measured source may change
        // ranking.
        let ip = addr("178.107.13.65");
        assert!(matches!(place(ip), Place::Geofeed { .. }));
        assert_eq!(display_point(ip), None);
        assert_eq!(chooser_point(ip), None);
    }

    #[test]
    fn an_empty_geofeed_table_resolves_to_nothing() {
        // A build may legitimately emit an empty table (no local feed); the
        // resolver must return None, not index into the tables or panic.
        assert_eq!(
            geofeed_at(&[], &[], &[], u32::from(addr("178.107.14.201"))),
            None
        );
        // A record whose index is past an empty place table is ignored too.
        assert_eq!(
            geofeed_at(
                GEOFEED,
                &[],
                SERVER_GEOFEED_SOURCES,
                u32::from(addr("178.107.14.201"))
            ),
            None
        );
    }

    #[test]
    fn geofeed_place_text_handles_sparse_feeds() {
        assert_eq!(
            geofeed_place_text("US", Some("US-VA"), Some("Manassas")),
            "Manassas, US-VA"
        );
        assert_eq!(geofeed_place_text("PE", None, Some("Lima")), "Lima, PE");
        assert_eq!(geofeed_place_text("US", Some("US-VA"), None), "US-VA, US");
        assert_eq!(geofeed_place_text("US", None, None), "US");
        assert_eq!(
            geofeed_place_text("US", Some(" "), Some("")),
            "US",
            "blank feed fields fall back instead of producing a dangling comma"
        );
    }

    #[test]
    fn geofeed_ranges_are_sorted_and_index_their_tables() {
        assert_eq!(
            GEOFEED.len() % RECORD,
            0,
            "geofeed blob is not whole records"
        );
        assert!(!GEOFEED.is_empty(), "geofeed blob is empty");
        assert!(!SERVER_GEOFEED_SOURCES.is_empty(), "no geofeed sources");
        assert!(!SERVER_GEOFEED_PLACES.is_empty(), "no geofeed places");
        let mut previous_end = None;
        for i in 0..GEOFEED.len() / RECORD {
            let o = i * RECORD;
            let start = u32::from_le_bytes(GEOFEED[o..o + 4].try_into().unwrap());
            let end = u32::from_le_bytes(GEOFEED[o + 4..o + 8].try_into().unwrap());
            let index = u16::from_le_bytes(GEOFEED[o + 8..o + 10].try_into().unwrap());
            assert!(start <= end, "geofeed range {i} is inverted");
            if let Some(previous) = previous_end {
                assert!(previous < start, "geofeed ranges overlap at {i}");
            }
            previous_end = Some(end);
            assert!(
                (index as usize) < SERVER_GEOFEED_PLACES.len(),
                "geofeed range {i} indexes past the place table"
            );
            assert!(
                (SERVER_GEOFEED_PLACES[index as usize].source as usize)
                    < SERVER_GEOFEED_SOURCES.len(),
                "geofeed place {index} indexes past the source table"
            );
        }
        for source in SERVER_GEOFEED_SOURCES {
            assert!(!source.provider.is_empty(), "a geofeed source has no name");
            assert!(
                source.url.starts_with("https://"),
                "a geofeed source has no public URL: {source:?}"
            );
            assert!(
                source.as_of.len() == 10 && source.as_of.as_bytes()[4] == b'-',
                "a geofeed source has no ISO as-of date: {source:?}"
            );
        }
    }

    #[test]
    fn the_committed_manifest_keeps_the_operator_attribution() {
        // The committed source manifest is the audit trail for what ships: the
        // owner decision, the feed URL, the attribution and the redistribution
        // marker must all be present in the artifact a reviewer can read.
        let manifest = include_str!("../data/server-geofeed.sources.json");
        for needle in [
            "no MaxMind/GeoLite2",
            "G-Core Labs S.A.",
            "https://geofeed.gcore.lu/IP-Range.csv",
            "shown as-is",
            "\"redistributed\": true",
            "takedown/deny",
        ] {
            assert!(manifest.contains(needle), "manifest lost {needle:?}");
        }
        assert!(
            manifest.contains("\"shipped\": true"),
            "the committed manifest must describe the shipped table"
        );
    }

    #[test]
    fn the_attribution_is_in_notice_and_the_privacy_policy() {
        let notice = include_str!("../../NOTICE");
        let privacy = include_str!("../../docs/privacy.md");
        let readme = include_str!("../../infra/geo/geofeeds/README.md");
        assert!(
            notice.contains("operator geofeed") && notice.contains("G-Core Labs"),
            "NOTICE lost the operator geofeed attribution"
        );
        assert!(
            notice.contains("https://geofeed.gcore.lu/IP-Range.csv"),
            "NOTICE lost the feed URL"
        );
        assert!(
            notice.contains("No MaxMind") || notice.contains("no MaxMind"),
            "NOTICE must state that no licensed database is used"
        );
        assert!(
            privacy.contains("operator geofeed") && privacy.contains("shown as-is"),
            "docs/privacy.md must describe the operator geofeed data"
        );
        assert!(
            readme.contains("takedown") && readme.contains("shown as-is"),
            "the geofeed README must state the takedown policy and as-is display"
        );
    }
}
