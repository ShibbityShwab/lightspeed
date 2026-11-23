//! # QUIC Control Plane
//!
//! Manages the reliable control channel between client and proxy nodes
//! using QUIC (via quinn). The control plane handles:
//! - Proxy discovery and registration
//! - Health checking and latency probing
//! - Configuration synchronization
//! - Route negotiation
//!
//! The data plane (game packets) uses raw UDP for minimum latency.
//! Only control messages use QUIC for reliability.

pub mod discovery;
pub mod health;

use std::sync::atomic::{AtomicU64, Ordering};

/// Default session-token rotation interval in seconds, matching
/// `config::TunnelConfig`'s serde default and staying below the relay's
/// [`lightspeed_protocol::SESSION_TOKEN_TTL`].
pub const DEFAULT_TOKEN_ROTATION_SECS: u64 = lightspeed_protocol::SESSION_TOKEN_ROTATION.as_secs();

/// Session-token rotation interval in seconds (0 = disabled). The control-plane
/// supervisor reads it; the engine applies the configured value through
/// [`set_token_rotation_secs`].
static TOKEN_ROTATION_SECS: AtomicU64 = AtomicU64::new(DEFAULT_TOKEN_ROTATION_SECS);

/// Set the session-token rotation interval in seconds (`0` disables rotation).
pub fn set_token_rotation_secs(secs: u64) {
    TOKEN_ROTATION_SECS.store(secs, Ordering::Relaxed);
}

/// The session-token rotation interval in seconds (0 = disabled).
pub(crate) fn token_rotation_secs() -> u64 {
    TOKEN_ROTATION_SECS.load(Ordering::Relaxed)
}

/// The UDP payload the QUIC control plane may use on an IP path whose MTU is
/// `path_mtu` bytes.
///
/// The data plane has always budgeted the outer IPv4/UDP headers out of the
/// path MTU; the control plane did not, so it kept probing and sending
/// 1,472-byte datagrams toward the relay's control port even when the user had
/// lowered `[tunnel] mtu` (issue #178: `WSAEMSGSIZE` 10040 on a sub-1500 path).
/// This is the same budget, applied to the supervised QUIC socket.
///
/// The result is clamped to QUIC's 1,200-byte floor (RFC 9000 §14.1: no larger
/// packet before the path is known to allow it; quinn's own minimum is 1,200)
/// and to the data plane's hard cap
/// ([`crate::tunnel::pmtud::HARD_CAP_UDP_PAYLOAD`], 1,472), so the default
/// 1,500-byte configuration keeps exactly the previous upper bound.
pub(crate) fn quic_udp_payload_upper_bound(path_mtu: usize) -> u16 {
    let udp_payload = path_mtu.saturating_sub(
        lightspeed_protocol::OUTER_IPV4_HEADER_SIZE + lightspeed_protocol::OUTER_UDP_HEADER_SIZE,
    );
    udp_payload.clamp(
        QUIC_MIN_UDP_PAYLOAD,
        crate::tunnel::pmtud::HARD_CAP_UDP_PAYLOAD,
    ) as u16
}

/// The smallest UDP payload QUIC permits on an unvalidated path, in bytes
/// (RFC 9000 §14.1, quinn's minimum initial MTU).
const QUIC_MIN_UDP_PAYLOAD: usize = 1200;

pub(crate) mod fingerprint;

#[cfg(feature = "quic")]
mod pinning;

// ── Feature-gated real implementation ───────────────────────────────

#[cfg(feature = "quic")]
mod inner {
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use quinn::crypto::rustls::QuicClientConfig;
    use tracing::{debug, info, warn};

    use lightspeed_protocol::control::ControlMessage;
    use lightspeed_protocol::PROTOCOL_VERSION;

    use super::quic_udp_payload_upper_bound;
    use crate::error::QuicError;

    /// Time budget for one token-rotation exchange over the control connection.
    /// A relay that predates the rotation message may never answer, so the
    /// exchange must not be able to stall pings or the reconnection loop.
    const ROTATE_TIMEOUT: Duration = Duration::from_secs(5);

    /// Process-lifetime memory of how one relay answered the versioned ALPN
    /// offer.
    ///
    /// `docs/quic-alpn-rollout.md` §3 row C: a relay built before the ALPN
    /// rollout rejects the versioned offer with alert 120; row C' is the same
    /// client retrying once with an empty list. The supervisor owns one of
    /// these per relay, so the doomed first attempt happens once per
    /// (process, relay) pair instead of on every reconnect (§9), and a relay
    /// upgrade is picked up on the next process start -- the right freshness
    /// for a capability bit (§5.2). No global, no on-disk cache.
    #[derive(Debug, Default)]
    pub(super) struct AlpnMemory {
        /// Set once this relay closed a handshake with alert 120.
        empty_offer_only: AtomicBool,
    }

    impl AlpnMemory {
        /// The ALPN list to offer this relay on the next attempt.
        pub(super) fn offer(&self) -> Vec<Vec<u8>> {
            if self.needs_empty_offer() {
                Vec::new()
            } else {
                lightspeed_protocol::quic_control_alpn()
            }
        }

        /// Remember that this relay rejected the versioned offer with alert 120.
        pub(super) fn record_alert_120(&self) {
            self.empty_offer_only.store(true, Ordering::Release);
        }

        /// Whether this relay is known to reject the versioned offer.
        pub(super) fn needs_empty_offer(&self) -> bool {
            self.empty_offer_only.load(Ordering::Acquire)
        }
    }

    /// Render an ALPN offer for the debug line, e.g. `["lightspeed-control/1"]`.
    fn alpn_offer_list(alpn: &[Vec<u8>]) -> String {
        let names: Vec<String> = alpn
            .iter()
            .map(|name| format!("{:?}", String::from_utf8_lossy(name)))
            .collect();
        format!("[{}]", names.join(", "))
    }

    /// Render a negotiated protocol for the info line: the name, or `none`.
    fn alpn_name(alpn: Option<&[u8]>) -> String {
        match alpn {
            Some(name) => String::from_utf8_lossy(name).into_owned(),
            None => "none".to_string(),
        }
    }

    /// The application protocol negotiated on an established connection.
    ///
    /// `None` means the handshake completed without ALPN, which is proof the
    /// relay predates the rollout. The handshake data is present once a
    /// connection is established; a downcast miss can only mean a non-rustls
    /// QUIC backend, so it is reported as no negotiation rather than guessed.
    fn negotiated_alpn(conn: &quinn::Connection) -> Option<Vec<u8>> {
        conn.handshake_data()
            .and_then(|data| data.downcast::<quinn::crypto::rustls::HandshakeData>().ok())
            .and_then(|data| data.protocol)
    }

    /// Build a quinn client config that pins the proxy's self-signed
    /// certificate (trust-on-first-use), verifies the TLS signature, and offers
    /// `alpn`.
    ///
    /// `alpn` is empty for a relay that answered alert 120 earlier in this
    /// process, and otherwise the versioned control-plane list: an empty list
    /// sends no ALPN extension at all, which is what a pre-rollout client did.
    ///
    /// `path_mtu_cap` is the effective `[tunnel] mtu` cap ([`HARD_CAP_PATH_MTU`]
    /// when unset). It bounds DPLPMTUD on this path so the control plane never
    /// probes or sends a datagram larger than the configured path allows
    /// (issue #178); see [`quic_udp_payload_upper_bound`].
    fn build_client_config(
        addr: SocketAddr,
        alpn: Vec<Vec<u8>>,
        path_mtu_cap: usize,
    ) -> Result<quinn::ClientConfig, QuicError> {
        // quinn pulls rustls with its aws-lc-rs default while this crate (and
        // the pinning verifier) uses ring, so a workspace build has two
        // providers compiled in and `ClientConfig::builder()` panics on
        // auto-detection. Pin ring as the process-wide provider, exactly as the
        // proxy does for its server config.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(super::pinning::TofuVerifier::new(addr)))
            .with_no_client_auth();
        // The ALPN extension is the whole wire-visible change of the
        // client-first rollout (docs/quic-alpn-rollout.md §2, §5.2). It must be
        // set before the config is converted for QUIC.
        crypto.alpn_protocols = alpn;
        // One line a packet capture can be compared against (§7.1).
        debug!(
            "QUIC control-plane TLS offer: alpn={} sni=lightspeed-proxy tls=1.3",
            alpn_offer_list(&crypto.alpn_protocols)
        );

        let quic_crypto =
            QuicClientConfig::try_from(crypto).map_err(|e| QuicError::Tls(e.to_string()))?;

        let mut client_config = quinn::ClientConfig::new(Arc::new(quic_crypto));

        // Tune transport
        let mut transport = quinn::TransportConfig::default();
        transport.keep_alive_interval(Some(Duration::from_secs(10)));
        transport.max_idle_timeout(Some(
            Duration::from_secs(60)
                .try_into()
                .map_err(|e: quinn::VarIntBoundsExceeded| QuicError::Tls(e.to_string()))?,
        ));
        // Bound quinn's built-in RFC 8899 DPLPMTUD so the client-to-relay path
        // is probed only up to the cap the data plane is willing to trust.
        // The configured `[tunnel] mtu` cap (1,500 when unset) is the same
        // budget the tunnel uses, not the hard cap: on a reduced path a
        // 1,472-byte control datagram is exactly the #178 `WSAEMSGSIZE` case.
        let mut mtu_discovery = quinn::MtuDiscoveryConfig::default();
        mtu_discovery.upper_bound(quic_udp_payload_upper_bound(path_mtu_cap));
        transport.mtu_discovery_config(Some(mtu_discovery));
        client_config.transport_config(Arc::new(transport));

        Ok(client_config)
    }

    /// Whether a failed handshake is the peer's alert 120
    /// (`no_application_protocol`, RFC 9001 §8.1).
    ///
    /// The one classifier both the fallback decision and
    /// [`handshake_failure`]'s verdict read, so the retry condition and the
    /// explanation cannot drift apart.
    fn is_alert_120(err: &quinn::ConnectionError) -> bool {
        // 0x78 is `AlertDescription::NoApplicationProtocol`, carried as QUIC
        // crypto error 0x100 + 0x78 (rendered as "error 120").
        matches!(
            err,
            quinn::ConnectionError::ConnectionClosed(close)
                if close.error_code == quinn::TransportErrorCode::crypto(0x78)
        )
    }

    /// Whether a failed handshake justifies the one empty-offer retry.
    ///
    /// Exactly one case does: a relay that rejected the versioned offer with
    /// alert 120, which is what a relay built before the ALPN rollout does
    /// (docs/quic-alpn-rollout.md §3 row C). A timeout, a different alert or an
    /// application close must not retry, so a middlebox that swallows the offer
    /// cannot double the wait (§5.2).
    fn should_retry_with_empty_alpn(err: &quinn::ConnectionError) -> bool {
        is_alert_120(err)
    }

    /// Explain a failed QUIC handshake in the terms the reader needs.
    ///
    /// quinn renders a peer-sent `CONNECTION_CLOSE` as "aborted by peer: the
    /// cryptographic handshake failed: error 120: peer doesn't support any known
    /// protocol", which reads like a local TLS fault. It is not one: alert 120
    /// (`no_application_protocol`) is what an RFC 9001 §8.1 conforming peer
    /// sends when the two ends cannot agree on an ALPN. Once relays can require
    /// ALPN there are two causes the client cannot separate from the handshake
    /// alone, so the verdict names both instead of claiming the responder
    /// cannot be a relay. This is only reached with alert 120 after the
    /// empty-offer attempt has already failed, so it says so (#140/#147).
    fn handshake_failure(addr: SocketAddr, err: &quinn::ConnectionError) -> String {
        if !is_alert_120(err) {
            return err.to_string();
        }
        format!(
            "{err}: the responder at {addr} closed the QUIC handshake with TLS alert 120 \
             (no_application_protocol), and the empty-offer retry also failed. The client \
             cannot tell these two causes apart from the handshake alone: (1) a \
             QUIC-intercepting middlebox on this path (VPN/AV/ISP) that does not carry the \
             ALPN extension (see #140); or (2) a LightSpeed relay on a different ALPN \
             revision -- version skew, where the relay requires an ALPN name this client \
             does not offer."
        )
    }

    /// QUIC control plane client (real implementation).
    pub struct ControlClient {
        /// Quinn endpoint for outgoing connections.
        endpoint: quinn::Endpoint,
        /// Active connection to the proxy.
        connection: Option<quinn::Connection>,
        /// Remote proxy address.
        remote_addr: Option<SocketAddr>,
        /// Session ID assigned by the proxy.
        session_id: Option<u32>,
        /// Data-plane session token (for tunnel header auth).
        session_token: Option<u32>,
        /// Proxy node ID.
        node_id: Option<String>,
        /// Proxy region.
        region: Option<String>,
        /// Whether the proxy advertised control-plane telemetry in its ack.
        telemetry_quic: bool,
        /// Region of the registered destination, as resolved by the relay's
        /// MMDB on the most recent registration ack.
        dest_region: Option<String>,
        /// Application protocol negotiated on the live control connection.
        ///
        /// `None` on a live connection means the relay accepted an ALPN-less
        /// handshake, i.e. it has not been upgraded to `lightspeed-control/1`.
        negotiated_alpn: Option<Vec<u8>>,
        /// Effective path-MTU cap for this client's path.
        ///
        /// Bounds DPLPMTUD on the control connection; the default is the hard
        /// cap ([`crate::tunnel::pmtud::HARD_CAP_PATH_MTU`]), which preserves
        /// the previous upper bound for probe and test clients. The supervised
        /// registration path lowers it to the configured `[tunnel] mtu` cap.
        path_mtu_cap: usize,
    }

    impl ControlClient {
        /// Create a new control plane client with the hard-cap default.
        ///
        /// Probe and test clients keep this default; the supervised
        /// registration path calls [`Self::set_path_mtu_cap`] with the
        /// configured cap before connecting.
        pub fn new() -> Result<Self, QuicError> {
            // Bind to any available port for the client endpoint. The default
            // client config (with the proxy's pinned certificate) is set per
            // connection in `connect` since it depends on the target address.
            let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            Ok(Self {
                endpoint,
                connection: None,
                remote_addr: None,
                session_id: None,
                session_token: None,
                node_id: None,
                region: None,
                telemetry_quic: false,
                dest_region: None,
                negotiated_alpn: None,
                path_mtu_cap: crate::tunnel::pmtud::HARD_CAP_PATH_MTU,
            })
        }

        /// Set the effective path-MTU cap used for this client's connections.
        ///
        /// The DPLPMTUD upper bound becomes the cap's UDP payload
        /// ([`quic_udp_payload_upper_bound`]). The supervised registration path
        /// sets the configured `[tunnel] mtu` cap here before connecting, so a
        /// reduced path stops probing 1,472-byte control datagrams (issue
        /// #178). The value is clamped by the helper, so an out-of-range cap
        /// cannot escape quinn's 1,200..=1,472 window.
        pub fn set_path_mtu_cap(&mut self, path_mtu_cap: usize) {
            self.path_mtu_cap = path_mtu_cap;
        }

        /// Connect to a proxy's control plane and register.
        ///
        /// `destination` is the game server the client intends to reach when it
        /// is already known. The relay resolves it against its MMDB and returns
        /// the region in the ack, which is cached via
        /// [`crate::route::destination::set_destination_region`] so the region
        /// prior can rank a relay that has never carried traffic to it.
        pub async fn connect(&mut self, addr: SocketAddr, game: u8) -> Result<(), QuicError> {
            self.connect_with_destination(addr, game, None).await
        }

        /// Connect and register, reporting the destination to the relay.
        ///
        /// Outside the per-relay supervisor (for example `--test-control`) there
        /// is no earlier answer to consult, so a fresh memory makes the first
        /// attempt the full offer -- the correct default.
        pub async fn connect_with_destination(
            &mut self,
            addr: SocketAddr,
            game: u8,
            destination: Option<Ipv4Addr>,
        ) -> Result<(), QuicError> {
            let memory = AlpnMemory::default();
            self.connect_with_relay_memory(addr, game, destination, &memory)
                .await
        }

        /// Connect and register, consulting this relay's ALPN answer memory.
        ///
        /// At most two handshake attempts: the offer from `alpn_memory`, then --
        /// only when the relay rejected it with alert 120 -- one retry with an
        /// empty offer (docs/quic-alpn-rollout.md §5.2). The alert is recorded
        /// in `alpn_memory`, so a later reconnect to the same relay in this
        /// process goes straight to the empty offer (§9). Nothing else retries.
        pub(super) async fn connect_with_relay_memory(
            &mut self,
            addr: SocketAddr,
            game: u8,
            destination: Option<Ipv4Addr>,
            alpn_memory: &AlpnMemory,
        ) -> Result<(), QuicError> {
            info!("Connecting QUIC control plane to {}", addr);

            let mut offer = alpn_memory.offer();
            if offer.is_empty() && alpn_memory.needs_empty_offer() {
                debug!(
                    relay = %addr,
                    "relay answered the ALPN offer with alert 120 earlier in this process; \
                     offering an empty list"
                );
            }

            loop {
                self.endpoint.set_default_client_config(build_client_config(
                    addr,
                    offer.clone(),
                    self.path_mtu_cap,
                )?);
                let connecting = self
                    .endpoint
                    .connect(addr, "lightspeed-proxy")
                    .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
                match connecting.await {
                    Ok(conn) => {
                        return self
                            .register_on_connection(conn, addr, game, destination)
                            .await;
                    }
                    Err(err) => {
                        // Exactly one retry, and only for alert 120: an empty
                        // offer is never retried again.
                        if offer.is_empty() || !should_retry_with_empty_alpn(&err) {
                            return Err(QuicError::ConnectionFailed(handshake_failure(addr, &err)));
                        }
                        warn!(
                            relay = %addr,
                            "relay rejected the ALPN offer with alert 120 \
                             (no_application_protocol); retrying with an empty offer"
                        );
                        alpn_memory.record_alert_120();
                        offer.clear();
                    }
                }
            }
        }

        /// Register this session on an established control connection.
        async fn register_on_connection(
            &mut self,
            conn: quinn::Connection,
            addr: SocketAddr,
            game: u8,
            destination: Option<Ipv4Addr>,
        ) -> Result<(), QuicError> {
            let alpn = negotiated_alpn(&conn);
            info!(
                relay = %addr,
                alpn = %alpn_name(alpn.as_deref()),
                "QUIC connection established"
            );

            // Open a bidirectional stream for registration
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            // Send Register message
            let register = ControlMessage::Register {
                protocol_version: PROTOCOL_VERSION,
                game,
                data_port: 0,
                destination,
            };
            register
                .write_to(&mut send)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            // Read RegisterAck
            let response = ControlMessage::read_from(&mut recv)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            match response {
                Some(ControlMessage::RegisterAck {
                    session_id,
                    session_token,
                    node_id,
                    region,
                    telemetry_quic,
                    dest_region,
                    ..
                }) => {
                    info!(
                        "Registered with proxy: session={}, token={}, node={}, region={}",
                        session_id, session_token, node_id, region
                    );
                    self.session_id = Some(session_id);
                    self.session_token = Some(session_token);
                    self.node_id = Some(node_id);
                    self.region = Some(region);
                    self.telemetry_quic = telemetry_quic;
                    self.dest_region = dest_region.clone();
                    if let (Some(destination), Some(dest_region)) = (destination, dest_region) {
                        crate::route::destination::set_destination_region(
                            destination,
                            &dest_region,
                        );
                    }
                    crate::session::set_session_token(session_token);
                }
                Some(ControlMessage::Disconnect { reason }) => {
                    return Err(QuicError::ConnectionFailed(format!(
                        "Proxy rejected registration (reason={})",
                        reason
                    )));
                }
                other => {
                    return Err(QuicError::ConnectionFailed(format!(
                        "Unexpected response: {:?}",
                        other
                    )));
                }
            }

            // Finish the registration stream
            send.finish()
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            self.negotiated_alpn = alpn;
            self.connection = Some(conn);
            self.remote_addr = Some(addr);
            Ok(())
        }

        /// Send a ping and measure round-trip time in microseconds.
        pub async fn ping(&self) -> Result<u64, QuicError> {
            let conn = self
                .connection
                .as_ref()
                .ok_or_else(|| QuicError::ConnectionFailed("Not connected".into()))?;

            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let now_us = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            let ping = ControlMessage::Ping {
                timestamp_us: now_us,
            };
            ping.write_to(&mut send)
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;
            send.finish()
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let response = ControlMessage::read_from(&mut recv)
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let after_us = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            match response {
                Some(ControlMessage::Pong {
                    client_timestamp_us,
                    ..
                }) => {
                    let rtt_us = after_us.saturating_sub(client_timestamp_us);
                    debug!("Ping RTT: {}μs", rtt_us);
                    Ok(rtt_us)
                }
                other => Err(QuicError::HealthCheckFailed(format!(
                    "Unexpected ping response: {:?}",
                    other
                ))),
            }
        }

        /// Advertise the capabilities this client can consume.
        ///
        /// Sent once per control connection, best-effort and never awaited for a
        /// reply: a relay that predates [`ControlMessage::ClientCaps`] decodes
        /// an unknown type and drops the stream without touching the
        /// connection. The relay uses the bitmap to decide whether it may push
        /// a relay-measured server-leg report; a session that never sends this
        /// is simply never pushed one, which is what keeps a pre-R2 client from
        /// receiving frames it would not read.
        pub async fn advertise_client_caps(&self, caps: u8) -> Result<(), QuicError> {
            let conn = self
                .connection
                .as_ref()
                .ok_or_else(|| QuicError::ConnectionFailed("Not connected".into()))?;

            let (mut send, _recv) = conn
                .open_bi()
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            ControlMessage::ClientCaps { caps }
                .write_to(&mut send)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            send.finish()
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            Ok(())
        }

        /// Request a fresh data-plane session token from the relay.
        ///
        /// Returns `(new_token, old_token)`; the caller applies the rotation to
        /// the session store. Bounded by [`ROTATE_TIMEOUT`], so a relay that
        /// predates the rotation message and never answers cannot stall pings
        /// or the reconnection loop.
        pub async fn rotate_token(&self, current_token: u32) -> Result<(u32, u32), QuicError> {
            let exchange = async {
                let conn = self
                    .connection
                    .as_ref()
                    .ok_or_else(|| QuicError::ConnectionFailed("Not connected".into()))?;

                let (mut send, mut recv) = conn
                    .open_bi()
                    .await
                    .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

                let request = ControlMessage::RotateRequest { current_token };
                request
                    .write_to(&mut send)
                    .await
                    .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
                send.finish()
                    .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

                let response = ControlMessage::read_from(&mut recv)
                    .await
                    .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
                match response {
                    Some(ControlMessage::RotateAck {
                        new_token,
                        old_token,
                    }) => Ok((new_token, old_token)),
                    Some(ControlMessage::Disconnect { reason }) => {
                        Err(QuicError::ConnectionFailed(format!(
                            "relay rejected token rotation (reason={reason})"
                        )))
                    }
                    other => Err(QuicError::ConnectionFailed(format!(
                        "unexpected rotation response: {other:?}"
                    ))),
                }
            };

            tokio::time::timeout(ROTATE_TIMEOUT, exchange)
                .await
                .map_err(|_| QuicError::ConnectionFailed("token rotation timed out".into()))?
        }

        /// Disconnect from the proxy gracefully.
        pub async fn disconnect(&mut self) -> Result<(), QuicError> {
            if let Some(conn) = self.connection.take() {
                // Try to send a Disconnect message
                if let Ok((mut send, _recv)) = conn.open_bi().await {
                    let msg = ControlMessage::Disconnect {
                        reason: lightspeed_protocol::control::disconnect_reason::NORMAL,
                    };
                    let _ = msg.write_to(&mut send).await;
                    let _ = send.finish();
                }
                conn.close(0u32.into(), b"bye");
                info!("Disconnected from proxy");
            }
            self.remote_addr = None;
            self.session_id = None;
            self.session_token = None;
            self.node_id = None;
            self.region = None;
            self.dest_region = None;
            self.negotiated_alpn = None;
            Ok(())
        }

        /// Check if control plane is connected.
        pub fn is_connected(&self) -> bool {
            self.connection
                .as_ref()
                .map(|c| c.close_reason().is_none())
                .unwrap_or(false)
        }

        /// Get the session ID assigned by the proxy.
        pub fn session_id(&self) -> Option<u32> {
            self.session_id
        }

        /// Get the data-plane session token (for tunnel header auth).
        pub fn session_token(&self) -> Option<u32> {
            self.session_token
        }

        /// Get the proxy node ID.
        pub fn node_id(&self) -> Option<&str> {
            self.node_id.as_deref()
        }

        /// Get the proxy region.
        pub fn region(&self) -> Option<&str> {
            self.region.as_deref()
        }

        /// Get the region the relay resolved for the registered destination, if
        /// one was reported in the ack.
        pub fn destination_region(&self) -> Option<&str> {
            self.dest_region.as_deref()
        }

        /// Whether the proxy accepts telemetry on the control connection.
        pub fn supports_quic_telemetry(&self) -> bool {
            self.telemetry_quic
        }

        /// The application protocol negotiated on the live control connection.
        ///
        /// `Some(b"lightspeed-control/1")` proves the relay has the ALPN
        /// rollout; `None` on a live connection proves it has not (an
        /// ALPN-less handshake was accepted). This is the value `--test-control`
        /// prints and the interop test asserts.
        pub fn negotiated_alpn(&self) -> Option<&[u8]> {
            self.negotiated_alpn.as_deref()
        }

        /// Get the remote proxy address.
        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }

        /// The live QUIC connection, if any. Used by the supervisor to race
        /// `Connection::closed()` against the keepalive ping.
        pub fn connection(&self) -> Option<&quinn::Connection> {
            self.connection.as_ref()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{handshake_failure, should_retry_with_empty_alpn, AlpnMemory};
        use std::net::SocketAddr;

        fn connection_closed(
            code: quinn::TransportErrorCode,
            reason: &'static [u8],
        ) -> quinn::ConnectionError {
            quinn::ConnectionError::ConnectionClosed(quinn::ConnectionClose {
                error_code: code,
                frame_type: None,
                reason: bytes::Bytes::from_static(reason),
            })
        }

        fn alert_120() -> quinn::ConnectionError {
            connection_closed(
                quinn::TransportErrorCode::crypto(0x78),
                b"peer doesn't support any known protocol",
            )
        }

        fn application_closed() -> quinn::ConnectionError {
            quinn::ConnectionError::ApplicationClosed(quinn::ApplicationClose {
                error_code: 0u32.into(),
                reason: bytes::Bytes::from_static(b"bye"),
            })
        }

        // Updated with the rollout: the old test asserted the responder could
        // not be a relay. Once relays can require ALPN that claim is false, so
        // the same case now asserts the verdict names the ALPN disagreement
        // without pretending to know which side caused it.
        #[test]
        fn alert_120_is_explained_as_a_peer_that_cannot_be_our_relay() {
            let addr: SocketAddr = "203.0.113.10:4433".parse().expect("addr");
            let message = handshake_failure(addr, &alert_120());

            assert!(message.contains("no_application_protocol"), "{message}");
            assert!(message.contains("203.0.113.10:4433"), "{message}");
            assert!(
                !message.contains("not this relay"),
                "the rollout makes the old 'not this relay' claim false: {message}"
            );
        }

        #[test]
        fn alert_120_selects_the_empty_list_retry() {
            // Row C of docs/quic-alpn-rollout.md §3: a relay built before the
            // rollout answers the versioned offer with alert 120. That is the
            // only failure that earns row C' -- one retry with an empty list.
            assert!(
                should_retry_with_empty_alpn(&alert_120()),
                "alert 120 must select the empty-offer retry"
            );
        }

        #[test]
        fn a_certificate_alert_does_not_retry() {
            // A bad certificate is a real TLS fault and the empty list cannot
            // fix it.
            let certificate =
                connection_closed(quinn::TransportErrorCode::crypto(0x2a), b"bad certificate");
            assert!(
                !should_retry_with_empty_alpn(&certificate),
                "a certificate alert must not retry"
            );
        }

        #[test]
        fn a_timeout_does_not_retry() {
            // A middlebox that swallows the offering ClientHello must not double
            // the wait (§5.2).
            assert!(
                !should_retry_with_empty_alpn(&quinn::ConnectionError::TimedOut),
                "a timeout must not retry"
            );
        }

        #[test]
        fn an_application_close_does_not_retry() {
            assert!(
                !should_retry_with_empty_alpn(&application_closed()),
                "an application close must not retry"
            );
        }

        #[test]
        fn the_120_verdict_names_version_skew_after_the_rollout() {
            // §7.1 and falsifier F6 (§12): the verdict must name both causes
            // (middlebox or ALPN revision skew), carry the address, and say the
            // empty-offer retry failed, never claim the responder is not a relay.
            let addr: SocketAddr = "203.0.113.10:4433".parse().expect("addr");
            let message = handshake_failure(addr, &alert_120());

            assert!(
                message.contains("203.0.113.10:4433"),
                "the verdict must name the address: {message}"
            );
            assert!(
                message.contains("no_application_protocol"),
                "the verdict must name the alert: {message}"
            );
            assert!(
                message.contains("middlebox"),
                "the verdict must name a QUIC-intercepting middlebox: {message}"
            );
            assert!(
                message.contains("version skew") && message.contains("ALPN revision"),
                "the verdict must name version skew: {message}"
            );
            assert!(
                message.contains("empty-offer retry also failed"),
                "the verdict must say the empty-offer retry already failed: {message}"
            );
            assert!(
                message.contains("cannot tell these two causes apart"),
                "the client cannot distinguish the causes, and must say so: {message}"
            );
        }

        #[test]
        fn a_relays_alert_120_answer_is_remembered_for_the_process() {
            // §9: the doomed attempt happens once per (process, relay). A relay
            // that answered 120 is offered the empty list from then on, so a
            // reconnect does not repeat the failing handshake.
            let memory = AlpnMemory::default();
            assert!(!memory.needs_empty_offer());
            assert_eq!(
                memory.offer(),
                vec![lightspeed_protocol::QUIC_CONTROL_ALPN_V1.to_vec()],
                "an unknown relay gets the versioned offer"
            );

            memory.record_alert_120();
            assert!(memory.needs_empty_offer());
            assert!(
                memory.offer().is_empty(),
                "a relay that answered alert 120 is offered the empty list"
            );
        }

        #[test]
        fn other_handshake_failures_keep_the_raw_error() {
            // Only alert 120 carries this diagnosis. A bad certificate, a
            // timeout, or an application close must keep its own wording.
            let addr: SocketAddr = "203.0.113.10:4433".parse().expect("addr");
            let certificate =
                connection_closed(quinn::TransportErrorCode::crypto(0x2a), b"bad certificate");
            assert_eq!(
                handshake_failure(addr, &certificate),
                certificate.to_string(),
                "a certificate alert is not the no-ALPN diagnosis"
            );
            assert_eq!(
                handshake_failure(addr, &quinn::ConnectionError::TimedOut),
                quinn::ConnectionError::TimedOut.to_string()
            );
            let closed = application_closed();
            assert_eq!(
                handshake_failure(addr, &closed),
                closed.to_string(),
                "an application close is not the no-ALPN diagnosis"
            );
        }
    }
}

// ── Fallback stub (no quic feature) ─────────────────────────────────

#[cfg(not(feature = "quic"))]
mod inner {
    use std::net::{Ipv4Addr, SocketAddr};

    use crate::error::QuicError;

    /// QUIC control plane client (stub - compile without `quic` feature).
    pub struct ControlClient {
        connected: bool,
        remote_addr: Option<SocketAddr>,
    }

    impl ControlClient {
        pub fn new() -> Result<Self, QuicError> {
            Ok(Self {
                connected: false,
                remote_addr: None,
            })
        }

        /// Accepted for API parity with the enabled build; the stub has no
        /// transport whose MTU discovery could be bounded.
        pub fn set_path_mtu_cap(&mut self, _path_mtu_cap: usize) {}

        pub async fn connect(&mut self, addr: SocketAddr, game: u8) -> Result<(), QuicError> {
            self.connect_with_destination(addr, game, None).await
        }

        pub async fn connect_with_destination(
            &mut self,
            addr: SocketAddr,
            _game: u8,
            _destination: Option<Ipv4Addr>,
        ) -> Result<(), QuicError> {
            tracing::info!("QUIC disabled - stub connect to {}", addr);
            self.remote_addr = Some(addr);
            self.connected = true;
            Ok(())
        }

        pub async fn ping(&self) -> Result<u64, QuicError> {
            Ok(0)
        }

        pub async fn disconnect(&mut self) -> Result<(), QuicError> {
            self.connected = false;
            self.remote_addr = None;
            Ok(())
        }

        pub fn is_connected(&self) -> bool {
            self.connected
        }

        pub fn session_id(&self) -> Option<u32> {
            None
        }

        pub fn session_token(&self) -> Option<u32> {
            None
        }

        pub fn node_id(&self) -> Option<&str> {
            None
        }

        pub fn region(&self) -> Option<&str> {
            None
        }

        pub fn supports_quic_telemetry(&self) -> bool {
            false
        }

        /// No control connection exists without the `quic` feature, so no
        /// application protocol can be negotiated.
        pub fn negotiated_alpn(&self) -> Option<&[u8]> {
            None
        }

        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }
    }
}

pub use inner::ControlClient;

// ── Supervised control-plane registration ───────────────────────────

#[cfg(feature = "quic")]
mod supervisor {
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    use tokio::sync::watch;
    use tokio::task::JoinHandle;
    use tracing::{debug, info, warn};

    use lightspeed_protocol::control::{caps, ControlMessage};

    use super::inner::{AlpnMemory, ControlClient};
    use crate::app::lifecycle::SessionCloseReason;
    use crate::error::QuicError;

    pub(super) const PING_INTERVAL: Duration = Duration::from_secs(15);
    pub(super) const MIN_BACKOFF: Duration = Duration::from_millis(250);
    pub(super) const MAX_BACKOFF: Duration = Duration::from_secs(5);

    /// Time budget for one best-effort telemetry send on the control
    /// connection. Telemetry must never delay the control plane or gameplay.
    const TELEMETRY_SEND_TIMEOUT: Duration = Duration::from_secs(2);

    /// First-attempt outcome awaited by [`register_session`](super::register_session).
    ///
    /// `Done(token)` is a real result. `Discarded` is the P0-3 settlement: the
    /// supervisor was retired (a newly learned destination replaced it) or
    /// stopped before its first attempt completed, so the result is not a
    /// failure for presentation — it is superseded, and the engine discards it
    /// without touching `registration_error` (invariant I5).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Initial {
        Pending,
        Done(u32),
        Discarded(SessionCloseReason),
    }

    /// A live control connection plus the capabilities the relay advertised in
    /// its registration ack. Published and cleared as one unit so a telemetry
    /// send can never observe a connection without its capability.
    #[derive(Clone)]
    struct ControlTarget {
        connection: quinn::Connection,
        telemetry_quic: bool,
    }

    /// Observable handle for a running per-relay supervisor.
    #[derive(Clone)]
    pub struct SupervisorHandle {
        /// Monotonic id; a newer supervisor for the same relay has a higher one.
        pub generation: u64,
        /// Set once the supervisor has been asked to stop.
        pub stopped: Arc<AtomicBool>,
        /// Most recent non-zero token the relay issued (0 until registered).
        pub current_token: Arc<AtomicU32>,
        /// Live connection + capability while registered; cleared on disconnect
        /// or reconnect. Telemetry sends on this same connection.
        target: Arc<Mutex<Option<ControlTarget>>>,
    }

    struct Entry {
        handle: SupervisorHandle,
        initial_rx: watch::Receiver<Initial>,
        stop_tx: watch::Sender<bool>,
        destination: Arc<Mutex<Option<Ipv4Addr>>>,
        /// This relay's process-lifetime ALPN answer, kept here so a supervisor
        /// replacement (a newly learned destination) does not forget it.
        alpn_memory: Arc<AlpnMemory>,
        control_port: u16,
        /// Effective path-MTU cap the live supervisor was created with; a
        /// request with a different cap replaces the supervisor so the new cap
        /// reaches the next control connection.
        path_mtu_cap: usize,
        _join: JoinHandle<()>,
    }

    struct SupervisorContext {
        data_addr: SocketAddrV4,
        generation: u64,
        stopped: Arc<AtomicBool>,
        current_token: Arc<AtomicU32>,
        target: Arc<Mutex<Option<ControlTarget>>>,
        destination: Arc<Mutex<Option<Ipv4Addr>>>,
        alpn_memory: Arc<AlpnMemory>,
        path_mtu_cap: usize,
    }

    static REGISTRY: OnceLock<Mutex<HashMap<SocketAddrV4, Entry>>> = OnceLock::new();
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

    fn lock_registry() -> MutexGuard<'static, HashMap<SocketAddrV4, Entry>> {
        REGISTRY
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A supervisor may only write state while it is still the live one for its
    /// relay, which makes a stopped or superseded task a no-op.
    fn generation_is_current(data_addr: SocketAddrV4, generation: u64) -> bool {
        lock_registry().get(&data_addr).is_some_and(|entry| {
            entry.handle.generation == generation && !entry.handle.stopped.load(Ordering::Acquire)
        })
    }

    /// Exponential backoff with downward jitter, bounded to
    /// `[MIN_BACKOFF, MAX_BACKOFF]`.
    pub(super) struct Backoff {
        base: Duration,
    }

    impl Backoff {
        pub(super) fn new() -> Self {
            Self { base: MIN_BACKOFF }
        }

        pub(super) fn reset(&mut self) {
            self.base = MIN_BACKOFF;
        }

        /// Advance the exponential schedule and return the previous base.
        pub(super) fn advance(&mut self) -> Duration {
            let base = self.base;
            self.base = (self.base * 2).min(MAX_BACKOFF);
            base
        }

        /// Next delay: the exponential base jittered uniformly down to half,
        /// clamped to the configured floor and ceiling.
        pub(super) fn next_delay(&mut self) -> Duration {
            let base = self.advance();
            let low = (base / 2).max(MIN_BACKOFF);
            let high = base.max(MIN_BACKOFF);
            if high <= low {
                return low;
            }
            let span_ms = (high - low).as_millis() as u64 + 1;
            low + Duration::from_millis(rand::random::<u64>() % span_ms)
        }
    }

    /// Ensure exactly one supervisor exists for `data_addr`, returning the
    /// receiver that reports its first registration outcome.
    ///
    /// `path_mtu_cap` is the effective `[tunnel] mtu` cap; a supervisor created
    /// with a different cap is replaced so the next control connection uses the
    /// new value (like a newly learned destination, a cap change is a
    /// re-registration reason).
    pub(super) fn ensure_supervisor(
        data_addr: SocketAddrV4,
        control_port: u16,
        destination: Option<Ipv4Addr>,
        path_mtu_cap: usize,
    ) -> (u64, watch::Receiver<Initial>) {
        let mut registry = lock_registry();
        if let Some(entry) = registry.get(&data_addr) {
            if !entry.handle.stopped.load(Ordering::Acquire) {
                let known = *entry
                    .destination
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if (destination.is_none() || known == destination)
                    && entry.path_mtu_cap == path_mtu_cap
                {
                    return (entry.handle.generation, entry.initial_rx.clone());
                }
            }
        }
        // One replacement point: retire the old supervisor and keep its relay's
        // ALPN answer. The answer is a capability of the relay, not of the task,
        // so a destination change must not pay the fallback handshake again
        // (docs/quic-alpn-rollout.md §9).
        let alpn_memory = match registry.remove(&data_addr) {
            Some(entry) => {
                entry.handle.stopped.store(true, Ordering::Release);
                let _ = entry.stop_tx.send(true);
                entry.alpn_memory
            }
            None => Arc::new(AlpnMemory::default()),
        };

        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let stopped = Arc::new(AtomicBool::new(false));
        let current_token = Arc::new(AtomicU32::new(0));
        let target = Arc::new(Mutex::new(None));
        let destination = Arc::new(Mutex::new(destination));
        let (initial_tx, initial_rx) = watch::channel(Initial::Pending);
        let (stop_tx, stop_rx) = watch::channel(false);

        let handle = SupervisorHandle {
            generation,
            stopped: Arc::clone(&stopped),
            current_token: Arc::clone(&current_token),
            target: Arc::clone(&target),
        };
        let context = SupervisorContext {
            data_addr,
            generation,
            stopped,
            current_token,
            target,
            destination: Arc::clone(&destination),
            alpn_memory: Arc::clone(&alpn_memory),
            path_mtu_cap,
        };
        let join = tokio::spawn(supervisor_loop(context, control_port, initial_tx, stop_rx));

        registry.insert(
            data_addr,
            Entry {
                handle,
                initial_rx: initial_rx.clone(),
                stop_tx,
                destination,
                alpn_memory,
                control_port,
                path_mtu_cap,
                _join: join,
            },
        );
        (generation, initial_rx)
    }

    /// Await the first registration outcome for `data_addr`, spawning the
    /// supervisor if needed. Returns the initial token, or `None` if the first
    /// attempt failed (the supervisor keeps retrying in the background).
    pub(super) async fn register_and_wait(
        data_addr: SocketAddrV4,
        control_port: u16,
        destination: Option<Ipv4Addr>,
        path_mtu_cap: usize,
    ) -> Option<u32> {
        let attempt =
            register_and_wait_attempt(data_addr, control_port, destination, path_mtu_cap).await;
        attempt.session
    }

    /// Await the first registration outcome with its generation and supersede
    /// verdict (P0-3). The engine's generation guard consumes this instead of
    /// mapping every non-`Ok` to a failure.
    pub(super) async fn register_and_wait_attempt(
        data_addr: SocketAddrV4,
        control_port: u16,
        destination: Option<Ipv4Addr>,
        path_mtu_cap: usize,
    ) -> super::RegistrationAttemptResult {
        let (generation, mut initial_rx) =
            ensure_supervisor(data_addr, control_port, destination, path_mtu_cap);
        if matches!(*initial_rx.borrow(), Initial::Pending) {
            let _ = initial_rx.changed().await;
        }
        let outcome = *initial_rx.borrow();
        match outcome {
            Initial::Done(token) if token != 0 => super::RegistrationAttemptResult {
                generation,
                outcome: crate::app::lifecycle::RegistrationOutcome::Ok,
                session: Some(token),
                close_reason: None,
            },
            Initial::Done(_) => super::RegistrationAttemptResult {
                generation,
                outcome: crate::app::lifecycle::RegistrationOutcome::Fail,
                session: None,
                close_reason: None,
            },
            // Retired/stopped before the first attempt completed: not a failure
            // (invariant I5). The engine records `superseded`, never an error.
            Initial::Discarded(reason) => super::RegistrationAttemptResult {
                generation,
                outcome: crate::app::lifecycle::RegistrationOutcome::Superseded,
                session: None,
                close_reason: Some(reason),
            },
            Initial::Pending => super::RegistrationAttemptResult {
                generation,
                outcome: crate::app::lifecycle::RegistrationOutcome::Superseded,
                session: None,
                close_reason: Some(SessionCloseReason::Unknown),
            },
        }
    }

    /// Spawn a supervisor for `data_addr` without waiting for the first result.
    pub fn ensure_registration(data_addr: SocketAddrV4, control_port: u16, path_mtu_cap: usize) {
        let _ = ensure_supervisor(data_addr, control_port, None, path_mtu_cap);
    }

    /// Spawn a supervisor for `data_addr`, reporting the game server so the
    /// relay can resolve its region. A later call that supplies a destination
    /// retires the live supervisor and re-registers so the new destination is
    /// actually sent.
    pub fn ensure_registration_with_destination(
        data_addr: SocketAddrV4,
        control_port: u16,
        destination: Option<Ipv4Addr>,
        path_mtu_cap: usize,
    ) {
        let _ = ensure_supervisor(data_addr, control_port, destination, path_mtu_cap);
    }

    /// Report the game server to the currently selected relay so it resolves
    /// and caches the destination region. The supervisor's destination equality
    /// check makes a repeat a no-op, so a relay without an MMDB is never
    /// re-registered on every sample.
    pub fn report_destination(server: Ipv4Addr) {
        let Some(data_addr) = crate::session::current_proxy() else {
            return;
        };
        let registered = lock_registry()
            .get(&data_addr)
            .map(|entry| (entry.control_port, entry.path_mtu_cap));
        let Some((control_port, path_mtu_cap)) = registered else {
            return;
        };
        ensure_registration_with_destination(data_addr, control_port, Some(server), path_mtu_cap);
    }

    /// Stop the supervisor for `data_addr`, if one is running.
    pub fn stop_supervisor(data_addr: SocketAddrV4) -> bool {
        match lock_registry().remove(&data_addr) {
            Some(entry) => {
                entry.handle.stopped.store(true, Ordering::Release);
                let _ = entry.stop_tx.send(true);
                true
            }
            None => false,
        }
    }

    /// Observe the supervisor for `data_addr`, if one is running.
    pub fn supervisor_handle(data_addr: SocketAddrV4) -> Option<SupervisorHandle> {
        lock_registry()
            .get(&data_addr)
            .map(|entry| entry.handle.clone())
    }

    /// Whether a live supervisor is managing `data_addr`.
    pub fn is_supervised(data_addr: SocketAddrV4) -> bool {
        supervisor_handle(data_addr).is_some_and(|handle| !handle.stopped.load(Ordering::Acquire))
    }

    /// The live supervisor's registration for `data_addr`: its generation and
    /// the current non-zero token, when it is registered (P0-3).
    ///
    /// This is the authoritative "the current generation succeeded" fact the
    /// engine's snapshot reconciles against, so a replacement registration that
    /// nobody awaited still has a writer (invariant I5).
    pub fn supervised_registration(data_addr: SocketAddrV4) -> Option<(u64, u32)> {
        let handle = supervisor_handle(data_addr)?;
        if handle.stopped.load(Ordering::Acquire) {
            return None;
        }
        let token = handle.current_token.load(Ordering::Acquire);
        (token != 0).then_some((handle.generation, token))
    }

    /// The most recent token a supervisor's relay issued, if any.
    pub fn supervised_token(data_addr: SocketAddrV4) -> Option<u32> {
        let token = supervisor_handle(data_addr)?
            .current_token
            .load(Ordering::Acquire);
        (token != 0).then_some(token)
    }

    /// The control connection's current DPLPMTUD result for `data_addr`, as the
    /// largest UDP payload the client-to-relay path supports.
    pub fn supervised_path_mtu(data_addr: SocketAddrV4) -> Option<u16> {
        supervised_target(data_addr).map(|target| target.connection.stats().path.current_mtu)
    }

    async fn supervisor_loop(
        context: SupervisorContext,
        control_port: u16,
        initial_tx: watch::Sender<Initial>,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        let SupervisorContext {
            data_addr,
            generation,
            stopped,
            current_token,
            target,
            destination,
            alpn_memory,
            path_mtu_cap,
        } = context;
        let control_addr = SocketAddr::V4(SocketAddrV4::new(*data_addr.ip(), control_port));
        let mut backoff = Backoff::new();
        let mut first_attempt = true;

        loop {
            if stopped.load(Ordering::Acquire) || !generation_is_current(data_addr, generation) {
                break;
            }

            let destination = *destination
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match connect_and_register(control_addr, destination, &alpn_memory, path_mtu_cap).await
            {
                Ok(client) => {
                    let token = client.session_token().unwrap_or(0);
                    let live = client.connection().cloned();
                    let published = live.clone().map(|connection| ControlTarget {
                        connection,
                        telemetry_quic: client.supports_quic_telemetry(),
                    });
                    if !publish_target(data_addr, generation, &target, published) {
                        break;
                    }
                    if token != 0 {
                        if !publish_token(data_addr, generation, &current_token, token) {
                            break;
                        }
                        backoff.reset();
                        info!(relay = %data_addr, token, "control-plane registered");
                    }
                    // Tell the relay what this client can consume before holding
                    // the connection: a relay only pushes a relay-measured
                    // server-leg report to a session that advertised it can read
                    // one. Best-effort; a relay that predates the message drops
                    // the stream and the session is unaffected.
                    if let Err(error) = client.advertise_client_caps(caps::SERVER_LEG).await {
                        debug!(
                            relay = %data_addr,
                            %error,
                            "capability advertisement failed; server-leg reports may not be pushed"
                        );
                    }
                    if first_attempt {
                        let _ = initial_tx.send_replace(Initial::Done(token));
                        first_attempt = false;
                    }
                    let hold = hold_connection(
                        &client,
                        data_addr,
                        generation,
                        &current_token,
                        live,
                        &mut stop_rx,
                    )
                    .await;
                    clear_target(data_addr, generation, &target);
                    if hold == HoldOutcome::Stopped {
                        break;
                    }
                }
                Err(error) => {
                    warn!(relay = %data_addr, %error, "control-plane registration failed; retrying");
                    if first_attempt {
                        let _ = initial_tx.send_replace(Initial::Done(0));
                        first_attempt = false;
                    }
                }
            }

            let delay = backoff.next_delay();
            debug!(relay = %data_addr, delay_ms = delay.as_millis(), "control-plane reconnect backoff");
            if sleep_or_stop(delay, &stopped, &mut stop_rx).await {
                break;
            }
        }

        clear_target(data_addr, generation, &target);
        if first_attempt {
            // P0-3: the first attempt never completed. Distinguish a retired
            // generation (a replacement supervisor now owns the relay) from a
            // deliberate stop; neither is a registration failure, and the
            // generation guard needs the reason so the ledger can close it.
            let replaced = lock_registry()
                .get(&data_addr)
                .is_some_and(|entry| entry.handle.generation != generation);
            let reason = if replaced {
                SessionCloseReason::SessionReplaced
            } else {
                SessionCloseReason::ClientQuit
            };
            let _ = initial_tx.send_replace(Initial::Discarded(reason));
        }
        debug!(relay = %data_addr, "control-plane supervisor stopped");
    }

    /// Publish the connection + capability only while this task is still the
    /// live supervisor for its relay. Returns `false` if it has been stopped or
    /// superseded.
    fn publish_target(
        data_addr: SocketAddrV4,
        generation: u64,
        slot: &Mutex<Option<ControlTarget>>,
        target: Option<ControlTarget>,
    ) -> bool {
        if !generation_is_current(data_addr, generation) {
            return false;
        }
        *slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = target;
        true
    }

    /// Clear the published target, unless a newer supervisor owns the slot.
    fn clear_target(data_addr: SocketAddrV4, generation: u64, slot: &Mutex<Option<ControlTarget>>) {
        if generation_is_current(data_addr, generation) {
            *slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }

    /// The live control connection and advertised capability for `data_addr`.
    fn supervised_target(data_addr: SocketAddrV4) -> Option<ControlTarget> {
        let handle = supervisor_handle(data_addr)?;
        if handle.stopped.load(Ordering::Acquire) {
            return None;
        }
        let target = handle
            .target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        target
    }

    /// Send one telemetry report on the relay's live control connection.
    ///
    /// Best-effort and time-bounded: a missing connection, a relay that did not
    /// advertise control-plane telemetry, or any write error returns `Err` so
    /// the caller can fall back to HTTP. It never retries and never blocks the
    /// keepalive or registration streams, which use separate QUIC streams.
    pub(super) async fn send_telemetry(
        data_addr: SocketAddrV4,
        report_json: &[u8],
    ) -> Result<(), QuicError> {
        if report_json.len() > lightspeed_protocol::telemetry::MAX_TELEMETRY_BODY {
            return Err(QuicError::ConnectionFailed(
                "telemetry body exceeds cap".into(),
            ));
        }
        let target = supervised_target(data_addr)
            .ok_or_else(|| QuicError::ConnectionFailed("no live control connection".into()))?;
        if !target.telemetry_quic {
            return Err(QuicError::ConnectionFailed(
                "relay does not accept control-plane telemetry".into(),
            ));
        }
        let message = ControlMessage::Telemetry {
            report_json: report_json.to_vec(),
        };
        let send = async {
            let (mut send, _recv) = target
                .connection
                .open_bi()
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            message
                .write_to(&mut send)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            send.finish()
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            Ok::<(), QuicError>(())
        };
        tokio::time::timeout(TELEMETRY_SEND_TIMEOUT, send)
            .await
            .map_err(|_| QuicError::ConnectionFailed("telemetry send timed out".into()))?
    }

    /// Publish a fresh token unless this task has been stopped or superseded.
    /// A zero token is never published, so a valid token is never clobbered.
    fn publish_token(
        data_addr: SocketAddrV4,
        generation: u64,
        current_token: &AtomicU32,
        token: u32,
    ) -> bool {
        if token == 0 || !generation_is_current(data_addr, generation) {
            return false;
        }
        current_token.store(token, Ordering::Release);
        crate::session::set_path_token(data_addr, token);
        true
    }

    async fn connect_and_register(
        control_addr: SocketAddr,
        destination: Option<Ipv4Addr>,
        alpn_memory: &AlpnMemory,
        path_mtu_cap: usize,
    ) -> Result<ControlClient, QuicError> {
        let mut client = ControlClient::new()?;
        client.set_path_mtu_cap(path_mtu_cap);
        client
            .connect_with_relay_memory(
                control_addr,
                lightspeed_protocol::game_id::UNKNOWN,
                destination,
                alpn_memory,
            )
            .await?;
        Ok(client)
    }

    /// The relay's process-lifetime ALPN answer, for tests and diagnostics.
    #[cfg(test)]
    pub(super) fn supervised_alpn_memory(data_addr: SocketAddrV4) -> Option<Arc<AlpnMemory>> {
        lock_registry()
            .get(&data_addr)
            .map(|entry| Arc::clone(&entry.alpn_memory))
    }

    /// The effective path-MTU cap a live supervisor was created with, for tests.
    ///
    /// This is the value the supervisor's next `build_client_config` call uses
    /// as the DPLPMTUD upper bound (`quic_udp_payload_upper_bound`), so a test
    /// can assert the configured cap actually reached the supervised path
    /// without reproducing a sub-1500 network.
    #[cfg(test)]
    pub(super) fn supervised_path_mtu_cap(data_addr: SocketAddrV4) -> Option<usize> {
        lock_registry()
            .get(&data_addr)
            .map(|entry| entry.path_mtu_cap)
    }

    #[derive(PartialEq, Eq)]
    enum HoldOutcome {
        Stopped,
        Disconnected,
    }

    /// Ask the relay for a fresh token and apply it to the session store.
    ///
    /// Best-effort: any error (including a relay that predates the rotation
    /// message) is logged and the current token stays in force until its
    /// expiry. Rotation never tears the control connection or the session down.
    async fn rotate_once(
        client: &ControlClient,
        data_addr: SocketAddrV4,
        generation: u64,
        current_token: &AtomicU32,
    ) {
        let current = current_token.load(Ordering::Acquire);
        match client.rotate_token(current).await {
            Ok((new_token, old_token)) => {
                if new_token == 0 || !generation_is_current(data_addr, generation) {
                    return;
                }
                current_token.store(new_token, Ordering::Release);
                crate::session::rotate_session_token(data_addr, new_token);
                info!(
                    relay = %data_addr,
                    old_token,
                    new_token,
                    "session token rotated"
                );
            }
            Err(error) => {
                warn!(
                    relay = %data_addr,
                    %error,
                    "token rotation failed; keeping the current token"
                );
            }
        }
    }

    /// Time budget for reading one relay-pushed control message.
    ///
    /// Report streams are opened by the relay and are short; a stream that goes
    /// quiet (relay restarted mid-write, half-finished frame) must not pin the
    /// reader task forever.
    const SERVER_LEG_READ_TIMEOUT: Duration = Duration::from_secs(5);

    async fn hold_connection(
        client: &ControlClient,
        data_addr: SocketAddrV4,
        generation: u64,
        current_token: &AtomicU32,
        connection: Option<quinn::Connection>,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> HoldOutcome {
        let Some(connection) = connection else {
            return HoldOutcome::Disconnected;
        };
        // The relay pushes relay-measured server-leg reports on server-initiated
        // unidirectional streams. They are read here, bound to this supervisor's
        // relay, for as long as the connection lives.
        let report_reader = tokio::spawn(read_server_leg_reports(
            connection.clone(),
            data_addr,
            generation,
            stop_rx.clone(),
        ));
        let mut interval = tokio::time::interval(PING_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;

        // Token rotation is opt-out: a 0 interval disables the timer entirely,
        // so a client that does not rotate behaves exactly as it did before.
        let rotation_secs = super::token_rotation_secs();
        let mut rotation = (rotation_secs > 0).then(|| {
            let mut interval = tokio::time::interval(Duration::from_secs(rotation_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval
        });
        if let Some(interval) = rotation.as_mut() {
            // The first tick fires immediately; count the interval from now.
            interval.tick().await;
        }

        let outcome = loop {
            tokio::select! {
                _ = stop_requested(stop_rx) => break HoldOutcome::Stopped,
                _ = connection.closed() => {
                    warn!("control-plane connection closed; reconnecting");
                    break HoldOutcome::Disconnected;
                }
                _ = interval.tick() => {
                    if client.ping().await.is_err() {
                        warn!("control-plane ping failed; reconnecting");
                        break HoldOutcome::Disconnected;
                    }
                }
                _ = rotation_tick(&mut rotation) => {
                    rotate_once(client, data_addr, generation, current_token).await;
                }
            }
        };
        report_reader.abort();
        outcome
    }

    /// Read relay-pushed control messages from server-initiated uni streams.
    ///
    /// The relay pushes two server-leg report types on this path:
    /// [`ControlMessage::ServerLeg`] (its ICMP `S`) and
    /// [`ControlMessage::ServerLegApp`] (its passive forward→first-response
    /// sample). Each is stored under this supervisor's relay already, so neither
    /// can describe another relay, and it is dropped if this supervisor has been
    /// superseded before it arrived. Every other message, and every malformed
    /// frame, is ignored - the control plane's job is not to police a relay it
    /// already trusts to carry traffic, and a bad frame must never tear the
    /// session down.
    async fn read_server_leg_reports(
        connection: quinn::Connection,
        data_addr: SocketAddrV4,
        generation: u64,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        loop {
            let accepted = tokio::select! {
                _ = stop_requested(&mut stop_rx) => return,
                accepted = connection.accept_uni() => accepted,
            };
            let Ok(mut recv) = accepted else {
                return;
            };
            loop {
                let message = tokio::time::timeout(
                    SERVER_LEG_READ_TIMEOUT,
                    ControlMessage::read_from(&mut recv),
                )
                .await;
                match message {
                    Ok(Ok(Some(ControlMessage::ServerLeg {
                        server,
                        median_us,
                        jitter_us,
                        samples,
                        age_ms,
                    }))) => {
                        // `record` returns whether the report was accepted;
                        // exactly one stable info line is emitted per accepted
                        // report for the E1 harvest.
                        if generation_is_current(data_addr, generation)
                            && crate::server_leg::record(
                                data_addr, server, median_us, jitter_us, samples, age_ms,
                            )
                        {
                            let at_unix_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            info!(
                                "{}",
                                crate::server_leg::server_leg_log_line(
                                    data_addr,
                                    server,
                                    u64::from(median_us),
                                    u64::from(jitter_us),
                                    samples,
                                    age_ms,
                                    at_unix_ms,
                                )
                            );
                        }
                    }
                    Ok(Ok(Some(ControlMessage::ServerLegApp {
                        server,
                        passive_median_us,
                        passive_samples,
                        age_ms,
                    }))) => {
                        // `record_app` returns whether the report was accepted;
                        // exactly one stable info line is emitted per accepted
                        // report for the E1 harvest.
                        if generation_is_current(data_addr, generation)
                            && crate::server_leg::record_app(
                                data_addr,
                                server,
                                passive_median_us,
                                passive_samples,
                                age_ms,
                            )
                        {
                            let at_unix_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            info!(
                                "{}",
                                crate::server_leg::server_leg_app_log_line(
                                    data_addr,
                                    server,
                                    u64::from(passive_median_us),
                                    passive_samples,
                                    age_ms,
                                    at_unix_ms,
                                )
                            );
                        }
                    }
                    Ok(Ok(Some(ControlMessage::ServerTrace {
                        server,
                        hops,
                        last_hop,
                        last_hop_rtt_us,
                        flags,
                        age_ms,
                    }))) => {
                        // `record_trace` validates the shape and returns whether
                        // the report was accepted; exactly one stable info line
                        // is emitted per accepted report.
                        if generation_is_current(data_addr, generation)
                            && crate::server_leg::record_trace(
                                data_addr,
                                server,
                                hops,
                                last_hop,
                                last_hop_rtt_us,
                                flags,
                                age_ms,
                            )
                        {
                            let at_unix_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis();
                            let normalised_hop = (!last_hop.is_unspecified()).then_some(last_hop);
                            let normalised_rtt = normalised_hop
                                .and_then(|_| (last_hop_rtt_us > 0).then_some(last_hop_rtt_us));
                            info!(
                                "{}",
                                crate::server_leg::server_leg_trace_log_line(
                                    data_addr,
                                    server,
                                    hops,
                                    normalised_hop,
                                    normalised_rtt,
                                    flags & lightspeed_protocol::control::trace_flags::COMPLETE
                                        != 0,
                                    at_unix_ms,
                                )
                            );
                        }
                    }
                    Ok(Ok(Some(_))) => {}
                    // A cleanly finished stream, a truncated frame or a timeout
                    // all end this stream; the connection itself is still good.
                    Ok(Ok(None)) | Ok(Err(_)) | Err(_) => break,
                }
            }
        }
    }

    /// Await the next rotation tick, or pend forever when rotation is disabled.
    async fn rotation_tick(rotation: &mut Option<tokio::time::Interval>) {
        match rotation {
            Some(interval) => {
                interval.tick().await;
            }
            None => std::future::pending::<()>().await,
        }
    }

    async fn stop_requested(stop_rx: &mut watch::Receiver<bool>) {
        let _ = stop_rx.wait_for(|stop| *stop).await;
    }

    async fn sleep_or_stop(
        delay: Duration,
        stopped: &AtomicBool,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> bool {
        if stopped.load(Ordering::Acquire) {
            return true;
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => false,
            _ = stop_requested(stop_rx) => true,
        }
    }
}

#[cfg(feature = "quic")]
pub use supervisor::SupervisorHandle;

/// The first-attempt outcome of a supervised registration, with the facts
/// P0-3's generation guard needs (roadmap D4 item 1).
///
/// A retired or stopped supervisor's first attempt is `Superseded`, not a
/// failure: the engine records it as discarded and never writes
/// `registration_error` (invariant I5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RegistrationAttemptResult {
    /// The supervisor generation this result belongs to.
    pub generation: u64,
    /// What the first attempt did.
    pub outcome: crate::app::lifecycle::RegistrationOutcome,
    /// The relay-issued token on success.
    pub session: Option<u32>,
    /// Why the session ended, when the result is a discarded one.
    pub close_reason: Option<crate::app::lifecycle::SessionCloseReason>,
}

#[cfg(feature = "quic")]
pub fn ensure_registration(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    path_mtu_cap: usize,
) {
    supervisor::ensure_registration(data_addr, control_port, path_mtu_cap);
}

#[cfg(feature = "quic")]
pub fn ensure_registration_with_destination(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    destination: Option<std::net::Ipv4Addr>,
    path_mtu_cap: usize,
) {
    supervisor::ensure_registration_with_destination(
        data_addr,
        control_port,
        destination,
        path_mtu_cap,
    );
}

#[cfg(feature = "quic")]
pub fn report_destination(server: std::net::Ipv4Addr) {
    supervisor::report_destination(server);
}

#[cfg(feature = "quic")]
pub fn stop_supervisor(data_addr: std::net::SocketAddrV4) -> bool {
    supervisor::stop_supervisor(data_addr)
}

#[cfg(feature = "quic")]
pub fn is_supervised(data_addr: std::net::SocketAddrV4) -> bool {
    supervisor::is_supervised(data_addr)
}

#[cfg(feature = "quic")]
pub fn supervised_token(data_addr: std::net::SocketAddrV4) -> Option<u32> {
    supervisor::supervised_token(data_addr)
}

#[cfg(feature = "quic")]
pub fn supervised_path_mtu(data_addr: std::net::SocketAddrV4) -> Option<u16> {
    supervisor::supervised_path_mtu(data_addr)
}

#[cfg(feature = "quic")]
pub fn supervisor_handle(data_addr: std::net::SocketAddrV4) -> Option<SupervisorHandle> {
    supervisor::supervisor_handle(data_addr)
}

/// Send one telemetry report over the relay's live control connection.
///
/// Returns `Err` when the `quic` feature is disabled or no supervised control
/// connection is live, so callers fall back to the HTTP endpoint.
#[cfg(feature = "quic")]
pub async fn send_telemetry(
    data_addr: std::net::SocketAddrV4,
    report_json: &[u8],
) -> Result<(), crate::error::QuicError> {
    supervisor::send_telemetry(data_addr, report_json).await
}

/// Telemetry over QUIC is unavailable without the `quic` feature.
#[cfg(not(feature = "quic"))]
pub async fn send_telemetry(
    _data_addr: std::net::SocketAddrV4,
    _report_json: &[u8],
) -> Result<(), crate::error::QuicError> {
    Err(crate::error::QuicError::ConnectionFailed(
        "quic feature disabled".into(),
    ))
}

#[cfg(not(feature = "quic"))]
pub fn ensure_registration(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _path_mtu_cap: usize,
) {
}

#[cfg(not(feature = "quic"))]
pub fn ensure_registration_with_destination(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _destination: Option<std::net::Ipv4Addr>,
    _path_mtu_cap: usize,
) {
}

#[cfg(not(feature = "quic"))]
pub fn report_destination(_server: std::net::Ipv4Addr) {}

#[cfg(not(feature = "quic"))]
pub fn stop_supervisor(_data_addr: std::net::SocketAddrV4) -> bool {
    false
}

#[cfg(not(feature = "quic"))]
pub fn is_supervised(_data_addr: std::net::SocketAddrV4) -> bool {
    false
}

#[cfg(not(feature = "quic"))]
pub fn supervised_token(_data_addr: std::net::SocketAddrV4) -> Option<u32> {
    None
}

#[cfg(not(feature = "quic"))]
pub fn supervised_path_mtu(_data_addr: std::net::SocketAddrV4) -> Option<u16> {
    None
}

/// Register with the proxy's control plane to obtain a data-plane session token.
///
/// Best-effort: returns `None` (leaving the token at its default of `0`) when
/// the `quic` feature is disabled, the proxy has no control plane, or the first
/// registration attempt is rejected. The data plane then sends token `0`, which
/// the proxy accepts only when `require_auth = false`.
///
/// The registration is supervised per relay: the returned token is the initial
/// one, and a background task holds the connection open and reconnects with
/// backoff after a control-plane loss, refreshing the per-path token. Repeated
/// calls for the same relay are idempotent and do not spawn duplicate tasks.
///
/// `path_mtu_cap` is the effective `[tunnel] mtu` cap (pass
/// [`crate::tunnel::pmtud::HARD_CAP_PATH_MTU`] when there is none). It bounds
/// the control connection's DPLPMTUD so a reduced path never probes or sends
/// 1,472-byte datagrams toward the relay's control port (issue #178).
pub async fn register_session(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    path_mtu_cap: usize,
) -> Option<u32> {
    register_session_inner(data_addr, control_port, path_mtu_cap).await
}

/// Register with the relay, reporting the game server so the relay can resolve
/// its region from its MMDB. Identical to [`register_session`] otherwise,
/// including the `path_mtu_cap` bound on the control connection's DPLPMTUD.
pub async fn register_session_with_destination(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    destination: Option<std::net::Ipv4Addr>,
    path_mtu_cap: usize,
) -> Option<u32> {
    register_session_with_destination_inner(data_addr, control_port, destination, path_mtu_cap)
        .await
}

/// [`register_session`] with the generation and supersede verdict the engine's
/// state machine consumes (P0-3).
pub async fn register_session_attempt(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    register_session_attempt_inner(data_addr, control_port, path_mtu_cap).await
}

/// [`register_session_with_destination`] with the generation/supersede facts.
pub async fn register_session_with_destination_attempt(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    destination: Option<std::net::Ipv4Addr>,
    path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    register_session_with_destination_attempt_inner(
        data_addr,
        control_port,
        destination,
        path_mtu_cap,
    )
    .await
}

/// The live supervisor's registration for `data_addr`, when it has one.
pub fn supervised_registration(data_addr: std::net::SocketAddrV4) -> Option<(u64, u32)> {
    supervised_registration_inner(data_addr)
}

#[cfg(feature = "quic")]
async fn register_session_attempt_inner(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    supervisor::register_and_wait_attempt(data_addr, control_port, None, path_mtu_cap).await
}

#[cfg(feature = "quic")]
async fn register_session_with_destination_attempt_inner(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    destination: Option<std::net::Ipv4Addr>,
    path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    supervisor::register_and_wait_attempt(data_addr, control_port, destination, path_mtu_cap).await
}

#[cfg(feature = "quic")]
fn supervised_registration_inner(data_addr: std::net::SocketAddrV4) -> Option<(u64, u32)> {
    supervisor::supervised_registration(data_addr)
}

#[cfg(not(feature = "quic"))]
async fn register_session_attempt_inner(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    RegistrationAttemptResult::default()
}

#[cfg(not(feature = "quic"))]
async fn register_session_with_destination_attempt_inner(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _destination: Option<std::net::Ipv4Addr>,
    _path_mtu_cap: usize,
) -> RegistrationAttemptResult {
    RegistrationAttemptResult::default()
}

#[cfg(not(feature = "quic"))]
fn supervised_registration_inner(_data_addr: std::net::SocketAddrV4) -> Option<(u64, u32)> {
    None
}

#[cfg(feature = "quic")]
async fn register_session_inner(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    path_mtu_cap: usize,
) -> Option<u32> {
    supervisor::register_and_wait(data_addr, control_port, None, path_mtu_cap).await
}

#[cfg(feature = "quic")]
async fn register_session_with_destination_inner(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
    destination: Option<std::net::Ipv4Addr>,
    path_mtu_cap: usize,
) -> Option<u32> {
    supervisor::register_and_wait(data_addr, control_port, destination, path_mtu_cap).await
}

#[cfg(not(feature = "quic"))]
async fn register_session_inner(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _path_mtu_cap: usize,
) -> Option<u32> {
    None
}

#[cfg(not(feature = "quic"))]
async fn register_session_with_destination_inner(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
    _destination: Option<std::net::Ipv4Addr>,
    _path_mtu_cap: usize,
) -> Option<u32> {
    None
}

#[cfg(all(test, feature = "quic"))]
mod supervisor_tests {
    use super::supervisor::{
        ensure_registration, ensure_registration_with_destination, is_supervised,
        report_destination, send_telemetry, stop_supervisor, supervised_alpn_memory,
        supervised_path_mtu_cap, supervisor_handle, Backoff, MAX_BACKOFF, MIN_BACKOFF,
    };
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::sync::Arc;
    use std::time::Duration;

    /// The default cap: these tests exercise the unchanged default behaviour.
    const PATH_MTU_CAP: usize = crate::tunnel::pmtud::HARD_CAP_PATH_MTU;

    #[test]
    fn backoff_advances_exponentially_and_caps() {
        let mut backoff = Backoff::new();
        let expected = [
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_millis(1000),
            Duration::from_millis(2000),
            Duration::from_millis(4000),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        ];
        for want in expected {
            assert_eq!(backoff.advance(), want);
        }
    }

    #[test]
    fn backoff_delays_stay_bounded_and_reset() {
        let mut backoff = Backoff::new();
        for _ in 0..64 {
            let delay = backoff.next_delay();
            assert!(delay >= MIN_BACKOFF, "delay {delay:?} below floor");
            assert!(delay <= MAX_BACKOFF, "delay {delay:?} above ceiling");
            backoff.reset();
        }
    }

    #[tokio::test]
    async fn registration_is_idempotent_per_relay() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42101);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59991, PATH_MTU_CAP);
        let first = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        ensure_registration(addr, 59991, PATH_MTU_CAP);
        let second = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert_eq!(first, second, "a second request must not respawn");
        assert!(is_supervised(addr));
        assert!(stop_supervisor(addr));
        assert!(!is_supervised(addr));
    }

    #[tokio::test]
    async fn restart_after_stop_bumps_generation() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42102);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59992, PATH_MTU_CAP);
        let first = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(stop_supervisor(addr));
        ensure_registration(addr, 59992, PATH_MTU_CAP);
        let second = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(second > first, "restart must advance the generation");
        assert!(stop_supervisor(addr));
    }

    /// Given: no live supervised control connection. When: telemetry is sent.
    /// Then: it reports failure so the caller can fall back to HTTP instead of
    /// dropping the report silently.
    #[tokio::test]
    async fn telemetry_without_a_live_connection_errors() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42103);
        let _ = stop_supervisor(addr);
        assert!(
            send_telemetry(addr, b"{}").await.is_err(),
            "no live connection must surface as an error"
        );
    }

    #[tokio::test]
    async fn a_newly_learned_destination_restarts_the_supervisor() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42104);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59993, PATH_MTU_CAP);
        let first = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;

        ensure_registration_with_destination(
            addr,
            59993,
            Some(Ipv4Addr::new(8, 8, 8, 8)),
            PATH_MTU_CAP,
        );
        let second = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(
            second > first,
            "a newly learned destination must re-register to fetch its region"
        );

        ensure_registration_with_destination(
            addr,
            59993,
            Some(Ipv4Addr::new(8, 8, 8, 8)),
            PATH_MTU_CAP,
        );
        let repeated = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert_eq!(
            repeated, second,
            "a repeated identical destination must not re-register"
        );

        ensure_registration(addr, 59993, PATH_MTU_CAP);
        let third = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert_eq!(
            third, second,
            "an unknown destination must not disturb a live supervisor"
        );
        assert!(stop_supervisor(addr));
    }

    #[tokio::test]
    async fn report_destination_re_registers_once_per_new_server() {
        let _guard = crate::session::token_test_guard();
        let relay = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42105);
        let server = Ipv4Addr::new(198, 51, 100, 7);
        let _ = stop_supervisor(relay);
        crate::session::set_current_proxy(relay);
        ensure_registration(relay, 59994, PATH_MTU_CAP);
        let first = supervisor_handle(relay)
            .expect("supervisor registered")
            .generation;

        report_destination(server);
        let second = supervisor_handle(relay)
            .expect("supervisor registered")
            .generation;
        assert!(
            second > first,
            "the first report of a server must re-register the relay"
        );

        report_destination(server);
        let repeated = supervisor_handle(relay)
            .expect("supervisor registered")
            .generation;
        assert_eq!(
            repeated, second,
            "a repeated report of the same server must not re-register"
        );

        assert!(stop_supervisor(relay));
        crate::session::set_current_proxy(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));
    }

    /// Given: a relay that answered the versioned ALPN offer with alert 120.
    /// When: a newly learned destination replaces its supervisor. Then: the
    /// answer survives the replacement, so the reconnect goes straight to the
    /// empty offer instead of repeating the doomed handshake
    /// (docs/quic-alpn-rollout.md §9).
    #[tokio::test]
    async fn a_replaced_supervisor_keeps_the_relays_alpn_answer() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42106);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59995, PATH_MTU_CAP);
        let memory = supervised_alpn_memory(addr).expect("supervisor registered");
        assert!(!memory.needs_empty_offer(), "a relay starts unanswered");
        memory.record_alert_120();

        ensure_registration_with_destination(
            addr,
            59995,
            Some(Ipv4Addr::new(9, 9, 9, 9)),
            PATH_MTU_CAP,
        );
        let carried = supervised_alpn_memory(addr).expect("supervisor registered");
        assert!(
            carried.needs_empty_offer(),
            "the relay's ALPN answer must survive a supervisor replacement"
        );
        assert!(
            Arc::ptr_eq(&memory, &carried),
            "the process-lifetime memory is reused across replacements, not rebuilt"
        );
        assert!(stop_supervisor(addr));
    }

    /// Given: a reduced `[tunnel] mtu` cap. When: a supervisor is spawned for
    /// the relay. Then: the supervisor carries that cap (so the next control
    /// connection's DPLPMTUD upper bound is `quic_udp_payload_upper_bound(cap)`,
    /// not the hard cap), and a later cap change replaces it instead of being
    /// silently ignored. The default 1,500-byte cap is unchanged (issue #178).
    #[tokio::test]
    async fn a_configured_path_mtu_cap_reaches_the_supervisor() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42107);
        let _ = stop_supervisor(addr);

        ensure_registration(addr, 59996, 1400);
        assert_eq!(
            supervised_path_mtu_cap(addr),
            Some(1400),
            "the configured cap must reach the supervisor"
        );
        let reduced = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;

        ensure_registration(addr, 59996, 1400);
        assert_eq!(
            supervisor_handle(addr)
                .expect("supervisor registered")
                .generation,
            reduced,
            "the same cap must not respawn the supervisor"
        );

        ensure_registration(addr, 59996, 1492);
        assert_eq!(supervised_path_mtu_cap(addr), Some(1492));
        let restored = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(
            restored > reduced,
            "a changed cap must re-register so the next connection uses it"
        );
        assert!(stop_supervisor(addr));
    }
}

/// The control plane's UDP-payload budget follows the configured path-MTU cap.
///
/// These are the pure range tests for the #178 fix: the supervised control
/// connection must not probe or send a datagram larger than the path allows,
/// while the default 1,500-byte configuration keeps the previous 1,472-byte
/// DPLPMTUD upper bound.
#[cfg(test)]
mod mtu_tests {
    use super::quic_udp_payload_upper_bound;
    use crate::tunnel::pmtud::{HARD_CAP_PATH_MTU, HARD_CAP_UDP_PAYLOAD, MIN_PATH_MTU};

    #[test]
    fn the_control_plane_udp_payload_follows_the_configured_path_mtu() {
        // The default cap keeps exactly the previous upper bound ...
        assert_eq!(quic_udp_payload_upper_bound(HARD_CAP_PATH_MTU), 1472);
        // ... and every lower cap sheds the same 28-byte outer IP/UDP header.
        assert_eq!(quic_udp_payload_upper_bound(1492), 1464);
        assert_eq!(quic_udp_payload_upper_bound(1400), 1372);
        // The clamp floor is the documented `MIN_PATH_MTU` (1280 -> 1252).
        assert_eq!(quic_udp_payload_upper_bound(MIN_PATH_MTU), 1252);
        // QUIC's own 1,200-byte floor is never crossed ...
        assert_eq!(quic_udp_payload_upper_bound(1200), 1200);
        assert_eq!(quic_udp_payload_upper_bound(900), 1200);
        assert_eq!(quic_udp_payload_upper_bound(0), 1200);
        // ... and the data plane's hard cap is the ceiling.
        assert_eq!(
            quic_udp_payload_upper_bound(2000),
            HARD_CAP_UDP_PAYLOAD as u16
        );
        assert_eq!(
            quic_udp_payload_upper_bound(usize::MAX),
            HARD_CAP_UDP_PAYLOAD as u16
        );
        // Every documented cap lands inside quinn's [1,200, 1,472] window.
        for cap in [MIN_PATH_MTU, 1300, 1400, 1492, HARD_CAP_PATH_MTU] {
            let bound = quic_udp_payload_upper_bound(cap);
            assert!(
                (1200..=HARD_CAP_UDP_PAYLOAD as u16).contains(&bound),
                "cap {cap} -> {bound}"
            );
        }
    }
}
