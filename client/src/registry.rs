//! # Community Proxy Registry
//!
//! Signed node list + revocation list for the community relay network.
//!
//! The registry is a small JSON document listing available relay nodes and
//! revoked node identities, signed with the operator's Ed25519 key. The client
//! fetches it over HTTPS, verifies the signature against the operator's
//! embedded public key, and only trusts nodes that verify and are not revoked.
//!
//! ## Trust model
//! The signature covers the exact raw bytes of the registry JSON string, so no
//! canonicalization is needed: the signer and verifier see identical bytes.
//! Nodes are identified by their own Ed25519 public key (generated on first
//! boot - see `infra/scripts/provision-oci.sh`).

use serde::{Deserialize, Serialize};

/// Default community registry URL used for zero-config proxy discovery, when
/// the user supplies neither `--registry` nor a `registry.url` in their config.
pub const DEFAULT_REGISTRY_URL: &str = "https://shibbityshwab.github.io/lightspeed/registry.json";

/// Operator Ed25519 public key (base64) used to verify the default registry.
pub const DEFAULT_OPERATOR_PUBKEY_B64: &str = "f5l9G2l4a3OrOX3NnWdMrNHQFbjOt27jN+st1z/J388=";

/// A relay node listed in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryNode {
    pub node_id: String,
    pub region: String,
    /// Data-plane address ("ip:port") the client tunnels game traffic to.
    pub data_addr: String,
    pub health_url: String,
    /// Base64-encoded Ed25519 public key - the node's identity.
    pub pubkey: String,
    /// Hex SHA-256 fingerprint of the node's QUIC certificate, used to pin the
    /// control plane against the registry (empty = fall back to TOFU).
    #[serde(default)]
    pub cert_fingerprint: String,
    #[serde(default)]
    pub note: String,
}

/// The registry payload - the exact bytes the signature covers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registry {
    pub schema_version: u32,
    /// Unix timestamp (seconds) when this registry was published.
    pub published_at: u64,
    pub nodes: Vec<RegistryNode>,
    /// Base64-encoded Ed25519 public keys of revoked nodes.
    #[serde(default)]
    pub revoked: Vec<String>,
}

/// A signed registry: the raw registry JSON plus its Ed25519 signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignedRegistry {
    /// The exact registry JSON string that `signature` covers.
    pub registry: String,
    /// Base64-encoded Ed25519 signature over `registry.as_bytes()`.
    pub signature: String,
}

/// A relay discovered from the signed registry: its node id and the
/// data-plane address ("ip:port") game traffic is tunneled through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayInfo {
    /// Registry node id (e.g. `relay-lax-1`).
    pub node_id: String,
    /// Data-plane address as `"ip:port"`.
    pub addr: String,
    /// Registry region label (e.g. `ap-southeast-2`), kept so the selector can
    /// place the relay before it has any measured evidence.
    pub region: String,
}

/// Verify a signed registry against an operator Ed25519 public key (base64)
/// and return the parsed, trusted registry. Fails on a bad signature, a bad
/// key, or malformed JSON.
pub fn verify_registry(
    signed: &SignedRegistry,
    operator_public_key_b64: &str,
) -> anyhow::Result<Registry> {
    use base64::Engine as _;
    use ring::signature::{UnparsedPublicKey, ED25519};

    let b64 = base64::engine::general_purpose::STANDARD;
    let pubkey = b64
        .decode(operator_public_key_b64)
        .map_err(|e| anyhow::anyhow!("invalid operator public key: {e}"))?;
    let signature = b64
        .decode(&signed.signature)
        .map_err(|e| anyhow::anyhow!("invalid signature encoding: {e}"))?;

    let public_key = UnparsedPublicKey::new(&ED25519, &pubkey);
    public_key
        .verify(signed.registry.as_bytes(), &signature)
        .map_err(|_| anyhow::anyhow!("registry signature verification failed"))?;

    let registry: Registry = serde_json::from_str(&signed.registry)
        .map_err(|e| anyhow::anyhow!("malformed registry payload: {e}"))?;
    Ok(registry)
}

/// Sign a registry with an operator Ed25519 key pair, producing a
/// `SignedRegistry`. Used by operator tooling (and tests) - the client only
/// ever verifies.
pub fn sign_registry(
    registry: &Registry,
    key_pair: &ring::signature::Ed25519KeyPair,
) -> SignedRegistry {
    use base64::Engine as _;

    let json = serde_json::to_string(registry).expect("registry serializes");
    let sig = key_pair.sign(json.as_bytes());
    SignedRegistry {
        registry: json,
        signature: base64::engine::general_purpose::STANDARD.encode(sig.as_ref()),
    }
}

/// Whether a node's public key appears in the registry's revocation list.
pub fn is_revoked(registry: &Registry, node: &RegistryNode) -> bool {
    registry.revoked.iter().any(|r| r == &node.pubkey)
}

/// Data-plane addresses ("ip:port") of the non-revoked nodes, for feeding into
/// the proxy probe/selection pipeline.
pub fn available_data_addrs(registry: &Registry) -> Vec<&str> {
    registry
        .nodes
        .iter()
        .filter(|n| !is_revoked(registry, n))
        .map(|n| n.data_addr.as_str())
        .collect()
}

/// Fetch a registry, verify it, pre-pin the discovered relays' certificate
/// fingerprints, and return the non-revoked nodes as `(node_id, data_addr)`
/// pairs for feeding into the proxy probe/selection pipeline.
pub fn discover_nodes(
    url: &str,
    operator_public_key_b64: &str,
    control_port: u16,
) -> anyhow::Result<Vec<(String, String)>> {
    validate_registry_url(url)?;
    discover_nodes_inner(url, operator_public_key_b64, control_port)
}

/// Fetch + verify a registry and return its `(node_id, data_addr)` pairs,
/// without the SSRF URL guard (used by tests that serve a local registry over
/// plain HTTP).
fn discover_nodes_inner(
    url: &str,
    operator_public_key_b64: &str,
    control_port: u16,
) -> anyhow::Result<Vec<(String, String)>> {
    let registry = fetch_registry_inner(url, operator_public_key_b64)?;
    pre_pin_nodes(&registry, control_port);
    Ok(registry
        .nodes
        .iter()
        .filter(|n| !is_revoked(&registry, n))
        .map(|n| (n.node_id.clone(), n.data_addr.clone()))
        .collect())
}

/// Fetch a registry, verify it, pre-pin the discovered relays' certificate
/// fingerprints, and return the non-revoked nodes' data-plane addresses as
/// owned "ip:port" strings.
pub fn discover_data_addrs(
    url: &str,
    operator_public_key_b64: &str,
    control_port: u16,
) -> anyhow::Result<Vec<String>> {
    Ok(discover_nodes(url, operator_public_key_b64, control_port)?
        .into_iter()
        .map(|(_, data_addr)| data_addr)
        .collect())
}

/// Discover community relays from a signed registry and return each
/// non-revoked node's id and data-plane address ("ip:port").
///
/// This is the small public discovery API for embedders (e.g. the GUI). It
/// applies the same SSRF URL guard, signature verification, rollback check,
/// and certificate pre-pinning as [`discover_nodes`].
pub fn discover_relays(
    url: &str,
    operator_public_key_b64: &str,
    control_port: u16,
) -> anyhow::Result<Vec<RelayInfo>> {
    validate_registry_url(url)?;
    discover_relays_inner(url, operator_public_key_b64, control_port)
}

/// Unguarded variant of [`discover_relays`] used by tests that serve a local
/// registry over plain HTTP.
fn discover_relays_inner(
    url: &str,
    operator_public_key_b64: &str,
    control_port: u16,
) -> anyhow::Result<Vec<RelayInfo>> {
    let registry = fetch_registry_inner(url, operator_public_key_b64)?;
    pre_pin_nodes(&registry, control_port);
    Ok(registry
        .nodes
        .iter()
        .filter(|n| !is_revoked(&registry, n))
        .map(|n| RelayInfo {
            node_id: n.node_id.clone(),
            addr: n.data_addr.clone(),
            region: n.region.clone(),
        })
        .collect())
}

/// Pre-pin the control-plane certificate fingerprint for each non-revoked
/// registry node, so the client verifies the relay's cert against the
/// registry's published value instead of trust-on-first-use. Returns the
/// number of nodes pinned.
pub fn pre_pin_nodes(registry: &Registry, control_port: u16) -> usize {
    let mut pinned = 0;
    for node in &registry.nodes {
        if is_revoked(registry, node) || node.cert_fingerprint.is_empty() {
            continue;
        }
        if let Some((ip, _)) = node.data_addr.rsplit_once(':') {
            if let Ok(ip) = ip.parse::<std::net::Ipv4Addr>() {
                let control_addr =
                    std::net::SocketAddr::V4(std::net::SocketAddrV4::new(ip, control_port));
                crate::quic::fingerprint::pre_pin(control_addr, &node.cert_fingerprint);
                pinned += 1;
            }
        }
    }
    pinned
}

/// Fetch a signed registry over HTTPS and verify it against the operator key.
/// Returns the parsed, trusted registry (revoked nodes are NOT filtered out -
/// callers filter via `is_revoked`).
pub fn fetch_registry(url: &str, operator_public_key_b64: &str) -> anyhow::Result<Registry> {
    validate_registry_url(url)?;
    fetch_registry_inner(url, operator_public_key_b64)
}

/// Reject non-https and private/internal registry URLs (SSRF guard).
fn validate_registry_url(url: &str) -> anyhow::Result<()> {
    if !url.starts_with("https://") {
        return Err(anyhow::anyhow!("registry URL must use https"));
    }
    if let Some(host) = url_host(url) {
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if is_private_or_unsafe(ip) {
                return Err(anyhow::anyhow!(
                    "registry URL host is a private/internal address"
                ));
            }
        }
    }
    Ok(())
}

/// Fetch + verify a registry, without the SSRF URL guard (used by tests that
/// serve a local registry over plain HTTP).
fn fetch_registry_inner(url: &str, operator_public_key_b64: &str) -> anyhow::Result<Registry> {
    let body = ureq::get(url)
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| anyhow::anyhow!("registry fetch failed: {e}"))?
        .into_string()
        .map_err(|e| anyhow::anyhow!("registry body read failed: {e}"))?;
    let signed: SignedRegistry = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("malformed signed registry: {e}"))?;
    let registry = verify_registry(&signed, operator_public_key_b64)?;
    enforce_freshness(&registry)?;
    Ok(registry)
}

/// Extract the host portion of an http(s) URL (no scheme, port, path, or
/// query). Handles bracketed IPv6 literals. Used to reject literal
/// private/internal IP hosts as an SSRF guard.
fn url_host(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed.split(']').next();
    }
    authority
        .rsplit_once(':')
        .map(|(host, _)| host)
        .or(Some(authority))
}

/// Reject loopback, private, link-local, multicast, and unspecified addresses
/// (the SSRF-reachable ranges) so a hostile registry URL can't target internal
/// hosts.
fn is_private_or_unsafe(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Reject a registry whose `published_at` is older than the most recently
/// accepted one, then persist the new timestamp. This stops an attacker from
/// replaying an old (still validly signed) registry to resurrect a revoked
/// node.
fn enforce_freshness(registry: &Registry) -> anyhow::Result<()> {
    let path = registry_state_path();
    if let Ok(prev) = std::fs::read_to_string(&path) {
        if let Ok(prev_ts) = prev.trim().parse::<u64>() {
            if registry.published_at < prev_ts {
                return Err(anyhow::anyhow!(
                    "registry rollback detected: published_at {} is older than last-seen {}",
                    registry.published_at,
                    prev_ts
                ));
            }
        }
    }
    let _ = std::fs::write(&path, registry.published_at.to_string());
    Ok(())
}

fn registry_state_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("LIGHTSPEED_REGISTRY_STATE") {
        return std::path::PathBuf::from(path);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return std::path::PathBuf::from(home).join(".lightspeed-registry-state");
    }
    std::path::PathBuf::from(".lightspeed-registry-state")
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use ring::rand::SystemRandom;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn new_key_pair() -> Ed25519KeyPair {
        let rng = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        Ed25519KeyPair::from_pkcs8_maybe_unchecked(pkcs8.as_ref()).unwrap()
    }

    fn sample_registry() -> Registry {
        Registry {
            schema_version: 1,
            published_at: 1_700_000_000,
            nodes: vec![RegistryNode {
                node_id: "proxy-ewr".into(),
                region: "us-east-1".into(),
                data_addr: "1.2.3.4:4434".into(),
                health_url: "http://1.2.3.4:8080/health".into(),
                pubkey: "node-pubkey".into(),
                cert_fingerprint: String::new(),
                note: String::new(),
            }],
            revoked: vec![],
        }
    }

    #[test]
    fn test_verify_accepts_valid_registry() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();

        let signed = sign_registry(&registry, &key_pair);
        let verified = verify_registry(&signed, &pubkey_b64).unwrap();
        assert_eq!(verified, registry);
    }

    #[test]
    fn test_verify_rejects_tampered_payload() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();

        let mut signed = sign_registry(&registry, &key_pair);
        signed.registry = signed.registry.replace("\"nodes\":[", "\"nodes\":[{");

        assert!(verify_registry(&signed, &pubkey_b64).is_err());
    }

    #[test]
    fn test_verify_rejects_wrong_key() {
        let key_pair = new_key_pair();
        let other = new_key_pair();
        let other_pubkey_b64 = b64(other.public_key().as_ref());
        let registry = sample_registry();

        let signed = sign_registry(&registry, &key_pair);
        assert!(verify_registry(&signed, &other_pubkey_b64).is_err());
    }

    #[test]
    fn test_verify_rejects_malformed_json() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let payload = "{ not valid json".to_string();
        let signed = SignedRegistry {
            registry: payload.clone(),
            signature: b64(key_pair.sign(payload.as_bytes()).as_ref()),
        };

        assert!(verify_registry(&signed, &pubkey_b64).is_err());
    }

    #[test]
    fn test_revocation_list() {
        let registry = Registry {
            schema_version: 1,
            published_at: 1,
            nodes: vec![],
            revoked: vec!["revoked-key".into()],
        };
        let node = RegistryNode {
            node_id: "x".into(),
            region: "r".into(),
            data_addr: "1.1.1.1:4434".into(),
            health_url: "h".into(),
            pubkey: "revoked-key".into(),
            cert_fingerprint: String::new(),
            note: String::new(),
        };
        assert!(is_revoked(&registry, &node));
    }

    #[test]
    fn test_available_data_addrs_filters_revoked() {
        let registry = Registry {
            schema_version: 1,
            published_at: 1,
            nodes: vec![
                RegistryNode {
                    node_id: "good".into(),
                    region: "us-east-1".into(),
                    data_addr: "1.2.3.4:4434".into(),
                    health_url: "http://1.2.3.4:8080/health".into(),
                    pubkey: "good-key".into(),
                    cert_fingerprint: String::new(),
                    note: String::new(),
                },
                RegistryNode {
                    node_id: "bad".into(),
                    region: "us-east-1".into(),
                    data_addr: "5.6.7.8:4434".into(),
                    health_url: "http://5.6.7.8:8080/health".into(),
                    pubkey: "revoked-key".into(),
                    cert_fingerprint: String::new(),
                    note: String::new(),
                },
            ],
            revoked: vec!["revoked-key".into()],
        };
        assert_eq!(available_data_addrs(&registry), vec!["1.2.3.4:4434"]);
    }

    #[test]
    fn test_fetch_and_verify() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();
        let signed = sign_registry(&registry, &key_pair);

        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        let fetched = fetch_registry_inner(&url, &pubkey_b64).unwrap();
        assert_eq!(fetched, registry);
        handle.join().unwrap();
    }

    #[test]
    fn test_fetch_rejects_wrong_key() {
        let key_pair = new_key_pair();
        let other = new_key_pair();
        let other_pubkey_b64 = b64(other.public_key().as_ref());
        let signed = sign_registry(&sample_registry(), &key_pair);

        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        assert!(fetch_registry_inner(&url, &other_pubkey_b64).is_err());
        handle.join().unwrap();
    }

    #[test]
    fn test_validate_registry_url_rejects_unsafe_targets() {
        assert!(validate_registry_url("http://example.com/nodes").is_err());
        assert!(validate_registry_url("https://127.0.0.1/nodes").is_err());
        assert!(validate_registry_url("https://10.0.0.8/nodes").is_err());
        assert!(validate_registry_url("https://192.168.1.1/nodes").is_err());
        assert!(validate_registry_url("https://[::1]/nodes").is_err());
        assert!(validate_registry_url("https://example.com/nodes").is_ok());
    }

    #[test]
    fn test_discover_data_addrs_returns_expected_addr() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();
        let expected_addr = registry.nodes[0].data_addr.clone();
        let signed = sign_registry(&registry, &key_pair);

        // discover_data_addrs() applies the SSRF URL guard (rejects plain-HTTP
        // localhost), so exercise the same pipeline the zero-config fallback
        // runs: fetch -> verify -> available_data_addrs.
        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        let fetched = fetch_registry_inner(&url, &pubkey_b64).unwrap();
        let addrs: Vec<String> = available_data_addrs(&fetched)
            .into_iter()
            .map(str::to_string)
            .collect();
        handle.join().unwrap();

        assert_eq!(addrs, vec![expected_addr]);
    }

    #[test]
    fn test_discover_data_addrs_rejects_tampered_signature() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let mut signed = sign_registry(&sample_registry(), &key_pair);
        signed.registry = signed.registry.replace("1.2.3.4:4434", "9.9.9.9:4434");

        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        assert!(fetch_registry_inner(&url, &pubkey_b64).is_err());
        handle.join().unwrap();
    }

    #[test]
    fn test_discover_nodes_returns_expected_pairs() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();
        let expected = vec![(
            registry.nodes[0].node_id.clone(),
            registry.nodes[0].data_addr.clone(),
        )];
        let signed = sign_registry(&registry, &key_pair);

        // discover_nodes() applies the SSRF URL guard (rejects plain-HTTP
        // localhost), so exercise its unguarded inner pipeline, mirroring
        // test_discover_data_addrs_returns_expected_addr.
        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        let nodes = discover_nodes_inner(&url, &pubkey_b64, 4433).unwrap();
        handle.join().unwrap();

        assert_eq!(nodes, expected);
    }

    #[test]
    fn test_discover_relays_returns_expected_relays() {
        let key_pair = new_key_pair();
        let pubkey_b64 = b64(key_pair.public_key().as_ref());
        let registry = sample_registry();
        let expected = vec![RelayInfo {
            node_id: registry.nodes[0].node_id.clone(),
            addr: registry.nodes[0].data_addr.clone(),
            region: registry.nodes[0].region.clone(),
        }];
        let signed = sign_registry(&registry, &key_pair);

        let (url, handle) = serve_once(&serde_json::to_string(&signed).unwrap());
        let relays = discover_relays_inner(&url, &pubkey_b64, 4433).unwrap();
        handle.join().unwrap();

        assert_eq!(relays, expected);
        assert_eq!(relays[0].region, "us-east-1");
    }

    #[test]
    fn test_discover_relays_rejects_unsafe_url() {
        assert!(discover_relays("http://example.com/nodes", "key", 4433).is_err());
        assert!(discover_relays("https://127.0.0.1/nodes", "key", 4433).is_err());
    }

    #[test]
    fn test_default_registry_consts_are_valid() {
        assert!(DEFAULT_REGISTRY_URL.starts_with("https://"));
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(DEFAULT_OPERATOR_PUBKEY_B64)
            .expect("default operator key must be valid base64");
        assert_eq!(decoded.len(), 32, "Ed25519 public keys are 32 bytes");
    }

    #[test]
    fn test_web_registry_verifies_against_operator_key() {
        let signed_json = std::fs::read_to_string("../web/registry.json").unwrap();
        let signed: SignedRegistry = serde_json::from_str(&signed_json).unwrap();
        let registry = verify_registry(&signed, DEFAULT_OPERATOR_PUBKEY_B64).unwrap();
        assert_eq!(registry.schema_version, 1);
        assert_eq!(registry.nodes.len(), 8);
        for expected in [
            "relay-lax-1",
            "relay-ewr-1",
            "relay-sgp-1",
            "relay-fra",
            "relay-nrt",
            "relay-bom-1",
            "relay-mad-1",
        ] {
            assert!(
                registry.nodes.iter().any(|n| n.node_id == expected),
                "committed registry is missing {expected}"
            );
        }
        assert!(registry.revoked.is_empty());
    }

    /// Point the rollback-freshness state file at a per-process temp file so
    /// these tests are hermetic regardless of the developer's real
    /// `~/.lightspeed-registry-state`.
    fn isolate_registry_state() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let path = std::env::temp_dir().join(format!(
                "lightspeed-registry-state-test-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            std::env::set_var("LIGHTSPEED_REGISTRY_STATE", &path);
        });
    }

    /// Serve `payload` exactly once over HTTP on an ephemeral local port.
    /// Returns the URL and the server thread's join handle.
    fn serve_once(payload: &str) -> (String, std::thread::JoinHandle<()>) {
        isolate_registry_state();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let payload = payload.to_string();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}/nodes"), handle)
    }
}
