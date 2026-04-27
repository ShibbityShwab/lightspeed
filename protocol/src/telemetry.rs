//! # Client Telemetry Protocol
//!
//! Defines the anonymised latency report that clients may (opt-in) POST to the
//! proxy's `/telemetry` endpoint.
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

    // ── Client version (for compatibility tracking only) ─────────────────────
    /// SemVer string of the `lightspeed` client binary.
    pub client_version: String,
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
        if self.sample_count > 100_000 {
            return Err("sample_count unreasonably large");
        }
        if self.client_country.len() > 2 {
            return Err("client_country must be 2-char ISO or empty");
        }
        if self.client_version.len() > 32 {
            return Err("client_version too long");
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
            client_version: "0.4.0-dev".to_string(),
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
            client_version: "0.4.0".to_string(),
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
            client_version: "0.4.0".to_string(),
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
            client_version: "0.4.0".to_string(),
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
            client_version: "0.4.0".to_string(),
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
}
