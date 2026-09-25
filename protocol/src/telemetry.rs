//! # Client Telemetry Protocol
//!
//! Defines the anonymised latency report that opt-in clients send to the
//! proxy, either as a `Telemetry` control message on the QUIC control
//! connection or, as a fallback, in the body of a `POST /telemetry`.
//!
//! ## Privacy design
//!
//! The report contains **only aggregated, anonymised metrics**.  The following
//! fields are **explicitly absent**:
//!
//! - IP address (the proxy already knows it; it is never stored)
//! - User / account / session identifier
//! - Hostname, username, or any device fingerprint
//! - Game server IP or port
//! - Raw packet payloads or timing sequences
//!
//! The proxy sums reports by `game_id` + `country` and exposes the aggregated
//! totals as Prometheus metrics.  No per-client records are retained.

use serde::{Deserialize, Serialize};

/// Maximum number of per-path observations a single report may carry.
pub const MAX_ROUTE_LEGS: usize = 8;

/// Maximum size in bytes of a serialised telemetry report body.
///
/// Shared by the HTTP `/telemetry` endpoint and the QUIC control-plane
/// telemetry message so both ingest paths enforce the same cap.
pub const MAX_TELEMETRY_BODY: usize = 2048;

/// A bounded, anonymised observation of a single relay leg.
///
/// The `relay` field is a stable registry node identifier (for example
/// `"relay-fra"`), never a raw address, so no network location is disclosed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathObservation {
    /// Stable relay identifier from the registry ("relay-fra"); never a raw
    /// address.
    #[serde(default)]
    pub relay: String,
    /// Median round-trip latency observed on this leg (ms).
    #[serde(default)]
    pub rtt_p50_ms: f32,
    /// 95th-percentile round-trip latency observed on this leg (ms).
    #[serde(default)]
    pub rtt_p95_ms: f32,
    /// 99th-percentile round-trip latency observed on this leg (ms).
    #[serde(default)]
    pub rtt_p99_ms: f32,
    /// Average of |consecutive RTT deltas| on this leg (ms).
    #[serde(default)]
    pub jitter_ms: f32,
    /// Number of RTT samples this leg observation is based on.
    #[serde(default)]
    pub samples: u32,
    /// Packets observed lost on this leg.
    #[serde(default)]
    pub lost: u32,
    /// Packets recovered by FEC on this leg.
    #[serde(default)]
    pub recovered: u32,
    /// Duplicate packets suppressed by the dedup window on this leg.
    #[serde(default)]
    pub dedup_saved: u32,
}

/// Anonymised latency report sent by opt-in clients after each session or
/// every 15 minutes of continuous use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReport {
    // ── Routing context ──────────────────────────────────────────────────────
    /// Numeric game ID (see `protocol::control::game_id`).  Used to bucket
    /// metrics per game type; never stored per-client.
    pub game_id: u8,

    /// Two-character ISO 3166-1 alpha-2 country code, obtained from the local
    /// system locale — **not** from the client's IP address.
    /// Empty string `""` means the client chose not to provide this field.
    #[serde(default)]
    pub client_country: String,

    // ── Aggregated latency (all values are rounded to 1 decimal place ms) ───
    /// Median round-trip latency to the proxy (ms).
    pub p50_ms: f32,
    /// 95th-percentile round-trip latency (ms).
    pub p95_ms: f32,
    /// 99th-percentile round-trip latency (ms).
    pub p99_ms: f32,
    /// Average of |consecutive RTT deltas| — a jitter proxy (ms).
    pub jitter_ms: f32,
    /// Number of RTT samples this report is based on.
    pub sample_count: u32,

    // ── FEC effectiveness ────────────────────────────────────────────────────
    /// Packets recovered by FEC during this session segment.
    #[serde(default)]
    pub fec_recoveries: u32,
    /// FEC blocks where recovery was not possible (2+ losses).
    #[serde(default)]
    pub fec_losses: u32,

    // ── Direct vs relayed latency (LightSpeed's core value metric) ───────────
    /// Median ICMP echo RTT to the detected game server over the direct,
    /// un-relayed path (ms). `None` when the direct probe has not produced a
    /// usable reply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_p50_ms: Option<f32>,
    /// Median round-trip of tunnelled game traffic through the relay
    /// (client → relay → game server → relay → client) (ms). `None` when no
    /// tunnelled round trip has been measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relayed_p50_ms: Option<f32>,
    /// Median direct application RTT to the detected game server over the
    /// un-relayed path (ms), measured by timing a small sample of the game's
    /// OWN packets. This is game-packet-to-game-packet with `relayed_p50_ms`,
    /// so the two are directly comparable, unlike the ICMP-based
    /// `direct_p50_ms`. `None` when no direct sample has been observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direct_app_p50_ms: Option<f32>,

    // ── Client version (for compatibility tracking only) ─────────────────────
    /// SemVer string of the `lightspeed` client binary.
    pub client_version: String,

    // ── Per-path observations (opt-in multipath quality reporting) ───────────
    /// Bounded per-relay observations.  An empty vector is valid (single-path
    /// clients omit this entirely).
    #[serde(default)]
    pub route_legs: Vec<PathObservation>,
}

impl TelemetryReport {
    /// Validate the report.  Returns `Err` if fields are obviously out of range
    /// (protects the proxy from malformed/malicious POST bodies).
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.p50_ms < 0.0 || self.p50_ms > 10_000.0 {
            return Err("p50_ms out of range");
        }
        if self.p95_ms < self.p50_ms || self.p95_ms > 10_000.0 {
            return Err("p95_ms out of range or less than p50");
        }
        if self.p99_ms < self.p95_ms || self.p99_ms > 10_000.0 {
            return Err("p99_ms out of range or less than p95");
        }
        if self.jitter_ms < 0.0 || self.jitter_ms > 10_000.0 {
            return Err("jitter_ms out of range");
        }
        if let Some(v) = self.direct_p50_ms {
            if !v.is_finite() || !(0.0..=10_000.0).contains(&v) {
                return Err("direct_p50_ms out of range");
            }
        }
        if let Some(v) = self.direct_app_p50_ms {
            if !v.is_finite() || !(0.0..=10_000.0).contains(&v) {
                return Err("direct_app_p50_ms out of range");
            }
        }
        if let Some(v) = self.relayed_p50_ms {
            if !v.is_finite() || !(0.0..=10_000.0).contains(&v) {
                return Err("relayed_p50_ms out of range");
            }
        }
        if self.sample_count > 100_000 {
            return Err("sample_count unreasonably large");
        }
        if self.client_country.len() > 2 {
            return Err("client_country must be 2-char ISO or empty");
        }
        if self.client_version.len() > 32 {
            return Err("client_version too long");
        }
        if self.route_legs.len() > MAX_ROUTE_LEGS {
            return Err("too many route_legs");
        }
        for leg in &self.route_legs {
            if leg.relay.is_empty() || leg.relay.len() > 64 {
                return Err("route_leg relay id must be 1..=64 chars");
            }
            if !leg
                .relay
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            {
                return Err("route_leg relay id contains invalid characters");
            }
            let rtts = [
                leg.rtt_p50_ms,
                leg.rtt_p95_ms,
                leg.rtt_p99_ms,
                leg.jitter_ms,
            ];
            if rtts.iter().any(|v| !v.is_finite()) {
                return Err("route_leg latency must be finite");
            }
            if rtts.iter().any(|v| *v < 0.0) {
                return Err("route_leg latency must be non-negative");
            }
            if leg.rtt_p95_ms < leg.rtt_p50_ms {
                return Err("route_leg rtt_p95 less than rtt_p50");
            }
            if leg.rtt_p99_ms < leg.rtt_p95_ms {
                return Err("route_leg rtt_p99 less than rtt_p95");
            }
            if leg.samples == 0 {
                return Err("route_leg samples must be positive");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_roundtrip() {
        let report = TelemetryReport {
            game_id: 1,
            client_country: "TH".to_string(),
            p50_ms: 31.2,
            p95_ms: 45.8,
            p99_ms: 67.0,
            jitter_ms: 2.1,
            sample_count: 180,
            fec_recoveries: 3,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0-dev".to_string(),
            route_legs: vec![],
        };

        let json = serde_json::to_string(&report).unwrap();
        let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.game_id, 1);
        assert_eq!(decoded.client_country, "TH");
        assert_eq!(decoded.sample_count, 180);
        assert_eq!(decoded.fec_recoveries, 3);
        assert_eq!(decoded.client_version, "0.4.0-dev");
    }

    #[test]
    fn test_telemetry_validation_ok() {
        let report = TelemetryReport {
            game_id: 0,
            client_country: "".to_string(),
            p50_ms: 30.0,
            p95_ms: 50.0,
            p99_ms: 80.0,
            jitter_ms: 3.0,
            sample_count: 60,
            fec_recoveries: 0,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0".to_string(),
            route_legs: vec![],
        };
        assert!(report.validate().is_ok());
    }

    #[test]
    fn test_telemetry_validation_bad_percentile_order() {
        let report = TelemetryReport {
            game_id: 1,
            client_country: "".to_string(),
            p50_ms: 100.0,
            p95_ms: 50.0, // p95 < p50 — invalid
            p99_ms: 200.0,
            jitter_ms: 1.0,
            sample_count: 10,
            fec_recoveries: 0,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0".to_string(),
            route_legs: vec![],
        };
        assert!(report.validate().is_err());
    }

    #[test]
    fn test_telemetry_validation_bad_country() {
        let report = TelemetryReport {
            game_id: 1,
            client_country: "TOOLONG".to_string(), // > 2 chars
            p50_ms: 30.0,
            p95_ms: 50.0,
            p99_ms: 80.0,
            jitter_ms: 1.0,
            sample_count: 10,
            fec_recoveries: 0,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0".to_string(),
            route_legs: vec![],
        };
        assert!(report.validate().is_err());
    }

    /// Ensure no PII-adjacent fields are present in the serialised output.
    #[test]
    fn test_no_pii_fields_in_json() {
        let report = TelemetryReport {
            game_id: 1,
            client_country: "US".to_string(),
            p50_ms: 50.0,
            p95_ms: 80.0,
            p99_ms: 100.0,
            jitter_ms: 2.0,
            sample_count: 100,
            fec_recoveries: 1,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0".to_string(),
            route_legs: vec![PathObservation {
                relay: "relay-fra".to_string(),
                rtt_p50_ms: 18.0,
                rtt_p95_ms: 25.0,
                rtt_p99_ms: 30.0,
                jitter_ms: 1.2,
                samples: 60,
                lost: 3,
                recovered: 2,
                dedup_saved: 1,
            }],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains("ip"),
            "IP address must not appear in telemetry JSON"
        );
        assert!(
            !json.contains("user"),
            "User ID must not appear in telemetry JSON"
        );
        assert!(
            !json.contains("session"),
            "Session ID must not appear in telemetry JSON"
        );
        assert!(
            !json.contains("host"),
            "Hostname must not appear in telemetry JSON"
        );
    }

    #[test]
    fn test_per_path_report_roundtrip() {
        let report = TelemetryReport {
            game_id: 1,
            client_country: "DE".to_string(),
            p50_ms: 20.0,
            p95_ms: 30.0,
            p99_ms: 40.0,
            jitter_ms: 1.5,
            sample_count: 120,
            fec_recoveries: 2,
            fec_losses: 1,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.5.0".to_string(),
            route_legs: vec![
                PathObservation {
                    relay: "relay-fra".to_string(),
                    rtt_p50_ms: 18.0,
                    rtt_p95_ms: 25.0,
                    rtt_p99_ms: 30.0,
                    jitter_ms: 1.2,
                    samples: 60,
                    lost: 3,
                    recovered: 2,
                    dedup_saved: 1,
                },
                PathObservation {
                    relay: "relay-ams".to_string(),
                    rtt_p50_ms: 22.0,
                    rtt_p95_ms: 33.0,
                    rtt_p99_ms: 41.0,
                    jitter_ms: 2.0,
                    samples: 60,
                    lost: 5,
                    recovered: 4,
                    dedup_saved: 0,
                },
            ],
        };

        let json = serde_json::to_string(&report).unwrap();
        let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.route_legs.len(), 2);
        assert_eq!(decoded.route_legs, report.route_legs);
    }

    #[test]
    fn test_route_legs_bounds() {
        let base_leg = PathObservation {
            relay: "relay-fra".to_string(),
            rtt_p50_ms: 18.0,
            rtt_p95_ms: 25.0,
            rtt_p99_ms: 30.0,
            jitter_ms: 1.2,
            samples: 60,
            lost: 0,
            recovered: 0,
            dedup_saved: 0,
        };
        let base = |legs: Vec<PathObservation>| TelemetryReport {
            game_id: 1,
            client_country: "DE".to_string(),
            p50_ms: 20.0,
            p95_ms: 30.0,
            p99_ms: 40.0,
            jitter_ms: 1.5,
            sample_count: 120,
            fec_recoveries: 0,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.5.0".to_string(),
            route_legs: legs,
        };

        let too_many = vec![base_leg.clone(); MAX_ROUTE_LEGS + 1];
        assert!(
            base(too_many).validate().is_err(),
            "more than MAX_ROUTE_LEGS legs must be rejected"
        );

        let mut empty_relay = base_leg.clone();
        empty_relay.relay = String::new();
        assert!(
            base(vec![empty_relay]).validate().is_err(),
            "empty relay id must be rejected"
        );

        let mut bad_order = base_leg.clone();
        bad_order.rtt_p95_ms = bad_order.rtt_p50_ms - 1.0;
        assert!(
            base(vec![bad_order]).validate().is_err(),
            "rtt_p95 below rtt_p50 must be rejected"
        );
    }

    fn minimal_report() -> TelemetryReport {
        TelemetryReport {
            game_id: 1,
            client_country: "US".to_string(),
            p50_ms: 30.0,
            p95_ms: 50.0,
            p99_ms: 80.0,
            jitter_ms: 2.0,
            sample_count: 100,
            fec_recoveries: 0,
            fec_losses: 0,
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            client_version: "0.4.0".to_string(),
            route_legs: vec![],
        }
    }

    #[test]
    fn direct_and_relayed_latency_roundtrip() {
        let mut report = minimal_report();
        report.direct_p50_ms = Some(48.5);
        report.relayed_p50_ms = Some(31.25);

        let json = serde_json::to_string(&report).unwrap();
        let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.direct_p50_ms, Some(48.5));
        assert_eq!(decoded.relayed_p50_ms, Some(31.25));
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn absent_latency_fields_are_omitted_from_json() {
        let json = serde_json::to_string(&minimal_report()).unwrap();
        assert!(
            !json.contains("direct_p50_ms"),
            "None direct_p50_ms must not appear in JSON: {json}"
        );
        assert!(
            !json.contains("relayed_p50_ms"),
            "None relayed_p50_ms must not appear in JSON: {json}"
        );
    }

    #[test]
    fn latency_fields_accept_legacy_reports_without_them() {
        let json = r#"{"game_id":2,"client_country":"TH","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":10,"client_version":"0.4.0"}"#;
        let decoded: TelemetryReport = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.direct_p50_ms, None);
        assert_eq!(decoded.relayed_p50_ms, None);
    }

    #[test]
    fn latency_validation_rejects_out_of_range() {
        let mut too_high = minimal_report();
        too_high.direct_p50_ms = Some(10_001.0);
        assert!(too_high.validate().is_err());

        let mut negative = minimal_report();
        negative.relayed_p50_ms = Some(-1.0);
        assert!(negative.validate().is_err());

        let mut not_finite = minimal_report();
        not_finite.direct_p50_ms = Some(f32::NAN);
        assert!(not_finite.validate().is_err());
    }

    /// Given: a report carrying a direct application RTT. When: it round-trips
    /// through JSON and validation. Then: the value survives, and out-of-range
    /// or non-finite values are rejected with the same bounds as `direct_p50_ms`.
    #[test]
    fn direct_app_latency_roundtrip_and_validation() {
        let mut report = minimal_report();
        report.direct_app_p50_ms = Some(47.5);

        let json = serde_json::to_string(&report).unwrap();
        let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.direct_app_p50_ms, Some(47.5));
        assert!(decoded.validate().is_ok());

        for bad in [10_001.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut r = minimal_report();
            r.direct_app_p50_ms = Some(bad);
            assert!(
                r.validate().is_err(),
                "direct_app_p50_ms {bad} must be rejected"
            );
        }
    }

    /// Given: a report with no direct application RTT. When: it is serialised.
    /// Then: the absent field is omitted from JSON entirely.
    #[test]
    fn absent_direct_app_field_is_omitted_from_json() {
        let json = serde_json::to_string(&minimal_report()).unwrap();
        assert!(
            !json.contains("direct_app"),
            "None direct_app_p50_ms must not appear in JSON: {json}"
        );
    }

    /// Given: a legacy report body predating the field. When: it is decoded.
    /// Then: the field defaults to `None`.
    #[test]
    fn direct_app_field_accepts_legacy_reports_without_it() {
        let json = r#"{"game_id":2,"client_country":"TH","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":10,"client_version":"0.4.0"}"#;
        let decoded: TelemetryReport = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.direct_app_p50_ms, None);
    }
}
