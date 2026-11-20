//! The hosting-network classifier the client's server detector consults.
//!
//! Server detection runs inside `lightspeed-client` (the WinDivert data plane),
//! which has no copy of this repository's shipped location indexes. Those live
//! here, in the GUI. So rather than duplicating R1 data into the client, the app
//! installs one function pointer at startup
//! ([`lightspeed_client::interceptor::install_server_provider_lookup`]) and the
//! detector calls it when it needs to know whether a candidate game server is on
//! the *same hosting network* as the server it already pinned.
//!
//! Why that matters: the 2026-11-20 field mis-lock re-locked a live session from
//! a G-Core match server onto an unrelated AWS endpoint. Even when process
//! attribution is unavailable (or the flow really is the game process's own
//! side-channel), "this endpoint is on a different hosting network than the
//! server the match was on" is a reason to keep the lock instead of moving it.
//!
//! Classes are opaque to the client: it only ever compares them for equality.
//! Three namespaces are encoded in one `u64`, disjoint from each other:
//! cloud providers (hashed into the top two bits), operator geofeed networks
//! (hashed into the next namespace), and hosting ASNs (their real number, far
//! below the tags). A country-level answer is not a hosting network and yields
//! `None` (no opinion).

use std::net::Ipv4Addr;

use crate::server_place::Place;

/// Marks a hashed cloud-provider class; real ASNs are far below this bit.
const CLOUD_CLASS_TAG: u64 = 1 << 63;
/// Marks a hashed operator-geofeed network class.
const GEOFED_CLASS_TAG: u64 = 1 << 62;
/// Bits a hashed name may occupy: everything below the two tag namespaces, so
/// the tags themselves say unambiguously which namespace a class is in.
const HASH_MASK: u64 = GEOFED_CLASS_TAG - 1;

/// The hosting-network class of `ip`, or `None` when the shipped indexes cannot
/// name one.
///
/// Two addresses on the same cloud provider, the same operator geofeed network
/// or the same hosting ASN return the same value; anything else returns a
/// different value or `None`. This is the whole contract the detector relies on.
pub fn provider_class(ip: Ipv4Addr) -> Option<u64> {
    class_of(crate::server_place::place(ip))
}

/// The class of one place, split out so the namespace rules are testable without
/// depending on any particular row of the shipped indexes.
fn class_of(place: Place) -> Option<u64> {
    match place {
        Place::Cloud { provider, .. } => Some(CLOUD_CLASS_TAG | name_hash(provider)),
        Place::Geofeed { provider, .. } => Some(GEOFED_CLASS_TAG | name_hash(provider)),
        Place::Hosting { asn, .. } => Some(u64::from(asn)),
        // A country estimate is not a hosting network, and no answer is not an
        // answer: the detector gets no opinion and keeps its pid-based rules.
        Place::Country { .. } | Place::Unknown => None,
    }
}

/// FNV-1a over the network name, kept inside [`HASH_MASK`]: stable across runs
/// and platforms, no dependency, and only ever compared with values produced by
/// the same build.
fn name_hash(text: &str) -> u64 {
    fnv1a64(text) & HASH_MASK
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn addr(s: &str) -> Ipv4Addr {
        Ipv4Addr::from_str(s).unwrap()
    }

    fn cloud(provider: &'static str) -> Place {
        Place::Cloud {
            provider,
            region: "test-region",
            at: None,
        }
    }

    fn hosting(asn: u32, name: &'static str) -> Place {
        Place::Hosting { asn, name }
    }

    fn geofeed(provider: &'static str) -> Place {
        Place::Geofeed {
            provider,
            country: "US",
            region: None,
            city: None,
            source: "https://example.invalid/geofeed.csv",
            as_of: "2026-11-20",
        }
    }

    #[test]
    fn a_hosting_network_is_its_own_asn_class() {
        assert_eq!(class_of(hosting(199_524, "G-Core Labs")), Some(199_524));
        // An ASN class never carries either of the hashed namespaces' tags.
        let class = class_of(hosting(199_524, "G-Core Labs")).unwrap();
        assert_eq!(class & CLOUD_CLASS_TAG, 0);
        assert_eq!(class & GEOFED_CLASS_TAG, 0);
    }

    #[test]
    fn cloud_providers_are_distinct_classes_out_of_the_asn_range() {
        let aws = class_of(cloud("aws")).expect("a cloud provider is a class");
        let gcp = class_of(cloud("gcp")).expect("a cloud provider is a class");
        assert_ne!(aws, gcp, "different clouds are different networks");
        assert_eq!(aws & CLOUD_CLASS_TAG, CLOUD_CLASS_TAG);
        assert_eq!(aws & GEOFED_CLASS_TAG, 0);
        assert_ne!(
            aws,
            class_of(hosting(199_524, "G-Core Labs")).unwrap(),
            "a cloud class is not an ASN"
        );
    }

    #[test]
    fn operator_geofeed_networks_are_their_own_namespace() {
        let gcore = class_of(geofeed("G-Core")).expect("a geofeed operator is a class");
        assert_eq!(gcore, class_of(geofeed("G-Core")).unwrap());
        assert_ne!(gcore, class_of(geofeed("Other Host")).unwrap());
        assert_eq!(gcore & GEOFED_CLASS_TAG, GEOFED_CLASS_TAG);
        assert_eq!(gcore & CLOUD_CLASS_TAG, 0);
        assert_ne!(
            gcore,
            class_of(cloud("aws")).unwrap(),
            "the operator's geofeed is not a cloud provider"
        );
    }

    #[test]
    fn a_country_estimate_or_unknown_has_no_hosting_class() {
        assert_eq!(
            class_of(Place::Country {
                code: "US",
                at: (0.0, 0.0),
            }),
            None
        );
        assert_eq!(class_of(Place::Unknown), None);
    }

    #[test]
    fn the_same_provider_name_hashes_to_the_same_class() {
        assert_eq!(fnv1a64("aws"), fnv1a64("aws"));
        assert_ne!(fnv1a64("aws"), fnv1a64("gcp"));
    }

    #[test]
    fn a_private_address_has_no_hosting_network() {
        assert_eq!(provider_class(addr("192.168.1.1")), None);
        assert_eq!(provider_class(addr("127.0.0.1")), None);
    }

    #[test]
    fn a_real_server_answer_is_stable_and_network_bounded() {
        // Relationship, not a hard-coded class: whatever index answers, the same
        // address must stay in the same class and a server on a different index
        // must not collide with it. The pinned server's network is what the
        // detector compares, so stability is the contract that matters.
        let gcore = provider_class(addr("178.107.14.201"));
        assert!(gcore.is_some(), "the live G-Core server must have a class");
        assert_eq!(gcore, provider_class(addr("178.107.14.201")));
        let aws = provider_class(addr("3.237.221.191"));
        assert!(aws.is_some(), "the AWS range must have a class");
        assert_ne!(
            gcore, aws,
            "the pinned G-Core server and an AWS endpoint are different networks"
        );
    }
}
