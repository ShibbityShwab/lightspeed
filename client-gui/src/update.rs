//! Self-update availability check.
//!
//! Wraps `axoupdater` (cargo-dist's updater) to consult the install receipt
//! and the GitHub releases API. This module only CHECKS; it never invokes
//! `AxoUpdater::run`, so no installer is ever downloaded or executed.
//!
//! When there is no install receipt - the manual Program Files copy and MSI
//! installs have none - the check falls back to the GitHub releases API
//! directly. The resolved zip + sha256 asset pair is handed to
//! [`crate::self_update`], which performs the actual install.

use axoupdater::{AxoUpdater, Version};

/// The outcome of an update availability check.
pub struct UpdateStatus {
    /// The version this binary was compiled with (`CARGO_PKG_VERSION`).
    pub current: String,
    /// The latest published version, when it could be determined.
    pub latest: Option<String>,
    /// Whether `latest` is newer than `current`.
    pub update_available: bool,
    /// The zip + sha256 asset pair for the in-place installer, resolved when
    /// the check ran through the GitHub fallback and an update is available.
    /// `None` on the shell-installer path, on non-Windows, or when the release
    /// has no matching assets.
    pub installer: Option<ReleaseAsset>,
}

/// Download coordinates for the in-place (receipt-less) installer.
#[derive(Clone)]
pub struct ReleaseAsset {
    /// The release tag, e.g. `v1.6.14`.
    pub tag: String,
    /// Browser download URL of the GUI zip.
    pub zip_url: String,
    /// Browser download URL of the zip's `.sha256` file.
    pub sha256_url: String,
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
        let current_segment = current_segments.get(index).copied().unwrap_or(0);
        let latest_segment = latest_segments.get(index).copied().unwrap_or(0);
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

    // A missing receipt means a non-shell install (the manual Program Files
    // copy, or an MSI). The shell updater cannot service those, so fall back
    // to the GitHub releases API: the tag names the version, and the zip +
    // sha256 asset pair feeds the in-place installer.
    if updater.load_receipt().is_err() {
        return github_check(current).await;
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
        installer: None,
    })
}

/// GitHub API fallback for receipt-less installs. Pure HTTP + JSON; never
/// touches axoupdater's install machinery.
async fn github_check(current: String) -> Result<UpdateStatus, String> {
    const RELEASES_LATEST: &str =
        "https://api.github.com/repos/ShibbityShwab/lightspeed/releases/latest";

    let client = http_client()?;
    let response = client
        .get(RELEASES_LATEST)
        .send()
        .await
        .map_err(|e| format!("release lookup failed: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("release lookup failed: HTTP {status}"));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("release lookup failed: {e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "release lookup failed: no tag_name".to_string())?
        .to_string();

    let update_available = compare_versions(&current, &tag);
    let installer = if update_available {
        zip_assets(&json, &tag)
    } else {
        None
    };

    Ok(UpdateStatus {
        current,
        latest: Some(tag),
        update_available,
        installer,
    })
}

/// Resolve the Windows GUI zip and its `.sha256` sibling from a release
/// payload. Non-Windows builds never offer the in-place install.
#[cfg(windows)]
fn zip_assets(json: &serde_json::Value, tag: &str) -> Option<ReleaseAsset> {
    const GUI_ZIP: &str = "lightspeed-gui-x86_64-pc-windows-msvc.zip";

    let assets = json.get("assets")?.as_array()?;
    let find = |name: &str| -> Option<String> {
        assets.iter().find_map(|asset| {
            if asset.get("name")?.as_str()? == name {
                asset
                    .get("browser_download_url")?
                    .as_str()
                    .map(str::to_string)
            } else {
                None
            }
        })
    };
    Some(ReleaseAsset {
        tag: tag.to_string(),
        zip_url: find(GUI_ZIP)?,
        sha256_url: find(&format!("{GUI_ZIP}.sha256"))?,
    })
}

#[cfg(not(windows))]
fn zip_assets(_json: &serde_json::Value, _tag: &str) -> Option<ReleaseAsset> {
    None
}

/// Shared reqwest client for the check and the installer: a User-Agent (the
/// GitHub API rejects UA-less requests) and a generous timeout for the ~11 MB
/// asset on a slow link.
pub(crate) fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(format!(
            "lightspeed-gui-updater/{}",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())
}

/// Maps the outcome of an update check to a single user-facing status line.
///
/// Translated, so the caller sees the active language; `Err` surfaces the
/// underlying failure (e.g. a missing install receipt on non-shell installs)
/// verbatim, since it is diagnostic text rather than copy.
pub fn update_status_line(result: &Result<UpdateStatus, String>) -> String {
    match result {
        Ok(status) if status.update_available => crate::i18n::t("update.available").into_owned(),
        Ok(_) => crate::i18n::t("update.up_to_date").into_owned(),
        Err(err) => err.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{compare_versions, update_status_line, UpdateStatus};

    #[test]
    fn update_status_line_reports_available() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        let status = UpdateStatus {
            current: "1.0.0".into(),
            latest: Some("1.1.0".into()),
            update_available: true,
            installer: None,
        };
        assert_eq!(update_status_line(&Ok(status)), "Update available");
    }

    #[test]
    fn update_status_line_reports_up_to_date() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        let status = UpdateStatus {
            current: "1.0.0".into(),
            latest: None,
            update_available: false,
            installer: None,
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
