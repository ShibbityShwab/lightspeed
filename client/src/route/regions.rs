//! # Coarse region geography for the destination-leg prior
//!
//! The destination-aware selector ranks relays by the whole path, but the
//! relay-to-destination leg can only be measured for a relay that is actually
//! carrying traffic to that destination. An unselected relay therefore has no
//! measured destination leg and can never win, even when it is the obviously
//! better region. This module supplies the fallback: a coarse, region-level
//! estimate of the relay-to-destination round trip derived from great-circle
//! distance between region centroids.
//!
//! The catalog mirrors `infra/geo/regions.json` (the eight regions, the
//! free-form registry aliases, and the country-to-region map) so the client
//! shares the operator's region vocabulary without a runtime data dependency.
//! Distances are deliberately coarse: a region centroid is not a server
//! position, so the prior is a tie-breaker for unmeasured relays, never a
//! replacement for a real measurement.
//!
//! The conversion factor is 20 microseconds of round trip per kilometre, with
//! a 10 ms floor so two relays in the same region are not treated as
//! zero-latency. That is a round-number approximation of typical backbone
//! RTT (fibre is roughly 5 us/km one way) plus a little headroom for the last
//! mile.

/// One region's approximate centroid, in decimal degrees WGS 84.
pub struct RegionCentroid {
    /// Canonical lowercase region key.
    pub key: &'static str,
    /// Approximate centroid latitude (positive north).
    pub lat: f64,
    /// Approximate centroid longitude (positive east).
    pub lon: f64,
}

/// The eight coarse regions and their centroids, copied from
/// `infra/geo/regions.json`.
pub const REGIONS: &[RegionCentroid] = &[
    RegionCentroid {
        key: "na",
        lat: 39.8,
        lon: -98.6,
    },
    RegionCentroid {
        key: "latam",
        lat: -14.2,
        lon: -51.9,
    },
    RegionCentroid {
        key: "eu",
        lat: 50.1,
        lon: 14.4,
    },
    RegionCentroid {
        key: "mena",
        lat: 26.8,
        lon: 45.0,
    },
    RegionCentroid {
        key: "africa",
        lat: -8.8,
        lon: 25.0,
    },
    RegionCentroid {
        key: "sasia",
        lat: 20.6,
        lon: 78.9,
    },
    RegionCentroid {
        key: "apac",
        lat: 15.0,
        lon: 115.0,
    },
    RegionCentroid {
        key: "oceania",
        lat: -25.3,
        lon: 133.8,
    },
];

/// Free-form region strings already present in the signed relay registry,
/// mapped to their canonical region key. Copied from
/// `infra/geo/regions.json` `region_aliases`.
pub const REGION_ALIASES: &[(&str, &str)] = &[
    ("us-west", "na"),
    ("us-east", "na"),
    ("us-east-1", "na"),
    ("us-west-1", "na"),
    ("us-west-2", "na"),
    ("us-central", "na"),
    ("us-central-1", "na"),
    ("ca-central", "na"),
    ("ca-central-1", "na"),
    ("eu-central", "eu"),
    ("eu-central-1", "eu"),
    ("eu-west", "eu"),
    ("eu-west-1", "eu"),
    ("eu-west-2", "eu"),
    ("eu-north", "eu"),
    ("eu-north-1", "eu"),
    ("eu-south", "eu"),
    ("eu-south-1", "eu"),
    ("ap-southeast", "apac"),
    ("ap-southeast-2", "oceania"),
    ("ap-southeast-1", "apac"),
    ("ap-northeast", "apac"),
    ("ap-northeast-1", "apac"),
    ("ap-northeast-2", "apac"),
    ("ap-east", "apac"),
    ("ap-east-1", "apac"),
    ("ap-south", "sasia"),
    ("ap-south-1", "sasia"),
    ("ap-south-2", "sasia"),
    ("sa-east", "latam"),
    ("sa-east-1", "latam"),
    ("me-south", "mena"),
    ("me-south-1", "mena"),
    ("me-central", "mena"),
    ("me-central-1", "mena"),
    ("af-south", "africa"),
    ("af-south-1", "africa"),
];

/// ISO 3166-1 alpha-2 country code to canonical region key. Copied from
/// `infra/geo/regions.json` `countries`.
fn country_region(code: &str) -> Option<&'static str> {
    match code {
        "CA" | "US" => Some("na"),
        "AR" | "BO" | "BR" | "CL" | "CO" | "CR" | "CU" | "DO" | "EC" | "GT" | "HN" | "HT"
        | "JM" | "MX" | "NI" | "PA" | "PE" | "PR" | "PY" | "SV" | "TT" | "UY" | "VE" => {
            Some("latam")
        }
        "AD" | "AL" | "AT" | "BA" | "BE" | "BG" | "BY" | "CH" | "CY" | "CZ" | "DE" | "DK"
        | "EE" | "ES" | "FI" | "FR" | "GB" | "GR" | "HR" | "HU" | "IE" | "IS" | "IT" | "LT"
        | "LU" | "LV" | "MC" | "MD" | "ME" | "MK" | "MT" | "NL" | "NO" | "PL" | "PT" | "RO"
        | "RS" | "RU" | "SE" | "SI" | "SK" | "UA" => Some("eu"),
        "AE" | "BH" | "IL" | "IQ" | "IR" | "JO" | "KW" | "LB" | "OM" | "PS" | "QA" | "SA"
        | "SY" | "TR" => Some("mena"),
        "AO" | "BF" | "BW" | "CD" | "CI" | "CM" | "DZ" | "EG" | "ET" | "GH" | "GM" | "GN"
        | "KE" | "LY" | "MA" | "MG" | "ML" | "MU" | "MZ" | "NA" | "NE" | "NG" | "RW" | "SN"
        | "TN" | "TZ" | "UG" | "ZA" | "ZM" | "ZW" => Some("africa"),
        "AF" | "BD" | "BT" | "IN" | "LK" | "MV" | "NP" | "PK" => Some("sasia"),
        "BN" | "CN" | "HK" | "ID" | "JP" | "KH" | "KR" | "LA" | "MM" | "MN" | "MO" | "MY"
        | "PH" | "SG" | "TH" | "TW" | "VN" => Some("apac"),
        "AU" | "FJ" | "NZ" | "PF" | "PG" | "SB" | "WS" => Some("oceania"),
        _ => None,
    }
}

/// Resolve a free-form region string or ISO country code to a canonical region
/// key.
///
/// Accepts (case-insensitively) a canonical region key, a registry alias such
/// as `ap-southeast-2`, or an uppercase two-letter country code such as `AU`.
/// Returns `None` when nothing matches, so the caller keeps its previous
/// behaviour rather than inventing a region.
pub fn resolve_region(raw: &str) -> Option<&'static str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(region) = REGIONS.iter().find(|r| r.key == lower) {
        return Some(region.key);
    }
    if let Some((_, region)) = REGION_ALIASES.iter().find(|(alias, _)| *alias == lower) {
        return Some(region);
    }
    if trimmed.len() == 2 && trimmed.bytes().all(|b| b.is_ascii_alphabetic()) {
        return country_region(&trimmed.to_ascii_uppercase());
    }
    None
}

/// The centroid for a canonical region key or alias, if known.
pub fn centroid(region: &str) -> Option<(&'static str, f64, f64)> {
    let key = resolve_region(region)?;
    REGIONS
        .iter()
        .find(|r| r.key == key)
        .map(|r| (r.key, r.lat, r.lon))
}

/// Mean Earth radius in kilometres (IUGG).
const EARTH_RADIUS_KM: f64 = 6371.0088;
/// Round-trip microseconds per kilometre used by the coarse prior.
const US_PER_KM_ROUND_TRIP: f64 = 20.0;
/// Intra-region floor: two relays in the same region are not zero-latency.
const INTRA_REGION_FLOOR_US: u64 = 10_000;

/// Great-circle distance between two `(lat, lon)` points, in kilometres.
pub fn haversine_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = a;
    let (lat2, lon2) = b;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * h.sqrt().asin()
}

/// Coarse relay-to-destination round-trip prior in microseconds for a relay in
/// `relay_region` and a destination in `dest_region`.
///
/// `max(round(haversine_km * 20.0), 10_000)`, or `None` when either region
/// cannot be resolved. The floor makes co-located regions a real latency rather
/// than zero, so the prior never beats a genuine measurement on cost alone.
pub fn prior_dest_leg_us(relay_region: &str, dest_region: &str) -> Option<u64> {
    let relay = centroid(relay_region)?;
    let dest = centroid(dest_region)?;
    let km = haversine_km((relay.1, relay.2), (dest.1, dest.2));
    let us = (km * US_PER_KM_ROUND_TRIP).round() as u64;
    Some(us.max(INTRA_REGION_FLOOR_US))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_keys_and_aliases_resolve() {
        assert_eq!(resolve_region("oceania"), Some("oceania"));
        assert_eq!(resolve_region("OCEANIA"), Some("oceania"));
        assert_eq!(resolve_region("ap-southeast-2"), Some("oceania"));
        assert_eq!(resolve_region("ap-south"), Some("sasia"));
        assert_eq!(resolve_region("eu-south"), Some("eu"));
        assert_eq!(resolve_region("ap-northeast"), Some("apac"));
        assert_eq!(resolve_region("unknown"), None);
        assert_eq!(resolve_region(""), None);
    }

    #[test]
    fn country_codes_resolve_to_their_region() {
        assert_eq!(resolve_region("AU"), Some("oceania"));
        assert_eq!(resolve_region("in"), Some("sasia"));
        assert_eq!(resolve_region("US"), Some("na"));
        assert_eq!(resolve_region("FJ"), Some("oceania"));
        // A code with no mapping stays unknown rather than guessed.
        assert_eq!(resolve_region("AQ"), None);
    }

    #[test]
    fn same_region_hits_the_intra_region_floor() {
        assert_eq!(prior_dest_leg_us("oceania", "oceania"), Some(10_000));
        // An alias and its canonical key are the same region.
        assert_eq!(prior_dest_leg_us("ap-southeast-2", "OCEANIA"), Some(10_000));
    }

    #[test]
    fn cross_region_prior_grows_with_distance() {
        let same = prior_dest_leg_us("oceania", "oceania").unwrap();
        let nearby = prior_dest_leg_us("apac", "oceania").unwrap();
        let far = prior_dest_leg_us("na", "oceania").unwrap();
        assert!(same < nearby, "same-region floor must be the smallest");
        assert!(nearby < far, "farther regions must score worse");
        assert!(far > 100_000, "a trans-Pacific prior is a real cost");
    }

    #[test]
    fn unknown_region_yields_no_prior() {
        assert_eq!(prior_dest_leg_us("moon", "oceania"), None);
        assert_eq!(prior_dest_leg_us("oceania", "unknown"), None);
    }

    #[test]
    fn haversine_matches_known_distance() {
        // London to New York is roughly 5570 km.
        let km = haversine_km((51.5074, -0.1278), (40.7128, -74.0060));
        assert!((km - 5570.0).abs() < 50.0, "unexpected distance {km}");
    }
}
