//! TOFU (trust-on-first-use) certificate pinning for the QUIC control plane.
//!
//! The proxy serves a self-signed certificate. Instead of skipping verification
//! (which leaves the control plane open to MITM), the client pins the
//! certificate fingerprint on first connect and rejects any change thereafter,
//! the SSH `known_hosts` model. It still verifies the TLS handshake signature so
//! a peer must hold the private key matching the pinned certificate.

use std::net::SocketAddr;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, Error, SignatureScheme};

use super::fingerprint::{load_fingerprints, save_fingerprint, store_path};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Verifies the proxy's self-signed certificate by pinning its fingerprint.
#[derive(Debug)]
pub struct TofuVerifier {
    addr: String,
    expected: Option<String>,
}

impl TofuVerifier {
    pub fn new(addr: SocketAddr) -> Self {
        let addr = addr.to_string();
        let expected = load_fingerprints().get(&addr).cloned();
        Self { addr, expected }
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let digest = ring::digest::digest(&ring::digest::SHA256, end_entity.as_ref());
        let fp = hex(digest.as_ref());
        match &self.expected {
            Some(expected) if expected == &fp => Ok(ServerCertVerified::assertion()),
            Some(expected) => Err(Error::General(format!(
                "proxy certificate changed for {} (possible MITM): expected {}, got {}. Delete {} to re-pin",
                self.addr,
                expected,
                fp,
                store_path().display()
            ))),
            None => {
                save_fingerprint(&self.addr, &fp);
                tracing::info!("Pinned proxy certificate for {} ({})", self.addr, fp);
                Ok(ServerCertVerified::assertion())
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
