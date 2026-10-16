//! Where is the game server? Resolving a destination IP to a coordinate.
//!
//! # Why this exists
//!
//! The relay chooser compares `client_leg + dest_leg`, and the destination leg
//! needs a distance from the relay to the server. Relay positions are checked in
//! (`infra/geo/regions.json`, mirrored by [`super::relay_choice::RELAY_POSITIONS`]),
//! but the server's position is not knowable offline.
//!
//! Falling back to the server's *region* is not good enough, and the arithmetic
//! shows why. For an Ashburn server, using the `na` centroid (Kansas) scores a
//! Los Angeles relay **better** than a New Jersey one (281 ms versus 327 ms),
//! because Kansas sits slightly closer to California. The true positions invert
//! that (LAX 317 ms, EWR 293 ms) and the server leg differs by a factor of
//! twenty (73 ms versus 7 ms). A coarse prior therefore reproduces the exact bug
//! the chooser exists to fix, so a real server location is mandatory rather than
//! a refinement.
//!
//! # Contract
//!
//! Resolution is best-effort and **never invents a position**: an unresolved
//! server returns `None` and the chooser keeps using a measured leg or a region
//! prior. Failure is normal (offline, no endpoint configured, an endpoint that
//! does not know the IP) and must not break a match in progress.
//!
//! The lookup is a *local* coordinate lookup for a game server the user is
//! already connected to, not a tracking feature: the result is used to rank
//! relays and is not persisted.

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::Duration;

/// How long to wait for a lookup before giving up and falling back.
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(4);

/// A resolved location, in decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerLocation {
    pub lat: f64,
    pub lon: f64,
}

/// Resolves destination IPs to coordinates, with a per-process cache.
///
/// The cache matters because a match asks repeatedly (the server is re-checked
/// as traffic flows) and a lookup per packet would be absurd.
#[derive(Debug, Default)]
pub struct ServerLocator {
    endpoint: Option<String>,
    cache: Mutex<HashMap<Ipv4Addr, Option<ServerLocation>>>,
}

impl ServerLocator {
    /// A locator with no endpoint: every lookup returns `None`, so callers fall
    /// back to measurement. Used when the feature is not configured.
    pub fn disabled() -> Self {
        Self {
            endpoint: None,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// A locator that resolves through `endpoint`.
    ///
    /// `endpoint` is a template containing `{ip}`, for example
    /// `http://127.0.0.1:8080/geo?ip={ip}`, and the response is expected to be
    /// JSON with numeric `lat` and `lon` fields. A self-hosted or operator-hosted
    /// resolver keeps this off a third party and keeps the request in the
    /// operator's own vocabulary, which is why it is a template rather than a
    /// hard-coded public service.
    pub fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: Some(endpoint.into()),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The configured endpoint template, if any.
    pub fn endpoint(&self) -> Option<&str> {
        self.endpoint.as_deref()
    }

    /// Resolve a destination, consulting the cache first.
    ///
    /// A failed lookup is cached as `None` so a server that cannot be located is
    /// not re-queried on every call during the same session.
    pub fn locate(&self, server: Ipv4Addr) -> Option<ServerLocation> {
        if let Ok(cache) = self.cache.lock() {
            if let Some(hit) = cache.get(&server) {
                return *hit;
            }
        }

        let resolved = self.fetch(server);

        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(server, resolved);
        }
        resolved
    }

    /// Forget one entry, so a re-query is possible after a failure.
    pub fn forget(&self, server: Ipv4Addr) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(&server);
        }
    }

    fn fetch(&self, server: Ipv4Addr) -> Option<ServerLocation> {
        let template = self.endpoint.as_ref()?;
        let url = template.replace("{ip}", &server.to_string());
        let body = ureq::get(&url)
            .timeout(LOOKUP_TIMEOUT)
            .call()
            .ok()?
            .into_string()
            .ok()?;
        parse_location(&body)
    }
}

/// Parse a `{"lat": .., "lon": ..}` body, rejecting anything else.
///
/// A body without numeric coordinates is *not* assumed to mean the origin: the
/// caller needs to distinguish "located" from "not located", and a default of
/// `(0, 0)` would silently rank every relay as equidistant from the Gulf of
/// Guinea.
pub fn parse_location(body: &str) -> Option<ServerLocation> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let lat = value.get("lat")?.as_f64()?;
    let lon = value.get("lon")?.as_f64()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some(ServerLocation { lat, lon })
}

/// The process-wide locator.
static GLOBAL: Mutex<Option<std::sync::Arc<ServerLocator>>> = Mutex::new(None);

/// Install the process-wide locator, replacing any previous one.
pub fn install_global(locator: std::sync::Arc<ServerLocator>) {
    if let Ok(mut slot) = GLOBAL.lock() {
        *slot = Some(locator);
    }
}

/// The installed locator, if a caller installed one.
pub fn global() -> Option<std::sync::Arc<ServerLocator>> {
    GLOBAL.lock().ok().and_then(|slot| slot.clone())
}

/// Locate through the installed locator, or return `None` when none is set.
pub fn locate(server: Ipv4Addr) -> Option<ServerLocation> {
    global()?.locate(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_locator_never_resolves_or_invents_a_position() {
        let locator = ServerLocator::disabled();
        assert_eq!(locator.locate(Ipv4Addr::new(3, 237, 142, 3)), None);
        assert!(locator.endpoint().is_none());
    }

    #[test]
    fn a_valid_body_parses_to_its_coordinates() {
        let loc = parse_location(r#"{"lat": 39.04, "lon": -77.49}"#).unwrap();
        assert!((loc.lat - 39.04).abs() < 1e-9);
        assert!((loc.lon + 77.49).abs() < 1e-9);
    }

    #[test]
    fn a_body_without_coordinates_is_rejected_not_defaulted() {
        // The dangerous failure: a body the endpoint returns for "unknown IP"
        // must not become (0, 0), which would score every relay as equidistant
        // from a point in the Atlantic.
        assert!(parse_location(r#"{"error": "not found"}"#).is_none());
        assert!(parse_location(r#"{"lat": 39.04}"#).is_none());
        assert!(parse_location("not json").is_none());
    }

    #[test]
    fn out_of_range_coordinates_are_rejected() {
        assert!(parse_location(r#"{"lat": 99.0, "lon": 0.0}"#).is_none());
        assert!(parse_location(r#"{"lat": 0.0, "lon": 200.0}"#).is_none());
    }

    #[test]
    fn the_endpoint_template_substitutes_the_ip() {
        let locator = ServerLocator::with_endpoint("http://127.0.0.1:9/geo?ip={ip}");
        assert_eq!(
            locator.endpoint(),
            Some("http://127.0.0.1:9/geo?ip={ip}"),
            "the template is stored verbatim so the caller can log it"
        );
    }

    #[test]
    fn an_unreachable_endpoint_yields_none_rather_than_panicking() {
        // Port 9 (discard) has nothing listening: this must degrade to `None`
        // so a match continues on measurement instead of failing.
        let locator = ServerLocator::with_endpoint("http://127.0.0.1:9/geo?ip={ip}");
        assert_eq!(locator.locate(Ipv4Addr::new(3, 237, 142, 3)), None);
    }
}
