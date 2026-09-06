//! Self-update availability check.
//!
//! Wraps `axoupdater` (cargo-dist's updater) to consult the install receipt and
//! the GitHub releases API. This module only CHECKS; it never invokes
//! `AxoUpdater::run`, so no installer is ever downloaded or executed.

// The UI wiring that calls this module lands in a later task; until then none
// of these items have a caller, so allow dead_code to keep `cargo build` green
// under the repo's `RUSTFLAGS = -Dwarnings` gate.
#![allow(dead_code)]

use axoupdater::{AxoUpdater, Version};

/// The outcome of an update availability check.
pub struct UpdateStatus {
    /// The version this binary was compiled with (`CARGO_PKG_VERSION`).
    pub current: String,
    /// The latest published version, when it could be determined.
    pub latest: Option<String>,
    /// Whether `latest` is newer than `current`.
    pub update_available: bool,
}

/// Returns whether `latest` is newer than `current`.
///
/// Semver-aware and tolerant of a leading `v`/`V` prefix. Numeric segments are
/// compared as unsigned integers; non-numeric or empty segments are treated as
/// `0`, so malformed input never panics and never compares greater than a
/// well-formed version sharing the same numeric prefix.
pub fn compare_versions(current: &str, latest: &str) -> bool {
    let current_segments = numeric_segments(strip_v_prefix(current));
    let latest_segments = numeric_segments(strip_v_prefix(latest));

    let count = current_segments.len().max(latest_segments.len());
    for index in 0..count {
        let current_segment = current_segments.get(index).copied().map_or(0, |v| v);
        let latest_segment = latest_segments.get(index).copied().map_or(0, |v| v);
        match current_segment.cmp(&latest_segment) {
            std::cmp::Ordering::Less => return true,
            std::cmp::Ordering::Greater => return false,
            std::cmp::Ordering::Equal => {}
        }
    }
    false
}

fn strip_v_prefix(version: &str) -> &str {
    let version = version.trim();
    if let Some(stripped) = version.strip_prefix('v') {
        stripped
    } else if let Some(stripped) = version.strip_prefix('V') {
        stripped
    } else {
        version
    }
}

fn numeric_segments(version: &str) -> Vec<u64> {
    version.split('.').map(leading_number).collect()
}

fn leading_number(segment: &str) -> u64 {
    let mut value: u64 = 0;
    for ch in segment.chars() {
        match ch.to_digit(10) {
            Some(digit) => value = value.saturating_mul(10).saturating_add(u64::from(digit)),
            None => break,
        }
    }
    value
}

/// Runs the asynchronous axoupdater check to completion on a throwaway
/// current-thread runtime. Only checks; never performs the update. Every
/// failure is mapped to a `String`; this function never panics.
pub fn check_for_update_blocking() -> Result<UpdateStatus, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to create tokio runtime: {e}"))?;

    runtime.block_on(check_for_update(current))
}

async fn check_for_update(current: String) -> Result<UpdateStatus, String> {
    let mut updater = AxoUpdater::new_for("lightspeed-gui");

    // A missing receipt just means a non-shell (e.g. MSI) install. Log it and
    // continue; the compiled-in version is authoritative for `current` anyway,
    // and `is_update_needed` will surface the resulting misconfiguration.
    if let Err(err) = updater.load_receipt() {
        tracing::debug!("no install receipt; relying on compiled-in version: {err}");
    }

    let current_parsed =
        Version::parse(&current).map_err(|e| format!("invalid package version: {e}"))?;
    updater
        .set_current_version(current_parsed)
        .map_err(|e| e.to_string())?;

    let update_available = updater
        .is_update_needed()
        .await
        .map_err(|e| e.to_string())?;

    let latest = if update_available {
        updater
            .query_new_version()
            .await
            .map_err(|e| e.to_string())?
            .map(|v| v.to_string())
    } else {
        None
    };

    Ok(UpdateStatus {
        current,
        latest,
        update_available,
    })
}

#[cfg(test)]
mod tests {
    use super::compare_versions;

    #[test]
    fn latest_patch_bump_is_newer() {
        assert!(compare_versions("1.3.2", "v1.3.3"));
    }

    #[test]
    fn equal_versions_are_not_newer() {
        assert!(!compare_versions("1.3.2", "1.3.2"));
    }

    #[test]
    fn older_latest_is_not_newer() {
        assert!(!compare_versions("1.4.0", "1.3.9"));
    }

    #[test]
    fn multi_digit_segment_compared_numerically() {
        assert!(compare_versions("1.3.9", "1.3.10"));
    }

    #[test]
    fn leading_v_prefix_is_tolerated() {
        assert!(compare_versions("v1.3.2", "v1.3.3"));
        assert!(compare_versions("1.3.2", "V1.3.3"));
        assert!(!compare_versions("1.3.3", "V1.3.3"));
    }

    #[test]
    fn missing_segments_compare_as_zero() {
        assert!(compare_versions("1.3", "1.3.1"));
        assert!(!compare_versions("1.3.1", "1.3"));
    }

    #[test]
    fn malformed_and_empty_segments_do_not_panic() {
        assert!(!compare_versions("", ""));
        assert!(compare_versions("", "1.0.0"));
        assert!(!compare_versions("abc", "abc"));
        assert!(compare_versions("abc", "1.0.0"));
        assert!(!compare_versions("1.3.2", "1.3.2-beta"));
        assert!(!compare_versions("1..2", "1..2"));
    }
}
