//! Self-update availability check.
//!
//! Wraps `axoupdater` (cargo-dist's updater) to consult the install receipt and
//! the GitHub releases API. This module only CHECKS; it never invokes
//! `AxoUpdater::run`, so no installer is ever downloaded or executed.

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
#[allow(dead_code)] // Test-only: the runtime path delegates version comparison to axoupdater.
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

    // A missing receipt means a non-shell (e.g. Windows MSI) install, which the
    // standalone updater cannot service. Surface a friendly message rather than
    // letting `is_update_needed` fail later with a raw configuration error.
    if let Err(err) = updater.load_receipt() {
        tracing::debug!("no install receipt; update check unavailable: {err}");
        return Err(
            "update check unavailable: not installed via the shell installer \
             (e.g. Windows MSI); update through a package manager or reinstall \
             from the latest release"
                .to_string(),
        );
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

/// Maps the outcome of an update check to a single user-facing status line.
///
/// `Ok` with `update_available` true reads "Update available"; `Ok` otherwise
/// reads "You're up to date"; `Err` surfaces the underlying failure (e.g. a
/// missing install receipt on non-shell installs) verbatim.
pub fn update_status_line(result: &Result<UpdateStatus, String>) -> String {
    match result {
        Ok(status) if status.update_available => "Update available".to_string(),
        Ok(_) => "You're up to date".to_string(),
        Err(err) => err.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_versions, update_status_line, UpdateStatus};

    #[test]
    fn update_status_line_reports_available() {
        let status = UpdateStatus {
            current: "1.0.0".into(),
            latest: Some("1.1.0".into()),
            update_available: true,
        };
        assert_eq!(update_status_line(&Ok(status)), "Update available");
    }

    #[test]
    fn update_status_line_reports_up_to_date() {
        let status = UpdateStatus {
            current: "1.0.0".into(),
            latest: None,
            update_available: false,
        };
        assert_eq!(update_status_line(&Ok(status)), "You're up to date");
    }

    #[test]
    fn update_status_line_reports_error() {
        assert_eq!(
            update_status_line(&Err("no install receipt found".to_string())),
            "no install receipt found"
        );
    }

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
