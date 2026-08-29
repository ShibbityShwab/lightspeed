//! Operator tool: sign a registry node list with the operator's Ed25519 key.
//!
//! Produces the `SignedRegistry` JSON that the Cloudflare Worker (or a static
//! file host) serves, and that clients verify with `registry::verify_registry`.
//!
//! Usage:
//!   cargo run --example sign_registry -- <pkcs8-key-file> <registry-json-file>

use lightspeed_client::registry::{sign_registry, Registry};
use ring::signature::Ed25519KeyPair;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        anyhow::bail!("usage: sign_registry <pkcs8-key-file> <registry-json-file>");
    }

    let key_bytes = std::fs::read(&args[1])?;
    let key_pair = Ed25519KeyPair::from_pkcs8_maybe_unchecked(&key_bytes)
        .map_err(|_| anyhow::anyhow!("invalid PKCS8 Ed25519 key"))?;

    let registry_json = std::fs::read_to_string(&args[2])?;
    let registry: Registry = serde_json::from_str(&registry_json)?;

    let signed = sign_registry(&registry, &key_pair);
    println!("{}", serde_json::to_string_pretty(&signed)?);
    Ok(())
}
