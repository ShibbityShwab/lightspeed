//! # Proxy-side IP-to-country geolocation
//!
//! Maps a peer IPv4 address to its ISO 3166-1 alpha-2 country code for the
//! proxy-observed session telemetry. Only the two-letter code leaves this
//! module: no raw IP is ever stored, logged, or exported.
//!
//! Databases are read fully into memory (rather than memory-mapped) so an
//! operator can replace the `.mmdb` file on disk while the proxy is running
//! without an mmap pointing at the unlinked inode.

use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;
use std::sync::Arc;

use maxminddb::{path, Reader};
use tracing::warn;

/// Which side of a relayed session a country lookup was performed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeoSide {
    /// The client side of the tunnel.
    Src,
    /// The game-server side of the tunnel.
    Dst,
}

/// Resolves an IPv4 address to its two-letter country code.
///
/// Implementations must be cheap enough to call synchronously on the session
/// creation path and safe to share across relay tasks.
pub trait GeoResolver: Send + Sync {
    /// Return the normalized uppercase country code, or `None` when the
    /// address cannot be resolved.
    fn country(&self, ip: Ipv4Addr) -> Option<String>;
}

impl<F> GeoResolver for F
where
    F: Fn(Ipv4Addr) -> Option<String> + Send + Sync,
{
    fn country(&self, ip: Ipv4Addr) -> Option<String> {
        self(ip)
    }
}

/// Resolve the region of a destination the client registered.
///
/// The MMDB only knows countries, so the destination region is the normalized
/// two-letter ISO 3166-1 code. The client coarsens it to its own region
/// vocabulary, so the relay never carries a region catalog. `None` means the
/// address could not be placed and the ack simply omits the region.
pub fn destination_region(resolver: &dyn GeoResolver, ip: Ipv4Addr) -> Option<String> {
    resolver.country(ip)
}

/// Normalize a raw country string into exactly two uppercase ASCII letters.
///
/// Anything that is not exactly two ASCII letters returns `None`, so a
/// malformed database entry can never inject an arbitrary or high-cardinality
/// label value.
fn normalize_country_code(raw: &str) -> Option<String> {
    let upper = raw.trim().to_ascii_uppercase();
    if upper.len() == 2 && upper.bytes().all(|b| b.is_ascii_uppercase()) {
        Some(upper)
    } else {
        None
    }
}

/// [`GeoResolver`] backed by a MaxMind DB read fully into memory.
struct MmdbResolver {
    reader: Reader<Vec<u8>>,
}

impl GeoResolver for MmdbResolver {
    fn country(&self, ip: Ipv4Addr) -> Option<String> {
        let iso: String = self
            .reader
            .lookup(IpAddr::V4(ip))
            .ok()?
            .decode_path(&path!["country", "iso_code"])
            .ok()??;
        normalize_country_code(&iso)
    }
}

/// Load an MMDB from `path` for proxy-side session geo aggregation.
///
/// The file is read into memory so it can be replaced on disk at runtime
/// without an mmap retaining the old inode. Returns `None` (after exactly one
/// warning) when the file is missing, unreadable, or not a valid MaxMind DB,
/// so a bad database degrades to "no geo" instead of failing startup.
pub fn load_resolver(path: &Path) -> Option<Arc<dyn GeoResolver>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            warn!(path = %path.display(), %error, "GeoIP database unreadable; geo aggregation disabled");
            return None;
        }
    };
    match Reader::from_source(bytes) {
        Ok(reader) => Some(Arc::new(MmdbResolver { reader })),
        Err(error) => {
            warn!(path = %path.display(), %error, "GeoIP database invalid; geo aggregation disabled");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    fn fake_resolver_resolves_known_and_none() {
        let resolver: Arc<dyn GeoResolver> = Arc::new(|ip: Ipv4Addr| {
            if ip == Ipv4Addr::new(1, 1, 1, 1) {
                Some("AU".to_string())
            } else {
                None
            }
        });
        assert_eq!(
            resolver.country(Ipv4Addr::new(1, 1, 1, 1)),
            Some("AU".to_string())
        );
        assert_eq!(resolver.country(Ipv4Addr::new(8, 8, 8, 8)), None);
    }

    #[test]
    fn destination_region_forwards_the_resolved_country() {
        let resolver: Arc<dyn GeoResolver> =
            Arc::new(|ip: Ipv4Addr| (ip == Ipv4Addr::new(8, 8, 8, 8)).then(|| "AU".to_string()));
        assert_eq!(
            destination_region(resolver.as_ref(), Ipv4Addr::new(8, 8, 8, 8)),
            Some("AU".to_string())
        );
        assert_eq!(
            destination_region(resolver.as_ref(), Ipv4Addr::new(9, 9, 9, 9)),
            None
        );
    }

    #[test]
    fn missing_mmdb_returns_none() {
        let path = Path::new("/nonexistent/lightspeed/does-not-exist.mmdb");
        assert!(
            load_resolver(path).is_none(),
            "a missing MMDB must disable geo rather than panic"
        );
    }

    #[test]
    fn corrupt_mmdb_returns_none() {
        let dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = dir.join(format!(
            "lightspeed-geo-corrupt-{}-{nanos}.mmdb",
            std::process::id()
        ));
        std::fs::write(&path, b"this is definitely not a maxmind database").unwrap();
        assert!(
            load_resolver(&path).is_none(),
            "a corrupt MMDB must disable geo rather than panic"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn country_code_normalization() {
        assert_eq!(normalize_country_code("us"), Some("US".to_string()));
        assert_eq!(normalize_country_code(" us "), Some("US".to_string()));
        assert_eq!(normalize_country_code("DE"), Some("DE".to_string()));
        // Anything that is not exactly two ASCII letters collapses to None.
        assert_eq!(normalize_country_code("usa"), None);
        assert_eq!(normalize_country_code("u"), None);
        assert_eq!(normalize_country_code("U1"), None);
        assert_eq!(normalize_country_code(""), None);
        assert_eq!(normalize_country_code("??"), None);
    }
}
