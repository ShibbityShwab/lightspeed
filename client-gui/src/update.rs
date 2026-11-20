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

// The pure update rules moved to the shared core at WF-081 W2-3b S6; this
// module keeps the network check and the GUI-facing status type. Re-exported so
// `self_update` keeps naming the same type.
pub use lightspeed_client::app::update::{compare_versions, ReleaseAsset};

use crate::platform::InstallChannel;

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
    /// The install channel this check ran through (W2-4 (g)). It drives the
    /// sheet's result line and its no-dead-button rule: only
    /// `WindowsInPlace` offers the in-place Install action.
    pub channel: InstallChannel,
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
        // The receipt path is the cargo-dist shell installer (any OS): the
        // installer/updater owns the update, not this sheet.
        channel: InstallChannel::ShellInstaller,
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

    // The receipt-less fallback: Windows can offer the in-place installer when
    // the release carries the zip + checksum; every other OS names the channel
    // it must update through (W2-4 (g): never a dead install button).
    let channel = crate::platform::update_channel(false, installer.is_some());

    Ok(UpdateStatus {
        current,
        latest: Some(tag),
        update_available,
        installer,
        channel,
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

/// Maps the outcome of an update check to the sheet's result line.
///
/// Translated, so the caller sees the active language. On an update, the
/// channel decides the copy: the Windows in-place install shows the plain
/// "Update available" (the button below acts), while a channel with no install
/// action appends its instruction, so the sheet never ends on an update the
/// user cannot act on (W2-4 (g)). `Err` surfaces the underlying failure
/// verbatim, since it is diagnostic text rather than copy.
pub fn update_status_line(result: &Result<UpdateStatus, String>) -> String {
    match result {
        Ok(status) if status.update_available => {
            crate::platform::update_available_line(status.channel)
        }
        Ok(_) => crate::i18n::t("update.up_to_date").into_owned(),
        Err(err) => err.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::{update_status_line, ReleaseAsset, UpdateStatus};
    use crate::platform::InstallChannel;

    fn status(
        update_available: bool,
        installer: Option<ReleaseAsset>,
        channel: InstallChannel,
    ) -> UpdateStatus {
        UpdateStatus {
            current: "1.0.0".into(),
            latest: update_available.then(|| "1.1.0".to_string()),
            update_available,
            installer,
            channel,
        }
    }

    fn asset() -> ReleaseAsset {
        ReleaseAsset {
            tag: "v1.1.0".into(),
            zip_url: "https://example.invalid/gui.zip".into(),
            sha256_url: "https://example.invalid/gui.zip.sha256".into(),
        }
    }

    #[test]
    fn update_status_line_reports_available_on_the_install_channel() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        let result = Ok(status(true, Some(asset()), InstallChannel::WindowsInPlace));
        assert_eq!(update_status_line(&result), "Update available");
    }

    #[test]
    fn an_instruction_channel_says_what_to_do() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        // The no-dead-button rule: a channel that cannot install must never
        // leave the sheet on a bare "Update available" with no action.
        let result = Ok(status(true, None, InstallChannel::BareTarball));
        let line = update_status_line(&result);
        assert!(line.starts_with("Update available — "), "{line}");
        assert!(!line.contains("Install"), "{line}");
    }

    #[test]
    fn update_status_line_reports_up_to_date() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        let result = Ok(status(false, None, InstallChannel::ShellInstaller));
        assert_eq!(update_status_line(&result), "You're up to date");
    }

    #[test]
    fn update_status_line_reports_error() {
        assert_eq!(
            update_status_line(&Err("no install receipt found".to_string())),
            "no install receipt found"
        );
    }
}
