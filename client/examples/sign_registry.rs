//! Operator tool: sign a registry node list with the operator's Ed25519 key.
//!
//! Produces the `SignedRegistry` JSON that the Cloudflare Worker (or a static
//! file host) serves, and that clients verify with `registry::verify_registry`.
//!
//! Usage:
//!   cargo run --example sign_registry -- <pkcs8-key-file> <registry-json-file> [--keep-timestamps]
//!
//! By default this stamps a fresh publication:
//!   - `published_at` = max(now, previous + 1): clients reject a registry whose
//!     `published_at` is lower than the last one seen for the same URL, so a
//!     re-sign must never move it backwards. It is not an expiry - see
//!     `Registry::published_at`.
//!   - `issued_at` = now: informational only; preserved so the document carries
//!     a truthful authoring timestamp.
//!
//! `--keep-timestamps` signs the input document unchanged (verification runs,
//! or an emergency re-sign of the exact payload that is already live).

use lightspeed_client::registry::{sign_registry, Registry};
use ring::signature::Ed25519KeyPair;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs()
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mut keep_timestamps = false;
    let mut positional: Vec<String> = Vec::new();
    for arg in args.by_ref() {
        if arg == "--keep-timestamps" {
            keep_timestamps = true;
        } else {
            positional.push(arg);
        }
    }
    if positional.len() != 2 {
        anyhow::bail!(
            "usage: sign_registry <pkcs8-key-file> <registry-json-file> [--keep-timestamps]"
        );
    }

    let key_bytes = std::fs::read(&positional[0])?;
    let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&key_bytes)
        .map_err(|_| anyhow::anyhow!("invalid PKCS8 Ed25519 key"))?;

    let registry_json = std::fs::read_to_string(&positional[1])?;
    let mut registry: Registry = serde_json::from_str(&registry_json)?;

    if !keep_timestamps {
        let now = now_unix();
        // Monotonic: a re-signed registry must never look older than the last
        // one a client could have seen (the client persists this value and
        // rejects a lower one as a rollback).
        registry.published_at = now.max(registry.published_at.saturating_add(1));
        registry.issued_at = Some(now);
    }

    let signed = sign_registry(&registry, &key_pair);
    println!("{}", serde_json::to_string_pretty(&signed)?);
    Ok(())
}
