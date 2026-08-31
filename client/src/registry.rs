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
//! boot — see `infra/scripts/provision-oci.sh`).

use serde::{Deserialize, Serialize};

/// A relay node listed in the registry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegistryNode {
    pub node_id: String,
    pub region: String,
    /// Data-plane address ("ip:port") the client tunnels game traffic to.
    pub data_addr: String,
    pub health_url: String,
    /// Base64-encoded Ed25519 public key — the node's identity.
    pub pubkey: String,
    #[serde(default)]
    pub note: String,
}

/// The registry payload — the exact bytes the signature covers.
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
/// `SignedRegistry`. Used by operator tooling (and tests) — the client only
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

/// Fetch a registry, verify it, and return the non-revoked nodes' data-plane
/// addresses as owned "ip:port" strings.
pub fn discover_data_addrs(
    url: &str,
    operator_public_key_b64: &str,
) -> anyhow::Result<Vec<String>> {
    let registry = fetch_registry(url, operator_public_key_b64)?;
    Ok(available_data_addrs(&registry)
        .into_iter()
        .map(str::to_string)
        .collect())
}

/// Fetch a signed registry over HTTPS and verify it against the operator key.
/// Returns the parsed, trusted registry (revoked nodes are NOT filtered out —
/// callers filter via `is_revoked`).
pub fn fetch_registry(url: &str, operator_public_key_b64: &str) -> anyhow::Result<Registry> {
    let body = ureq::get(url)
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
                    note: String::new(),
                },
                RegistryNode {
                    node_id: "bad".into(),
                    region: "us-east-1".into(),
                    data_addr: "5.6.7.8:4434".into(),
                    health_url: "http://5.6.7.8:8080/health".into(),
                    pubkey: "revoked-key".into(),
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
        let fetched = fetch_registry(&url, &pubkey_b64).unwrap();
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
        assert!(fetch_registry(&url, &other_pubkey_b64).is_err());
        handle.join().unwrap();
    }

    /// Serve `payload` exactly once over HTTP on an ephemeral local port.
    /// Returns the URL and the server thread's join handle.
    fn serve_once(payload: &str) -> (String, std::thread::JoinHandle<()>) {
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
